//! The mobile splat-projection context
//! (render3d-mobile-compute lane, 2026-11-27).
//!
//! The production [`super::vulkan`] backend speaks
//! one precision: the fp64 projection of DESIGN.md
//! §6.1, gated on `shaderFloat64`. That gate is the
//! whole problem on a phone — Adreno / Xclipse / Mali
//! Vulkan drivers expose no fp64 shader arithmetic,
//! so the desktop projection cannot execute on a
//! Galaxy Flip 7-class device at all, and the mobile
//! app (which never depended on this crate) falls
//! back to nothing. This module is the second
//! projection path, behind the same packets:
//!
//! - `project_f64` runs the EXISTING
//!   `shaders/splat_project.comp` — the same shader
//!   source, the same [`SplatGpuParams`] /
//!   [`GpuSplatRecord`] / [`ProjectedRecord`] packets
//!   — so a device with fp64 (every desktop, some
//!   tablets) gets the estate's bit-exact path
//!   through this context too, and the bench can
//!   verify that claim per device.
//! - `project_f32` runs
//!   `shaders/splat_project_f32.comp`, the f32 twin
//!   written for the phone GPU class: same math, same
//!   order, `precise` (NoContraction) discipline in
//!   place of the Mojo lane's `--fp-mode contract=off`
//!   — see the shader's own header for the discipline
//!   and its honest limits. Its accuracy against the
//!   CPU reference is a measured gate (Tier 2 band),
//!   not a claim made here.
//!
//! Scope refusals, typed rather than silent:
//! `project_f32` refuses a layer whose TRS rotation
//! is not the identity and takes no light/shadow
//! inputs at all — those layers stay on the fp64
//! path (or the CPU pack) until a mobile design for
//! them is separately gated. The f32 path also does
//! not sort: the estate's radix keys are an f64-depth
//! contract, and the mobile sort is a follow-on
//! design, measured from the depth output this path
//! does emit.
//!
//! Buffers are host-visible/coherent on purpose:
//! phone SoCs are unified-memory machines where that
//! IS the device memory, and on desktop it keeps this
//! bench-grade context simple. The production
//! backend's device-local staging is the shape a
//! shipping integration would adopt.

use std::ffi::CStr;
use std::time::Instant;

use ash::{Entry, vk};

use crate::RenderError;
use crate::splat::SplatTransform;
use crate::splat_gpu::{
    GpuSplatRecord, MobileProjectParams, ProjectedRecord, ProjectedRecordF32, SplatCamera,
    SplatGpuParams,
};

fn be(detail: impl Into<String>) -> RenderError {
    RenderError::Backend(detail.into())
}

fn vk_err(context: &str) -> impl Fn(vk::Result) -> RenderError + '_ {
    move |r| be(format!("{context}: {r:?}"))
}

/// What one Vulkan device offers this context.
#[derive(Debug, Clone)]
pub struct MobileDeviceInfo {
    /// Device name as the driver reports it.
    pub name: String,
    /// Device type label (discrete / integrated /
    /// cpu / virtual / other).
    pub kind: &'static str,
    /// Vulkan version (major, minor, patch).
    pub api: (u32, u32, u32),
    /// The fp64 projection's hard requirement.
    pub shader_float64: bool,
    /// The radix sort's hard requirement.
    pub shader_int64: bool,
}

/// Phase timings of one projection call, milliseconds.
/// `dispatch_ms` is the median over the call's
/// repetitions with the records already resident —
/// the number comparable to a kernel-only benchmark;
/// upload and readback are reported, never folded in.
#[derive(Debug, Clone, Copy)]
pub struct ProjectTimings {
    /// Host writes of params + records.
    pub upload_ms: f64,
    /// Median submit-to-fence over the repetitions.
    pub dispatch_ms: f64,
    /// Host read of the projected records (+ keys).
    pub readback_ms: f64,
}

/// The fp64 projection's readback: the projected
/// records per asset index, the raw radix key words
/// per asset index (`[index, key_lo, key_hi, 0]`),
/// and the phase timings.
pub struct F64Projection {
    /// Projected records, indexed by asset index.
    pub records: Vec<ProjectedRecord>,
    /// Radix key words, indexed by asset index.
    pub keys: Vec<[u32; 4]>,
    /// Upload / dispatch / readback timings.
    pub timings: ProjectTimings,
}

struct Buf {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
}

impl Buf {
    fn new(
        device: &ash::Device,
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
    ) -> Result<Buf, RenderError> {
        // SAFETY: handles created here are owned by
        // the returned Buf and destroyed in destroy();
        // every fallible call is checked.
        unsafe {
            let buffer = device
                .create_buffer(
                    &vk::BufferCreateInfo::default().size(size).usage(usage),
                    None,
                )
                .map_err(vk_err("mobile buffer create"))?;
            let req = device.get_buffer_memory_requirements(buffer);
            let want =
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
            let mut pick: Option<u32> = None;
            let mut fallback: Option<u32> = None;
            for i in 0..mem_props.memory_type_count {
                let ty = mem_props.memory_types[i as usize];
                if req.memory_type_bits & (1 << i) == 0 || !ty.property_flags.contains(want) {
                    continue;
                }
                if fallback.is_none() {
                    fallback = Some(i);
                }
                if ty
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                {
                    pick = Some(i);
                    break;
                }
            }
            let Some(type_index) = pick.or(fallback) else {
                device.destroy_buffer(buffer, None);
                return Err(be("no host-visible coherent memory type"));
            };
            let memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(type_index),
                    None,
                )
                .map_err(vk_err("mobile buffer alloc"))?;
            device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(vk_err("mobile buffer bind"))?;
            Ok(Buf {
                buffer,
                memory,
                size: req.size,
            })
        }
    }

    fn write<T: bytemuck::Pod>(&self, device: &ash::Device, data: &[T]) -> Result<(), RenderError> {
        let bytes = bytemuck::cast_slice(data);
        assert!(bytes.len() as u64 <= self.size, "buffer write overflow");
        // SAFETY: the buffer is host-visible/coherent
        // (creation invariant), not in use by any
        // in-flight submission when callers write
        // (uploads happen before submit, counter resets
        // after a fence wait), and the slice length is
        // bounded by the allocation above.
        unsafe {
            let ptr = device
                .map_memory(
                    self.memory,
                    0,
                    bytes.len() as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .map_err(vk_err("mobile buffer map"))?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            device.unmap_memory(self.memory);
        }
        Ok(())
    }

    fn read<T: bytemuck::Pod>(
        &self,
        device: &ash::Device,
        count: usize,
    ) -> Result<Vec<T>, RenderError> {
        let bytes_len = count * std::mem::size_of::<T>();
        assert!(bytes_len as u64 <= self.size, "buffer read overflow");
        let mut out = vec![T::zeroed(); count];
        // SAFETY: same mapping invariant as write();
        // reads happen only after the dispatch fence
        // has been waited, so the GPU is done.
        unsafe {
            let ptr = device
                .map_memory(
                    self.memory,
                    0,
                    bytes_len as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .map_err(vk_err("mobile buffer map read"))?;
            std::ptr::copy_nonoverlapping(
                ptr.cast::<u8>(),
                bytemuck::cast_slice_mut(&mut out).as_mut_ptr(),
                bytes_len,
            );
            device.unmap_memory(self.memory);
        }
        Ok(out)
    }

    fn destroy(&self, device: &ash::Device) {
        // SAFETY: owned handles, destroyed once.
        unsafe {
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
    }
}

struct Pipe {
    desc_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

impl Pipe {
    fn destroy(&self, device: &ash::Device) {
        // SAFETY: owned handles, destroyed once, after
        // any submission using them has been waited.
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.desc_layout, None);
        }
    }
}

/// A compute-only Vulkan context that projects splat
/// layers on one enumerated device. See the module
/// docs for the two precision paths and their scopes.
pub struct MobileProjector {
    /// The loader entry, kept alive for the context's
    /// whole life: it owns the dlopened Vulkan
    /// library, and dropping it early unmaps the code
    /// every later call jumps into (a teardown
    /// segfault taught that once — 2026-11-27).
    #[allow(dead_code)]
    entry: Entry,
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    info: MobileDeviceInfo,
    cmd_pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    pipe64: Option<Pipe>,
    pipe32: Option<Pipe>,
    shadow: Option<(vk::Image, vk::DeviceMemory, vk::ImageView, vk::Sampler)>,
}

fn device_info(instance: &ash::Instance, pd: vk::PhysicalDevice) -> MobileDeviceInfo {
    // SAFETY: property/feature queries on a live
    // physical device handle from enumeration.
    unsafe {
        let props = instance.get_physical_device_properties(pd);
        let name = CStr::from_ptr(props.device_name.as_ptr())
            .to_string_lossy()
            .into_owned();
        let feats = instance.get_physical_device_features(pd);
        let api = props.api_version;
        MobileDeviceInfo {
            name,
            kind: match props.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => "discrete",
                vk::PhysicalDeviceType::INTEGRATED_GPU => "integrated",
                vk::PhysicalDeviceType::VIRTUAL_GPU => "virtual",
                vk::PhysicalDeviceType::CPU => "cpu",
                _ => "other",
            },
            api: (api >> 22, (api >> 12) & 0x3ff, api & 0xfff),
            shader_float64: feats.shader_float64 == vk::TRUE,
            shader_int64: feats.shader_int64 == vk::TRUE,
        }
    }
}

fn make_instance(entry: &Entry) -> Result<ash::Instance, RenderError> {
    let app_info = vk::ApplicationInfo::default()
        .application_name(c"torogao-render3d-mobile")
        .api_version(vk::API_VERSION_1_2);
    // SAFETY: standard instance creation; the handle
    // is owned by the caller.
    unsafe {
        entry
            .create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app_info),
                None,
            )
            .map_err(vk_err("mobile create_instance"))
    }
}

impl MobileProjector {
    /// Enumerate the devices this context could open,
    /// without opening one (the capability census: the
    /// fp64 column is the phone story in one line).
    pub fn list_devices() -> Result<Vec<MobileDeviceInfo>, RenderError> {
        // SAFETY: the temporary instance is destroyed
        // before returning; handles do not escape.
        unsafe {
            let entry = Entry::load().map_err(|e| be(format!("vulkan loader: {e}")))?;
            let instance = make_instance(&entry)?;
            let devices = instance
                .enumerate_physical_devices()
                .map_err(vk_err("mobile enumerate"))?;
            let infos: Vec<MobileDeviceInfo> = devices
                .iter()
                .map(|&pd| device_info(&instance, pd))
                .collect();
            instance.destroy_instance(None);
            Ok(infos)
        }
    }

    /// Open the device at `device_index` in
    /// enumeration order (the order `list_devices`
    /// reports).
    pub fn new(device_index: usize) -> Result<MobileProjector, RenderError> {
        // SAFETY: every handle created here is owned
        // by the returned struct (or destroyed on the
        // error path) and torn down in Drop.
        unsafe {
            let entry = Entry::load().map_err(|e| be(format!("vulkan loader: {e}")))?;
            let instance = make_instance(&entry)?;
            let devices = instance
                .enumerate_physical_devices()
                .map_err(vk_err("mobile enumerate"))?;
            let Some(&physical) = devices.get(device_index) else {
                instance.destroy_instance(None);
                return Err(be(format!(
                    "vulkan device index {device_index} out of range ({} devices)",
                    devices.len()
                )));
            };
            let info = device_info(&instance, physical);
            let families = instance.get_physical_device_queue_family_properties(physical);
            let queue_family = families
                .iter()
                .position(|f| {
                    f.queue_flags
                        .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
                })
                .or_else(|| {
                    families
                        .iter()
                        .position(|f| f.queue_flags.contains(vk::QueueFlags::COMPUTE))
                });
            let Some(queue_family) = queue_family else {
                instance.destroy_instance(None);
                return Err(be(format!("device {} has no compute queue", info.name)));
            };
            let queue_family = queue_family as u32;
            // Enable exactly the features the device
            // exposes; a path whose feature is absent
            // refuses at call time with the reason.
            let enabled = vk::PhysicalDeviceFeatures::default()
                .shader_float64(info.shader_float64)
                .shader_int64(info.shader_int64);
            let queue_info = vk::DeviceQueueCreateInfo::default()
                .queue_family_index(queue_family)
                .queue_priorities(&[1.0]);
            let device = instance
                .create_device(
                    physical,
                    &vk::DeviceCreateInfo::default()
                        .queue_create_infos(&[queue_info])
                        .enabled_features(&enabled),
                    None,
                )
                .map_err(vk_err("mobile create_device"))?;
            let queue = device.get_device_queue(queue_family, 0);
            let mem_props = instance.get_physical_device_memory_properties(physical);
            let cmd_pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(queue_family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(vk_err("mobile command pool"))?;
            let cmd = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(cmd_pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(vk_err("mobile command buffer"))?[0];
            let fence = device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(vk_err("mobile fence"))?;
            Ok(MobileProjector {
                entry,
                instance,
                device,
                queue,
                mem_props,
                info,
                cmd_pool,
                cmd,
                fence,
                pipe64: None,
                pipe32: None,
                shadow: None,
            })
        }
    }

    /// The opened device's census line.
    pub fn info(&self) -> &MobileDeviceInfo {
        &self.info
    }

    fn shader_module(&self, spv: &[u8]) -> Result<vk::ShaderModule, RenderError> {
        // SAFETY: the SPIR-V words come from the
        // crate's own build (glslc over the shaders in
        // this repo); the module is owned by the
        // caller.
        unsafe {
            let words: Vec<u32> = spv
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
                .map_err(vk_err("mobile shader module"))
        }
    }

    fn build_pipe(
        &self,
        spv: &[u8],
        storage_bindings: u32,
        with_sampler: bool,
    ) -> Result<Pipe, RenderError> {
        // SAFETY: all handles are owned by the
        // returned Pipe and destroyed in Pipe::destroy.
        unsafe {
            let mut bindings: Vec<vk::DescriptorSetLayoutBinding> = (0..storage_bindings)
                .map(|b| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(b)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::COMPUTE)
                })
                .collect();
            if with_sampler {
                bindings.push(
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(storage_bindings)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::COMPUTE),
                );
            }
            let desc_layout = self
                .device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(vk_err("mobile desc layout"))?;
            let mut pool_sizes = vec![
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(storage_bindings),
            ];
            if with_sampler {
                pool_sizes.push(
                    vk::DescriptorPoolSize::default()
                        .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .descriptor_count(1),
                );
            }
            let pool = self
                .device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .pool_sizes(&pool_sizes)
                        .max_sets(1),
                    None,
                )
                .map_err(vk_err("mobile desc pool"))?;
            let set = self
                .device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(pool)
                        .set_layouts(&[desc_layout]),
                )
                .map_err(vk_err("mobile desc set"))?[0];
            let push_range = vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .offset(0)
                .size(8);
            let pipeline_layout = self
                .device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&[desc_layout])
                        .push_constant_ranges(&[push_range]),
                    None,
                )
                .map_err(vk_err("mobile pipeline layout"))?;
            let module = self.shader_module(spv)?;
            let stage = vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::COMPUTE)
                .module(module)
                .name(c"main");
            let pipeline = self
                .device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::ComputePipelineCreateInfo::default()
                        .stage(stage)
                        .layout(pipeline_layout)],
                    None,
                )
                .map_err(|(_, e)| vk_err("mobile compute pipeline")(e))?
                .into_iter()
                .next()
                .ok_or_else(|| be("mobile compute pipeline: none created"))?;
            self.device.destroy_shader_module(module, None);
            Ok(Pipe {
                desc_layout,
                pool,
                set,
                pipeline_layout,
                pipeline,
            })
        }
    }

    fn bind_buffers(&self, pipe: &Pipe, buffers: &[&Buf]) {
        let infos: Vec<vk::DescriptorBufferInfo> = buffers
            .iter()
            .map(|b| {
                vk::DescriptorBufferInfo::default()
                    .buffer(b.buffer)
                    .offset(0)
                    .range(b.size)
            })
            .collect();
        let writes: Vec<vk::WriteDescriptorSet> = infos
            .iter()
            .enumerate()
            .map(|(i, info)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(pipe.set)
                    .dst_binding(i as u32)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(std::slice::from_ref(info))
            })
            .collect();
        // SAFETY: the set is not in use (no in-flight
        // submission references it when callers bind).
        unsafe {
            self.device.update_descriptor_sets(&writes, &[]);
        }
    }

    /// Record and run the projection dispatch `reps`
    /// times over resident buffers; returns the median
    /// submit-to-fence wall per repetition. When
    /// `zero_counter` is set, a fill + barrier resets
    /// the fp64 path's tally buffer inside the same
    /// command buffer, ahead of the dispatch.
    fn dispatch(
        &self,
        pipe: &Pipe,
        count: u32,
        zero_counter: Option<vk::Buffer>,
        reps: usize,
    ) -> Result<f64, RenderError> {
        let reps = reps.max(1);
        let mut walls: Vec<f64> = Vec::with_capacity(reps);
        // SAFETY: the command buffer is reset and
        // re-recorded per repetition only after the
        // previous submission's fence has been waited;
        // all bound resources outlive the call.
        unsafe {
            for _ in 0..reps {
                self.device
                    .reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty())
                    .map_err(vk_err("mobile cmd reset"))?;
                self.device
                    .begin_command_buffer(self.cmd, &vk::CommandBufferBeginInfo::default())
                    .map_err(vk_err("mobile cmd begin"))?;
                if let Some(counter) = zero_counter {
                    self.device.cmd_fill_buffer(self.cmd, counter, 0, 16, 0);
                    let barrier = vk::BufferMemoryBarrier::default()
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(
                            vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
                        )
                        .buffer(counter)
                        .offset(0)
                        .size(16);
                    self.device.cmd_pipeline_barrier(
                        self.cmd,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[barrier],
                        &[],
                    );
                }
                self.device.cmd_bind_pipeline(
                    self.cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipe.pipeline,
                );
                self.device.cmd_bind_descriptor_sets(
                    self.cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipe.pipeline_layout,
                    0,
                    &[pipe.set],
                    &[],
                );
                let push: [u32; 2] = [count, 0];
                self.device.cmd_push_constants(
                    self.cmd,
                    pipe.pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    bytemuck::cast_slice(&push),
                );
                self.device
                    .cmd_dispatch(self.cmd, count.div_ceil(256), 1, 1);
                let barrier = vk::MemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::HOST_READ);
                self.device.cmd_pipeline_barrier(
                    self.cmd,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::PipelineStageFlags::HOST,
                    vk::DependencyFlags::empty(),
                    &[barrier],
                    &[],
                    &[],
                );
                self.device
                    .end_command_buffer(self.cmd)
                    .map_err(vk_err("mobile cmd end"))?;
                self.device
                    .reset_fences(&[self.fence])
                    .map_err(vk_err("mobile fence reset"))?;
                let t0 = Instant::now();
                self.device
                    .queue_submit(
                        self.queue,
                        &[vk::SubmitInfo::default().command_buffers(&[self.cmd])],
                        self.fence,
                    )
                    .map_err(vk_err("mobile submit"))?;
                self.device
                    .wait_for_fences(&[self.fence], true, u64::MAX)
                    .map_err(vk_err("mobile fence wait"))?;
                walls.push(t0.elapsed().as_secs_f64() * 1e3);
            }
        }
        walls.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Ok(walls[walls.len() / 2])
    }

    fn ensure_shadow(&mut self) -> Result<(), RenderError> {
        if self.shadow.is_some() {
            return Ok(());
        }
        // SAFETY: the image/view/sampler are owned by
        // self.shadow and destroyed in Drop; the
        // layout transition runs on the context's own
        // command buffer and is fence-waited here.
        unsafe {
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::R8_UNORM)
                        .extent(vk::Extent3D {
                            width: 1,
                            height: 1,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(vk::ImageUsageFlags::SAMPLED)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .map_err(vk_err("mobile shadow image"))?;
            let req = self.device.get_image_memory_requirements(image);
            let mut pick: Option<u32> = None;
            for i in 0..self.mem_props.memory_type_count {
                if req.memory_type_bits & (1 << i) != 0 {
                    pick = Some(i);
                    break;
                }
            }
            let Some(type_index) = pick else {
                self.device.destroy_image(image, None);
                return Err(be("no memory type for the shadow dummy"));
            };
            let memory = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(type_index),
                    None,
                )
                .map_err(vk_err("mobile shadow alloc"))?;
            self.device
                .bind_image_memory(image, memory, 0)
                .map_err(vk_err("mobile shadow bind"))?;
            let view = self
                .device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(vk::Format::R8_UNORM)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        }),
                    None,
                )
                .map_err(vk_err("mobile shadow view"))?;
            let sampler = self
                .device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::NEAREST)
                        .min_filter(vk::Filter::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .map_err(vk_err("mobile shadow sampler"))?;
            // UNDEFINED -> SHADER_READ_ONLY on the
            // context's command buffer.
            self.device
                .reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty())
                .map_err(vk_err("mobile shadow cmd reset"))?;
            self.device
                .begin_command_buffer(self.cmd, &vk::CommandBufferBeginInfo::default())
                .map_err(vk_err("mobile shadow cmd begin"))?;
            let barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            self.device
                .end_command_buffer(self.cmd)
                .map_err(vk_err("mobile shadow cmd end"))?;
            self.device
                .reset_fences(&[self.fence])
                .map_err(vk_err("mobile shadow fence reset"))?;
            self.device
                .queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default().command_buffers(&[self.cmd])],
                    self.fence,
                )
                .map_err(vk_err("mobile shadow submit"))?;
            self.device
                .wait_for_fences(&[self.fence], true, u64::MAX)
                .map_err(vk_err("mobile shadow wait"))?;
            self.shadow = Some((image, memory, view, sampler));
            Ok(())
        }
    }

    /// The fp64 projection through this context: the
    /// production shader and packets, for devices
    /// that expose `shaderFloat64`. Returns the
    /// projected records and the raw radix key words
    /// per splat (`[index, key_lo, key_hi, 0]`), so a
    /// caller can gate depth bitwise against
    /// [`crate::splat_gpu::depth_key_words`].
    pub fn project_f64(
        &mut self,
        records: &[GpuSplatRecord],
        params: &SplatGpuParams,
        reps: usize,
    ) -> Result<F64Projection, RenderError> {
        if !self.info.shader_float64 {
            return Err(be(format!(
                "device {} exposes no shaderFloat64; the fp64 projection cannot run \
                 (the f32 mobile path is the one for this device class)",
                self.info.name
            )));
        }
        if self.pipe64.is_none() {
            let spv = include_bytes!(concat!(env!("OUT_DIR"), "/splat_project.comp.spv"));
            self.pipe64 = Some(self.build_pipe(spv, 5, true)?);
        }
        self.ensure_shadow()?;
        let count = records.len();
        let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let params_buf = Buf::new(
            &self.device,
            &self.mem_props,
            std::mem::size_of::<SplatGpuParams>() as u64,
            usage,
        )?;
        let records_buf = Buf::new(
            &self.device,
            &self.mem_props,
            std::mem::size_of_val(records) as u64,
            usage,
        )?;
        let projected_buf = Buf::new(
            &self.device,
            &self.mem_props,
            (count * std::mem::size_of::<ProjectedRecord>()) as u64,
            usage,
        )?;
        let keys_buf = Buf::new(&self.device, &self.mem_props, (count * 16) as u64, usage)?;
        let counter_buf = Buf::new(&self.device, &self.mem_props, 16, usage)?;
        let t0 = Instant::now();
        params_buf.write(&self.device, std::slice::from_ref(params))?;
        records_buf.write(&self.device, records)?;
        let upload_ms = t0.elapsed().as_secs_f64() * 1e3;
        let pipe = self.pipe64.as_ref().expect("pipe built above");
        self.bind_buffers(
            pipe,
            &[
                &params_buf,
                &records_buf,
                &projected_buf,
                &keys_buf,
                &counter_buf,
            ],
        );
        // The shadow binding (5): a valid 1x1 sampled
        // image. This context never lights a layer, so
        // the texel is never read; the descriptor must
        // still be well-formed.
        let (_, _, shadow_view, shadow_sampler) = self.shadow.expect("shadow ensured above");
        let image_info = vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(shadow_view)
            .sampler(shadow_sampler);
        let write = vk::WriteDescriptorSet::default()
            .dst_set(pipe.set)
            .dst_binding(5)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(std::slice::from_ref(&image_info));
        // SAFETY: the set is not in use (see
        // bind_buffers).
        unsafe {
            self.device.update_descriptor_sets(&[write], &[]);
        }
        let dispatch_ms = self.dispatch(pipe, count as u32, Some(counter_buf.buffer), reps)?;
        let t1 = Instant::now();
        let projected = projected_buf.read(&self.device, count)?;
        let keys = keys_buf.read(&self.device, count)?;
        let readback_ms = t1.elapsed().as_secs_f64() * 1e3;
        for b in [
            params_buf,
            records_buf,
            projected_buf,
            keys_buf,
            counter_buf,
        ] {
            b.destroy(&self.device);
        }
        Ok(F64Projection {
            records: projected,
            keys,
            timings: ProjectTimings {
                upload_ms,
                dispatch_ms,
                readback_ms,
            },
        })
    }

    /// The f32 mobile projection. Refuses (typed) a
    /// layer whose TRS rotation is not the identity —
    /// see the module docs for the scope.
    pub fn project_f32(
        &mut self,
        records: &[GpuSplatRecord],
        camera: &SplatCamera,
        transform: &SplatTransform,
        reps: usize,
    ) -> Result<(Vec<ProjectedRecordF32>, ProjectTimings), RenderError> {
        if !crate::splat_rotate::is_identity_quat(transform.rotation) {
            let rotation = transform.rotation;
            return Err(be(format!(
                "mobile f32 projection: layer rotation {rotation:?} is not the \
                 identity; rotated layers stay on the fp64/CPU path in this scope"
            )));
        }
        if self.pipe32.is_none() {
            let spv = include_bytes!(concat!(env!("OUT_DIR"), "/splat_project_f32.comp.spv"));
            self.pipe32 = Some(self.build_pipe(spv, 3, false)?);
        }
        let params = MobileProjectParams::new(camera, transform);
        let count = records.len();
        let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let params_buf = Buf::new(
            &self.device,
            &self.mem_props,
            std::mem::size_of::<MobileProjectParams>() as u64,
            usage,
        )?;
        let records_buf = Buf::new(
            &self.device,
            &self.mem_props,
            std::mem::size_of_val(records) as u64,
            usage,
        )?;
        let projected_buf = Buf::new(
            &self.device,
            &self.mem_props,
            (count * std::mem::size_of::<ProjectedRecordF32>()) as u64,
            usage,
        )?;
        let t0 = Instant::now();
        params_buf.write(&self.device, std::slice::from_ref(&params))?;
        records_buf.write(&self.device, records)?;
        let upload_ms = t0.elapsed().as_secs_f64() * 1e3;
        let pipe = self.pipe32.as_ref().expect("pipe built above");
        self.bind_buffers(pipe, &[&params_buf, &records_buf, &projected_buf]);
        let dispatch_ms = self.dispatch(pipe, count as u32, None, reps)?;
        let t1 = Instant::now();
        let projected = projected_buf.read(&self.device, count)?;
        let readback_ms = t1.elapsed().as_secs_f64() * 1e3;
        for b in [params_buf, records_buf, projected_buf] {
            b.destroy(&self.device);
        }
        Ok((
            projected,
            ProjectTimings {
                upload_ms,
                dispatch_ms,
                readback_ms,
            },
        ))
    }
}

impl Drop for MobileProjector {
    fn drop(&mut self) {
        // SAFETY: every submission this context made
        // was fence-waited before returning, so the
        // device is idle here; handles are destroyed
        // once, children before parents.
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some(pipe) = self.pipe64.take() {
                pipe.destroy(&self.device);
            }
            if let Some(pipe) = self.pipe32.take() {
                pipe.destroy(&self.device);
            }
            if let Some((image, memory, view, sampler)) = self.shadow.take() {
                self.device.destroy_sampler(sampler, None);
                self.device.destroy_image_view(view, None);
                self.device.destroy_image(image, None);
                self.device.free_memory(memory, None);
            }
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.cmd_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

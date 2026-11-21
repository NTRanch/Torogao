//! The Vulkan backend (D-1217): ash, headless, one
//! graphics+compute queue, offscreen colour+depth targets,
//! a sun shadow map, and frame readback. Everything Vulkan
//! is in this file; the rest of the crate never names an
//! ash type.
//!
//! Choices, with reasons (rung report carries these):
//! - **Memory: hand-rolled dedicated allocations.** The
//!   allocation pattern is small and static (a few dozen
//!   buffers/images, ~15 MB), so each resource gets its own
//!   `VkDeviceMemory`. Suballocation (gpu-allocator) would
//!   add a dependency with its own ash-version coupling to
//!   save memory this rung does not need to save; it
//!   becomes interesting when the cast/scene count grows,
//!   and the allocator seam is this file's `create_buffer`
//!   / `create_image` pair.
//! - **Synchronisation: one queue, fence per submission,
//!   wait-idle discipline.** The proof is offline and
//!   deterministic; throughput tricks (pipelining, timeline
//!   semaphores) buy nothing and cost auditability.
//! - **Dynamic buffers are host-visible + host-coherent**
//!   and rewritten between submissions, so no flush or
//!   race reasoning is needed under wait-idle.
//! - **Shadow sampling is a manual 3x3 compare**, not a
//!   comparison sampler: hardware PCF behaviour varies
//!   across vendors, and R6 wants the frame to be a
//!   function of the inputs on a fixed adapter.
//! - **No MSAA, no validation layers in evidence runs**
//!   (validation is available via RENDER3D_VALIDATION=1
//!   for smoke runs). EEVEE's plates carry temporal
//!   antialiasing; the single-sample edge delta is a named
//!   rung-1 residual measured by the comparison.

use std::ffi::CStr;

use ash::vk;
use bytemuck::{Pod, Zeroable};

use super::{
    DrawFrame, PackedScene, RenderBackend, RenderedFrame, SkinnedMesh, light_view_proj,
    piece_draw_data,
};
use crate::error::RenderError;
use crate::math::V3;

fn be(detail: impl Into<String>) -> RenderError {
    RenderError::Backend(detail.into())
}

fn vk_err(context: &str) -> impl Fn(vk::Result) -> RenderError + '_ {
    move |result| be(format!("{context}: {result:?}"))
}

const SHADOW_SIZE: u32 = 2048;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [f32; 16],
    light_view_proj: [f32; 16],
    sun: [f32; 4],
    fill: [f32; 4],
    world: [f32; 4],
    camera_pos: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CharPush {
    base_color: [f32; 4],
    roughness: f32,
    metallic: f32,
    _pad: [f32; 2],
}

/// Draw-data entry for one set piece (std430 layout of the
/// shader's `PieceDraw`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PieceDrawRaw {
    model: [f32; 16],
    normal_cols: [[f32; 4]; 3],
    albedo_rough: [f32; 4],
    emission: [f32; 4],
}

struct Buffer {
    handle: vk::Buffer,
    memory: vk::DeviceMemory,
    size: u64,
    mapped: Option<*mut u8>,
}

impl Buffer {
    fn destroy(&self, device: &ash::Device) {
        // SAFETY: the handle was created by this device and
        // is destroyed exactly once, at backend teardown,
        // after the queue has gone idle.
        unsafe {
            if self.mapped.is_some() {
                device.unmap_memory(self.memory);
            }
            device.destroy_buffer(self.handle, None);
            device.free_memory(self.memory, None);
        }
    }

    /// Write POD data into a persistently mapped buffer.
    fn write<T: Pod>(&self, data: &[T]) {
        let bytes = bytemuck::cast_slice(data);
        assert!(bytes.len() as u64 <= self.size, "buffer write overflow");
        // SAFETY: the buffer is host-visible and persistently
        // mapped for its lifetime; callers write only between
        // submissions (wait-idle discipline), so no GPU work
        // can be reading it concurrently.
        unsafe {
            let dst = self.mapped.expect("buffer not mapped");
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
        }
    }
}

struct ImageRes {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}

impl ImageRes {
    fn destroy(&self, device: &ash::Device) {
        // SAFETY: as with Buffer::destroy — unique handles,
        // torn down once, queue idle.
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

struct MeshGpu {
    set: vk::DescriptorSet,
    compute_set: vk::DescriptorSet,
    index_buffer: Buffer,
    index_count: u32,
    vertex_count: u32,
    /// Static SSBOs (device-local), kept for teardown.
    statics: Vec<Buffer>,
    out_positions: Buffer,
    out_normals: Buffer,
    stage_positions: Buffer,
    stage_normals: Buffer,
}

struct PieceGpu {
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    index_count: u32,
}

struct SceneGpu {
    colour: ImageRes,
    depth: ImageRes,
    shadow: ImageRes,
    shadow_sampler: vk::Sampler,
    render_pass: vk::RenderPass,
    shadow_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    shadow_framebuffer: vk::Framebuffer,
    char_layout: vk::DescriptorSetLayout,
    set_layout: vk::DescriptorSetLayout,
    compute_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    char_pipeline: vk::Pipeline,
    char_depth_pipeline: vk::Pipeline,
    set_pipeline: vk::Pipeline,
    set_depth_pipeline: vk::Pipeline,
    compute_pipeline: vk::Pipeline,
    char_pipeline_layout: vk::PipelineLayout,
    set_pipeline_layout: vk::PipelineLayout,
    compute_pipeline_layout: vk::PipelineLayout,
    uniforms: Buffer,
    joints: Buffer,
    weight_buffers: Vec<Buffer>,
    draw_data: Buffer,
    readback: Buffer,
    meshes: Vec<MeshGpu>,
    mesh_pushes: Vec<CharPush>,
    pieces: Vec<PieceGpu>,
    /// Packed pieces, kept for per-frame draw data and
    /// visibility (the Toy's material + every window).
    piece_meta: Vec<super::PackedSetPiece>,
    set_set: vk::DescriptorSet,
    width: u32,
    height: u32,
    clear: [f32; 4],
}

/// The Vulkan backend. Owns the instance/device and, once
/// `upload_scene` has run, the scene's GPU resources.
pub struct VulkanBackend {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    description: String,
    cmd_pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    scene: Option<SceneGpu>,
}

impl VulkanBackend {
    /// Create the instance and pick a device: the A6000
    /// first (estate heavy-compute policy), then any
    /// discrete GPU, then any device with a
    /// graphics+compute queue.
    pub fn new() -> Result<VulkanBackend, RenderError> {
        // SAFETY: ash entry points are loaded from the
        // system Vulkan loader; every call below checks its
        // Result, and handles are owned by this struct and
        // destroyed in Drop.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| be(format!("vulkan loader: {e}")))?;
        let app_info = vk::ApplicationInfo::default()
            .application_name(c"torogao-render3d")
            .api_version(vk::API_VERSION_1_2);
        let mut layers: Vec<*const i8> = Vec::new();
        let validation = std::env::var_os("RENDER3D_VALIDATION").is_some();
        if validation {
            layers.push(c"VK_LAYER_KHRONOS_validation".as_ptr());
        }
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app_info)
                    .enabled_layer_names(&layers),
                None,
            )
        }
        .map_err(vk_err("create_instance"))?;

        let devices = unsafe { instance.enumerate_physical_devices() }
            .map_err(vk_err("enumerate_physical_devices"))?;
        let mut best: Option<(i32, vk::PhysicalDevice, u32, String)> = None;
        for pd in devices {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            let families = unsafe { instance.get_physical_device_queue_family_properties(pd) };
            let queue_family = families.iter().position(|f| {
                f.queue_flags
                    .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
            });
            let Some(queue_family) = queue_family else {
                continue;
            };
            let mut score = 0;
            if props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU {
                score += 10;
            }
            if name.contains("A6000") {
                score += 100;
            } else if name.contains("NVIDIA") {
                score += 5;
            }
            if best.as_ref().is_none_or(|(s, _, _, _)| score > *s) {
                best = Some((score, pd, queue_family as u32, name));
            }
        }
        let Some((_, physical, queue_family, device_name)) = best else {
            // SAFETY: instance teardown on the error path.
            unsafe { instance.destroy_instance(None) };
            return Err(be(
                "no Vulkan device with a graphics+compute queue was found",
            ));
        };
        let props = unsafe { instance.get_physical_device_properties(physical) };
        let driver = props.driver_version;
        // Vulkan version bit layout: major << 22 |
        // minor << 12 | patch.
        let api = props.api_version;
        let description = format!(
            "{device_name} (Vulkan {}.{}.{}, driver raw {driver})",
            api >> 22,
            (api >> 12) & 0x3ff,
            api & 0xfff,
        );
        let queue_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&[1.0]);
        let device = unsafe {
            instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default().queue_create_infos(&[queue_info]),
                None,
            )
        }
        .map_err(vk_err("create_device"))?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };
        let mem_props = unsafe { instance.get_physical_device_memory_properties(physical) };
        let cmd_pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
        }
        .map_err(vk_err("create_command_pool"))?;
        let cmd = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(cmd_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }
        .map_err(vk_err("allocate_command_buffers"))?[0];
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .map_err(vk_err("create_fence"))?;
        Ok(VulkanBackend {
            _entry: entry,
            instance,
            device,
            queue,
            mem_props,
            description,
            cmd_pool,
            cmd,
            fence,
            scene: None,
        })
    }

    fn find_memory_type(
        &self,
        type_bits: u32,
        flags: vk::MemoryPropertyFlags,
    ) -> Result<u32, RenderError> {
        for i in 0..self.mem_props.memory_type_count {
            let ty = self.mem_props.memory_types[i as usize];
            if type_bits & (1 << i) != 0 && ty.property_flags.contains(flags) {
                return Ok(i);
            }
        }
        Err(be(format!("no memory type for {flags:?}")))
    }

    fn create_buffer(
        &self,
        size: u64,
        usage: vk::BufferUsageFlags,
        flags: vk::MemoryPropertyFlags,
        mapped: bool,
    ) -> Result<Buffer, RenderError> {
        assert!(size > 0, "zero-size buffer requested");
        // SAFETY: device calls with checked results; the
        // returned handles are uniquely owned by the Buffer.
        unsafe {
            let handle = self
                .device
                .create_buffer(
                    &vk::BufferCreateInfo::default().size(size).usage(usage),
                    None,
                )
                .map_err(vk_err("create_buffer"))?;
            let req = self.device.get_buffer_memory_requirements(handle);
            let memory_type = self.find_memory_type(req.memory_type_bits, flags)?;
            let memory = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(memory_type),
                    None,
                )
                .map_err(vk_err("allocate_memory"))?;
            self.device
                .bind_buffer_memory(handle, memory, 0)
                .map_err(vk_err("bind_buffer_memory"))?;
            let mapped_ptr = if mapped {
                let ptr = self
                    .device
                    .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())
                    .map_err(vk_err("map_memory"))?;
                Some(ptr as *mut u8)
            } else {
                None
            };
            Ok(Buffer {
                handle,
                memory,
                size,
                mapped: mapped_ptr,
            })
        }
    }

    fn create_image(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<ImageRes, RenderError> {
        // SAFETY: as with create_buffer; the ImageRes owns
        // image + memory + view uniquely.
        unsafe {
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(usage)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .map_err(vk_err("create_image"))?;
            let req = self.device.get_image_memory_requirements(image);
            let memory_type =
                self.find_memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
            let memory = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(memory_type),
                    None,
                )
                .map_err(vk_err("allocate_memory(image)"))?;
            self.device
                .bind_image_memory(image, memory, 0)
                .map_err(vk_err("bind_image_memory"))?;
            let view = self
                .device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(aspect)
                                .level_count(1)
                                .layer_count(1),
                        ),
                    None,
                )
                .map_err(vk_err("create_image_view"))?;
            Ok(ImageRes {
                image,
                memory,
                view,
            })
        }
    }

    /// Record `record` into the shared command buffer,
    /// submit, and wait for completion.
    fn submit_and_wait(&self, record: impl FnOnce(vk::CommandBuffer)) -> Result<(), RenderError> {
        // SAFETY: the command buffer is used by one
        // submission at a time and always waited on before
        // reuse (wait-idle discipline).
        unsafe {
            self.device
                .reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty())
                .map_err(vk_err("reset_command_buffer"))?;
            self.device
                .begin_command_buffer(self.cmd, &vk::CommandBufferBeginInfo::default())
                .map_err(vk_err("begin_command_buffer"))?;
            record(self.cmd);
            self.device
                .end_command_buffer(self.cmd)
                .map_err(vk_err("end_command_buffer"))?;
            self.device
                .reset_fences(&[self.fence])
                .map_err(vk_err("reset_fences"))?;
            self.device
                .queue_submit(
                    self.queue,
                    &[vk::SubmitInfo::default().command_buffers(&[self.cmd])],
                    self.fence,
                )
                .map_err(vk_err("queue_submit"))?;
            self.device
                .wait_for_fences(&[self.fence], true, u64::MAX)
                .map_err(vk_err("wait_for_fences"))?;
        }
        Ok(())
    }

    /// Upload bytes into a fresh device-local buffer.
    fn upload_buffer(
        &self,
        data: &[u8],
        usage: vk::BufferUsageFlags,
    ) -> Result<Buffer, RenderError> {
        let staging = self.create_buffer(
            data.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            true,
        )?;
        // SAFETY: staging is host-visible and mapped; the
        // copy is bounds-checked by the slice lengths.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                staging.mapped.expect("staging mapped"),
                data.len(),
            );
        }
        let device_buffer = self.create_buffer(
            data.len() as u64,
            usage | vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            false,
        )?;
        let src = staging.handle;
        let dst = device_buffer.handle;
        let len = data.len() as u64;
        // SAFETY: both buffers are live and sized `len`.
        self.submit_and_wait(|cmd| unsafe {
            self.device
                .cmd_copy_buffer(cmd, src, dst, &[vk::BufferCopy::default().size(len)]);
        })?;
        staging.destroy(&self.device);
        Ok(device_buffer)
    }

    fn shader_module(&self, bytes: &[u8]) -> Result<vk::ShaderModule, RenderError> {
        let words = ash::util::read_spv(&mut std::io::Cursor::new(bytes))
            .map_err(|e| be(format!("spv read: {e}")))?;
        // SAFETY: the SPIR-V words come from this crate's
        // build (glslc over in-tree sources).
        unsafe {
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        }
        .map_err(vk_err("create_shader_module"))
    }
}

fn binding(
    index: u32,
    ty: vk::DescriptorType,
    stages: vk::ShaderStageFlags,
) -> vk::DescriptorSetLayoutBinding<'static> {
    vk::DescriptorSetLayoutBinding::default()
        .binding(index)
        .descriptor_type(ty)
        .descriptor_count(1)
        .stage_flags(stages)
}

impl RenderBackend for VulkanBackend {
    fn describe(&self) -> String {
        self.description.clone()
    }

    fn upload_scene(
        &mut self,
        scene: &PackedScene,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        let device = &self.device;
        // -- Targets --------------------------------------
        let colour = self.create_image(
            width,
            height,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
            vk::ImageAspectFlags::COLOR,
        )?;
        let depth = self.create_image(
            width,
            height,
            vk::Format::D32_SFLOAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let shadow = self.create_image(
            SHADOW_SIZE,
            SHADOW_SIZE,
            vk::Format::D32_SFLOAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::DEPTH,
        )?;
        // SAFETY: sampler creation with fixed parameters.
        let shadow_sampler = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::NEAREST)
                    .min_filter(vk::Filter::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )
        }
        .map_err(vk_err("create_sampler"))?;

        // -- Render passes --------------------------------
        let colour_attachment = vk::AttachmentDescription::default()
            .format(vk::Format::R8G8B8A8_UNORM)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL);
        let depth_attachment = vk::AttachmentDescription::default()
            .format(vk::Format::D32_SFLOAT)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
        let colour_ref = vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        let depth_ref = vk::AttachmentReference::default()
            .attachment(1)
            .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
        let subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(std::slice::from_ref(&colour_ref))
            .depth_stencil_attachment(&depth_ref);
        let main_deps = [
            vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                )
                .dst_stage_mask(
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                )
                .dst_access_mask(
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                ),
            vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ),
        ];
        // SAFETY: render-pass creation; attachments and
        // references above outlive the call.
        let render_pass = unsafe {
            device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&[colour_attachment, depth_attachment])
                    .subpasses(std::slice::from_ref(&subpass))
                    .dependencies(&main_deps),
                None,
            )
        }
        .map_err(vk_err("create_render_pass"))?;

        let shadow_attachment = vk::AttachmentDescription::default()
            .format(vk::Format::D32_SFLOAT)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let shadow_ref = vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
        let shadow_subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .depth_stencil_attachment(&shadow_ref);
        let shadow_deps = [
            vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(
                    vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                )
                .dst_stage_mask(vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE),
            vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::LATE_FRAGMENT_TESTS)
                .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ),
        ];
        let shadow_pass = unsafe {
            device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&[shadow_attachment])
                    .subpasses(std::slice::from_ref(&shadow_subpass))
                    .dependencies(&shadow_deps),
                None,
            )
        }
        .map_err(vk_err("create_shadow_pass"))?;

        // SAFETY: framebuffers bind the views created above.
        let framebuffer = unsafe {
            device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(render_pass)
                    .attachments(&[colour.view, depth.view])
                    .width(width)
                    .height(height)
                    .layers(1),
                None,
            )
        }
        .map_err(vk_err("create_framebuffer"))?;
        let shadow_framebuffer = unsafe {
            device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(shadow_pass)
                    .attachments(&[shadow.view])
                    .width(SHADOW_SIZE)
                    .height(SHADOW_SIZE)
                    .layers(1),
                None,
            )
        }
        .map_err(vk_err("create_shadow_framebuffer"))?;

        // -- Descriptor layouts ---------------------------
        let v = vk::ShaderStageFlags::VERTEX;
        let f = vk::ShaderStageFlags::FRAGMENT;
        let ubo = vk::DescriptorType::UNIFORM_BUFFER;
        let ssbo = vk::DescriptorType::STORAGE_BUFFER;
        let char_bindings = [
            binding(0, ubo, v | f),
            binding(1, ssbo, v),
            binding(2, ssbo, v),
            binding(3, ssbo, v),
            binding(4, ssbo, v),
            binding(5, ssbo, v),
            binding(6, ssbo, v),
            binding(7, ssbo, v),
            binding(8, ssbo, v),
            binding(9, ssbo, v),
            binding(10, ssbo, v),
            binding(11, ssbo, v),
            binding(12, ssbo, v),
            binding(13, vk::DescriptorType::COMBINED_IMAGE_SAMPLER, f),
        ];
        let set_bindings = [
            binding(0, ubo, v | f),
            binding(1, vk::DescriptorType::COMBINED_IMAGE_SAMPLER, f),
            binding(2, ssbo, v),
        ];
        let c = vk::ShaderStageFlags::COMPUTE;
        let compute_bindings: Vec<_> = (0..12).map(|i| binding(i, ssbo, c)).collect();
        // SAFETY: layout creation from the binding tables.
        let char_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&char_bindings),
                None,
            )
        }
        .map_err(vk_err("char layout"))?;
        let set_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&set_bindings),
                None,
            )
        }
        .map_err(vk_err("set layout"))?;
        let compute_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&compute_bindings),
                None,
            )
        }
        .map_err(vk_err("compute layout"))?;

        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(ubo)
                .descriptor_count(8),
            vk::DescriptorPoolSize::default()
                .ty(ssbo)
                .descriptor_count(160),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(8),
        ];
        let pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(24)
                    .pool_sizes(&pool_sizes),
                None,
            )
        }
        .map_err(vk_err("descriptor pool"))?;

        // -- Shared dynamic buffers -----------------------
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let uniforms_buffer = self.create_buffer(
            std::mem::size_of::<Uniforms>() as u64,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            host,
            true,
        )?;
        let sun_dir = V3::new(
            f64::from(scene.sun[0]),
            f64::from(scene.sun[1]),
            f64::from(scene.sun[2]),
        );
        let uniforms = Uniforms {
            view_proj: scene.view_proj,
            light_view_proj: light_view_proj(sun_dir).to_f32_cols(),
            sun: scene.sun,
            fill: scene.fill,
            world: scene.world,
            camera_pos: scene.camera_pos,
        };
        uniforms_buffer.write(&[uniforms]);
        let joints_buffer = self.create_buffer(
            (scene.joint_count * 64) as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            host,
            true,
        )?;
        let mut weight_buffers = Vec::new();
        for mesh in &scene.meshes {
            // Sized for the mesh's morph-weight array (one
            // dummy float for target-less meshes, whose
            // weight buffer is never read through a range).
            let floats = mesh.target_count.max(1);
            weight_buffers.push(self.create_buffer(
                u64::from(floats) * 4,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                host,
                true,
            )?);
        }
        let mesh_pushes: Vec<CharPush> = scene
            .meshes
            .iter()
            .map(|mesh| {
                let material = scene.materials[mesh.material];
                CharPush {
                    // COLOR_1 paint already carries the
                    // material's base colour (measured on
                    // Prima: hair COLOR_1 == its material
                    // factor exactly) — multiplying again
                    // would double-apply it.
                    base_color: if mesh.paint_from_color1 {
                        [1.0, 1.0, 1.0, 1.0]
                    } else {
                        material.base_color
                    },
                    roughness: material.roughness,
                    metallic: material.metallic,
                    _pad: [0.0; 2],
                }
            })
            .collect();

        // -- Meshes ---------------------------------------
        let mut meshes_gpu = Vec::new();
        // SAFETY: set allocation from the pool above.
        let char_sets = {
            let layouts: Vec<_> = scene.meshes.iter().map(|_| char_layout).collect();
            unsafe {
                device.allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(pool)
                        .set_layouts(&layouts),
                )
            }
            .map_err(vk_err("allocate char sets"))?
        };
        let compute_sets = {
            let layouts: Vec<_> = scene.meshes.iter().map(|_| compute_layout).collect();
            unsafe {
                device.allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(pool)
                        .set_layouts(&layouts),
                )
            }
            .map_err(vk_err("allocate compute sets"))?
        };
        for (mi, mesh) in scene.meshes.iter().enumerate() {
            let usage = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER;
            let sbuf = |data: &[u8]| self.upload_buffer(data, vk::BufferUsageFlags::STORAGE_BUFFER);
            let positions = sbuf(bytemuck::cast_slice(&mesh.positions))?;
            let normals = sbuf(bytemuck::cast_slice(&mesh.normals))?;
            let color0 = sbuf(bytemuck::cast_slice(&mesh.color0))?;
            let color1 = sbuf(bytemuck::cast_slice(&mesh.color1))?;
            let joints_b = sbuf(bytemuck::cast_slice(&mesh.joints))?;
            let weights_b = sbuf(bytemuck::cast_slice(&mesh.weights))?;
            // Empty morph arrays still need a (dummy) buffer:
            // Vulkan forbids null descriptors. One-entry
            // zero arrays stand in; the CSR ranges for such
            // meshes are all [0,0) so nothing reads them.
            let ranges_data: &[[u32; 2]] = if mesh.morph_ranges.is_empty() {
                &[[0, 0]]
            } else {
                &mesh.morph_ranges
            };
            let target_data: &[u32] = if mesh.morph_entry_target.is_empty() {
                &[0]
            } else {
                &mesh.morph_entry_target
            };
            let dpos_data: &[[f32; 4]] = if mesh.morph_entry_dpos.is_empty() {
                &[[0.0; 4]]
            } else {
                &mesh.morph_entry_dpos
            };
            let dnrm_data: &[[f32; 4]] = if mesh.morph_entry_dnrm.is_empty() {
                &[[0.0; 4]]
            } else {
                &mesh.morph_entry_dnrm
            };
            let ranges = sbuf(bytemuck::cast_slice(ranges_data))?;
            let targets = sbuf(bytemuck::cast_slice(target_data))?;
            let dpos = sbuf(bytemuck::cast_slice(dpos_data))?;
            let dnrm = sbuf(bytemuck::cast_slice(dnrm_data))?;
            let index_buffer = self.upload_buffer(bytemuck::cast_slice(&mesh.indices), usage)?;
            let out_positions = self.create_buffer(
                (mesh.vertex_count as u64) * 16,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                false,
            )?;
            let out_normals = self.create_buffer(
                (mesh.vertex_count as u64) * 16,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                false,
            )?;
            let stage_positions = self.create_buffer(
                (mesh.vertex_count as u64) * 16,
                vk::BufferUsageFlags::TRANSFER_DST,
                host,
                true,
            )?;
            let stage_normals = self.create_buffer(
                (mesh.vertex_count as u64) * 16,
                vk::BufferUsageFlags::TRANSFER_DST,
                host,
                true,
            )?;

            // Descriptor writes for the graphics set.
            let info = |buffer: &Buffer| {
                [vk::DescriptorBufferInfo::default()
                    .buffer(buffer.handle)
                    .range(vk::WHOLE_SIZE)]
            };
            let ubo_info = info(&uniforms_buffer);
            let joint_info = info(&joints_buffer);
            let pos_info = info(&positions);
            let nrm_info = info(&normals);
            let c0_info = info(&color0);
            let c1_info = info(&color1);
            let j_info = info(&joints_b);
            let w_info = info(&weights_b);
            let r_info = info(&ranges);
            let dp_info = info(&dpos);
            let dn_info = info(&dnrm);
            let t_info = info(&targets);
            let mw_info = info(&weight_buffers[mi]);
            let shadow_info = [vk::DescriptorImageInfo::default()
                .sampler(shadow_sampler)
                .image_view(shadow.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let set = char_sets[mi];
            let write = |binding: u32, ty: vk::DescriptorType| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(binding)
                    .descriptor_type(ty)
            };
            let writes = [
                write(0, ubo).buffer_info(&ubo_info),
                write(1, ssbo).buffer_info(&joint_info),
                write(2, ssbo).buffer_info(&pos_info),
                write(3, ssbo).buffer_info(&nrm_info),
                write(4, ssbo).buffer_info(&c0_info),
                write(5, ssbo).buffer_info(&c1_info),
                write(6, ssbo).buffer_info(&j_info),
                write(7, ssbo).buffer_info(&w_info),
                write(8, ssbo).buffer_info(&r_info),
                write(9, ssbo).buffer_info(&dp_info),
                write(10, ssbo).buffer_info(&dn_info),
                write(11, ssbo).buffer_info(&t_info),
                write(12, ssbo).buffer_info(&mw_info),
                write(13, vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&shadow_info),
            ];
            // SAFETY: all referenced buffers/views are live
            // and owned by this scene.
            unsafe { device.update_descriptor_sets(&writes, &[]) };

            // Compute set (bindings per skin.comp).
            let cset = compute_sets[mi];
            let op_info = info(&out_positions);
            let on_info = info(&out_normals);
            let cwrite = |binding: u32| {
                vk::WriteDescriptorSet::default()
                    .dst_set(cset)
                    .dst_binding(binding)
                    .descriptor_type(ssbo)
            };
            let cwrites = [
                cwrite(0).buffer_info(&joint_info),
                cwrite(1).buffer_info(&pos_info),
                cwrite(2).buffer_info(&nrm_info),
                cwrite(3).buffer_info(&j_info),
                cwrite(4).buffer_info(&w_info),
                cwrite(5).buffer_info(&r_info),
                cwrite(6).buffer_info(&dp_info),
                cwrite(7).buffer_info(&dn_info),
                cwrite(8).buffer_info(&t_info),
                cwrite(9).buffer_info(&mw_info),
                cwrite(10).buffer_info(&op_info),
                cwrite(11).buffer_info(&on_info),
            ];
            unsafe { device.update_descriptor_sets(&cwrites, &[]) };

            meshes_gpu.push(MeshGpu {
                set,
                compute_set: cset,
                index_buffer,
                index_count: mesh.indices.len() as u32,
                vertex_count: mesh.vertex_count,
                statics: vec![
                    positions, normals, color0, color1, joints_b, weights_b, ranges, targets, dpos,
                    dnrm,
                ],
                out_positions,
                out_normals,
                stage_positions,
                stage_normals,
            });
        }

        // -- Set pieces -----------------------------------
        let draw_data = self.create_buffer(
            (scene.set_pieces.len() as u64) * std::mem::size_of::<PieceDrawRaw>() as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            host,
            true,
        )?;
        let draw_entries: Vec<PieceDrawRaw> = scene
            .set_pieces
            .iter()
            .map(|p| PieceDrawRaw {
                model: p.model,
                normal_cols: p.normal_cols,
                albedo_rough: p.albedo_rough,
                emission: p.emission,
            })
            .collect();
        draw_data.write(&draw_entries);
        let mut pieces_gpu = Vec::new();
        for piece in &scene.set_pieces {
            let mut interleaved = Vec::with_capacity(piece.positions.len() * 6);
            for (p, n) in piece.positions.iter().zip(piece.normals.iter()) {
                interleaved.extend_from_slice(p);
                interleaved.extend_from_slice(n);
            }
            let vertex_buffer = self.upload_buffer(
                bytemuck::cast_slice(&interleaved),
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            let index_buffer = self.upload_buffer(
                bytemuck::cast_slice(&piece.indices),
                vk::BufferUsageFlags::INDEX_BUFFER,
            )?;
            pieces_gpu.push(PieceGpu {
                vertex_buffer,
                index_buffer,
                index_count: piece.indices.len() as u32,
            });
        }
        let set_set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&[set_layout]),
            )
        }
        .map_err(vk_err("allocate set set"))?[0];
        {
            let ubo_info = [vk::DescriptorBufferInfo::default()
                .buffer(uniforms_buffer.handle)
                .range(vk::WHOLE_SIZE)];
            let shadow_info = [vk::DescriptorImageInfo::default()
                .sampler(shadow_sampler)
                .image_view(shadow.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let dd_info = [vk::DescriptorBufferInfo::default()
                .buffer(draw_data.handle)
                .range(vk::WHOLE_SIZE)];
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(set_set)
                    .dst_binding(0)
                    .descriptor_type(ubo)
                    .buffer_info(&ubo_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(set_set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&shadow_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(set_set)
                    .dst_binding(2)
                    .descriptor_type(ssbo)
                    .buffer_info(&dd_info),
            ];
            // SAFETY: referenced resources are live.
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        }

        // -- Pipelines ------------------------------------
        let char_vert = self.shader_module(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/character.vert.spv"
        )))?;
        let char_frag = self.shader_module(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/character.frag.spv"
        )))?;
        let skin_vert =
            self.shader_module(include_bytes!(concat!(env!("OUT_DIR"), "/skin.vert.spv")))?;
        let depth_frag = self.shader_module(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/depthonly.frag.spv"
        )))?;
        let set_vert =
            self.shader_module(include_bytes!(concat!(env!("OUT_DIR"), "/set.vert.spv")))?;
        let set_frag =
            self.shader_module(include_bytes!(concat!(env!("OUT_DIR"), "/set.frag.spv")))?;
        let setdepth_vert = self.shader_module(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/setdepth.vert.spv"
        )))?;
        let skin_comp =
            self.shader_module(include_bytes!(concat!(env!("OUT_DIR"), "/skin.comp.spv")))?;

        let char_push = vk::PushConstantRange::default()
            .stage_flags(v | f)
            .size(std::mem::size_of::<CharPush>() as u32);
        let set_push = vk::PushConstantRange::default().stage_flags(v).size(4);
        let compute_push = vk::PushConstantRange::default().stage_flags(c).size(4);
        // SAFETY: pipeline layouts from the layouts above.
        let char_pipeline_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[char_layout])
                    .push_constant_ranges(&[char_push]),
                None,
            )
        }
        .map_err(vk_err("char pipeline layout"))?;
        let set_pipeline_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[set_layout])
                    .push_constant_ranges(&[set_push]),
                None,
            )
        }
        .map_err(vk_err("set pipeline layout"))?;
        let compute_pipeline_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[compute_layout])
                    .push_constant_ranges(&[compute_push]),
                None,
            )
        }
        .map_err(vk_err("compute pipeline layout"))?;

        let stage = |module: vk::ShaderModule, stage_flags: vk::ShaderStageFlags| {
            vk::PipelineShaderStageCreateInfo::default()
                .stage(stage_flags)
                .module(module)
                .name(c"main")
        };
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let dynamic = vk::PipelineDynamicStateCreateInfo::default()
            .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS);
        let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA);
        let blend = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(std::slice::from_ref(&blend_attachment));
        let no_blend = vk::PipelineColorBlendStateCreateInfo::default();
        let empty_vertex = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

        let set_binding = vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(24)
            .input_rate(vk::VertexInputRate::VERTEX);
        let set_attrs = [
            vk::VertexInputAttributeDescription::default()
                .binding(0)
                .location(0)
                .format(vk::Format::R32G32B32_SFLOAT)
                .offset(0),
            vk::VertexInputAttributeDescription::default()
                .binding(0)
                .location(1)
                .format(vk::Format::R32G32B32_SFLOAT)
                .offset(12),
        ];
        let set_vertex = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(std::slice::from_ref(&set_binding))
            .vertex_attribute_descriptions(&set_attrs);

        let make_graphics = |stages: &[vk::PipelineShaderStageCreateInfo<'_>],
                             vertex_input: &vk::PipelineVertexInputStateCreateInfo<'_>,
                             layout: vk::PipelineLayout,
                             pass: vk::RenderPass,
                             with_blend: bool|
         -> Result<vk::Pipeline, RenderError> {
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(stages)
                .vertex_input_state(vertex_input)
                .input_assembly_state(&input_assembly)
                .viewport_state(&viewport_state)
                .rasterization_state(&raster)
                .multisample_state(&multisample)
                .depth_stencil_state(&depth_stencil)
                .color_blend_state(if with_blend { &blend } else { &no_blend })
                .dynamic_state(&dynamic)
                .layout(layout)
                .render_pass(pass);
            // SAFETY: all referenced state structs outlive
            // the call; shaders are this crate's modules.
            unsafe {
                device
                    .create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
                    .map_err(|(_, result)| vk_err("create_graphics_pipelines")(result))?
                    .into_iter()
                    .next()
                    .ok_or_else(|| be("pipeline creation returned none"))
            }
        };

        let char_pipeline = make_graphics(
            &[stage(char_vert, v), stage(char_frag, f)],
            &empty_vertex,
            char_pipeline_layout,
            render_pass,
            true,
        )?;
        let char_depth_pipeline = make_graphics(
            &[stage(skin_vert, v), stage(depth_frag, f)],
            &empty_vertex,
            char_pipeline_layout,
            shadow_pass,
            false,
        )?;
        let set_pipeline = make_graphics(
            &[stage(set_vert, v), stage(set_frag, f)],
            &set_vertex,
            set_pipeline_layout,
            render_pass,
            true,
        )?;
        let set_depth_pipeline = make_graphics(
            &[stage(setdepth_vert, v), stage(depth_frag, f)],
            &set_vertex,
            set_pipeline_layout,
            shadow_pass,
            false,
        )?;
        // SAFETY: compute pipeline from this crate's module.
        let compute_pipeline = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::ComputePipelineCreateInfo::default()
                        .stage(stage(skin_comp, c))
                        .layout(compute_pipeline_layout)],
                    None,
                )
                .map_err(|(_, result)| vk_err("create_compute_pipelines")(result))?
                .into_iter()
                .next()
                .ok_or_else(|| be("compute pipeline creation returned none"))
        }?;

        // Shader modules can go now that pipelines exist.
        // SAFETY: no pipeline references the modules after
        // creation.
        unsafe {
            for module in [
                char_vert,
                char_frag,
                skin_vert,
                depth_frag,
                set_vert,
                set_frag,
                setdepth_vert,
                skin_comp,
            ] {
                device.destroy_shader_module(module, None);
            }
        }

        let readback = self.create_buffer(
            u64::from(width) * u64::from(height) * 4,
            vk::BufferUsageFlags::TRANSFER_DST,
            host,
            true,
        )?;

        self.scene = Some(SceneGpu {
            colour,
            depth,
            shadow,
            shadow_sampler,
            render_pass,
            shadow_pass,
            framebuffer,
            shadow_framebuffer,
            char_layout,
            set_layout,
            compute_layout,
            pool,
            char_pipeline,
            char_depth_pipeline,
            set_pipeline,
            set_depth_pipeline,
            compute_pipeline,
            char_pipeline_layout,
            set_pipeline_layout,
            compute_pipeline_layout,
            uniforms: uniforms_buffer,
            joints: joints_buffer,
            weight_buffers,
            draw_data,
            readback,
            meshes: meshes_gpu,
            mesh_pushes,
            pieces: pieces_gpu,
            piece_meta: scene.set_pieces.clone(),
            set_set,
            width,
            height,
            clear: scene.clear_rgba,
        });
        Ok(())
    }

    fn render_frame(&mut self, draw: &DrawFrame) -> Result<RenderedFrame, RenderError> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| be("render before upload"))?;
        // Dynamic data first (host writes, before submit).
        scene.joints.write(&draw.joint_matrices);
        for (i, weights) in draw.mesh_weights.iter().enumerate() {
            if weights.is_empty() {
                scene.weight_buffers[i].write(&[0.0_f32; 1]);
            } else {
                scene.weight_buffers[i].write(weights);
            }
        }
        // Toy draw-data entry.
        for (i, piece) in scene.piece_meta.iter().enumerate() {
            if piece.is_toy {
                let (model, cols, albedo_rough, emission) = piece_draw_data(
                    draw.toy_model,
                    [
                        piece.albedo_rough[0],
                        piece.albedo_rough[1],
                        piece.albedo_rough[2],
                    ],
                    piece.albedo_rough[3],
                    [piece.emission[0], piece.emission[1], piece.emission[2]],
                    piece.emission[3],
                );
                let entry = PieceDrawRaw {
                    model,
                    normal_cols: cols,
                    albedo_rough,
                    emission,
                };
                let offset = (i * std::mem::size_of::<PieceDrawRaw>()) as isize;
                // SAFETY: draw_data is persistently mapped
                // and sized for all pieces; writes happen
                // between submissions only.
                unsafe {
                    let dst = scene
                        .draw_data
                        .mapped
                        .expect("draw data mapped")
                        .offset(offset);
                    std::ptr::copy_nonoverlapping(
                        bytemuck::bytes_of(&entry).as_ptr(),
                        dst,
                        std::mem::size_of::<PieceDrawRaw>(),
                    );
                }
            }
        }

        let device = &self.device;
        let frame = draw.frame;
        let include_set = draw.include_set;
        let visible_meshes = &draw.visible_meshes;
        let width = scene.width;
        let height = scene.height;
        let clear_colour = scene.clear;
        // SAFETY: command recording references only live
        // scene resources; the submission is waited on
        // before any of them is reused or read.
        self.submit_and_wait(|cmd| unsafe {
            // Shadow pass.
            let shadow_clear = [vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            }];
            device.cmd_begin_render_pass(
                cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(scene.shadow_pass)
                    .framebuffer(scene.shadow_framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                            width: SHADOW_SIZE,
                            height: SHADOW_SIZE,
                        },
                    })
                    .clear_values(&shadow_clear),
                vk::SubpassContents::INLINE,
            );
            let shadow_viewport = [vk::Viewport::default()
                .width(SHADOW_SIZE as f32)
                .height(SHADOW_SIZE as f32)
                .max_depth(1.0)];
            device.cmd_set_viewport(cmd, 0, &shadow_viewport);
            let shadow_scissor = [vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: SHADOW_SIZE,
                    height: SHADOW_SIZE,
                },
            }];
            device.cmd_set_scissor(cmd, 0, &shadow_scissor);
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                scene.char_depth_pipeline,
            );
            for (mi, mesh) in scene.meshes.iter().enumerate() {
                if !visible_meshes.get(mi).copied().unwrap_or(false) {
                    continue;
                }
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    scene.char_pipeline_layout,
                    0,
                    &[mesh.set],
                    &[],
                );
                device.cmd_bind_index_buffer(
                    cmd,
                    mesh.index_buffer.handle,
                    0,
                    vk::IndexType::UINT32,
                );
                device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
            }
            if include_set {
                device.cmd_bind_pipeline(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    scene.set_depth_pipeline,
                );
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    scene.set_pipeline_layout,
                    0,
                    &[scene.set_set],
                    &[],
                );
                for (pi, piece) in scene.pieces.iter().enumerate() {
                    if !scene.piece_meta[pi].visible(frame) {
                        continue;
                    }
                    device.cmd_push_constants(
                        cmd,
                        scene.set_pipeline_layout,
                        vk::ShaderStageFlags::VERTEX,
                        0,
                        bytemuck::bytes_of(&(pi as u32)),
                    );
                    device.cmd_bind_vertex_buffers(cmd, 0, &[piece.vertex_buffer.handle], &[0]);
                    device.cmd_bind_index_buffer(
                        cmd,
                        piece.index_buffer.handle,
                        0,
                        vk::IndexType::UINT32,
                    );
                    device.cmd_draw_indexed(cmd, piece.index_count, 1, 0, 0, 0);
                }
            }
            device.cmd_end_render_pass(cmd);

            // Main pass.
            let clears = [
                vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: clear_colour,
                    },
                },
                vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            ];
            device.cmd_begin_render_pass(
                cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(scene.render_pass)
                    .framebuffer(scene.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D { width, height },
                    })
                    .clear_values(&clears),
                vk::SubpassContents::INLINE,
            );
            let viewport = [vk::Viewport::default()
                .width(width as f32)
                .height(height as f32)
                .max_depth(1.0)];
            device.cmd_set_viewport(cmd, 0, &viewport);
            let scissor = [vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D { width, height },
            }];
            device.cmd_set_scissor(cmd, 0, &scissor);
            if include_set {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, scene.set_pipeline);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    scene.set_pipeline_layout,
                    0,
                    &[scene.set_set],
                    &[],
                );
                for (pi, piece) in scene.pieces.iter().enumerate() {
                    if !scene.piece_meta[pi].visible(frame) {
                        continue;
                    }
                    device.cmd_push_constants(
                        cmd,
                        scene.set_pipeline_layout,
                        vk::ShaderStageFlags::VERTEX,
                        0,
                        bytemuck::bytes_of(&(pi as u32)),
                    );
                    device.cmd_bind_vertex_buffers(cmd, 0, &[piece.vertex_buffer.handle], &[0]);
                    device.cmd_bind_index_buffer(
                        cmd,
                        piece.index_buffer.handle,
                        0,
                        vk::IndexType::UINT32,
                    );
                    device.cmd_draw_indexed(cmd, piece.index_count, 1, 0, 0, 0);
                }
            }
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, scene.char_pipeline);
            for (mi, mesh) in scene.meshes.iter().enumerate() {
                if !visible_meshes.get(mi).copied().unwrap_or(false) {
                    continue;
                }
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    scene.char_pipeline_layout,
                    0,
                    &[mesh.set],
                    &[],
                );
                device.cmd_push_constants(
                    cmd,
                    scene.char_pipeline_layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytemuck::bytes_of(&scene.mesh_pushes[mi]),
                );
                device.cmd_bind_index_buffer(
                    cmd,
                    mesh.index_buffer.handle,
                    0,
                    vk::IndexType::UINT32,
                );
                device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
            }
            device.cmd_end_render_pass(cmd);

            // Readback copy (colour is TRANSFER_SRC_OPTIMAL
            // after the render pass).
            device.cmd_copy_image_to_buffer(
                cmd,
                scene.colour.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                scene.readback.handle,
                &[vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })],
            );
        })?;

        let scene = self.scene.as_ref().expect("scene present");
        let byte_len = (width * height * 4) as usize;
        // SAFETY: readback is mapped and the submission has
        // completed (fence waited), so its contents are
        // final.
        let rgba = unsafe {
            let src = scene.readback.mapped.expect("readback mapped");
            std::slice::from_raw_parts(src, byte_len).to_vec()
        };
        Ok(RenderedFrame {
            width,
            height,
            rgba,
        })
    }

    fn skin_positions(
        &mut self,
        mesh_index: usize,
        joint_matrices: &[[f32; 16]],
        weights: &[f32],
    ) -> Result<SkinnedMesh, RenderError> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| be("skin before upload"))?;
        scene.joints.write(joint_matrices);
        if weights.is_empty() {
            scene.weight_buffers[mesh_index].write(&[0.0_f32; 1]);
        } else {
            scene.weight_buffers[mesh_index].write(weights);
        }
        let mesh = &scene.meshes[mesh_index];
        let vertex_count = mesh.vertex_count;
        let device = &self.device;
        // SAFETY: compute dispatch + copies over live
        // buffers; waited before the read.
        self.submit_and_wait(|cmd| unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, scene.compute_pipeline);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                scene.compute_pipeline_layout,
                0,
                &[mesh.compute_set],
                &[],
            );
            device.cmd_push_constants(
                cmd,
                scene.compute_pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                bytemuck::bytes_of(&vertex_count),
            );
            device.cmd_dispatch(cmd, vertex_count.div_ceil(256), 1, 1);
            let barrier = vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[barrier],
                &[],
                &[],
            );
            let len = u64::from(vertex_count) * 16;
            device.cmd_copy_buffer(
                cmd,
                mesh.out_positions.handle,
                mesh.stage_positions.handle,
                &[vk::BufferCopy::default().size(len)],
            );
            device.cmd_copy_buffer(
                cmd,
                mesh.out_normals.handle,
                mesh.stage_normals.handle,
                &[vk::BufferCopy::default().size(len)],
            );
        })?;
        let scene = self.scene.as_ref().expect("scene present");
        let mesh = &scene.meshes[mesh_index];
        let read = |buffer: &Buffer| -> Vec<[f32; 3]> {
            // SAFETY: staging buffers are mapped and the
            // copy has completed.
            unsafe {
                let src = buffer.mapped.expect("staging mapped") as *const [f32; 4];
                let rows = std::slice::from_raw_parts(src, vertex_count as usize);
                rows.iter().map(|r| [r[0], r[1], r[2]]).collect()
            }
        };
        Ok((read(&mesh.stage_positions), read(&mesh.stage_normals)))
    }
}

impl Drop for VulkanBackend {
    fn drop(&mut self) {
        // SAFETY: queue idle first; then every handle this
        // backend created is destroyed exactly once, in
        // dependency order (users of a resource before the
        // resource).
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some(scene) = self.scene.take() {
                let device = &self.device;
                for mesh in scene.meshes {
                    for buffer in mesh.statics {
                        buffer.destroy(device);
                    }
                    mesh.index_buffer.destroy(device);
                    mesh.out_positions.destroy(device);
                    mesh.out_normals.destroy(device);
                    mesh.stage_positions.destroy(device);
                    mesh.stage_normals.destroy(device);
                }
                for piece in scene.pieces {
                    piece.vertex_buffer.destroy(device);
                    piece.index_buffer.destroy(device);
                }
                scene.uniforms.destroy(device);
                scene.joints.destroy(device);
                for buffer in scene.weight_buffers {
                    buffer.destroy(device);
                }
                scene.draw_data.destroy(device);
                scene.readback.destroy(device);
                for pipeline in [
                    scene.char_pipeline,
                    scene.char_depth_pipeline,
                    scene.set_pipeline,
                    scene.set_depth_pipeline,
                    scene.compute_pipeline,
                ] {
                    device.destroy_pipeline(pipeline, None);
                }
                for layout in [
                    scene.char_pipeline_layout,
                    scene.set_pipeline_layout,
                    scene.compute_pipeline_layout,
                ] {
                    device.destroy_pipeline_layout(layout, None);
                }
                device.destroy_framebuffer(scene.framebuffer, None);
                device.destroy_framebuffer(scene.shadow_framebuffer, None);
                device.destroy_render_pass(scene.render_pass, None);
                device.destroy_render_pass(scene.shadow_pass, None);
                device.destroy_descriptor_pool(scene.pool, None);
                for layout in [scene.char_layout, scene.set_layout, scene.compute_layout] {
                    device.destroy_descriptor_set_layout(layout, None);
                }
                device.destroy_sampler(scene.shadow_sampler, None);
                scene.colour.destroy(device);
                scene.depth.destroy(device);
                scene.shadow.destroy(device);
            }
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.cmd_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

//! The real backend: stable-diffusion.cpp (ggml) over its C API.
//!
//! One context is created lazily on the first generation and kept
//! for the engine's life — model load is the expensive part, and
//! the seat discipline this mirrors keeps the model resident. The
//! context is created with the backend string the caller supplies
//! (`vulkan0`, `cpu`, … in ggml's device vocabulary; empty means
//! the library's default ordering).
//!
//! Progress: the C API offers one process-global progress
//! callback. The engine owns the only backend instance and runs
//! one generation at a time, so the callback writes into a static
//! slot the backend reads after each call — documented here
//! because it is the seam's one global: two native engines in one
//! process would interleave their progress reports (their results
//! would still be correct). The estate runs exactly one.

use std::ffi::{CStr, CString, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::Backend;
use crate::params::{DroidError, GenParams, RawImage};
use crate::sys;

static PROGRESS_STEP: AtomicU32 = AtomicU32::new(0);
static PROGRESS_STEPS: AtomicU32 = AtomicU32::new(0);

extern "C" fn progress_cb(step: i32, steps: i32, _time: f32, _data: *mut c_void) {
    if step > 0 {
        PROGRESS_STEP.store(step as u32, Ordering::Relaxed);
    }
    if steps > 0 {
        PROGRESS_STEPS.store(steps as u32, Ordering::Relaxed);
    }
}

/// stable-diffusion.cpp backend over one resident context.
pub struct NativeBackend {
    model_path: PathBuf,
    backend: String,
    ctx: *mut sys::SdCtx,
}

// The context is only ever touched from the engine's worker
// thread, which owns this backend; Send is sound under that
// ownership, and the engine enforces it structurally.
unsafe impl Send for NativeBackend {}

impl NativeBackend {
    /// A backend that will load `model_path` (safetensors or GGUF)
    /// on first use. `backend` is ggml's device assignment string
    /// (e.g. `vulkan0`); empty selects the library default.
    pub fn new(model_path: PathBuf, backend: String) -> NativeBackend {
        NativeBackend {
            model_path,
            backend,
            ctx: std::ptr::null_mut(),
        }
    }

    fn ensure_ctx(&mut self) -> Result<(), DroidError> {
        if !self.ctx.is_null() {
            return Ok(());
        }
        let model = CString::new(self.model_path.to_string_lossy().into_owned())
            .map_err(|_| DroidError::EngineUnavailable("model path contains NUL".to_owned()))?;
        unsafe {
            let opt = sys::sd_ctx_opt_create();
            if opt.is_null() {
                return Err(unavailable("sd_ctx_opt_create returned null"));
            }
            let mut status = set_string(opt, "model_path", &model);
            if status == 0 && !self.backend.is_empty() {
                let backend = CString::new(self.backend.clone())
                    .map_err(|_| DroidError::EngineUnavailable("backend string NUL".to_owned()))?;
                status = set_string(opt, "backend", &backend);
            }
            let mut ctx: *mut sys::SdCtx = std::ptr::null_mut();
            if status == 0 {
                status = sys::sd_ctx_create(opt, &mut ctx);
            }
            sys::sd_opt_free(opt);
            if status != 0 || ctx.is_null() {
                return Err(unavailable(&format!(
                    "context creation failed: {} ({})",
                    status_name(status),
                    last_error()
                )));
            }
            sys::sd_set_progress_callback(progress_cb, std::ptr::null_mut());
            self.ctx = ctx;
        }
        Ok(())
    }
}

impl Drop for NativeBackend {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { sys::sd_ctx_free(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
    }
}

impl Backend for NativeBackend {
    fn generate(
        &mut self,
        params: &GenParams,
        progress: &mut dyn FnMut(u32, u32),
    ) -> Result<RawImage, DroidError> {
        self.ensure_ctx()?;
        let prompt = cstring(&params.prompt)?;
        let negative = cstring(&params.negative_prompt)?;
        let sampler = cstring(&params.sampler)?;
        PROGRESS_STEP.store(0, Ordering::Relaxed);
        PROGRESS_STEPS.store(params.steps, Ordering::Relaxed);
        unsafe {
            let opt = sys::sd_img_gen_opt_create();
            if opt.is_null() {
                return Err(DroidError::Backend(
                    "sd_img_gen_opt_create returned null".into(),
                ));
            }
            let mut status = set_string(opt, "prompt", &prompt);
            if status == 0 {
                status = set_string(opt, "negative_prompt", &negative);
            }
            if status == 0 {
                status = sys::sd_opt_set_int(opt, key("width"), params.width as i64);
            }
            if status == 0 {
                status = sys::sd_opt_set_int(opt, key("height"), params.height as i64);
            }
            if status == 0 {
                status = sys::sd_opt_set_int(opt, key("sample.steps"), params.steps as i64);
            }
            if status == 0 {
                status =
                    sys::sd_opt_set_float(opt, key("sample.guidance.txt_cfg"), params.cfg as f64);
            }
            if status == 0 {
                status = sys::sd_opt_set_int(opt, key("seed"), params.seed);
            }
            if status == 0 {
                status = set_string(opt, "sample.method", &sampler);
            }
            if status == 0 {
                if let Some(scheduler) = &params.scheduler {
                    let scheduler = cstring(scheduler)?;
                    status = set_string(opt, "sample.scheduler", &scheduler);
                }
            }
            let mut result: *mut sys::SdResult = std::ptr::null_mut();
            if status == 0 {
                status = sys::sd_generate_image(self.ctx, opt, &mut result);
            }
            sys::sd_opt_free(opt);
            // The generation is synchronous; report the final
            // observed progress before surfacing the outcome.
            progress(
                PROGRESS_STEP.load(Ordering::Relaxed),
                PROGRESS_STEPS.load(Ordering::Relaxed),
            );
            if status != 0 || result.is_null() {
                return Err(DroidError::Backend(format!(
                    "generation failed: {} ({})",
                    status_name(status),
                    last_error()
                )));
            }
            let image = copy_first_image(result);
            sys::sd_result_free(result);
            image
        }
    }
}

/// Copy image 0 out of a result as HWC RGB bytes. The C API returns
/// the decoded image as a rank-3 uint8 tensor, CHW in this build.
unsafe fn copy_first_image(result: *mut sys::SdResult) -> Result<RawImage, DroidError> {
    unsafe {
        if sys::sd_result_get_image_count(result) == 0 {
            return Err(DroidError::Backend("result holds no image".into()));
        }
        let tensor = sys::sd_result_get_image(result, 0);
        if tensor.is_null() || sys::sd_tensor_rank(tensor) != 3 {
            return Err(DroidError::Backend("image tensor is not rank 3".into()));
        }
        let d0 = sys::sd_tensor_dim(tensor, 0);
        let d1 = sys::sd_tensor_dim(tensor, 1);
        let d2 = sys::sd_tensor_dim(tensor, 2);
        // The result tensor's data is HWC row-major bytes. Its
        // reported dims at the pinned commit are (3, width,
        // height) - ggml axis order, not CHW memory layout
        // (verified empirically: reading the buffer as HWC with
        // width=d1, height=d2 reproduces the still; a CHW
        // deinterleave produces striped noise). Accept the
        // (height, width, 3) spelling too.
        let (width, height) = if d0 == 3 {
            (d1, d2)
        } else if d2 == 3 {
            (d1, d0)
        } else {
            return Err(DroidError::Backend(format!(
                "unexpected image shape {d0}x{d1}x{d2}"
            )));
        };
        if width <= 0 || height <= 0 {
            return Err(DroidError::Backend(format!(
                "unexpected image shape {d0}x{d1}x{d2}"
            )));
        }
        let bytes = sys::sd_tensor_byte_size(tensor);
        let expected = (width * height * 3) as usize;
        if bytes < expected {
            return Err(DroidError::Backend(format!(
                "image tensor holds {bytes} bytes, expected {expected}"
            )));
        }
        let data = sys::sd_tensor_data(tensor) as *const u8;
        if data.is_null() {
            return Err(DroidError::Backend("image tensor has no data".into()));
        }
        let rgb = std::slice::from_raw_parts(data, expected).to_vec();
        Ok(RawImage {
            width: width as u32,
            height: height as u32,
            rgb,
        })
    }
}

fn key(name: &str) -> *const std::ffi::c_char {
    // Keys are compile-time-known ASCII literals without NULs; each
    // call site passes a distinct literal.
    match name {
        "width" => c"width".as_ptr(),
        "height" => c"height".as_ptr(),
        "sample.steps" => c"sample.steps".as_ptr(),
        "sample.guidance.txt_cfg" => c"sample.guidance.txt_cfg".as_ptr(),
        "seed" => c"seed".as_ptr(),
        _ => unreachable!("unknown option key {name}"),
    }
}

fn set_string(opt: *mut sys::SdOpt, name: &str, value: &CString) -> sys::SdStatus {
    let key = match name {
        "model_path" => c"model_path".as_ptr(),
        "backend" => c"backend".as_ptr(),
        "prompt" => c"prompt".as_ptr(),
        "negative_prompt" => c"negative_prompt".as_ptr(),
        "sample.method" => c"sample.method".as_ptr(),
        "sample.scheduler" => c"sample.scheduler".as_ptr(),
        _ => unreachable!("unknown string option {name}"),
    };
    unsafe { sys::sd_opt_set_string(opt, key, value.as_ptr()) }
}

fn cstring(s: &str) -> Result<CString, DroidError> {
    CString::new(s).map_err(|_| DroidError::InvalidParams("prompt contains NUL".to_owned()))
}

fn unavailable(why: &str) -> DroidError {
    DroidError::EngineUnavailable(why.to_owned())
}

fn last_error() -> String {
    unsafe {
        let ptr = sys::sd_get_last_error();
        if ptr.is_null() {
            return "no error text".to_owned();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// Human name for an `sd_status_t` value (stable-diffusion.h).
fn status_name(status: sys::SdStatus) -> &'static str {
    match status {
        0 => "SD_STATUS_OK",
        1 => "SD_STATUS_INVALID_ARGUMENT",
        2 => "SD_STATUS_UNKNOWN_KEY",
        3 => "SD_STATUS_TYPE_MISMATCH",
        4 => "SD_STATUS_OUT_OF_RANGE",
        5 => "SD_STATUS_UNSUPPORTED",
        6 => "SD_STATUS_OUT_OF_MEMORY",
        7 => "SD_STATUS_LOAD_FAILED",
        8 => "SD_STATUS_GENERATION_FAILED",
        9 => "SD_STATUS_BUSY",
        10 => "SD_STATUS_CANCELLED",
        _ => "SD_STATUS_?",
    }
}

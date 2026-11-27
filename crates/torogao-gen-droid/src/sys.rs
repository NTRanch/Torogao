//! Hand-written FFI bindings for the stable-diffusion.cpp C API
//! (`stable-diffusion.h` at the pinned commit in PROVENANCE.md).
//!
//! Only the surface the seam uses is bound. All types from the C
//! API are opaque; ownership rules follow the header: option
//! objects are freed with `sd_opt_free`, contexts with
//! `sd_ctx_free`, results with `sd_result_free`, and result images
//! are borrowed from their result (copied out before the result is
//! freed). This module is the crate's only unsafe surface.

use std::ffi::{c_char, c_void};

/// Opaque `sd_ctx_t`.
#[repr(C)]
pub struct SdCtx {
    _private: [u8; 0],
}

/// Opaque `sd_opt_t` base (context and generation options share the
/// setter functions through it).
#[repr(C)]
pub struct SdOpt {
    _private: [u8; 0],
}

/// Opaque `sd_result_t`.
#[repr(C)]
pub struct SdResult {
    _private: [u8; 0],
}

/// Opaque `sd_tensor_t`.
#[repr(C)]
pub struct SdTensor {
    _private: [u8; 0],
}

/// `sd_status_t` — `SD_STATUS_OK` is 0; the rest are named in
/// `native::status_name`.
pub type SdStatus = i32;

/// Progress callback shape: (step, steps, seconds, data).
pub type SdProgressCb = extern "C" fn(i32, i32, f32, *mut c_void);

unsafe extern "C" {
    /// `sd_ctx_opt_t* sd_ctx_opt_create(void)`
    pub fn sd_ctx_opt_create() -> *mut SdOpt;
    /// `sd_img_gen_opt_t* sd_img_gen_opt_create(void)`
    pub fn sd_img_gen_opt_create() -> *mut SdOpt;
    /// `void sd_opt_free(sd_opt_t*)`
    pub fn sd_opt_free(params: *mut SdOpt);
    /// `sd_status_t sd_opt_set_string(sd_opt_t*, const char*, const char*)`
    pub fn sd_opt_set_string(
        params: *mut SdOpt,
        key: *const c_char,
        value: *const c_char,
    ) -> SdStatus;
    /// `sd_status_t sd_opt_set_int(sd_opt_t*, const char*, int64_t)`
    pub fn sd_opt_set_int(params: *mut SdOpt, key: *const c_char, value: i64) -> SdStatus;
    /// `sd_status_t sd_opt_set_float(sd_opt_t*, const char*, double)`
    pub fn sd_opt_set_float(params: *mut SdOpt, key: *const c_char, value: f64) -> SdStatus;
    /// `sd_status_t sd_opt_set_bool(sd_opt_t*, const char*, bool)`
    pub fn sd_opt_set_bool(params: *mut SdOpt, key: *const c_char, value: bool) -> SdStatus;
    /// `sd_status_t sd_ctx_create(const sd_ctx_opt_t*, sd_ctx_t**)`
    pub fn sd_ctx_create(params: *const SdOpt, ctx_out: *mut *mut SdCtx) -> SdStatus;
    /// `void sd_ctx_free(sd_ctx_t*)`
    pub fn sd_ctx_free(ctx: *mut SdCtx);
    /// `sd_status_t sd_generate_image(sd_ctx_t*, const sd_img_gen_opt_t*, sd_result_t**)`
    pub fn sd_generate_image(
        ctx: *mut SdCtx,
        params: *const SdOpt,
        result_out: *mut *mut SdResult,
    ) -> SdStatus;
    /// `size_t sd_result_get_image_count(const sd_result_t*)`
    pub fn sd_result_get_image_count(result: *const SdResult) -> usize;
    /// `const sd_tensor_t* sd_result_get_image(const sd_result_t*, size_t)`
    pub fn sd_result_get_image(result: *const SdResult, index: usize) -> *const SdTensor;
    /// `void sd_result_free(sd_result_t*)`
    pub fn sd_result_free(result: *mut SdResult);
    /// `size_t sd_tensor_rank(const sd_tensor_t*)`
    pub fn sd_tensor_rank(tensor: *const SdTensor) -> usize;
    /// `int64_t sd_tensor_dim(const sd_tensor_t*, size_t)`
    pub fn sd_tensor_dim(tensor: *const SdTensor, axis: usize) -> i64;
    /// `size_t sd_tensor_byte_size(const sd_tensor_t*)`
    pub fn sd_tensor_byte_size(tensor: *const SdTensor) -> usize;
    /// `const void* sd_tensor_data(const sd_tensor_t*)`
    pub fn sd_tensor_data(tensor: *const SdTensor) -> *const c_void;
    /// `const char* sd_get_last_error(void)`
    pub fn sd_get_last_error() -> *const c_char;
    /// `void sd_set_progress_callback(sd_progress_cb_t, void*)`
    pub fn sd_set_progress_callback(cb: SdProgressCb, data: *mut c_void);
}

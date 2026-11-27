//! A deterministic stand-in backend for tests and shell bring-up:
//! it records what it was asked for and returns a small gradient
//! image, reporting progress step by step. It generates nothing
//! real — its job is to let the contract be exercised without the
//! native library or a checkpoint.

use std::sync::{Arc, Mutex};

use crate::engine::Backend;
use crate::params::{DroidError, GenParams, RawImage};

/// Recording stub backend.
#[derive(Debug, Default)]
pub struct StubBackend {
    /// Every parameter set submitted to this backend, in order.
    pub seen: Arc<Mutex<Vec<GenParams>>>,
    /// When set, `generate` fails with this message.
    pub fail_with: Option<String>,
}

impl Backend for StubBackend {
    fn generate(
        &mut self,
        params: &GenParams,
        progress: &mut dyn FnMut(u32, u32),
    ) -> Result<RawImage, DroidError> {
        self.seen
            .lock()
            .expect("seen poisoned")
            .push(params.clone());
        if let Some(why) = &self.fail_with {
            return Err(DroidError::Backend(why.clone()));
        }
        for step in 1..=params.steps {
            progress(step, params.steps);
        }
        let (width, height) = (64, 64);
        let mut rgb = vec![0u8; (width * height * 3) as usize];
        for y in 0..height {
            for x in 0..width {
                let i = ((y * width + x) * 3) as usize;
                rgb[i] = (x * 4) as u8;
                rgb[i + 1] = (y * 4) as u8;
                rgb[i + 2] = 128;
            }
        }
        Ok(RawImage { width, height, rgb })
    }
}

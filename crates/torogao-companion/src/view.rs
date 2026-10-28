//! View fitting: the renderer's canvas view maps the
//! model's bounds onto the full NDC square, so drawing it
//! into a surface of a different aspect would stretch her.
//! The companion's surface is sized from the canvas aspect
//! (so the fit is normally exact), but a compositor may
//! grant a different size; this fit letterboxes the view —
//! centred, aspect preserved, transparent margins — in
//! that case. Pure matrix arithmetic, unit-gated.

use torogao_app::render::Canvas;

/// Returns `canvas`'s view matrix adjusted to draw into a
/// `surface_w`×`surface_h` target without stretching: the
/// axis along which the surface is relatively larger is
/// scaled down so the model's bounds fit inside, centred.
pub fn fit_view(canvas: &Canvas, surface_w: u32, surface_h: u32) -> [f32; 16] {
    let mut view = canvas.view;
    if surface_w == 0 || surface_h == 0 || canvas.width == 0 || canvas.height == 0 {
        return view;
    }
    let canvas_aspect = f64::from(canvas.width) / f64::from(canvas.height);
    let surface_aspect = f64::from(surface_w) / f64::from(surface_h);
    if surface_aspect > canvas_aspect {
        // Surface is wider: pillarbox (shrink x).
        view[0] *= (canvas_aspect / surface_aspect) as f32;
    } else if surface_aspect < canvas_aspect {
        // Surface is taller: letterbox (shrink y).
        view[5] *= (surface_aspect / canvas_aspect) as f32;
    }
    view
}

/// Configures a wgpu surface for transparent presentation
/// (premultiplied alpha where the platform offers it) and
/// returns the format in use. Both presentations configure
/// identically, so the policy lives here once.
pub fn configure_surface(
    renderer: &torogao_app::render::Renderer,
    surface: &wgpu::Surface<'_>,
    size: (u32, u32),
) -> wgpu::TextureFormat {
    let format = renderer.surface_format(surface);
    let capabilities = surface.get_capabilities(renderer.adapter());
    let alpha_mode = capabilities
        .alpha_modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::CompositeAlphaMode::PreMultiplied)
        .or_else(|| {
            capabilities
                .alpha_modes
                .iter()
                .copied()
                .find(|mode| *mode == wgpu::CompositeAlphaMode::PostMultiplied)
        })
        .unwrap_or(wgpu::CompositeAlphaMode::Auto);
    surface.configure(
        renderer.device(),
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0.max(1),
            height: size.1.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        },
    );
    format
}

/// The surface size for a display height and a canvas
/// aspect: width follows the model's own proportions,
/// clamped to a sane band. Returns `(width, height)`.
pub fn surface_size_for_height(display_height: u32, canvas: &Canvas) -> (u32, u32) {
    if canvas.width == 0 || canvas.height == 0 {
        return (display_height, display_height);
    }
    let aspect = f64::from(canvas.width) / f64::from(canvas.height);
    let width = (f64::from(display_height) * aspect).round() as u32;
    (width.clamp(64, 4096), display_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(width: u32, height: u32) -> Canvas {
        let mut view = [0.0_f32; 16];
        view[0] = 2.0;
        view[5] = -2.0;
        view[10] = 1.0;
        view[15] = 1.0;
        Canvas {
            width,
            height,
            view,
        }
    }

    #[test]
    fn matching_aspect_leaves_the_view_untouched() {
        let c = canvas(512, 1024);
        assert_eq!(fit_view(&c, 256, 512), c.view);
    }

    #[test]
    fn wider_surface_pillarboxes_x() {
        let c = canvas(512, 1024); // aspect 0.5
        let view = fit_view(&c, 1024, 1024); // aspect 1.0
        assert!((view[0] - 1.0).abs() < 1e-6);
        assert_eq!(view[5], -2.0);
    }

    #[test]
    fn taller_surface_letterboxes_y() {
        let c = canvas(1024, 512); // aspect 2.0
        let view = fit_view(&c, 512, 512); // aspect 1.0
        assert_eq!(view[0], 2.0);
        assert!((view[5] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn surface_size_follows_canvas_aspect() {
        let c = canvas(512, 1024);
        assert_eq!(surface_size_for_height(480, &c), (240, 480));
    }
}

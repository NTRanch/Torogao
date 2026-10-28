//! The 8K texture check (RP4 inventory, app limits):
//! a model whose texture slot carries an 8192x8192 PNG
//! loads and renders. GPU-bound and heavy (a 256 MiB
//! texture), so it is `#[ignore]`d in the normal suite
//! and run explicitly by the Phase E primo gate with
//! TOROGAO_8K_MODEL pointing at a real model file.

use torogao_app::render::Renderer;
use torogao_app::scene::Scene;
use torogao_format::container::{CONTAINER_MAGIC, TEXTURE_MAGIC, read_inp};

/// Encodes an 8192x8192 RGBA gradient PNG.
fn gradient_png_8k() -> Vec<u8> {
    let size: u32 = 8192;
    let mut pixels = vec![0u8; size as usize * size as usize * 4];
    for y in 0..size as usize {
        for x in 0..size as usize {
            let at = (y * size as usize + x) * 4;
            pixels[at] = (x % 251) as u8;
            pixels[at + 1] = (y % 241) as u8;
            pixels[at + 2] = 128;
            pixels[at + 3] = 255;
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(&pixels).expect("png data");
    drop(writer);
    out
}

/// Rebuilds a container with texture slot 0 replaced.
fn replace_first_texture(original: &[u8], replacement: &[u8]) -> Vec<u8> {
    let parsed = read_inp(original).expect("source model parses");
    assert!(!parsed.textures.is_empty(), "model has textures");
    let mut out = Vec::new();
    out.extend_from_slice(CONTAINER_MAGIC);
    out.extend_from_slice(&(parsed.payload_bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(&parsed.payload_bytes);
    out.extend_from_slice(TEXTURE_MAGIC);
    out.extend_from_slice(&(parsed.textures.len() as u32).to_be_bytes());
    for (index, slot) in parsed.textures.iter().enumerate() {
        let data = if index == 0 { replacement } else { &slot.data };
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.push(slot.tag);
        out.extend_from_slice(data);
    }
    out
}

#[test]
#[ignore = "GPU gate test: run explicitly with TOROGAO_8K_MODEL set"]
fn model_with_an_8k_texture_loads_and_renders() {
    let path = std::env::var("TOROGAO_8K_MODEL")
        .expect("TOROGAO_8K_MODEL points at the base model for the 8K check");
    let original = std::fs::read(&path).expect("base model reads");
    let bytes = replace_first_texture(&original, &gradient_png_8k());
    let mut scene = Scene::load_bytes(&bytes, "8k-texture-check").expect("8K model loads");
    scene.evaluate_fresh(&[]).expect("8K model evaluates");
    let mut renderer = Renderer::new().expect("renderer creates");
    assert!(
        renderer.max_texture_dimension_2d() >= 8192,
        "adapter max texture dimension is {}",
        renderer.max_texture_dimension_2d()
    );
    let (image, _report) = renderer.render(&mut scene).expect("8K model renders");
    assert!(image.width > 0 && image.height > 0);
}

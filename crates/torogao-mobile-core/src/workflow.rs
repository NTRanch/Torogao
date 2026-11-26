//! The pure ComfyUI workflow builder.
//!
//! Turns (cast member, user prompt, seed) into the workflow document
//! the seat executes: checkpoint → positive/negative conditioning →
//! empty latent → sampler → VAE decode → save. Pure and total — the
//! same inputs always produce the same document, which is what makes
//! the recipe testable without a seat.

use serde_json::{json, Value};

use crate::cast::Character;
use crate::style;

/// Longest user prompt accepted, in bytes. Prompts are a garnish on
/// the canonical record, not a second style system.
pub const USER_PROMPT_BYTES_MAX: usize = 2000;

/// Compose the positive prompt: the shared style prefix, the cast
/// member's identity and signature, then the user's scene prompt.
/// An empty user prompt is legitimate (a canonical portrait).
pub fn compose_positive(character: &Character, user_prompt: &str) -> String {
    let mut out = String::with_capacity(
        style::STYLE_PREFIX.len()
            + character.identity.len()
            + character.signature.len()
            + user_prompt.len()
            + 4,
    );
    out.push_str(style::STYLE_PREFIX);
    out.push_str(character.identity);
    out.push_str(", ");
    out.push_str(character.signature);
    let scene = user_prompt.trim();
    if !scene.is_empty() {
        out.push_str(", ");
        out.push_str(scene);
    }
    out
}

/// Compose the negative prompt: the shared negative plus the cast
/// member's own guards (e.g. Sirena's freckles guard).
pub fn compose_negative(character: &Character) -> String {
    if character.negative_guards.is_empty() {
        style::DEFAULT_NEGATIVE.to_string()
    } else {
        format!("{}, {}", style::DEFAULT_NEGATIVE, character.negative_guards)
    }
}

/// Build the seat workflow document for one still.
///
/// Node ids are the document's own wiring and are stable: "1" is the
/// checkpoint loader every other node references.
pub fn build_workflow(character: &Character, user_prompt: &str, seed: u64) -> Value {
    json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": { "ckpt_name": style::CHECKPOINT }
        },
        "2": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": ["1", 1],
                "text": compose_positive(character, user_prompt)
            }
        },
        "3": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": ["1", 1],
                "text": compose_negative(character)
            }
        },
        "4": {
            "class_type": "EmptyLatentImage",
            "inputs": {
                "width": style::WIDTH,
                "height": style::HEIGHT,
                "batch_size": 1
            }
        },
        "5": {
            "class_type": "KSampler",
            "inputs": {
                "model": ["1", 0],
                "seed": seed,
                "steps": style::STEPS,
                "cfg": style::CFG,
                "sampler_name": style::SAMPLER,
                "scheduler": style::SCHEDULER,
                "positive": ["2", 0],
                "negative": ["3", 0],
                "latent_image": ["4", 0],
                "denoise": 1.0
            }
        },
        "6": {
            "class_type": "VAEDecode",
            "inputs": { "samples": ["5", 0], "vae": ["1", 2] }
        },
        "7": {
            "class_type": "SaveImage",
            "inputs": {
                "images": ["6", 0],
                "filename_prefix": style::FILENAME_PREFIX
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::{LANA, PRIMA, SIRENA};

    #[test]
    fn positive_opens_with_the_style_prefix_and_carries_identity() {
        let p = compose_positive(&PRIMA, "in a library, window light");
        assert!(p.starts_with(style::STYLE_PREFIX));
        assert!(p.contains("silver-white hair"));
        assert!(p.contains("in a library, window light"));
    }

    #[test]
    fn empty_scene_prompt_still_composes_a_canonical_portrait() {
        let p = compose_positive(&SIRENA, "   ");
        assert!(p.contains("plum-violet eyes"));
        assert!(!p.ends_with(", "));
    }

    #[test]
    fn negative_carries_character_guards() {
        assert!(compose_negative(&SIRENA).contains("freckles"));
        assert!(!compose_negative(&LANA).contains("freckles"));
        assert!(compose_negative(&PRIMA).starts_with("lowres"));
    }

    #[test]
    fn workflow_uses_the_checkpoint_and_recipe_of_record() {
        let w = build_workflow(&LANA, "laughing, cafe", 42);
        assert_eq!(w["1"]["inputs"]["ckpt_name"], style::CHECKPOINT);
        assert_eq!(w["5"]["inputs"]["steps"], style::STEPS);
        assert_eq!(w["5"]["inputs"]["sampler_name"], style::SAMPLER);
        assert_eq!(w["5"]["inputs"]["scheduler"], style::SCHEDULER);
        assert_eq!(w["5"]["inputs"]["seed"], 42);
        assert_eq!(w["4"]["inputs"]["width"], style::WIDTH);
        assert_eq!(w["4"]["inputs"]["height"], style::HEIGHT);
        let pos = w["2"]["inputs"]["text"].as_str().unwrap_or("");
        assert!(pos.contains("freckles"), "Lana's lock must reach the seat: {pos}");
    }

    #[test]
    fn workflow_is_deterministic() {
        assert_eq!(
            build_workflow(&PRIMA, "rain", 7),
            build_workflow(&PRIMA, "rain", 7)
        );
    }
}

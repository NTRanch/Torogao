//! The content screen.
//!
//! The estate's absolute screen, enforced deterministically over
//! prompt text: anything that steers generation toward sexual
//! content involving minors or child-depicted characters is
//! refused outright. The screen runs in the core before any seat
//! contact, and again seat-side in the facade over the composed
//! prompt — the same code in both places, so a tampered client
//! cannot talk the seat into what the app refused.
//!
//! Design: word-boundary token matching (never substrings, so
//! "babysitter" is not "baby"), two tiers.
//! - Hard terms refuse on sight in this app: there is no
//!   legitimate use for them in a prompt to a generator whose
//!   cast is adults of record. The known cost is false positives
//!   on adult cosplay phrasing ("schoolgirl outfit"); the estate
//!   posture is fail-closed, and the cost is accepted and named.
//! - Soft youth terms refuse when a sexual-context term is also
//!   present.
//! - Explicit ages under 18 refuse in any phrasing the scanner
//!   recognises ("15yo", "15 y/o", "15 years old", "age 15").
//!
//! Legal-adult work passes: the canonical cast records and
//! ordinary adult scene prompts contain none of the lexicon.

/// The screen's verdict on one piece of prompt text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenVerdict {
    /// The text may proceed to the seat.
    Pass,
    /// The text is refused; the string names the rule that fired,
    /// for the user-facing message and the facade's log.
    Refuse(String),
}

/// Terms that refuse on sight (hard tier).
const HARD_TERMS: &[&str] = &[
    "child",
    "children",
    "toddler",
    "preteen",
    "pre-teen",
    "underage",
    "minor",
    "minors",
    "loli",
    "lolicon",
    "shota",
    "shotacon",
    "schoolgirl",
    "schoolboy",
    "kindergarten",
    "daycare",
    "teen",
    "teens",
    "teenage",
    "teenager",
    "teenagers",
];

/// Youth terms that refuse with a sexual-context term (soft tier).
const SOFT_TERMS: &[&str] = &[
    "kid",
    "kids",
    "young",
    "youth",
    "juvenile",
    "adolescent",
    "adolescence",
    "puberty",
    "pubescent",
];

/// Sexual-context markers for the soft tier.
const SEXUAL_TERMS: &[&str] = &[
    "sex",
    "sexual",
    "sexually",
    "nude",
    "naked",
    "erotic",
    "porn",
    "pornographic",
    "lewd",
    "orgasm",
    "aroused",
    "arousal",
    "fetish",
    "hentai",
    "ecchi",
    "uncensored",
    "masturbating",
    "masturbation",
    "genitals",
    "genital",
];

/// Split text into lowercase word tokens on non-alphanumeric
/// boundaries. Hyphenated words yield their parts as well as the
/// joined form, so "pre-teen" matches its hard-tier entry.
fn tokens(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for raw in lower.split(|c: char| !(c.is_alphanumeric() || c == '-')) {
        if raw.is_empty() {
            continue;
        }
        out.push(raw.to_string());
        if raw.contains('-') {
            for part in raw.split('-') {
                if !part.is_empty() {
                    out.push(part.to_string());
                }
            }
        }
    }
    out
}

/// Scan for an explicit age under 18: a number 0..=17 directly
/// followed (after optional spaces) by "yo", "y/o", "years old",
/// "year old", or preceded by "age". Returns the age found.
fn underage_number(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let n: u32 = lower[start..i].parse().ok()?;
            if n <= 17 {
                let rest = lower[i..].trim_start();
                if rest.starts_with("yo")
                    || rest.starts_with("y/o")
                    || rest.starts_with("years old")
                    || rest.starts_with("year old")
                {
                    return Some(n);
                }
                let before = lower[..start].trim_end();
                if before.ends_with("age") {
                    return Some(n);
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Screen one piece of prompt text (a user scene prompt, or the
/// composed positive prompt seat-side).
pub fn screen_text(text: &str) -> ScreenVerdict {
    if let Some(age) = underage_number(text) {
        return ScreenVerdict::Refuse(format!(
            "the screen refuses prompts naming an age under 18 (found {age})"
        ));
    }
    let toks = tokens(text);
    for term in HARD_TERMS {
        if toks.iter().any(|t| t == term) {
            return ScreenVerdict::Refuse(format!(
                "the screen refuses prompts containing \"{term}\""
            ));
        }
    }
    let has_sexual = SEXUAL_TERMS.iter().any(|s| toks.iter().any(|t| t == s));
    if has_sexual {
        for term in SOFT_TERMS {
            if toks.iter().any(|t| t == term) {
                return ScreenVerdict::Refuse(format!(
                    "the screen refuses sexual prompts containing \"{term}\""
                ));
            }
        }
    }
    ScreenVerdict::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- validation: legal-adult work passes ----

    #[test]
    fn canonical_cast_prompts_pass() {
        for c in crate::cast::CAST {
            let composed = crate::workflow::compose_positive(c, "city night, rain");
            assert_eq!(
                screen_text(&composed),
                ScreenVerdict::Pass,
                "{}'s canonical prompt must pass",
                c.id
            );
        }
    }

    #[test]
    fn ordinary_adult_scenes_pass() {
        for p in [
            "adult woman in an evening dress, wine bar, candlelight",
            "mature office lady, glasses, confident smile",
            "nude beach at night, adult swimmer, moonlight",
            "college reunion party, old friends laughing",
            "a babysitter interviewed for a newspaper portrait, smiling",
            "mother and adult daughter at a cafe, both laughing",
        ] {
            assert_eq!(screen_text(p), ScreenVerdict::Pass, "must pass: {p}");
        }
    }

    #[test]
    fn empty_and_plain_prompts_pass() {
        assert_eq!(screen_text(""), ScreenVerdict::Pass);
        assert_eq!(
            screen_text("rain, neon signs, city night"),
            ScreenVerdict::Pass
        );
    }

    // ---- adversarial: the screen refuses what it must ----

    #[test]
    fn hard_terms_refuse() {
        for p in [
            "a child at the beach",
            "schoolgirl outfit, studio portrait",
            "loli style character",
            "teen model photoshoot",
            "underage drinking scene",
            "two children playing",
        ] {
            assert!(
                matches!(screen_text(p), ScreenVerdict::Refuse(_)),
                "must refuse: {p}"
            );
        }
    }

    #[test]
    fn explicit_underage_numbers_refuse() {
        for p in [
            "she is 15yo, portrait",
            "a 16 years old model",
            "age 12, swimming",
            "17 y/o at the pool",
        ] {
            assert!(
                matches!(screen_text(p), ScreenVerdict::Refuse(_)),
                "must refuse: {p}"
            );
        }
    }

    #[test]
    fn adult_ages_pass() {
        assert_eq!(screen_text("a 21yo bartender"), ScreenVerdict::Pass);
        assert_eq!(screen_text("woman, age 30, portrait"), ScreenVerdict::Pass);
        assert_eq!(
            screen_text("18 years old, adult model"),
            ScreenVerdict::Pass
        );
    }

    #[test]
    fn soft_terms_refuse_only_with_sexual_context() {
        assert!(matches!(
            screen_text("young and erotic dancer"),
            ScreenVerdict::Refuse(_)
        ));
        assert!(matches!(
            screen_text("kid in a lewd costume"),
            ScreenVerdict::Refuse(_)
        ));
        // The same youth words in a plainly non-sexual scene pass.
        assert_eq!(
            screen_text("a young oak tree in spring"),
            ScreenVerdict::Pass
        );
        assert_eq!(
            screen_text("kids menu design, restaurant illustration"),
            ScreenVerdict::Pass
        );
    }

    #[test]
    fn substring_traps_do_not_fire() {
        // Word-boundary matching: these contain hard-term
        // substrings but not the terms themselves.
        assert_eq!(screen_text("babysitter portrait"), ScreenVerdict::Pass);
        assert_eq!(
            screen_text("childish wonder, adult artist"),
            ScreenVerdict::Pass
        );
    }
}

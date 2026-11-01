//! The deterministic parser: recognized text in, a typed
//! outcome out. The parser never guesses: exactly one valid
//! match executes, anything else refuses with a `TG-VOICE-*`
//! code and the evidence a human needs to re-speak or pick.
//!
//! Matching is phrase-anchored scanning: a template's
//! leading words must prefix-match, then each argument slot
//! is introduced by its label phrase (`project`, `out`, …)
//! and slots may be spoken in any order — the labels, not
//! positions, carry the meaning. Scanning is a single
//! left-to-right pass with a fixed rule order (longest label
//! phrase first, bare slot last), so the same (transcript,
//! confidence, grammar) triple always yields the same
//! outcome, byte for byte when serialized.

use crate::grammar::{ConfirmClass, GrammarDoc, GrammarEntry, SlotKind, Target};
use serde::Serialize;
use serde_json::{Map, Value};

/// Minimum engine confidence for a parse to be trusted.
/// Below it, a unique parse refuses `TG-VOICE-LOW-CONFIDENCE`
/// instead of executing.
pub const CONFIDENCE_MIN: f32 = 0.60;

/// The reserved stop lexemes. A stop lexeme as the first
/// token of an utterance short-circuits the whole parse
/// (design §4.1): voice stop reaches the same synchronous
/// total-stop path as every other surface.
pub const STOP_LEXEMES: &[&str] = &["stop", "halt", "cancel", "safeword"];

/// Refusal code: no template matched the utterance.
pub const TG_VOICE_NO_MATCH: &str = "TG-VOICE-NO-MATCH";
/// Refusal code: more than one distinct parse matched.
pub const TG_VOICE_AMBIGUOUS: &str = "TG-VOICE-AMBIGUOUS";
/// Refusal code: the engine's confidence was below
/// [`CONFIDENCE_MIN`] for the unique parse.
pub const TG_VOICE_LOW_CONFIDENCE: &str = "TG-VOICE-LOW-CONFIDENCE";
/// Refusal code: a required argument slot was not filled.
pub const TG_VOICE_MISSING_ARG: &str = "TG-VOICE-MISSING-ARG";
/// Refusal code: a slot's spoken value has the wrong shape.
pub const TG_VOICE_ARG_TYPE: &str = "TG-VOICE-ARG-TYPE";

/// One recognized utterance, as the STT layer reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transcript {
    /// The recognized text.
    pub text: String,
    /// Mean token confidence in `0.0..=1.0`, when the engine
    /// reports one. `None` marks the parse *unverified* (the
    /// dispatch layer then treats confirm classes strictly).
    pub confidence: Option<f32>,
}

/// A fully parsed, type-checked command, ready for dispatch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedCommand {
    /// The grammar entry that matched.
    pub entry_id: String,
    /// What the command drives.
    pub target: Target,
    /// Posture label from the source registry.
    pub posture: String,
    /// Confirmation class from the grammar entry.
    pub confirm: ConfirmClass,
    /// Assembled arguments (the tool's JSON argument object,
    /// or the verb's spoken-argument record).
    pub args: Value,
    /// True when the engine reported no confidence.
    pub unverified: bool,
}

/// A typed parse refusal. Nothing executes on a refusal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceRefusal {
    /// Stable `TG-VOICE-*` machine code.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
    /// Candidate parses (entry ids), sorted; populated for
    /// ambiguity and low-confidence refusals so the user can
    /// see what was almost heard.
    pub candidates: Vec<String>,
    /// The normalized transcript that was refused.
    pub transcript: String,
}

/// The outcome of parsing one utterance.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum ParseOutcome {
    /// The stop class fired. Top priority, any confidence.
    Stop,
    /// Exactly one valid parse.
    Command(ParsedCommand),
    /// A typed refusal; nothing executes.
    Refusal(VoiceRefusal),
}

/// Number words the normalizer maps to digits.
const NUMBER_WORDS: &[(&str, &str)] = &[
    ("zero", "0"),
    ("one", "1"),
    ("two", "2"),
    ("three", "3"),
    ("four", "4"),
    ("five", "5"),
    ("six", "6"),
    ("seven", "7"),
    ("eight", "8"),
    ("nine", "9"),
    ("ten", "10"),
    ("eleven", "11"),
    ("twelve", "12"),
    ("thirteen", "13"),
    ("fourteen", "14"),
    ("fifteen", "15"),
    ("sixteen", "16"),
    ("seventeen", "17"),
    ("eighteen", "18"),
    ("nineteen", "19"),
    ("twenty", "20"),
    ("thirty", "30"),
    ("forty", "40"),
    ("fifty", "50"),
    ("sixty", "60"),
    ("seventy", "70"),
    ("eighty", "80"),
    ("ninety", "90"),
];

/// Normalizes recognized text into tokens: lowercase, strip
/// sentence punctuation, collapse whitespace, map number
/// words to digits. A period inside a token (a file name)
/// survives; sentence-final periods do not.
pub fn normalize(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    let spaced: String = lowered
        .chars()
        .map(|ch| {
            if matches!(ch, ',' | '!' | '?' | ';' | ':' | '\'' | '"' | '(' | ')') {
                ' '
            } else {
                ch
            }
        })
        .collect();
    spaced
        .split_whitespace()
        .map(|token| {
            let trimmed = token.trim_matches('.');
            NUMBER_WORDS
                .iter()
                .find(|(word, _)| *word == trimmed)
                .map(|(_, digits)| digits.to_string())
                .unwrap_or_else(|| trimmed.to_string())
        })
        .filter(|token| !token.is_empty())
        .collect()
}

/// One slot in a compiled template plan.
#[derive(Debug, Clone, PartialEq)]
struct PlanSlot {
    name: String,
    /// Label phrase introducing the slot (empty = bare: the
    /// short-form project slot, or a verb's argument span).
    phrase: Vec<String>,
    required: bool,
}

/// A compiled template: leading command words, then slots.
#[derive(Debug, Clone, PartialEq)]
struct TemplatePlan {
    lead: Vec<String>,
    slots: Vec<PlanSlot>,
}

/// How many leading words of a template are the command
/// itself (the rest before the first slot are that slot's
/// label phrase). Derived from the target's name: a tool's
/// hyphenated name as words; a verb's "author" + its name as
/// words; slot-less entries are all lead.
fn lead_word_count(entry: &GrammarEntry) -> usize {
    if entry.slots.is_empty() {
        return usize::MAX;
    }
    match &entry.target {
        Target::RegistryTool { name } => name.matches('-').count() + 1,
        Target::AuthorVerb { name } => 1 + name.matches('_').count() + 1,
        Target::CliMode { .. } | Target::AppCommand { .. } | Target::SafetyStop => usize::MAX,
    }
}

/// Splits one template string into word / slot / group
/// segments, dropping flag groups (flags are stripped from
/// the utterance in a pre-pass, by phrase).
fn compile_plan(template: &str, entry: &GrammarEntry) -> TemplatePlan {
    let lead_budget = lead_word_count(entry);
    let flag_phrases: Vec<Vec<String>> = entry
        .slots
        .iter()
        .filter(|slot| slot.kind == SlotKind::Flag)
        .map(|slot| {
            slot.phrase
                .split_whitespace()
                .map(ToString::to_string)
                .collect()
        })
        .collect();
    let mut lead: Vec<String> = Vec::new();
    let mut slots: Vec<PlanSlot> = Vec::new();
    let mut pending_words: Vec<String> = Vec::new();
    let mut seen_slot = false;
    for segment in split_segments(template) {
        let (text, optional) = match segment {
            Segment::Plain(text) => (text, false),
            Segment::Group(text) => (text, true),
        };
        let words: Vec<String> = text.split_whitespace().map(ToString::to_string).collect();
        if optional
            && !words.is_empty()
            && words.iter().all(|word| !word.starts_with('{'))
            && flag_phrases.contains(&words)
        {
            continue; // a flag group: handled by the pre-pass
        }
        for word in words {
            if word.starts_with('{') && word.ends_with('}') {
                let name = word[1..word.len() - 1].to_string();
                let required = !optional
                    && entry
                        .slots
                        .iter()
                        .find(|slot| slot.name == name)
                        .map(|slot| slot.required)
                        .unwrap_or(false);
                slots.push(PlanSlot {
                    name,
                    phrase: std::mem::take(&mut pending_words),
                    required,
                });
                seen_slot = true;
            } else if seen_slot || lead.len() >= lead_budget {
                pending_words.push(word);
            } else {
                lead.push(word);
            }
        }
    }
    TemplatePlan { lead, slots }
}

/// One template segment: bare text, or a `[ ]` group.
enum Segment {
    Plain(String),
    Group(String),
}

/// Splits a template into plain and bracketed segments.
fn split_segments(template: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut rest = template;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('[') {
            let Some(close) = stripped.find(']') else {
                break;
            };
            segments.push(Segment::Group(stripped[..close].to_string()));
            rest = &stripped[close + 1..];
        } else {
            let end = rest.find('[').unwrap_or(rest.len());
            segments.push(Segment::Plain(rest[..end].to_string()));
            rest = &rest[end..];
        }
        rest = rest.trim_start();
    }
    segments
}

/// Raw slot captures from one scan: slot name → tokens.
type Captures = Vec<(String, Vec<String>)>;

/// Why a scan failed, for the missing-argument diagnosis.
enum ScanFail {
    /// The tokens ran out with required slots unfilled.
    Exhausted { unfilled_required: Vec<String> },
    /// A label phrase was spoken but its value is missing.
    MissingValue { slot: String },
    /// A token fit no label phrase and no bare slot.
    UnknownToken,
}

/// Scans `tokens` against one plan. On success, returns the
/// captures. Rule order is fixed: at each position, labeled
/// slots are tried longest-phrase-first (then by name); the
/// bare slot is tried last. A slot captures one token,
/// except a `Rest`/`List` slot, which captures the whole
/// remaining span.
fn scan(
    plan: &TemplatePlan,
    tokens: &[String],
    entry: &GrammarEntry,
) -> Result<Captures, ScanFail> {
    if tokens.len() < plan.lead.len() || tokens[..plan.lead.len()] != plan.lead[..] {
        return Err(ScanFail::UnknownToken);
    }
    let kind_of = |name: &str| {
        entry
            .slots
            .iter()
            .find(|slot| slot.name == name)
            .map(|slot| slot.kind)
    };
    let mut labeled: Vec<&PlanSlot> = plan
        .slots
        .iter()
        .filter(|slot| !slot.phrase.is_empty())
        .collect();
    labeled.sort_by(|a, b| {
        b.phrase
            .len()
            .cmp(&a.phrase.len())
            .then_with(|| a.name.cmp(&b.name))
    });
    let bare: Vec<&PlanSlot> = plan
        .slots
        .iter()
        .filter(|slot| slot.phrase.is_empty())
        .collect();
    let mut filled: Captures = Vec::new();
    let is_filled = |filled: &Captures, name: &str| filled.iter().any(|(n, _)| n == name);
    let mut pos = plan.lead.len();
    while pos < tokens.len() {
        let mut progressed = false;
        for slot in &labeled {
            if is_filled(&filled, &slot.name) || !tokens[pos..].starts_with(&slot.phrase) {
                continue;
            }
            pos += slot.phrase.len();
            let value = capture_value(kind_of(&slot.name), tokens, &mut pos, &slot.name)?;
            filled.push((slot.name.clone(), value));
            progressed = true;
            break;
        }
        if !progressed {
            if let Some(slot) = bare.iter().find(|slot| !is_filled(&filled, &slot.name)) {
                let value = capture_value(kind_of(&slot.name), tokens, &mut pos, &slot.name)?;
                filled.push((slot.name.clone(), value));
            } else {
                return Err(ScanFail::UnknownToken);
            }
        }
    }
    let unfilled_required: Vec<String> = plan
        .slots
        .iter()
        .filter(|slot| slot.required && !is_filled(&filled, &slot.name))
        .map(|slot| slot.name.clone())
        .collect();
    if !unfilled_required.is_empty() {
        return Err(ScanFail::Exhausted { unfilled_required });
    }
    filled.sort();
    Ok(filled)
}

/// Captures one slot's value at `pos`, advancing it.
fn capture_value(
    kind: Option<SlotKind>,
    tokens: &[String],
    pos: &mut usize,
    slot_name: &str,
) -> Result<Vec<String>, ScanFail> {
    if *pos >= tokens.len() {
        return Err(ScanFail::MissingValue {
            slot: slot_name.to_string(),
        });
    }
    if matches!(kind, Some(SlotKind::Rest) | Some(SlotKind::List)) {
        let value = tokens[*pos..].to_vec();
        *pos = tokens.len();
        Ok(value)
    } else {
        let value = vec![tokens[*pos].clone()];
        *pos += 1;
        Ok(value)
    }
}

/// Strips flag phrases from the token stream (searching from
/// `start`), returning the remaining tokens and the flags
/// that fired.
fn strip_flags(
    entry: &GrammarEntry,
    tokens: &[String],
    start: usize,
) -> (Vec<String>, Vec<String>) {
    let mut rest: Vec<String> = tokens.to_vec();
    let mut fired: Vec<String> = Vec::new();
    for slot in entry
        .slots
        .iter()
        .filter(|slot| slot.kind == SlotKind::Flag)
    {
        let phrase: Vec<String> = slot
            .phrase
            .split_whitespace()
            .map(ToString::to_string)
            .collect();
        if phrase.is_empty() || phrase.len() > rest.len().saturating_sub(start) {
            continue;
        }
        let mut pos = start;
        while pos + phrase.len() <= rest.len() {
            if rest[pos..pos + phrase.len()] == phrase[..] {
                rest.drain(pos..pos + phrase.len());
                fired.push(slot.name.clone());
                break;
            }
            pos += 1;
        }
    }
    (rest, fired)
}

/// Assembles the argument object from one scan's captures.
/// Returns the arguments, or a type failure naming the slot
/// and the offending value.
fn assemble_args(
    entry: &GrammarEntry,
    captures: &Captures,
    flags: &[String],
) -> Result<Value, (String, String)> {
    let mut args = Map::new();
    for slot in &entry.slots {
        if slot.kind == SlotKind::Flag {
            if flags.contains(&slot.name) {
                args.insert(slot.name.clone(), Value::Bool(true));
            }
            continue;
        }
        let Some((_, tokens)) = captures.iter().find(|(name, _)| name == &slot.name) else {
            if slot.required {
                return Err((slot.name.clone(), String::new()));
            }
            continue;
        };
        let value = match slot.kind {
            SlotKind::Project | SlotKind::Path | SlotKind::Name => Value::String(tokens.join(" ")),
            SlotKind::Rest => Value::String(tokens.join(" ")),
            SlotKind::List => Value::Array(
                tokens
                    .iter()
                    .map(|token| Value::String(token.clone()))
                    .collect(),
            ),
            SlotKind::Number => parse_number(&tokens.join(" "))
                .ok_or_else(|| (slot.name.clone(), tokens.join(" ")))?,
            SlotKind::Flag => unreachable!("flags handled above"),
        };
        args.insert(slot.name.clone(), value);
    }
    Ok(Value::Object(args))
}

/// Parses a spoken number token into a JSON number.
fn parse_number(token: &str) -> Option<Value> {
    if let Ok(int) = token.parse::<i64>() {
        return Some(Value::Number(int.into()));
    }
    token
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map(Value::Number)
}

/// One structurally matched but type-invalid candidate.
struct TypeFailure {
    entry_id: String,
    slot: String,
    value: String,
}

/// Diagnoses a missing required argument across the grammar:
/// the first entry (in id order) whose scan failed only
/// because the utterance ran out.
fn missing_arg_diagnosis(grammar: &GrammarDoc, tokens: &[String]) -> Option<(String, String)> {
    let mut entries: Vec<&GrammarEntry> = grammar.entries.iter().collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    for entry in entries {
        let (stripped, _) = strip_flags(entry, tokens, 1.min(tokens.len()));
        for template in &entry.templates {
            let plan = compile_plan(template, entry);
            // An utterance that is a strict prefix of the
            // lead words also counts: the command was named,
            // its arguments never arrived.
            if tokens.len() < plan.lead.len() && plan.lead[..tokens.len()] == tokens[..] {
                if let Some(slot) = plan.slots.iter().find(|slot| slot.required) {
                    return Some((entry.id.clone(), slot.name.clone()));
                }
                continue;
            }
            match scan(&plan, &stripped, entry) {
                Err(ScanFail::MissingValue { slot }) => {
                    return Some((entry.id.clone(), slot));
                }
                Err(ScanFail::Exhausted { unfilled_required }) => {
                    if let Some(slot) = unfilled_required.first() {
                        return Some((entry.id.clone(), slot.clone()));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Parses one transcript against the grammar. See the module
/// docs for the determinism contract.
pub fn parse(grammar: &GrammarDoc, transcript: &Transcript) -> ParseOutcome {
    let tokens = normalize(&transcript.text);
    let normalized = tokens.join(" ");
    if tokens.is_empty() {
        return ParseOutcome::Refusal(VoiceRefusal {
            code: TG_VOICE_NO_MATCH.to_string(),
            message: "empty utterance".to_string(),
            candidates: Vec::new(),
            transcript: normalized,
        });
    }
    // STOP first: before templates, before confidence.
    if STOP_LEXEMES.contains(&tokens[0].as_str()) {
        return ParseOutcome::Stop;
    }
    let mut commands: Vec<ParsedCommand> = Vec::new();
    let mut type_failures: Vec<TypeFailure> = Vec::new();
    let mut entries: Vec<&GrammarEntry> = grammar.entries.iter().collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    for entry in entries {
        if entry.target == Target::SafetyStop {
            continue;
        }
        // The flag pre-pass searches after the first token:
        // a flag phrase is never the command's first word.
        let (stripped, flags) = strip_flags(entry, &tokens, 1.min(tokens.len()));
        for template in &entry.templates {
            let plan = compile_plan(template, entry);
            let Ok(captures) = scan(&plan, &stripped, entry) else {
                continue;
            };
            match assemble_args(entry, &captures, &flags) {
                Ok(args) => {
                    let command = ParsedCommand {
                        entry_id: entry.id.clone(),
                        target: entry.target.clone(),
                        posture: entry.posture.clone(),
                        confirm: entry.confirm,
                        args,
                        unverified: transcript.confidence.is_none(),
                    };
                    if !commands.contains(&command) {
                        commands.push(command);
                    }
                }
                Err((slot, value)) => {
                    if !value.is_empty() {
                        type_failures.push(TypeFailure {
                            entry_id: entry.id.clone(),
                            slot,
                            value,
                        });
                    }
                }
            }
        }
    }
    commands.sort_by(|a, b| {
        a.entry_id
            .cmp(&b.entry_id)
            .then_with(|| a.args.to_string().cmp(&b.args.to_string()))
    });
    match commands.len() {
        0 => {
            if let Some(failure) = type_failures.first() {
                return ParseOutcome::Refusal(VoiceRefusal {
                    code: TG_VOICE_ARG_TYPE.to_string(),
                    message: format!(
                        "argument '{}' of {} cannot take the value '{}'",
                        failure.slot, failure.entry_id, failure.value
                    ),
                    candidates: vec![failure.entry_id.clone()],
                    transcript: normalized,
                });
            }
            if let Some((entry_id, slot)) = missing_arg_diagnosis(grammar, &tokens) {
                return ParseOutcome::Refusal(VoiceRefusal {
                    code: TG_VOICE_MISSING_ARG.to_string(),
                    message: format!("{entry_id} needs a value for '{slot}'"),
                    candidates: vec![entry_id],
                    transcript: normalized,
                });
            }
            ParseOutcome::Refusal(VoiceRefusal {
                code: TG_VOICE_NO_MATCH.to_string(),
                message: "no command in the grammar matches the utterance".to_string(),
                candidates: Vec::new(),
                transcript: normalized,
            })
        }
        1 => {
            let command = commands[0].clone();
            if let Some(confidence) = transcript.confidence {
                if confidence < CONFIDENCE_MIN {
                    return ParseOutcome::Refusal(VoiceRefusal {
                        code: TG_VOICE_LOW_CONFIDENCE.to_string(),
                        message: format!(
                            "confidence {confidence:.2} is below the {CONFIDENCE_MIN:.2} floor; \
                             the closest parse was {} — confirm it visually or re-speak",
                            command.entry_id
                        ),
                        candidates: vec![command.entry_id.clone()],
                        transcript: normalized,
                    });
                }
            }
            ParseOutcome::Command(command)
        }
        _ => ParseOutcome::Refusal(VoiceRefusal {
            code: TG_VOICE_AMBIGUOUS.to_string(),
            message: "the utterance matches more than one command; nothing executed".to_string(),
            candidates: commands
                .iter()
                .map(|command| command.entry_id.clone())
                .collect(),
            transcript: normalized,
        }),
    }
}

//! Multi-block CLIP text batching (D-980): the oracle's
//! chunking semantics as a total function. Every content
//! token of a prompt lands in exactly one encoder block,
//! or the input is refused with a typed error — there is
//! no truncation path. The pre-D-980 pipeline resized and
//! truncated the encoded stream to a single 77-token
//! block, silently dropping every token past position 77,
//! EOS included (D-976 audit); this module exists so that
//! loss is unrepresentable in the API, not just unlikely.
//!
//! Semantics are ComfyUI's `SDTokenizer
//! ::tokenize_with_weights` (comfy/sd1_clip.py) for the
//! uniform-weight case: the content stream (the HF
//! encoding minus its BOS/EOS frame) is split into blocks
//! of at most `TEXT_BLOCK_CONTENT` tokens; each block is
//! framed `[BOS, content.., EOS]` and padded to
//! `TOKEN_LEN` with the encoder's pad id (CLIP-L pads with
//! EOS, CLIP-G with 0 — comfy/sdxl_clip.py). The oracle
//! groups tokens by weight segment before batching; with
//! uniform weights the whole string is a single group,
//! and a group of `max_word_length` (8) or more tokens
//! takes the oracle's `is_large` split, which is exactly
//! this fixed chunking. The estate's workflow strings are
//! all uniform-weight (no `(token:weight)` syntax, no
//! `embedding:` markers) — proven mechanically over the
//! keeper and battery strings by the D-980 evidence probe
//! (audit-tg/FIX-D980-REPORT.md). Weight syntax was never
//! parsed by this crate (it was BPE'd as literal
//! characters before D-980 too) and remains out of scope.
//!
//! The pooled embedding follows the oracle exactly:
//! `ClipTokenWeightEncoder.encode_token_weights` keeps
//! only the FIRST block's pooled output, read at that
//! block's first EOS position (comfy/clip_model.py takes
//! the argmax of the EOS equality — the first match).
//! Every block built here is EOS-framed by construction,
//! so that position always exists; there is no fallback
//! index.

use crate::error::{GenError, Result};
use crate::limits;

/// BOS token id shared by both SDXL CLIP tokenizers.
pub const BOS_ID: u32 = 49406;
/// EOS token id shared by both SDXL CLIP tokenizers.
pub const EOS_ID: u32 = 49407;

/// Split a content-token stream into encoder blocks of
/// `limits::TOKEN_LEN` ids each: `[BOS, up to
/// TEXT_BLOCK_CONTENT content tokens, EOS, pad..]`.
/// An empty stream yields one framed block (the oracle's
/// empty-prompt encoding). Loss is unrepresentable: the
/// concatenation of the blocks' framed content is exactly
/// `content`, or the call fails.
///
/// # Errors
/// Returns `gen_limit` (field `prompt_tokens`) when
/// `content` exceeds `limits::TEXT_CONTENT_MAX` tokens —
/// refused whole, before any allocation of encoder work,
/// never cut down to fit.
pub fn batch_blocks(content: &[u32], pad_id: u32) -> Result<Vec<[u32; limits::TOKEN_LEN]>> {
    if content.len() > limits::TEXT_CONTENT_MAX {
        return Err(GenError::Limit {
            field: "prompt_tokens",
            detail: format!(
                "{} content tokens exceeds {} ({} blocks of {})",
                content.len(),
                limits::TEXT_CONTENT_MAX,
                limits::TEXT_BLOCKS_MAX,
                limits::TEXT_BLOCK_CONTENT
            ),
        });
    }
    if content.is_empty() {
        return Ok(vec![framed_block(&[], pad_id)]);
    }
    Ok(content
        .chunks(limits::TEXT_BLOCK_CONTENT)
        .map(|chunk| framed_block(chunk, pad_id))
        .collect())
}

fn framed_block(chunk: &[u32], pad_id: u32) -> [u32; limits::TOKEN_LEN] {
    debug_assert!(chunk.len() <= limits::TEXT_BLOCK_CONTENT);
    let mut block = [pad_id; limits::TOKEN_LEN];
    block[0] = BOS_ID;
    block[1..=chunk.len()].copy_from_slice(chunk);
    block[chunk.len() + 1] = EOS_ID;
    block
}

/// The pooled-embedding source position in a block: the
/// index of its first EOS token. `batch_blocks` frames
/// every block with an EOS, so the position always
/// exists; the scan replaces the pre-D-980 fallback that
/// silently read position 76 (a content token, once the
/// real EOS had been truncated away).
pub fn pooled_position(block: &[u32; limits::TOKEN_LEN]) -> usize {
    for (i, id) in block.iter().enumerate() {
        if *id == EOS_ID {
            return i;
        }
    }
    // batch_blocks closes every block with EOS at
    // position chunk_len + 1 <= TOKEN_LEN - 1, so a block
    // reaching this line was not built by batch_blocks.
    unreachable!("text block without an EOS token")
}

/// Repeat counts aligning two CFG contexts to one
/// sequence length, the oracle's rule (comfy/conds.py
/// `CONDCrossAttn.concat`): the shorter context repeats
/// to the least common multiple of the two block counts.
/// Repeat-padding is exact for cross-attention (duplicated
/// keys split softmax mass over identical values), so no
/// approximation is involved; the oracle's own >4x
/// fallback runs the rows as separate batches with
/// mathematically identical output, so this crate aligns
/// unconditionally and bounds only the total size.
///
/// # Errors
/// Returns `gen_limit` (field `context_blocks`) when the
/// aligned length exceeds `limits::CONTEXT_BLOCKS_MAX`
/// blocks, or when a block count is zero (impossible from
/// `batch_blocks`, refused rather than dividing by it).
pub fn aligned_repeats(blocks_a: usize, blocks_b: usize) -> Result<(usize, usize)> {
    if blocks_a == 0 || blocks_b == 0 {
        return Err(GenError::Limit {
            field: "context_blocks",
            detail: "block count of zero cannot be aligned".into(),
        });
    }
    let lcm = blocks_a / gcd(blocks_a, blocks_b) * blocks_b;
    if lcm > limits::CONTEXT_BLOCKS_MAX {
        return Err(GenError::Limit {
            field: "context_blocks",
            detail: format!(
                "aligned context of {lcm} blocks exceeds {}",
                limits::CONTEXT_BLOCKS_MAX
            ),
        });
    }
    Ok((lcm / blocks_a, lcm / blocks_b))
}

fn gcd(a: usize, b: usize) -> usize {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prima's keeper prompt as a content-token stream
    /// (109 tokens; 111 with the BOS/EOS frame), tokenized
    /// from the record string in TG-P1.json by the real
    /// tokenizer.json — the D-976 audit's specimen, whose
    /// tail (the outfit spec) began past the old cut.
    const PRIMA_PROMPT_CONTENT: &[u32] = &[
        5797, 267, 272, 1611, 267, 7115, 2308, 267, 1476, 1774, 267, 2862, 267, 17011, 4230, 267,
        2184, 1093, 267, 5706, 536, 7578, 267, 5706, 8456, 1520, 633, 1774, 267, 3500, 8626, 267,
        7072, 8456, 7803, 267, 1375, 531, 11344, 530, 6481, 267, 10709, 1579, 5994, 267, 4129,
        5994, 267, 6313, 7597, 37348, 267, 8911, 29159, 267, 6469, 1844, 267, 3467, 2225, 267,
        1538, 31941, 2225, 267, 2225, 531, 18459, 267, 736, 3095, 267, 1936, 268, 10127, 13628,
        267, 3773, 15039, 1710, 267, 7011, 17011, 11357, 267, 14297, 3490, 267, 6254, 3289, 267,
        8675, 37637, 267, 28275, 1449, 12473, 267, 1579, 23525, 267, 1449, 11200, 12386, 267, 1449,
        1400, 10909,
    ];

    fn synthetic(len: usize) -> Vec<u32> {
        // Distinct ids, none equal to BOS/EOS, so the
        // preservation scan below cannot confuse content
        // with framing.
        (0..len).map(|i| 1000 + i as u32).collect()
    }

    fn framed_content(block: &[u32; limits::TOKEN_LEN]) -> &[u32] {
        let end = pooled_position(block);
        &block[1..end]
    }

    #[test]
    fn prima_prompt_batches_to_two_blocks_with_tail_intact() {
        assert_eq!(PRIMA_PROMPT_CONTENT.len(), 109);
        for pad_id in [EOS_ID, 0] {
            let blocks = batch_blocks(PRIMA_PROMPT_CONTENT, pad_id).expect("batches");
            assert_eq!(blocks.len(), 2);
            // Block 0 is full: BOS + 75 content + EOS at
            // the final position (the pooled source).
            assert_eq!(blocks[0][0], BOS_ID);
            assert_eq!(blocks[0][limits::TOKEN_LEN - 1], EOS_ID);
            assert_eq!(pooled_position(&blocks[0]), limits::TOKEN_LEN - 1);
            // Block 1 carries the remaining 34 tokens —
            // the face/body/outfit span the pre-D-980 cut
            // dropped — EOS-framed, then padded.
            assert_eq!(framed_content(&blocks[1]).len(), 34);
            assert_eq!(pooled_position(&blocks[1]), 35);
            assert!(blocks[1][36..].iter().all(|id| *id == pad_id));
            // The final content token of the prompt (the
            // last outfit word) survives, in place.
            let last = *PRIMA_PROMPT_CONTENT.last().expect("non-empty");
            assert_eq!(blocks[1][34], last);
            // Flattened framed content == the input, in
            // order, for both pad ids.
            let flat: Vec<u32> = blocks.iter().flat_map(framed_content).copied().collect();
            assert_eq!(flat, PRIMA_PROMPT_CONTENT);
        }
    }

    #[test]
    fn every_boundary_length_preserves_token_count_exactly() {
        // The D-980 boundary set: around one block (75
        // content), two blocks (150), and past them, plus
        // the named cap. ANY input length must preserve
        // its token count exactly — that is the property
        // whose absence was the D-976 defect.
        let lengths = [
            0usize, 1, 74, 75, 76, 77, 149, 150, 151, 152, 153, 200, 225, 1199, 1200,
        ];
        for len in lengths {
            let content = synthetic(len);
            let want_blocks = if len == 0 {
                1
            } else {
                len.div_ceil(limits::TEXT_BLOCK_CONTENT)
            };
            for pad_id in [EOS_ID, 0] {
                let blocks = batch_blocks(&content, pad_id).expect("batches");
                assert_eq!(blocks.len(), want_blocks, "len {len}");
                for block in &blocks {
                    assert_eq!(block[0], BOS_ID, "len {len}");
                    let end = pooled_position(block);
                    assert!((1..limits::TOKEN_LEN).contains(&end), "len {len}");
                    assert!(block[end + 1..].iter().all(|id| *id == pad_id), "len {len}");
                }
                let flat: Vec<u32> = blocks.iter().flat_map(framed_content).copied().collect();
                assert_eq!(flat, content, "len {len} pad {pad_id}");
            }
        }
    }

    #[test]
    fn over_cap_input_is_a_typed_error_never_a_cut() {
        let content = synthetic(limits::TEXT_CONTENT_MAX + 1);
        match batch_blocks(&content, EOS_ID) {
            Err(GenError::Limit { field, .. }) => assert_eq!(field, "prompt_tokens"),
            other => panic!("expected gen_limit, got {other:?}"),
        }
    }

    #[test]
    fn pooled_position_is_a_genuine_eos_on_multi_block_input() {
        let content = synthetic(200);
        let blocks = batch_blocks(&content, EOS_ID).expect("batches");
        assert_eq!(blocks.len(), 3);
        // Block 0 is full, so its first EOS is the closing
        // frame token at the final position — a genuine
        // EOS, not a content token mistaken for one.
        assert_eq!(pooled_position(&blocks[0]), limits::TOKEN_LEN - 1);
        assert_eq!(blocks[0][limits::TOKEN_LEN - 1], EOS_ID);
        // A short single block: EOS right after content.
        let blocks = batch_blocks(&synthetic(10), EOS_ID).expect("batches");
        assert_eq!(pooled_position(&blocks[0]), 11);
    }

    #[test]
    fn aligned_repeats_follow_the_lcm_rule_with_a_bound() {
        assert_eq!(aligned_repeats(1, 1).expect("aligns"), (1, 1));
        assert_eq!(aligned_repeats(1, 2).expect("aligns"), (2, 1));
        assert_eq!(aligned_repeats(2, 3).expect("aligns"), (3, 2));
        assert_eq!(aligned_repeats(3, 5).expect("aligns"), (5, 3));
        assert_eq!(aligned_repeats(16, 16).expect("aligns"), (1, 1));
        // LCM 240 and 35 both exceed the 32-block bound.
        for (a, b) in [(16usize, 15usize), (7, 5), (0, 1), (1, 0)] {
            match aligned_repeats(a, b) {
                Err(GenError::Limit { field, .. }) => {
                    assert_eq!(field, "context_blocks");
                }
                other => panic!("expected gen_limit for ({a}, {b}), got {other:?}"),
            }
        }
    }
}

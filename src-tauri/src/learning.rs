/// Pure diff/learning logic for the auto-learn edit-capture path.
/// No Tauri, no I/O — everything here is unit-testable.

use similar::{capture_diff_slices, Algorithm, DiffOp};
use strsim::levenshtein;

pub struct CorrectionCandidate {
    /// Lowercased word as Whisper heard it (the key used in the corrections map).
    pub heard: String,
    /// Original-cased word as the user typed it (the replacement value).
    pub corrected: String,
}

/// Compare `inserted` (what Wisspa pasted) with `observed` (what the field
/// contains 8 seconds later) and return conservative correction candidates.
///
/// `raw_transcript` is the pre-cleanup Whisper output; a candidate is only
/// kept when the `heard` token appears there as a whole word, because
/// `apply_corrections` runs on the raw transcript before Haiku.
pub fn diff_corrections(
    inserted: &str,
    observed: &str,
    raw_transcript: &str,
) -> Vec<CorrectionCandidate> {
    if inserted.is_empty() || observed.is_empty() {
        return vec![];
    }

    let inserted_tokens = tokenize(inserted);
    let observed_tokens = tokenize(observed);

    if inserted_tokens.is_empty() {
        return vec![];
    }

    let ops = capture_diff_slices(Algorithm::Myers, &inserted_tokens, &observed_tokens);

    // Overlap gate: >= 60% of `inserted` tokens must survive unchanged.
    let unchanged: usize = ops
        .iter()
        .map(|op| if let DiffOp::Equal { len, .. } = op { *len } else { 0 })
        .sum();
    if (unchanged as f64 / inserted_tokens.len() as f64) < 0.6 {
        return vec![];
    }

    // Collect only 1-for-1 substitutions.
    let substitutions: Vec<(usize, usize)> = ops
        .iter()
        .filter_map(|op| {
            if let DiffOp::Replace { old_index, old_len, new_index, new_len } = op {
                if *old_len == 1 && *new_len == 1 {
                    return Some((*old_index, *new_index));
                }
            }
            None
        })
        .collect();

    // Substitution count gate: 1–3 substitutions only.
    if substitutions.is_empty() || substitutions.len() > 3 {
        return vec![];
    }

    let raw_lower = raw_transcript.to_lowercase();

    substitutions
        .into_iter()
        .filter_map(|(old_idx, new_idx)| {
            let heard = &inserted_tokens[old_idx];
            let corrected = &observed_tokens[new_idx];

            let heard_lower = heard.to_lowercase();
            let corrected_lower = corrected.to_lowercase();

            // Both tokens must be non-empty and purely alphabetic.
            if heard.is_empty() || corrected.is_empty() {
                return None;
            }
            if !heard.chars().all(|c| c.is_alphabetic() || c == '\'') {
                return None;
            }
            if !corrected.chars().all(|c| c.is_alphabetic() || c == '\'') {
                return None;
            }

            // Differ by more than just case.
            if heard_lower == corrected_lower {
                return None;
            }

            // Phonetically/visually close enough to be a real mishearing.
            let dist = levenshtein(&heard_lower, &corrected_lower);
            let max_dist = std::cmp::max(2, corrected.len() / 2);
            if dist > max_dist {
                return None;
            }

            // The heard token must appear as a whole word in the raw transcript.
            if !contains_whole_word(&raw_lower, &heard_lower) {
                return None;
            }

            Some(CorrectionCandidate {
                heard: heard_lower,
                corrected: corrected.clone(),
            })
        })
        .collect()
}

/// Returns true when `word` occurs as a whole word in `text` (both lowercased).
fn contains_whole_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let word_len = word.len();
    let text_bytes = text.as_bytes();
    let mut start = 0;
    while start + word_len <= text.len() {
        match text[start..].find(word) {
            None => break,
            Some(pos) => {
                let abs = start + pos;
                let before_ok = abs == 0 || !text_bytes[abs - 1].is_ascii_alphanumeric();
                let after = abs + word_len;
                let after_ok =
                    after >= text.len() || !text_bytes[after].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    return true;
                }
                start = abs + 1;
            }
        }
    }
    false
}

/// Tokenize into runs of alphanumeric characters (plus apostrophes).
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_alphanumeric() || c == '\'' {
            let mut end = start + c.len_utf8();
            while let Some(&(_, nc)) = chars.peek() {
                if nc.is_alphanumeric() || nc == '\'' {
                    chars.next();
                    end += nc.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(text[start..end].to_string());
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_inserted_observed_yields_no_candidates() {
        let candidates = diff_corrections(
            "hello world test",
            "hello world test",
            "hello world test",
        );
        assert!(candidates.is_empty());
    }

    #[test]
    fn one_clean_substitution_in_raw_transcript_yields_candidate() {
        // "cloude" is a plausible Whisper mishearing of "Claude" (1 edit distance).
        let candidates = diff_corrections(
            "use the cloude api here",
            "use the Claude api here",
            "use the cloude api here",
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].heard, "cloude");
        assert_eq!(candidates[0].corrected, "Claude");
    }

    #[test]
    fn heard_not_in_raw_transcript_is_dropped() {
        // "cloude" in inserted but raw transcript has "openai" — no match.
        let candidates = diff_corrections(
            "use the cloude api here",
            "use the Claude api here",
            "use the openai api here",
        );
        assert!(candidates.is_empty());
    }

    #[test]
    fn case_only_change_is_dropped() {
        let candidates = diff_corrections(
            "use the Claude api here",
            "use the claude api here",
            "use the Claude api here",
        );
        assert!(candidates.is_empty());
    }

    #[test]
    fn low_overlap_observed_yields_no_candidates() {
        // User overwrote with entirely different text.
        let candidates = diff_corrections(
            "hello world foo",
            "this is completely different text here now",
            "hello world foo",
        );
        assert!(candidates.is_empty());
    }

    #[test]
    fn four_or_more_substitutions_yields_no_candidates() {
        // 10 tokens, 4 substitutions (quick→swift, brown→green, fox→cat, jumps→leaps).
        // 6 unchanged → overlap 0.6 passes the gate; substitution count gate fires.
        let candidates = diff_corrections(
            "the quick brown fox jumps over the lazy dog today",
            "the swift green cat leaps over the lazy dog today",
            "the quick brown fox jumps over the lazy dog today",
        );
        assert!(candidates.is_empty());
    }
}

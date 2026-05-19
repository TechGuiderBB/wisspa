use super::{registry, Action};
use strsim::levenshtein;

pub struct Match {
    pub action: Action,
    /// What remains of the transcript after the trigger phrase, for {query}.
    pub query: String,
}

pub struct Suggestion {
    pub action_id: String,
    pub action_name: String,
    pub trigger: String,
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_ascii_punctuation())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Walk `s` by whitespace tokens, skip the first `n`, and return the remainder
/// trimmed. Returns an empty string if `n >= token count`.
///
/// This preserves the user's original capitalisation, punctuation, and inner
/// whitespace within the remaining tokens — used to extract `{query}` from the
/// untouched transcript so voice notes don't lose case/punctuation. The
/// remainder is rejoined with single spaces because the SplitWhitespace
/// iterator discards inter-token whitespace runs; that's a deliberate tradeoff
/// to keep tokenisation parity with `normalize`.
fn skip_words(s: &str, n: usize) -> String {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    if n >= tokens.len() {
        return String::new();
    }
    tokens[n..].join(" ").trim().to_string()
}

/// Match a transcript against the registry. Two-pass:
/// 1. Exact prefix match against any trigger phrase (longest match wins so
///    "search GitHub for" beats "search").
/// 2. Fuzzy: Levenshtein ≤ 3 against the full transcript.
pub fn find_match(transcript: &str) -> Option<Match> {
    let normalised = normalize(transcript);
    if normalised.is_empty() {
        return None;
    }

    let actions = registry::snapshot();

    // Pass 1 — exact prefix match, prefer longest trigger.
    // We store the normalised trigger length for the longest-wins tie-break
    // (matches the original behaviour) AND the raw trigger's whitespace-token
    // count, because the query must be skipped from the *original* transcript
    // (not the normalised one) to preserve capitalisation and punctuation.
    // Using the raw trigger's whitespace-token count keeps the skip count
    // aligned with the transcript's own whitespace tokenisation, regardless
    // of how `normalize` handles any internal punctuation in either string.
    let mut best_exact: Option<(usize, &Action, usize)> = None;
    for action in &actions {
        for trigger in &action.triggers {
            let nt = normalize(trigger);
            if nt.is_empty() {
                continue;
            }
            let is_prefix = normalised == nt
                || normalised.starts_with(&format!("{nt} "));
            if is_prefix {
                let len = nt.len();
                let trigger_words = trigger.split_whitespace().count();
                if best_exact.map(|(l, _, _)| len > l).unwrap_or(true) {
                    best_exact = Some((len, action, trigger_words));
                }
            }
        }
    }
    if let Some((_nt_len, action, trigger_words)) = best_exact {
        let query = skip_words(transcript, trigger_words);
        return Some(Match {
            action: action.clone(),
            query,
        });
    }

    // Pass 2 — fuzzy (Levenshtein ≤ 3) against the whole transcript.
    let mut best_fuzzy: Option<(usize, &Action)> = None;
    for action in &actions {
        for trigger in &action.triggers {
            let nt = normalize(trigger);
            let d = levenshtein(&normalised, &nt);
            if d <= 3 && best_fuzzy.map(|(b, _)| d < b).unwrap_or(true) {
                best_fuzzy = Some((d, action));
            }
        }
    }
    best_fuzzy.map(|(_, action)| Match {
        action: action.clone(),
        query: String::new(),
    })
}

/// Top-N nearest triggers, for the "Did you mean: …?" toast.
pub fn suggest_top(transcript: &str, n: usize) -> Vec<Suggestion> {
    let normalised = normalize(transcript);
    let actions = registry::snapshot();
    let mut scored: Vec<(usize, &Action, &String)> = Vec::new();
    for action in &actions {
        for trigger in &action.triggers {
            let nt = normalize(trigger);
            scored.push((levenshtein(&normalised, &nt), action, trigger));
        }
    }
    scored.sort_by_key(|(d, _, _)| *d);
    scored
        .into_iter()
        .take(n)
        .map(|(_, a, t)| Suggestion {
            action_id: a.id.clone(),
            action_name: a.name.clone(),
            trigger: t.clone(),
        })
        .collect()
}

// TODO: tests — cover (a) hyphenated trigger "open new-note", (b) mixed-case
// punctuated transcript like "New Note! Buy Bread, please?" preserving case
// and punctuation in {query}, (c) longest-trigger tie-break still wins.

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

/// Skip the first `n` whitespace-delimited words from `s` and return the rest,
/// trimmed. Used to extract the query portion of a transcript after the trigger.
fn skip_words(s: &str, n: usize) -> String {
    let mut remaining = s.trim();
    for _ in 0..n {
        remaining = remaining.trim_start();
        remaining = remaining.trim_start_matches(|c: char| !c.is_whitespace());
    }
    remaining.trim().to_string()
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
    let mut best_exact: Option<(usize, usize, &Action)> = None; // (normalized_len, word_count, action)
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
                let word_count = nt.split_whitespace().count();
                if best_exact.map(|(l, _, _)| len > l).unwrap_or(true) {
                    best_exact = Some((len, word_count, action));
                }
            }
        }
    }
    if let Some((_, trigger_words, action)) = best_exact {
        // Extract query from the *original* transcript (not the normalised version)
        // so that capitalisation and punctuation are preserved in {query}.
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

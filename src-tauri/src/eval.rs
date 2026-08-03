//! Prompt-mode evaluation harness.
//!
//! Measures the REAL voice-to-prompt path — `llm::sonnet_prompt_rewrite` with
//! the real `prompts/sonnet_prompt.md` (pulled in via `include_str!`) — against
//! the fixture corpus in `eval/fixtures/`, so prompt edits ship on measured
//! pass rates rather than vibes.
//!
//! The live run is a single `#[ignore]`d test. It is NOT part of `cargo test`
//! and CI never runs it (no API keys in CI). Run it locally:
//!
//! ```sh
//! cd src-tauri && ANTHROPIC_API_KEY=... cargo test -- --ignored eval_prompt --nocapture
//! ```
//!
//! With no key in the environment (or the repo-root `.env`, which is picked up
//! the same way `main.rs::load_env` does it) the test prints "skipped" and
//! passes, so `--ignored` suites stay green without credentials.
//!
//! Everything else in this module — the property scorers and the fixture-corpus
//! validation — is fast and runs in the normal `cargo test` suite.
//!
//! ## Scoring philosophy
//!
//! No LLM judging: every assertion is a deterministic property check. The route
//! heuristics are deliberately CONSERVATIVE — they only fail an output on clear
//! evidence of the wrong branch, and score leniently when unsure (documented
//! per-function below). A heuristic that can't decide says "pass"; the sharper
//! per-fixture `must_include` / `max_chars` pins carry the real signal.

use crate::app_detector::BrowserContext;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

/// Pause between live API calls. Fixtures run strictly serially (one await
/// after another); the extra beat keeps the run polite against rate limits.
const INTER_CALL_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    name: String,
    transcript: String,
    active_app: String,
    browser_context: Option<FixtureBrowserContext>,
    selected_text: Option<String>,
    expect: Expect,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureBrowserContext {
    url: String,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    route: Route,
    #[serde(default)]
    must_include: Vec<String>,
    #[serde(default)]
    must_not_include: Vec<String>,
    max_chars: Option<usize>,
    max_sentences: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Route {
    /// AI-tool destination: output should be a prompt to paste, not the answer.
    Prompt,
    /// Non-AI destination: output should be the finished content itself.
    Content,
    /// Degenerate input: output should be ~the cleaned transcript verbatim.
    Passthrough,
}

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../eval/fixtures")
}

/// Load every `*.json` fixture, sorted by filename so report order is stable
/// across runs (read_dir order is filesystem-dependent).
fn load_fixtures() -> Result<Vec<Fixture>> {
    let dir = fixtures_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("read fixtures dir {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let raw = std::fs::read_to_string(p)
                .with_context(|| format!("read fixture {}", p.display()))?;
            serde_json::from_str(&raw).with_context(|| format!("parse fixture {}", p.display()))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Property scorers (pure, fast, unit-tested below)
// ---------------------------------------------------------------------------

/// Case-insensitive substring check. An empty needle always matches.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Deliberately naive sentence count: split on `.`/`!`/`?`/newline and count
/// segments holding at least one alphanumeric character. "Dr. Smith" counts
/// as two — this is a restraint gauge for terse destinations, not NLP, so
/// fixtures should use it with generous headroom.
fn count_sentences(s: &str) -> usize {
    s.split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|seg| seg.chars().any(|c| c.is_alphanumeric()))
        .count()
}

/// Prompt-structure markers: things a structured *prompt* carries that finished
/// content essentially never does. Generic markdown headings (`## `) are NOT
/// here on purpose — GitHub issues, Notion docs and emails legitimately use
/// headings, so only prompt-y section names count.
fn has_prompt_structure(output: &str) -> bool {
    const XML_MARKERS: &[&str] = &[
        "<task",
        "<context",
        "<constraint",
        "<output",
        "<role",
        "<example",
        "<goal",
        "<instruction",
        "</", // any closing XML tag
    ];
    const PROMPT_HEADINGS: &[&str] = &[
        "## context",
        "## task",
        "## constraint",
        "## output",
        "## role",
        "## example",
        "## goal",
        "**context:",
        "**task:",
        "**constraint",
        "**output",
        "**role:",
    ];
    const ROLE_FRAMING: &[&str] = &["you are a", "you are an", "your task", "act as"];
    let lower = output.to_lowercase();
    XML_MARKERS.iter().any(|m| lower.contains(m))
        || PROMPT_HEADINGS.iter().any(|m| lower.contains(m))
        || ROLE_FRAMING.iter().any(|m| lower.contains(m))
}

/// Imperative task framing on the first non-empty line: "Summarise this…",
/// "Fix the typo…", "Please write…", "Can you explain…". This is how a prompt
/// addresses the target AI; finished content (an email, a Slack message)
/// almost never opens with one of these verbs aimed at the reader.
fn starts_with_task_framing(output: &str) -> bool {
    const IMPERATIVE_VERBS: &[&str] = &[
        "write", "draft", "compose", "summarise", "summarize", "explain", "create", "generate",
        "list", "give", "help", "suggest", "review", "rewrite", "improve", "fix", "refactor",
        "implement", "translate", "compare", "analyse", "analyze", "outline", "plan", "describe",
        "produce", "convert", "make", "show", "find", "research", "brainstorm", "extract",
        "identify", "evaluate", "critique", "debug", "add", "build", "design", "recommend",
        "provide", "teach", "act", "ask", "check", "tell", "express", "turn", "break", "take",
        "come", "think", "walk", "pretend",
    ];
    const LEAD_INS: &[&str] = &[
        "please ",
        "can you ",
        "could you ",
        "i need you to ",
        "i want you to ",
    ];
    let Some(first_line) = output.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return false;
    };
    let lower = first_line.to_lowercase();
    let cleaned = lower.trim_start_matches(|c: char| !c.is_alphanumeric());
    let mut rest = cleaned;
    for prefix in LEAD_INS {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped;
            break;
        }
    }
    let word = rest
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(|c: char| !c.is_alphanumeric());
    IMPERATIVE_VERBS.contains(&word)
}

/// Leniency valve for the prompt route: a simple question kept (or rephrased)
/// as a question IS a valid prompt for simple Q&A tasks ("What's the
/// difference between a Roth IRA and a traditional IRA?"). Only consulted for
/// `route: "prompt"`, never to excuse content-shaped output.
fn is_bare_question(output: &str) -> bool {
    output.trim_end().ends_with('?')
}

/// Length ceiling for the passthrough route: input chars + half again + a
/// small constant for cleanup punctuation/casing. "Thank you" (9 chars) caps
/// at 33 — "Thank you." passes, any real prompt or content does not.
fn passthrough_len_cap(transcript: &str) -> usize {
    let t = transcript.chars().count();
    t + t / 2 + 20
}

/// Route heuristic. Conservative by design:
/// - `prompt` passes on prompt structure, imperative task framing, OR a bare
///   question (documented leniency above).
/// - `content` passes only when neither prompt structure nor task framing is
///   present. A bare question still passes content (a Slack "are we still on?"
///   is content), so content leans lenient too.
/// - `passthrough` passes when the output stays near the transcript's length
///   AND shows no prompt structure or task framing.
fn route_matches(route: Route, output: &str, transcript: &str) -> bool {
    match route {
        Route::Prompt => {
            has_prompt_structure(output)
                || starts_with_task_framing(output)
                || is_bare_question(output)
        }
        Route::Content => !has_prompt_structure(output) && !starts_with_task_framing(output),
        Route::Passthrough => {
            output.chars().count() <= passthrough_len_cap(transcript)
                && !has_prompt_structure(output)
                && !starts_with_task_framing(output)
        }
    }
}

/// Score one model output against a fixture's expectations. Returns the list
/// of failed assertions (empty = pass).
fn score_output(expect: &Expect, output: &str, transcript: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !route_matches(expect.route, output, transcript) {
        failures.push(format!(
            "route {:?} heuristic not satisfied",
            expect.route
        ));
    }
    for needle in &expect.must_include {
        if !contains_ci(output, needle) {
            failures.push(format!("must_include {needle:?} not found in output"));
        }
    }
    for needle in &expect.must_not_include {
        if contains_ci(output, needle) {
            failures.push(format!("must_not_include {needle:?} present in output"));
        }
    }
    if let Some(max) = expect.max_chars {
        let n = output.chars().count();
        if n > max {
            failures.push(format!("max_chars {max} exceeded: output is {n} chars"));
        }
    }
    if let Some(max) = expect.max_sentences {
        let n = count_sentences(output);
        if n > max {
            failures.push(format!("max_sentences {max} exceeded: counted {n}"));
        }
    }
    failures
}

// ---------------------------------------------------------------------------
// Live runner
// ---------------------------------------------------------------------------

/// Resolve the Anthropic key: process env first, then the repo-root `.env`
/// (same dev convention as `main.rs::load_env`, so the eval Just Works in a
/// dev checkout without exporting anything).
fn resolve_api_key() -> Option<String> {
    let from_env = std::env::var("ANTHROPIC_API_KEY").ok();
    if let Some(k) = from_env.filter(|k| !k.trim().is_empty()) {
        return Some(k);
    }
    let dotenv = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.env");
    if dotenv.exists() {
        let _ = dotenvy::from_path(&dotenv);
        return std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty());
    }
    None
}

/// Short single-line excerpt of a model output for failure reports.
fn excerpt(output: &str) -> String {
    const MAX: usize = 240;
    let one_line: String = output.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    if one_line.chars().count() > MAX {
        format!("{}…", one_line.chars().take(MAX).collect::<String>())
    } else {
        one_line
    }
}

/// Live eval over the full fixture corpus. Ignored by default: hits the real
/// Anthropic API and costs a few cents per run. Serial by construction — one
/// awaited call at a time with a short delay between, reusing the crate's real
/// retry/timeout behaviour via `sonnet_prompt_rewrite`. Fails unless every
/// fixture passes (pass rate must be 100%).
#[tokio::test]
#[ignore = "live Anthropic eval: run with ANTHROPIC_API_KEY set; see eval/README.md"]
async fn eval_prompt_rewrite_fixtures() {
    let Some(api_key) = resolve_api_key() else {
        println!("eval skipped: ANTHROPIC_API_KEY not set (see eval/README.md)");
        return;
    };
    let fixtures = load_fixtures().expect("fixture corpus must load");
    println!("eval: running {} fixtures against the live API", fixtures.len());

    let mut failures: Vec<(String, Vec<String>, String)> = Vec::new();
    for (i, fixture) in fixtures.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(INTER_CALL_DELAY).await;
        }
        let browser_context = fixture.browser_context.as_ref().map(|bc| BrowserContext {
            app: fixture.active_app.clone(),
            url: bc.url.clone(),
            title: bc.title.clone(),
        });
        let result = crate::llm::sonnet_prompt_rewrite(
            &api_key,
            &fixture.transcript,
            &fixture.active_app,
            browser_context.as_ref(),
            fixture.selected_text.as_deref().unwrap_or(""),
            // The eval corpus carries no user profile; the personalisation
            // block is omitted, matching a default install.
            "",
        )
        .await;
        match result {
            Ok(output) => {
                let failed = score_output(&fixture.expect, &output, &fixture.transcript);
                if failed.is_empty() {
                    println!("  PASS {}", fixture.name);
                } else {
                    println!("  FAIL {} ({} assertion(s))", fixture.name, failed.len());
                    failures.push((fixture.name.clone(), failed, output));
                }
            }
            Err(e) => {
                println!("  FAIL {} (API error)", fixture.name);
                failures.push((
                    fixture.name.clone(),
                    vec![format!("API error: {e:#}")],
                    String::new(),
                ));
            }
        }
    }

    let passed = fixtures.len() - failures.len();
    println!(
        "eval summary: {passed}/{} passed ({:.1}%)",
        fixtures.len(),
        100.0 * passed as f64 / fixtures.len() as f64
    );
    if !failures.is_empty() {
        println!("\nfailures:");
        for (name, failed, output) in &failures {
            println!("  {name}");
            for assertion in failed {
                println!("    - {assertion}");
            }
            println!("    output: {}", excerpt(output));
        }
    }
    assert!(
        failures.is_empty(),
        "eval pass rate below 100%: {} of {} fixtures failed (see report above)",
        failures.len(),
        fixtures.len()
    );
}

// ---------------------------------------------------------------------------
// Fast tests: scorers + corpus validation (part of the normal suite)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn expect(route: Route) -> Expect {
        Expect {
            route,
            must_include: vec![],
            must_not_include: vec![],
            max_chars: None,
            max_sentences: None,
        }
    }

    #[test]
    fn contains_ci_is_case_insensitive() {
        assert!(contains_ci("Hello World", "hello"));
        assert!(contains_ci("Hello World", "WORLD"));
        assert!(!contains_ci("Hello World", "bye"));
        assert!(contains_ci("anything", ""), "empty needle always matches");
    }

    #[test]
    fn count_sentences_splits_on_terminators_and_newlines() {
        assert_eq!(count_sentences(""), 0);
        assert_eq!(count_sentences("Hi Karen."), 1);
        assert_eq!(count_sentences("Hi. Bye!"), 2);
        assert_eq!(count_sentences("line one\nline two"), 2);
        assert_eq!(count_sentences("no terminator here"), 1);
        assert_eq!(count_sentences("..."), 0, "no alphanumeric, no sentence");
        assert_eq!(count_sentences("Wait... what?"), 2, "empty segments don't count");
    }

    #[test]
    fn prompt_structure_detects_xml_and_prompt_headings() {
        assert!(has_prompt_structure("<task>Do the thing</task>"));
        assert!(has_prompt_structure("## Context\nSome words"));
        assert!(has_prompt_structure("**Task:** summarise this"));
        assert!(has_prompt_structure("You are a senior lawyer."));
        assert!(!has_prompt_structure("Hi Karen, Thursday at 3 works for me."));
        // Generic markdown headings are legitimate in finished content
        // (GitHub issues, Notion docs) — not a prompt signal on their own.
        assert!(!has_prompt_structure("## Bug\nThe export button crashes."));
    }

    #[test]
    fn task_framing_detects_imperative_openers() {
        assert!(starts_with_task_framing("Summarise this email in two sentences"));
        assert!(starts_with_task_framing("Fix the typo on the login button"));
        assert!(starts_with_task_framing("Please write a poem about the sea"));
        assert!(starts_with_task_framing("Can you explain what an ETF is?"));
        assert!(!starts_with_task_framing("Hi Karen, thanks for the update."));
        assert!(!starts_with_task_framing("Thank you."));
        assert!(!starts_with_task_framing("The migration is done and deploy is green"));
        assert!(!starts_with_task_framing(""));
    }

    #[test]
    fn bare_question_detected() {
        assert!(is_bare_question("What's the difference between TCP and UDP?"));
        assert!(!is_bare_question("Explain TCP vs UDP."));
    }

    #[test]
    fn prompt_route_accepts_structure_framing_or_question() {
        assert!(route_matches(Route::Prompt, "<task>Summarise</task>", "t"));
        assert!(route_matches(Route::Prompt, "Summarise this article.", "t"));
        assert!(route_matches(Route::Prompt, "What is an ETF?", "t"));
        assert!(!route_matches(Route::Prompt, "Hi Karen, 3pm works.", "t"));
    }

    #[test]
    fn content_route_rejects_prompt_shapes() {
        assert!(route_matches(Route::Content, "Hi Karen, 3pm works for me.", "t"));
        assert!(route_matches(Route::Content, "Hey Sarah — is the Figma file final?", "t"));
        assert!(!route_matches(Route::Content, "<task>Write an email</task>", "t"));
        assert!(!route_matches(Route::Content, "Write an email to Karen.", "t"));
    }

    #[test]
    fn passthrough_route_requires_near_verbatim_output() {
        assert!(route_matches(Route::Passthrough, "Thank you.", "Thank you"));
        assert!(route_matches(Route::Passthrough, "", ""));
        assert!(
            !route_matches(Route::Passthrough, "Write a thank-you message to the team.", "Thank you"),
            "a generated prompt is not passthrough"
        );
        assert!(
            !route_matches(Route::Passthrough, &"word ".repeat(50), "hi"),
            "a long output is not passthrough"
        );
    }

    #[test]
    fn score_output_collects_each_failure_kind() {
        let e = Expect {
            route: Route::Content,
            must_include: vec!["karen".into()],
            must_not_include: vec!["here is".into()],
            max_chars: Some(20),
            max_sentences: Some(1),
        };
        let failures = score_output(&e, "<task>Here is the thing. Two sentences.</task>", "t");
        assert_eq!(failures.len(), 5, "every assertion should fail: {failures:?}");

        let passing = score_output(&expect(Route::Content), "Hi Karen, 3pm works.", "t");
        assert!(passing.is_empty(), "clean content should pass: {passing:?}");
    }

    #[test]
    fn score_output_passes_on_full_match() {
        let e = Expect {
            route: Route::Prompt,
            must_include: vec!["summar".into(), "bullet".into()],
            must_not_include: vec!["here is".into()],
            max_chars: Some(500),
            max_sentences: Some(5),
        };
        let failures = score_output(&e, "Summarise this article in three bullet points.", "t");
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn fixture_corpus_is_well_formed() {
        let fixtures = load_fixtures().expect("fixture corpus must parse");
        assert!(
            fixtures.len() >= 24,
            "corpus should hold at least 24 fixtures, found {}",
            fixtures.len()
        );
        let mut names: Vec<&str> = fixtures.iter().map(|f| f.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            fixtures.len(),
            "fixture names must be unique"
        );
        for route in [Route::Prompt, Route::Content, Route::Passthrough] {
            assert!(
                fixtures.iter().any(|f| f.expect.route == route),
                "corpus must cover route {route:?}"
            );
        }
        for f in &fixtures {
            assert!(!f.name.trim().is_empty(), "fixture name must be non-empty");
            for needle in f.expect.must_include.iter().chain(f.expect.must_not_include.iter()) {
                assert!(
                    !needle.is_empty(),
                    "empty substring assertion in fixture {:?} would be meaningless",
                    f.name
                );
            }
            if let Some(max) = f.expect.max_chars {
                assert!(max > 0, "max_chars must be positive in {:?}", f.name);
            }
        }
    }
}

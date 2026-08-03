# Prompt-mode eval harness

Measures the voice-to-prompt pipeline (`llm::sonnet_prompt_rewrite` + the real
`src-tauri/src/prompts/sonnet_prompt.md`, via `include_str!`) against a fixture
corpus, so prompt changes ship on measurements instead of vibes.

The harness is a Rust test inside the existing crate — no production-code
restructuring. It is marked `#[ignore]`, so normal `cargo test` and CI never
run it (CI has no API keys for this, by design).

## Run it

```bash
cd src-tauri && ANTHROPIC_API_KEY=... cargo test -- --ignored eval_prompt --nocapture
```

The key is read from the process environment first; if it's absent, the
repo-root `.env` is loaded (same dev convention as the app itself). With no
key anywhere the test prints `eval skipped` and passes, so `--ignored` suites
stay green without credentials.

The run is serial — one API call at a time with a short delay — and uses the
crate's real retry/timeout behaviour. At the end it prints a per-fixture
PASS/FAIL report and a summary line; the test fails unless the pass rate is
100%, printing each failure's name, failed assertions, and an output excerpt.

**Cost note:** each run is ~30 Sonnet calls (one per fixture), all sharing one
cached system prompt — a few cents per run.

## Fixture format

One JSON file per case in `eval/fixtures/` (numeric prefix groups categories;
files run in filename order):

```json
{
  "name": "slack_standup_update",
  "transcript": "post in the dev channel that the migration is done ...",
  "active_app": "Slack",
  "browser_context": { "url": "https://x.com/home", "title": "Home / X" },
  "selected_text": "text the user had selected, or null",
  "expect": {
    "route": "content",
    "must_include": ["migration"],
    "must_not_include": ["<task>", "here is"],
    "max_chars": 400,
    "max_sentences": 4
  }
}
```

- `browser_context` and `selected_text` may be `null`.
- `expect.route` is one of `prompt` (AI-tool destination → output is a prompt),
  `content` (non-AI destination → output is finished content), or `passthrough`
  (degenerate input → output ≈ cleaned transcript verbatim). Routes are judged
  by deterministic property heuristics, not an LLM.
- `must_include` / `must_not_include` are case-insensitive substrings.
- `max_chars` / `max_sentences` may be `null` (no bound). Sentence counting is
  deliberately naive (split on `.`/`!`/`?`/newline) — give it headroom.

The route heuristics are conservative: they only fail on clear evidence of the
wrong branch and score leniently when unsure (e.g. a bare question passes the
`prompt` route; generic markdown headings are not treated as prompt structure
because GitHub/Notion content legitimately uses them). The sharp signal lives
in the per-fixture substring and length pins — prefer those over route
strictness. Heuristic details are documented on the scorers in
`src-tauri/src/eval.rs`.

## Adding a fixture

1. Copy an existing file, rename with the next number in its category.
2. Write the transcript the way people actually dictate — fillers, run-ons,
   self-corrections — and set `active_app` / `browser_context` to the real
   destination.
3. Assert content words in `must_include`, preamble/structure violations in
   `must_not_include`, and length bounds for terse destinations.
4. Unknown JSON fields are rejected (`deny_unknown_fields`), and a fast
   non-ignored test validates the corpus on every `cargo test` — so a malformed
   fixture fails the normal suite immediately, without an API key.

## A/B-ing a prompt change

1. Baseline: run the eval on `main`, save the summary line and any failures.
2. Make the prompt edit in `src-tauri/src/prompts/sonnet_prompt.md`.
3. Re-run the eval in the same session (the system prompt is prompt-cached for
   5 minutes; wait it out or accept one cache-write on the first call).
4. Compare pass rates and per-fixture diffs. A prompt change that drops the
   pass rate needs either a better prompt or a deliberate fixture update with
   the reasoning in the PR.

## Current corpus (30 fixtures)

- Simple task → Claude (app + browser-tab variants), incl. selected text
- Complex multi-part tasks → Claude / Claude Code (structure markers expected)
- Casual short requests → Cursor (restraint: `max_sentences`)
- ChatGPT (Markdown headings for complex, no XML), Gemini, Perplexity
- Gmail with and without `selected_text` (finished email), Slack (terse)
- LinkedIn, X (280 chars), Notion, GitHub issue
- Degenerate inputs: empty, "Thank you", filler-only, 3-word fragment
- Injection attempts via `selected_text`, background speech in the transcript,
  and a malicious browser-tab title
- Ambiguous destination (defaults to prompt), self-correction, heavy run-ons

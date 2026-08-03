# Contributing to Wisspa

Thanks for your interest. Wisspa is a Tauri 2 app: Rust backend (`src-tauri/`), React/TypeScript frontend (`src/`). Read [`AGENTS.md`](AGENTS.md) first — it documents the architecture, conventions, and gotchas.

## Dev setup

```bash
git clone https://github.com/TechGuiderBB/wisspa.git
cd wisspa
pnpm install
echo "GROQ_API_KEY=..." > .env        # dev only, git-ignored
echo "ANTHROPIC_API_KEY=..." >> .env  # dev only, git-ignored
pnpm tauri dev
```

Prerequisites: macOS 13+ on Apple Silicon, Rust (rustup), Node 20+, pnpm, Xcode Command Line Tools, and your own Groq + Anthropic API keys.

## Before opening a PR

```bash
cd src-tauri && cargo test          # Rust unit tests
cd .. && pnpm build                 # frontend typecheck + build
```

CI runs both (plus gitleaks and `cargo audit`) on every PR.

## Conventions that bite

- **Prompts are files.** Haiku/Sonnet prompts live in `src-tauri/src/prompts/*.md` and are loaded with `include_str!()`. Never inline prompt strings in Rust.
- **Settings schema is mirrored.** Change `src-tauri/src/settings_store.rs` and `src/lib/settings.ts` together.
- **No secrets, ever.** Keys go in `.env` (dev) or macOS Keychain (prod). CI runs gitleaks over full history; pushes with keys will be rejected and the key must be rotated.
- **Keep diffs scoped.** Match existing patterns; don't reformat unrelated code.
- **Docs stay impersonal.** No personal names, machine-local paths, or private business details in committed files.

## Reporting bugs

Open an issue with: macOS version, Wisspa version (About tab), the mode you were using, what you expected, what happened, and any toast/log output (Settings → About → diagnostics export excludes your history and settings).

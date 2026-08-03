# Security Policy

## Reporting a vulnerability

Please do **not** open a public GitHub issue for security vulnerabilities.

Instead, use **GitHub Private Vulnerability Reporting**: the "Security" tab on this repo → "Report a vulnerability".

Include: affected version/commit, steps to reproduce, and the impact you see. We aim to acknowledge within 48 hours.

## Scope notes

Wisspa handles user-supplied API keys (Groq, Anthropic) and executes user-defined voice actions. The areas we most care about:

- API key handling (macOS Keychain storage, `.env` dev fallback, log redaction)
- Prompt-injection boundaries (`<selected_text_untrusted>`, `<browser_context_untrusted>` handling in `src-tauri/src/llm.rs`)
- The shell action allowlist (`src-tauri/src/actions/registry.rs`)
- Synthetic input / clipboard injection (`src-tauri/src/injector.rs`, `src-tauri/src/selection.rs`)

## Supported versions

Only the latest release receives security fixes.

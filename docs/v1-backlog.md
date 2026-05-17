# Wisspa v1 — Backlog (second brain)

> Items flagged during the CLAUDE.md workspace review on 2026-05-15. Captured here so they're visible whenever you (or Claude) open this repo.

## 1. Wire up `destructive: true` + `requires_permissions` enforcement

**Where:** `src-tauri/src/actions/executor.rs::execute`
**Today:** YAML actions can declare `destructive: true` and `requires_permissions: [...]`. Both fields are parsed by `actions/registry.rs` but **not enforced** at execution time.
**Risk:** Someone says "delete my Downloads folder" — voice transcription matches a `shell` action with `destructive: true`, executor runs it silently, no confirmation toast.
**Fix scope:** Low. Branch in the executor: if `destructive: true`, surface a native confirmation toast (already have `toast.rs`); block until user confirms via tray menu or hotkey. Permission gate is a similar pattern.
**Why it's worth doing before v1.0:** First "I lost a folder because of Wisspa" support ticket would be brand-damaging. The plumbing is already there.

## 2. Reconcile `WISSPA_PRD.md` ⇄ `wisspa-product-current-state.md`

**Today:** `WISSPA_PRD.md` (in this repo) is the *original* product PRD. `WisspaWEB/docs/wisspa-product-current-state.md` documents the *actual* shipped behaviour, sourced from this code. **They disagree** in known places:

- Haiku system prompt — code has an extra "CRITICAL: You are a text-cleanup function, NOT an assistant…" paragraph not in the PRD
- `destructive: true` enforcement claimed but not wired (also item #1 above)
- Other deltas listed in `wisspa-product-current-state.md`

**Action:** Pre-launch pass — merge the current-state deltas back into `WISSPA_PRD.md` so external readers (potential partners, hires, investors) see one source of truth.
**Why now:** v1.0 is a credibility moment. Having two contradicting product docs hurts perceived quality.

## 3. Lock the licensing footing for v1.0 — THE BLOCKER

From `WisspaWEB/CLAUDE.md §6 Open Decisions` (also in `LAUNCH.md`):

> Open source (MIT / Apache 2.0), proprietary free-to-use, or paid-only.

**Why this is the gating decision:**

- **Terms of Service copy** (`WisspaWEB/src/app/legal/terms/page.tsx`) — can't write until license is set
- **README license block** in this repo — same
- **GitHub repo visibility** — public source link from marketing site? Yes/no depends on this
- **LemonSqueezy ToS interaction** — paid-only changes the customer agreement shape
- **Community contribution path** — MIT/Apache invites PRs from the start; proprietary closes that door
- **Future enterprise pricing model** — open core vs proprietary changes what you can charge for at the upper tiers

**Three real options:**

| Option | Pros | Cons |
|---|---|---|
| **MIT / Apache 2.0 (full open source)** | Builds community, free distribution channel, lower acquisition friction, future hires can see the code | Anyone can fork; you compete with yourself; harder to charge unless you have a hosted/managed value layer |
| **Proprietary free-to-use** (binary distribution, source closed) | Full pricing control, prevents trivial forks, simplest legal stance | No community, no contributor goodwill, "trust" requires Apple notarisation + reputation |
| **Paid-only (no free tier, 14-day trial)** | Clean revenue signal from day 1, no freemium support burden | Higher friction at the top of funnel; trial-to-paid conversion is everything |

**Brooke's lean** (best guess from your stack/style — pressure-test): **proprietary free-to-use with a paid tier**. Keeps it commercial, defensible, but lowers friction. Open source is a longer-term play once you have a moat.

**Action:** Decide. The marketing site is ready to receive the answer.

---

## Add new items here

When something comes up that's "v1 polish or pre-launch decision", add it below this line so it doesn't get lost in chat.

## 4. `show_desktop` default action ships broken

**Where:** `default-actions/show_desktop.yaml` ships with `command: "fn+f11"`. `src-tauri/src/actions/executor.rs::combo_to_applescript` rejects the `fn` modifier with `"fn modifier is not supported by AppleScript keystroke"`, so the action errors when triggered.
**Two fixes possible:**
- Change the default keystroke to one that doesn't need `fn` (e.g. `cmd+f3` on some keyboard layouts triggers Show Desktop — but layout-dependent), or wire it as an `applescript` action calling Mission Control directly.
- Extend `combo_to_applescript` / `enigo`-based keystroke to support `fn` via `CGEventKeyboardSetUnicodeString` or similar.
**Why bother:** Default actions are the first thing users try after install. A 14-action list with one obviously broken default reads as low quality.



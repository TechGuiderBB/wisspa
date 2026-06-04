# Wisspa v1.0 readiness

> Last updated 2026-06-04. Status: planning.

## TL;DR

App functionality is essentially done (v0.1.0 + #26 through #29 + #35-#39 merged on `main` but unreleased). The licence + Cloudflare Worker + signing + public distribution tracks have not started, and they are the gate to a paid launch. Biggest blocker: Brooke's Apple Developer Program enrolment and Cloudflare/LemonSqueezy account creation, both of which Claude cannot do.

## Scope baseline (from LAUNCH.md)

What "v1.0 ships with" per `LAUNCH.md` "What ships in v1.0":

- Wisspa macOS app (95% there per LAUNCH.md, see git history)
- Signed and notarised `.dmg`, public download
- Auto-update mechanism (already wired)
- 14-day trial with all-inclusive APIs
- $15/mo subscription via LemonSqueezy
- BYO-API-keys power-user mode
- Cloudflare Worker proxy with rate limits

Additional v1.0 line items pulled from `LAUNCH.md` §"Account / service setup checklist" and §"Marketing-page checklist":

- Public download via `TechGuiderBB/wisspa-releases`
- Auto-updater endpoint pointing at the releases repo
- Licence + trial UUID system in the app (`src-tauri/src/license.rs`, Settings → Licence tab, paywall modal)
- Cloudflare Worker at `gateway.wisspa.app`
- LemonSqueezy account with "Wisspa Pro" product
- Landing page at `wisspa.app`
- Legal pages (Privacy Policy, Terms of Service)
- Packaged smoke checks on every release `.dmg`

---

## Status by track

### Track: App functionality (P1 fixes plus #26 / #27 / #28 / #29)

- **Status:** done on `main`, unreleased
- **Evidence:** `git log --oneline main -30` shows the v0.1.0 baseline plus #26 (paste lands in target app, pill follows the cursor), #27 (Quiet notifications toggle), #28 (Prompt Mode reads active browser tab), #29 (LSUIElement so the pill stops disappearing) all merged. Subsequent hardening also merged: #35 (bundle default-actions), #36 (treat Prompt Mode selected text as untrusted), #37 (recording session id), #38 (preserve all clipboard flavors), #39 (file logger with rotation + diagnostics export).
- **Gaps:** No `v0.1.1`+ tag pushed, so none of this is in users' hands. `src-tauri/tauri.conf.json:4` still pins `"version": "0.1.0"`.
- **Next action:** Bump `tauri.conf.json` version and tag a release once the signing and distribution tracks below are in place. Releasing now would publish an unsigned `.dmg`.
- **Owner:** Claude (on greenlight).

### Track: Signed and notarised DMG

- **Status:** not started
- **Evidence:** `.github/workflows/release.yml` lines 87-93 show the `APPLE_*` env block is still commented out. The header comment (lines 16-23) confirms the workflow is waiting on the Developer Program account. `TAURI_SIGNING_PRIVATE_KEY` is already wired (line 76).
- **Gaps:** Apple Developer Program enrolment ($99/yr), Developer ID Application certificate, six `APPLE_*` GitHub Actions secrets, uncommenting the env block.
- **Next action:** Brooke enrols at `https://developer.apple.com/programs/enroll/`. Once active, generate `.p12`, base64 it, add the six secrets per `LAUNCH.md` §"To do — Brooke" item 1. Claude then uncomments lines 88-93 of `release.yml`.
- **Owner:** Brooke (enrolment + secrets), Claude (workflow edit).

### Track: Public download via `wisspa-releases`

- **Status:** not started
- **Evidence:** No `TechGuiderBB/wisspa-releases` repo created yet (cannot verify here, but LAUNCH.md §Distribution describes it as "to be created"). `release.yml` line 98 references `${{ github.repository }}` which points at the source repo, not a separate releases repo.
- **Gaps:** Public repo creation, retargeting `release.yml` to push artefacts to `TechGuiderBB/wisspa-releases` instead of the source repo.
- **Next action:** Brooke creates the empty public repo. Claude updates the `tauri-action` step in `release.yml` to target `TechGuiderBB/wisspa-releases` (likely via a PAT secret + `repo` input on the action).
- **Owner:** Brooke (repo creation), Claude (workflow edit).

### Track: Auto-updater endpoint switch

- **Status:** not started, currently misconfigured
- **Evidence:** `src-tauri/tauri.conf.json:115-117` points `endpoints` at `https://github.com/TechGuiderau/wisspa/releases/latest/download/latest.json`. LAUNCH.md §Distribution specifies it should be `https://github.com/TechGuiderBB/wisspa-releases/releases/latest/download/latest.json`. Two changes needed: org (`TechGuiderau` -> `TechGuiderBB`) and repo (`wisspa` -> `wisspa-releases`).
- **Gaps:** Edit `tauri.conf.json` and rebuild. Existing installed users will continue to ping the old endpoint, but there are none in the wild yet.
- **Next action:** One-line edit to `tauri.conf.json:116` once `wisspa-releases` exists. Tie it to the same release that flips distribution.
- **Owner:** Claude.

### Track: Licence + trial UUID system

- **Status:** not started
- **Evidence:** `src-tauri/src/license.rs` does not exist (verified). No "Licence" tab observed in `src/components/settings/`. No paywall modal in `src/components/`.
- **Gaps:** Everything per LAUNCH.md §"To do — Claude" item 3:
  - `src-tauri/src/license.rs` for Keychain storage of licence key + trial UUID + first-launch timestamp
  - Settings → Licence tab (key entry, status badge, "Subscribe" button)
  - Paywall modal triggered on proxy 402/403
  - Onboarding wizard update to skip API-keys step for trial users
- **Next action:** Scaffold `license.rs` with Keychain entries (`com.techguider.wisspa.license_key`, `com.techguider.wisspa.trial_uuid`, `com.techguider.wisspa.trial_start`). Mirror schema additions in `src/lib/settings.ts`. Build can proceed against a stubbed Worker URL.
- **Owner:** Claude (on greenlight).

### Track: Cloudflare Worker proxy at `gateway.wisspa.app`

- **Status:** not started
- **Evidence:** `gateway/` directory does not exist (verified). `LAUNCH.md` §"To do — Claude" item 2 describes it as to-be-scaffolded.
- **Gaps:** Whole track. TypeScript Worker with:
  - `POST /transcribe` (Whisper STT proxy)
  - `POST /complete` (Anthropic Messages proxy)
  - Bearer-token auth (trial UUID or LemonSqueezy licence)
  - Per-user rate limits (300 dictations / 200K tokens per day per LAUNCH.md §API proxy)
  - Cloudflare AI Gateway forwarding
  - Durable Object for trial UUID + rate-limit state
- **Next action:** Brooke creates Cloudflare account, adds `wisspa.app`, points nameservers, enables AI Gateway. Claude scaffolds `gateway/` with `wrangler.toml` and stubs the routes against a local `wrangler dev`.
- **Owner:** Brooke (account + DNS + AI Gateway), Claude (Worker code).

### Track: App swap to proxy URLs

- **Status:** not started
- **Evidence:** `src-tauri/src/stt.rs` and `src-tauri/src/llm.rs` still call `api.groq.com` and `api.anthropic.com` directly per CLAUDE.md §3. LAUNCH.md §"To do — Claude" item 3 requires swapping to `gateway.wisspa.app` while retaining BYO-keys fallback.
- **Gaps:** Conditional logic: if licence key or trial UUID present and no BYO keys, route via proxy; otherwise direct to upstream.
- **Next action:** After Worker scaffold and `license.rs` exist, edit `stt.rs` and `llm.rs` to branch on licence state.
- **Owner:** Claude.

### Track: LemonSqueezy account + Wisspa Pro product

- **Status:** not started
- **Evidence:** LAUNCH.md §"To do — Brooke" item 3 lists registration, identity verification, product creation, and trial configuration as outstanding.
- **Gaps:** Account creation, business identity verification (passport / company docs), creating the "Wisspa Pro" subscription product at $15 USD/month, configuring trial logic (run trial in-app, use LS for paid only per LAUNCH.md note), generating the checkout URL.
- **Next action:** Brooke registers at `https://app.lemonsqueezy.com/register` and completes identity verification. Verification can take days, so start now.
- **Owner:** Brooke.

### Track: Landing page at `wisspa.app`

- **Status:** in-flight per LAUNCH.md
- **Evidence:** LAUNCH.md §"To do — Brooke" item 6 marks it "In progress. Brooke is building." Sibling repo is `~/dev/GitHub/WisspaWEB` per CLAUDE.md §2 (not verified in this audit).
- **Gaps:** Cannot verify completion of the marketing-page checklist (`LAUNCH.md` §"Marketing-page checklist") from this repo. Required CTAs: download URL pointing at the releases repo, subscribe URL pointing at LemonSqueezy checkout. Both targets do not exist yet.
- **Next action:** Brooke continues build. Once `wisspa-releases` repo and LemonSqueezy checkout URL exist, wire the CTAs.
- **Owner:** Brooke.

### Track: Legal pages (Privacy Policy, Terms of Service)

- **Status:** not started, blocked on licence decision
- **Evidence:** LAUNCH.md row "End-user licence" notes Terms of Use at `wisspa.app/legal/terms` is "currently draft, pending lawyer review". `docs/v1-backlog.md` item 3 lists the licence decision as gating ToS copy, but LAUNCH.md row "Source licensing" records the decision: "Proprietary, all rights reserved" decided 2026-05-17. So the licence question is resolved; ToS still needs drafting plus lawyer review.
- **Gaps:** Privacy Policy and Terms of Service drafts on the marketing site, then lawyer review. LemonSqueezy provides templates per LAUNCH.md §Marketing-page checklist.
- **Next action:** Draft from LemonSqueezy templates, send to lawyer (Olivia at Artemide Law per global CLAUDE.md is a natural first port of call for a quick review).
- **Owner:** Brooke (drafting + lawyer engagement).

### Track: Packaged smoke checks

- **Status:** done as a documented procedure, not yet automated
- **Evidence:** `LAUNCH.md` §"Packaged smoke checks" lists the two checks: count 14 YAML files in the mounted `.dmg`, and fresh-user install confirms 14 defaults in Settings → Actions. The `hdiutil` one-liner is provided.
- **Gaps:** Not wired into `release.yml`. Currently a manual pre-publish step. Acceptable for v1.0 since releases are infrequent.
- **Next action:** Run the smoke checks manually before publishing each release. Optionally add as a post-build step in `release.yml` later.
- **Owner:** Claude (run check on release), Brooke (decide if it should be automated).

---

## Blocking dependencies

Ordered. Skipping any of these blocks subsequent work.

1. **Brooke: Apple Developer Program enrolment.** Without this, no signed `.dmg`, no notarisation, Gatekeeper warns on every install. Friends-and-family testers per the PR #24 commit can tolerate `xattr -d com.apple.quarantine`; paying customers cannot.
2. **Brooke: Create `TechGuiderBB/wisspa-releases` public repo.** Blocks the auto-updater endpoint switch, blocks the public download URL, blocks the marketing-page CTA.
3. **Brooke: Cloudflare account + `wisspa.app` DNS + AI Gateway enablement.** Blocks Worker deploy, which blocks the trial pipeline, which blocks the paywall flow.
4. **Brooke: LemonSqueezy account + identity verification.** Verification can take days. Blocks subscription checkout. Trial logic can be built and tested without this; paid conversion cannot.
5. **Claude: scaffold `gateway/` and `src-tauri/src/license.rs`.** Once 3 and 4 are unblocked, this is ~2 days work per LAUNCH.md "Estimated implementation time once accounts are set up: 2 days."
6. **Brooke: Privacy Policy and Terms of Service published on `wisspa.app/legal/`.** Required before LemonSqueezy will approve the product for paid checkout; also a Cloudflare ToS hygiene matter.
7. **Claude: switch updater endpoint and retarget release workflow.** Trivial once 2 exists.
8. **Brooke + Claude: finalise landing page CTAs.** Final glue step.

---

## Estimated time to ship

Honest breakdown.

**Work Claude can do on greenlight (sequenced after dependencies clear):**

- Scaffold `gateway/` Worker, deploy via wrangler: 1 day
- `src-tauri/src/license.rs` + Settings → Licence tab + paywall modal: 1 day
- Swap STT/LLM call sites to proxy + retain BYO-keys fallback: half day
- Onboarding wizard update for trial users: quarter day
- End-to-end trial expiry test (mock timestamp): quarter day
- Retarget `release.yml` to `wisspa-releases`, switch updater endpoint, bump version, tag: half day

Subtotal Claude work: ~3.5 days focused.

**Work that requires Brooke:**

- Apple Developer enrolment: $99, approval often 1-2 business days but can stretch to a week or more
- Cert generation + 6 GitHub secrets: 1 hour
- `wisspa-releases` repo creation: 5 minutes
- Cloudflare account + DNS + AI Gateway: half day including waiting for nameserver propagation
- LemonSqueezy account + identity verification: hours of his time, days of waiting on LS
- Landing page completion: in-flight, unknown remainder
- Legal pages drafting + lawyer review: 1-3 days depending on Olivia's turnaround

**Realistic wall-clock from greenlight to public paid launch:** 2-3 weeks, dominated by Apple and LemonSqueezy approval queues. Engineering work is ~3.5 days and parallelisable with the waiting.

---

## Recommended next move

**Brooke: start the Apple Developer Program enrolment today.** It is the longest-pole dependency, it costs $99, it cannot be parallelised by Claude, and it unblocks the entire signing + distribution chain. Every other track can wait until enrolment is in motion; nothing else can start without it being in motion.

---

## Open questions for Brooke

- **Annual price.** LAUNCH.md §"Open decisions" lists $144/yr (20% off), $135/yr (25% off), or skip annual for launch. Need a call before LemonSqueezy product setup.
- **Trial email capture.** LAUNCH.md §"Open decisions" recommends anonymous trial (email only at paywall). Confirm so Claude can wire the license.rs flow without an email field.
- **Cloudflare Worker rate limits.** LAUNCH.md §API proxy suggests "300 dictations / 200K tokens per day". Confirm those numbers before the Worker is coded, since changing them after launch is harder once paying users exist.
- **Power-user BYO-keys default.** Should BYO keys be surfaced in onboarding under Advanced, or only discoverable in Settings → API Keys after install? LAUNCH.md §"To do — Claude" item 4 is ambiguous.
- **Privacy Policy review path.** Use LemonSqueezy template + Olivia, or a different lawyer? Cost / turnaround difference is non-trivial.
- **Show the source repo publicly.** LAUNCH.md row "Source licensing" says repo stays private; CLAUDE.md §2 confirms. Re-confirm this hasn't drifted given the licence is "Proprietary, all rights reserved" — no reason to expose source.

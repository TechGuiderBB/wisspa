# Wisspa launch plan

> Living document. Status: planning. Last updated 2026-05-14.

Everything that needs to be true before paying customers can download Wisspa from `wisspa.app`, use it, get billed, and receive updates. Pick up from any section without losing context.

---

## Confirmed decisions

| Decision | Value |
|---|---|
| Product name | Wisspa |
| Domain | `wisspa.app` (registered 2026-05-14) |
| Target platform | macOS 13+ on Apple Silicon (v1) |
| Pricing model | Single tier, all-inclusive APIs |
| Price | **$15 USD / month** |
| Trial | **14 days, all-inclusive, no credit card required** |
| Billing platform | LemonSqueezy (Merchant of Record — handles GST/VAT) |
| API proxy | Cloudflare Worker + Cloudflare AI Gateway |
| Code repo | `github.com/TechGuiderBB/wisspa` (private) |
| Distribution repo | `github.com/TechGuiderBB/wisspa-releases` (public — to be created) |
| Code signing | Apple Developer ID (in progress) |
| Source licensing | **Proprietary, all rights reserved** (see `LICENSE`). Source repo stays private; binary distributed via the public releases repo. Decided 2026-05-17. |
| End-user licence | Terms of Use at `wisspa.app/legal/terms` (currently draft, pending lawyer review) |

### Open decisions still to make

- [ ] **Annual price** (e.g. $144/yr = 20% off, or $135/yr = 25% off, or skip annual for launch)
- [ ] **Family / team plans** — defer to v1.1 unless we hear repeated demand
- [ ] **Lifetime tier** — not recommended; revisit only after a year of subscription data
- [ ] **EDU / OSS discounts** — skip for v1
- [ ] **LemonSqueezy vs Polar.sh** — defaulting to LemonSqueezy unless we discover a reason to switch
- [ ] **Trial email capture** — recommend leaving the trial completely anonymous (download, install, use); only ask for email at the paywall when they choose to subscribe. Maximises conversion vs gated trial.

---

## The user journey

```
┌──────────────────────────────────────────────────────────────────┐
│ wisspa.app  (landing page)                                       │
│                                                                  │
│   [ Download for Mac — 14 days free ]   [ Subscribe — $15/mo ]   │
│           │                                       │              │
│           ▼                                       ▼              │
└─── public .dmg download ──────────── LemonSqueezy checkout ──────┘
            │                                       │
            ▼                                       ▼
  ┌─────────────────────┐               ┌─────────────────────────┐
  │ User installs Wisspa│               │ User pays via card      │
  │ → Applications.     │               │ LemonSqueezy emails     │
  │ Opens app.          │               │ a licence key.          │
  └──────────┬──────────┘               └────────────┬────────────┘
             │                                       │
             ▼                                       │
  ┌─────────────────────────────────┐                │
  │ Onboarding wizard runs.         │                │
  │ App generates a UUID, stores in │                │
  │ Keychain. Calls our proxy →     │                │
  │ trial registered with 14-day    │                │
  │ expiry. User starts dictating   │                │
  │ immediately, no API setup.      │                │
  └──────────┬──────────────────────┘                │
             │                                       │
             ▼                                       │
  ┌─────────────────────────────────┐                │
  │ Day 14: proxy rejects calls.    │                │
  │ Paywall modal:                  │                │
  │   [ Subscribe — $15/mo ] ───────┼────────────────┤
  │   [ Enter licence key ]  ◄──────┼────────────────┘
  └─────────────────────────────────┘
             │
             ▼
  User pastes licence key into Settings → Licence.
  App calls proxy → associates licence with UUID.
  Wisspa unlocks.  Auto-update enabled.
```

---

## Architecture

### Distribution

The `.dmg` is publicly hosted on a **separate public repo**, so the source code repo can stay private:

```
github.com/TechGuiderBB/wisspa-releases   (public, holds .dmg + latest.json)
github.com/TechGuiderBB/wisspa            (private, holds source)
```

**Public download URL** (after first release):
```
https://github.com/TechGuiderBB/wisspa-releases/releases/latest/download/Wisspa.dmg
```

**Auto-updater endpoint** (already wired in `tauri.conf.json`, will switch to):
```
https://github.com/TechGuiderBB/wisspa-releases/releases/latest/download/latest.json
```

The `.dmg` is signed and notarised by Apple Developer ID (once the cert lands).

### API proxy

```
Wisspa app ──auth: license key / trial UUID──► Cloudflare Worker
                                                      │
                                                      ▼
                                            Cloudflare AI Gateway
                                                      │
                                              ┌───────┴──────┐
                                              ▼              ▼
                                            Groq API     Anthropic API
                                          (master key) (master key)
```

The Cloudflare Worker (`gateway.wisspa.app`):

- Accepts `POST /transcribe` (Whisper STT) and `POST /complete` (Anthropic Messages)
- Validates the bearer token: either a trial UUID (registered ≤14d ago) or a LemonSqueezy licence key (active subscription)
- Enforces per-user rate limits (e.g. 300 dictations / 200K tokens per day)
- Forwards through Cloudflare AI Gateway, which adds logging, budget alerts, and (eventually) response caching
- Master Groq / Anthropic keys never leave the Worker

**Power-user override:** Settings → API Keys still accepts user-provided Groq + Anthropic keys. When present, the app bypasses the proxy and calls upstream directly. Same $15/mo subscription either way — they're paying for the *product*, not the API access.

### DNS layout for `wisspa.app`

| Subdomain | Purpose | Hosted on |
|---|---|---|
| `wisspa.app` | Landing page | Cloudflare Pages / Vercel / similar |
| `gateway.wisspa.app` | API proxy Worker | Cloudflare Workers |
| `update.wisspa.app` *(optional)* | Pretty alias for the updater endpoint | redirects to GitHub Releases |

---

## Account / service setup checklist

Items in priority order. Wisspa code work can run in parallel with this.

### Already done

- [x] Domain `wisspa.app` registered
- [x] GitHub org `TechGuiderBB` exists
- [x] Wisspa source repo (private) on GitHub
- [x] Tauri auto-updater plugin wired in app
- [x] Update signing keypair generated (private key at `~/.tauri/wisspa-updater.key`, public key in `tauri.conf.json`)
- [x] CI security workflow (gitleaks, cargo audit, pnpm audit, semgrep)
- [x] Release workflow `release.yml` (builds + signs on `v*` tag push)
- [x] `TAURI_SIGNING_PRIVATE_KEY` uploaded to repo Actions secrets

### To do — Brooke

1. **Apple Developer Program enrolment** ($99 USD/year)
   - Visit https://developer.apple.com/programs/enroll/
   - Once active: generate a **Developer ID Application** certificate, export as `.p12`, set an export password
   - Add 6 secrets to GitHub: `APPLE_CERTIFICATE` (base64 of `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific password from appleid.apple.com), `APPLE_TEAM_ID`
   - Uncomment the `APPLE_*` env block in `.github/workflows/release.yml`

2. **Create `TechGuiderBB/wisspa-releases` public repo** on GitHub
   - Empty repo, public, no README
   - I'll update `release.yml` to target this repo for binaries

3. **LemonSqueezy account** at https://app.lemonsqueezy.com/register
   - Verify business identity (passport / company docs)
   - Create product: "Wisspa Pro", subscription, $15 USD/month
   - Configure trial: 14 days (or run trial in-app and use LS for paid only — see note below)
   - Note: LemonSqueezy's trial feature requires payment method up front; for the friction-free trial we want, we run trial logic *in the app* and only use LS for paid subscriptions
   - Create checkout link → paste into website "Subscribe" button

4. **Cloudflare account** at https://dash.cloudflare.com/sign-up
   - Add `wisspa.app` domain, point nameservers from registrar
   - Enable **AI Gateway** (free tier) — create a gateway, copy its base URL
   - Set per-user budget alerts ($25/month default)

5. **Cloudflare Workers** for the proxy
   - Free tier covers 100K requests/day, well above launch volume
   - Worker code lives in the Wisspa monorepo at `gateway/` (I'll scaffold)
   - Deploy with `wrangler deploy` and a Cloudflare API token

6. **Landing page at `wisspa.app`**
   - In progress. Brooke is building.
   - Required CTAs:
     - **Download for Mac (free 14-day trial)** → `https://github.com/TechGuiderBB/wisspa-releases/releases/latest/download/Wisspa.dmg`
     - **Subscribe — $15/mo** → LemonSqueezy checkout URL
   - Required sections: features, pricing, privacy (lean into "audio never leaves your Mac except to Whisper for STT"), support contact

### To do — Claude (on greenlight)

1. **Switch release workflow** to publish to `wisspa-releases` instead of `wisspa`
2. **Scaffold Cloudflare Worker** (`gateway/`): TypeScript, auth, rate limits, proxy to Groq/Anthropic via AI Gateway, simple Durable Object for trial UUID + rate-limit state
3. **Wisspa app changes:**
   - `src-tauri/src/license.rs` — Keychain storage for licence key + trial UUID + first-launch timestamp
   - Settings → Licence tab with key entry, status badge, "Subscribe" button (opens browser)
   - Paywall modal that triggers when proxy returns 402/403
   - Swap `https://api.groq.com/...` and `https://api.anthropic.com/...` for `https://gateway.wisspa.app/...` (retain BYO-keys fallback)
   - First-launch UUID generation, proxy registration on first transcribe call
4. **Update onboarding wizard** — skip the API-keys step for trial users; surface it under "Advanced" for BYO-keys override
5. **End-to-end test** — install, dictate (trial), wait for expiry (mock by changing trial-start timestamp), see paywall, paste licence, dictate again

Estimated implementation time once accounts are set up: **2 days**.

---

## Cost model (back-of-envelope)

For a typical user dictating ~30 times/day, lightly using prompt mode:

| Cost item | Per day | Per month | Trial (14d) |
|---|---|---|---|
| Groq Whisper | $0.04 | $1.20 | $0.55 |
| Claude Haiku (cleanup) | $0.02 | $0.65 | $0.30 |
| Claude Sonnet (prompt mode) | $0.03 | $1.00 | $0.40 |
| LemonSqueezy fee (~5% + 30¢) | — | $1.05 | — |
| **Total cost per user** | $0.09 | **~$3.90** | **~$1.25** |
| **Revenue per user** | — | **$15.00** | **$0** |
| **Gross margin per user** | — | **~74%** | (acquisition cost) |

A heavy user (3× normal) is still ~50% margin. Hard rate limits prevent runaway costs even on edge cases.

Break-even on trial: if 8.4% of trials convert, you've covered trial API spend. Industry benchmark for productivity SaaS trials is 15–25% — well above break-even.

---

## What ships in v1.0 (this plan)

- Wisspa macOS app (already 95% there — see git history)
- Signed + notarised `.dmg`, public download
- Auto-update mechanism (already wired)
- 14-day trial with all-inclusive APIs
- $15/mo subscription via LemonSqueezy
- BYO-API-keys power-user mode
- Cloudflare Worker proxy with rate limits

## What's deferred to v1.1+

- **Wake-word listening** ("Hey Wisspa") — fully planned (see `~/.claude/plans/you-are-building-wisspa-cuddly-floyd.md`, "Plan B" section). 2–3 days work.
- **In-GUI action editor** — currently YAML files; nice-to-have, not blocking.
- **Family / team plans, EDU discount, lifetime tier**
- **Windows / Linux support** — explicitly v2 per PRD §12.

---

## Marketing-page checklist (for `wisspa.app`)

Stuff that matters for conversion when the site goes live. Brooke is owning the build; I can review copy/structure.

- [ ] Big headline + one-sentence pitch
- [ ] 20-second demo GIF or video (essential — voice products are very hard to convey in text)
- [ ] Three-pillar features section (Dictate / Act / Prompt)
- [ ] Pricing block: $15/mo, 14-day free trial, no credit card to start
- [ ] Trust / privacy section: what audio is captured, what leaves the machine, where keys are stored
- [ ] FAQ (mic permissions, "is my voice stored?", what apps it works with)
- [ ] Footer: support email, links to legal pages, GitHub badge if/when public
- [ ] Legal pages (Privacy Policy, Terms of Service) — LemonSqueezy provides templates
- [ ] **Download** CTA links to GitHub Releases direct-download URL
- [ ] **Subscribe** CTA links to LemonSqueezy checkout
- [ ] OG tags + social preview image (use the Wisspa W mark on a gradient)
- [ ] No tracking scripts that require a cookie banner — keep it clean

---

## Risks + mitigations

| Risk | Mitigation |
|---|---|
| Trial abuse (signups with fresh UUIDs to get unlimited free API access) | Cloudflare Worker can fingerprint by device + ASN; rate limit per IP block; LemonSqueezy email verification at paywall |
| Whisper hallucinations on silent input | Already mitigated (silence guard + denylist + Haiku prompt hardening) |
| API price changes (Groq/Anthropic raise prices) | Per-user rate limits give buffer; could move to BYO-only if margins compress |
| LemonSqueezy outage | Cache last-validated licence locally for 7 days; users keep working through transient failures |
| Apple Developer cert expires | Set calendar reminder at month 11; renewal is ~10 min |
| Refund disputes | LemonSqueezy handles; refund policy publicly stated as "30-day full refund, no questions" — generous, low-friction |

---

## Index of related docs

- `WISSPA_PRD.md` — original product requirements
- `DECISIONS.md` — minor architectural calls made during build
- `README.md` — dev setup + how to build locally
- `~/.claude/plans/you-are-building-wisspa-cuddly-floyd.md` — Plan A (silence resilience, ✅ shipped) and Plan B (wake-word, planned)

When ready to start executing Plan C, ping me and we'll work through the to-do lists in order.

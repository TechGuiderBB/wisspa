# Loop audit — wisspa-user-feedback-fold-in

> 2026-06-04. Audit of the wisspa-user-feedback-fold-in loop after tick digests were found writing into the repo working tree despite a documented pause.

## TL;DR

The loop was paused on 2026-05-25 ("no Wisspa users yet, re-activate at launch") with documented re-activation criteria. It is currently running anyway (tick count 59, last tick 2026-06-04 08:30 UTC). Neither macOS crontab nor Hermes cron lists it as scheduled, so the runner is unknown. The loop writes its state into the repo working tree (`.loop-state-user-feedback.json`, `.tick-digest-*.md`) because `repo_root` in the config points at `/Users/agentbb/dev/GitHub/wisspa`. Recommend re-enforcing the pause and moving state writes out of the repo.

## Evidence

### The pause decision

`~/Vault/01-Projects/_shared/loops/2026-05-25 Paused — wisspa-user-feedback-fold-in.md` documents:

- Paused 2026-05-25 with `# PAUSED 2026-05-25 (no Wisspa users yet — re-activate at launch)` cron-comment intent.
- Reason: no real signal flowing (no GitHub issues, no HN/Reddit/ProductHunt mentions, Gmail MCP unavailable, no in-app widget).
- ~$60/mo wasted on null ticks. Brooke verbatim: "Probably don't need it right now because we don't have any users."
- Re-activation triggers (ALL required):
  - Public launch shipped (semgrep green + landing page live + first ~50 users).
  - At least one feedback channel producing real signal.
  - Gmail MCP gap resolved.

### What's actually happening

- `crontab -l | grep wisspa-user-feedback`: no entries (commented or otherwise).
- `hermes cron list`: 3 jobs total (morning-briefing, end-of-day-debrief, weekly-audit). None are this loop.
- `~/loops/state/wisspa-user-feedback-fold-in.json` shows `tick_count: 59`, `last_tick: 2026-06-04T08:30:00Z`, `last_status: ok`. Modified today.
- Repo working tree contains `.tick-digest-2026-05-31.md` and `.tick-digest-2026-06-03.md` and `.loop-state-user-feedback.json`. Both digests confirm the loop ran in blocked state (Gmail/Slack MCP not registered, no signal collected).
- One feature-request captured to date: `vocab-custom-words-001` (signal from Brooke's own PR #18, sub-threshold, single occurrence).

### Where the runner is hiding

Unknown. Three candidates worth checking:

1. A launchd plist (`launchctl list | grep -i wisspa`).
2. A second crontab under a different user (`sudo crontab -l` if applicable).
3. A loop-orchestrator script that watches `~/loops/state/` and reschedules without using cron or Hermes.

Whichever it is, it has bypassed the pause decision.

### Why state writes leak into the repo

`~/Vault/01-Projects/_shared/loops/Loop-Configs/wisspa-user-feedback-fold-in.yaml` line 23:

```yaml
repo_root: /Users/agentbb/dev/GitHub/wisspa
```

The loop runner uses `repo_root` as the default write directory for state and digests. Hence the dotfile drift into the repo.

## Options

### A. Honour the pause decision (recommended)

Stop the loop, leave state preserved, wait for the documented re-activation triggers.

1. Find the runner (`launchctl list | grep wisspa`, `ps aux | grep wisspa-user-feedback-fold`, `find ~/loops/bin -newer ~/loops/state/wisspa-user-feedback-fold-in.json`).
2. Disable it (`launchctl unload <plist>` or comment out / remove the cron entry that's actually firing).
3. Add an audit-trail entry to the Vault decision file noting that the loop was found running on 2026-06-04, the runner was identified, and the pause was re-enforced.
4. Move `repo_root` out of the wisspa repo to `~/loops/state/wisspa-user-feedback-fold-in/` so future ticks (when re-activated) don't pollute the working tree.

**Why this is the right call:** the original analysis (no users, no signal, $60/mo wasted) hasn't changed. The launch is still pending. Zero feature-requests captured in 27+ ticks pre-pause and another 32 ticks post-pause. The single feature request that did surface came from Brooke's own PR commits, which we can capture without a clustering loop.

### B. Redesign for the pre-launch reality

Acknowledge the loop has been running anyway, narrow its scope to what's actually achievable today, and let it continue at a lower cadence.

Changes:
- Drop Gmail/Slack MCP dependencies (none are registered).
- Source signal from: GitHub issues + PRs in `TechGuiderBB/wisspa`, GitHub issues + PRs in `TechGuiderBB/WisspaWEB`, and web search for `wisspa` mentions.
- Cadence: weekly instead of every 6h. One tick on Friday mornings into the existing `weekly-audit` Hermes job.
- Output: append to a doc, no Slack routing until a feedback channel actually exists.
- Move `repo_root` out of the wisspa repo as above.

Cost: ~$5/mo instead of ~$60/mo. Captures any real signal that does appear (PR-body feedback, web mentions) without re-introducing the MCP dependencies.

### C. Retire entirely until launch + 50 users

Delete the loop config, state file, and any orchestration entry. Re-create from scratch at launch, when the architecture is fresh.

Cost saving: full ~$60/mo. Risk: any signal during the pre-launch quiet period is dropped on the floor. Probably acceptable given there is no signal.

## Recommendation

**A first, then a planned move to B at v0.4.0 readiness.**

- Re-pause cleanly today. Cost recovered, working-tree pollution stopped.
- Plan B as a pre-launch capability so that on day 1 of paid availability, the loop is ready to fold in real signal (GitHub + web only at first; add Gmail + Slack once those MCPs land).

## Concrete next actions

1. Identify the runner (`launchctl list | grep -i wisspa`, then `ps aux` if not present, then check for any loop-runner wrapper that might re-add cron entries).
2. Disable it.
3. Edit `~/Vault/01-Projects/_shared/loops/Loop-Configs/wisspa-user-feedback-fold-in.yaml`: set `repo_root` to `~/loops/state/wisspa-user-feedback-fold-in/` or similar.
4. Move existing `.tick-digest-*.md` and `.loop-state-user-feedback.json` out of the wisspa repo working tree (already covered in this PR's `.gitignore` update so they can't drift into future commits).
5. Append an audit note to `~/Vault/01-Projects/_shared/loops/2026-05-25 Paused — wisspa-user-feedback-fold-in.md` capturing what was found and re-enforced.

Brooke decides on 1-5. Claude can execute 1-4 on greenlight. Step 5 (Vault edit) belongs to Brooke.

## Open questions for Brooke

- Where do you think this loop is being scheduled from? launchd? a watcher script? something Hermes-adjacent?
- Are you happy with Option A (re-pause, plan B for v0.4.0), or do you want the redesign now?
- The single captured feature request `vocab-custom-words-001` (custom vocabulary / word substitution) — is this still on the v1.0+ radar? Worth a stand-alone issue regardless of the loop's fate.

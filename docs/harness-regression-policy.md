# Harness regression policy

The EE driver-only harness is the baseline used to prove proxy behavior. Treat it
as a separate test fixture from packet translation work.

## Automation live-capture cadence

The recurring proxy2 automation must keep real traffic evidence fresh. Every
run must first check the newest real live HG capture produced by a harnessed
connection. A capture counts as current only if it reached gameplay and is no
more than 24 hours old. If the newest gameplay-reaching capture is older than
24 hours, missing, or failed before gameplay, run a fresh live HG harness
capture before ordinary proxy work. If the previous run did not reach gameplay,
fix or instrument that harness/server-connection blocker first and rerun.

For real HG/Diamond source traffic, use:

```powershell
.\tools\build-diamond-probe.ps1 -Configuration Release
.\tools\test-diamond-client-capture.ps1 -Server 213 -Account 5 -RunRoot C:\nwnbridge\<descriptive-run>
```

For every harness run, record the run root, probe log, packet-dump directory,
packet count, furthest observed stage, and whether the run reached gameplay,
module load, character vault, or only BN/login/vault traffic. A launch that
produces only early BN or vault packets is useful evidence, but it does not
count as a gameplay replay for live-object/placeable work.

If unattended automation stalls before character/module entry, fix or instrument
the harness as the next production slice before continuing packet-family work.
The 2026-06-25 manual review run
`C:\nwnbridge\codex-review-diamond-client-20260625-174949` proved the Diamond
capture path still records real HG traffic, but also showed the auto-character
step can fire while the PRE_PLAYMOD list is still empty.

Latest known live HG status, as of 2026-07-03 07:13 +10: the current
gameplay-reaching capture is
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`, with packet dumps
under `diamond-client-packets`, probe log `diamond-client-probe.log`, 219
packet files, and packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`.
Gameplay was reached through BNVR/PRE_PLAYMOD, tempclient BIC read, repeated
HG `P/05/01` live-object traffic, and in-game chat/reset messages; at the
2026-07-03 07:13 +10 gate, the newest gameplay packet was about 16 hours old
and still inside the 24-hour freshness window. The first
strict proxy2 replay
`C:\nwnbridge\codex-proxy2-replay-fresh-live-20260702-20260702-151020`
reported 414 strict allows, 0 strict quarantines, but 1 semantic quarantine and
2 live-object quarantine bins from sequence 179: a mixed inventory Feature-25,
placeable add, and terminal all-bits `U/09 0xFFFFFFF7` placeable update stream.
The 2026-07-02 production fix teaches the compact tail9 door/placeable updater
to remove the exact terminal six legacy packed-name fragment bits only after
the bounded tail9 name payload is proven. Confirming strict replay
`C:\nwnbridge\codex-proxy2-replay-fresh-live-fixed-20260702-153427` reported
414 strict allows, 0 strict quarantines, 0 semantic quarantine matches, 0
quarantine files, 27 live-object exact shape/rewrite matches, 147 exact
lifecycle claim matches, 39 stream-probe quickbar summaries, and 1 committed
quickbar rewrite summary. The committed quickbar still has 0 item buttons, 29
blank slots, 5 spell slots, and 2 preserved general buttons; the next useful
capture pressure remains item-bearing quickbar materialization or the next
fresh live-object exact-shape gap.

Update as of 2026-07-01 11:45 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-item-decision-automation-20260701-114413`
against the same fresh capture stayed at 0 quarantines, 308 strict allows, 79
direct live-object frames, 19 exact live-object rewrites, 98 exact lifecycle
claim summaries, 10 area rewrites, and 1 committed quickbar rewrite summary.
Production quickbar logs now emit a committed item materialization decision
trace for every parsed item button, and the replay summary exports
`QuickbarItemDecisionTraceMatches`, `QuickbarItemDecisionsAccepted`, and
`QuickbarItemDecisionsRejected`. This capture still carries no committed
quickbar item buttons, so all three new decision counters were 0.

Update as of 2026-07-01 12:45 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-shape-status-automation-20260701-124219`
against the same fresh capture stayed at 0 quarantines, 308 strict allows, 79
direct live-object frames, 19 exact live-object rewrites, 98 exact lifecycle
claim summaries, 10 area rewrites, and 1 committed quickbar rewrite summary.
The production quickbar writer now uses one typed item-object shape classifier
for emission, missing-state diagnostics, and item-decision trace labels. The
item-decision trace also records base item, appearance type/length, and
active-property presence/count for primary and secondary item objects. This
capture still carries no committed quickbar item buttons, so item-decision
counts remain 0 until an item-bearing `SetAllButtons` stream is captured or
replayed.

Update as of 2026-07-01 13:47 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-materialization-helper-automation-20260701-1350`
against the same fresh capture stayed at 0 quarantines, 308 strict allows, 79
direct live-object frames, 19 exact live-object rewrites, 98 exact lifecycle
claim summaries, 10 area rewrites, and 1 committed quickbar rewrite summary.
The M-frame quickbar materialization helper now shares semantic item-proof
status/proof mapping between direct dispatch and buffered zlib-stream handling.
Context-aware quickbar stream probes logged 39 `committed=false` summaries, and
only the final emitted quickbar rewrite logged `committed=true`. The committed
quickbar still has 0 item buttons, 29 blank slots, 5 spell slots, and 2
preserved general buttons, so the next useful capture remains an item-bearing
`SetAllButtons` stream after verified Feature-25 refs.

Update as of 2026-07-01 14:48 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-probe-counters-automation-20260701-1448`
against the same fresh capture stayed at 0 quarantines, 308 strict allows, 79
direct live-object frames, 19 exact live-object rewrites, 98 exact lifecycle
claim summaries, 10 area rewrites, 39 stream-probe quickbar summaries, and 1
committed quickbar rewrite summary. Quickbar summaries now include
`slot_records_owned`, and the replay harness exports stream-probe counters
separately from committed counters. The committed rewrite owned all 36 slot
records and still had 0 item buttons, 29 blank slots, 5 spell slots, and 2
preserved general buttons; stream probes also saw 0 item buttons. The next
useful capture remains an item-bearing `SetAllButtons` stream after verified
Feature-25 refs.

Update as of 2026-07-01 16:14 +10: live-data gate used the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
`2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, with the newest packet about
9h04m old at gate time and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-split-shadow-state-automation-20260701-161120`
stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
rewrites, 39 stream-probe quickbar summaries, and 1 committed quickbar summary.
The committed quickbar still had 0 item buttons, 29 blank slots, 5 spell slots,
and 2 preserved general buttons. Split inflated stream rewriting now shadows
semantic object state and refreshed area context between claimed units, so an
earlier same-buffer area reset or state-bearing unit can affect later quickbar
or live-object translation without mutating the real session state before the
accepted-payload reducer runs.

Update as of 2026-07-01 16:48 +10: live-data gate used the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
`2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, with the newest packet about
10h13m old at replay time and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-slot-profile-state-automation-20260701-1649`
stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
rewrites, 39 stream-probe quickbar summaries, and 1 committed quickbar
summary. Committed quickbar semantic state now stores an exact-reader slot
profile separately from placeholder frames; this replay recorded 36 slots, 29
blanks, 5 spells, 2 general buttons, 0 items, and 7 visible first-page slots.
The capture still carries no committed item buttons, so the next useful live or
local evidence remains an item-bearing `SetAllButtons` stream after verified
Feature-25 refs.

Update as of 2026-07-02 16:17 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-feature25-materialization-state-automation-20260702-1605`
against the current fresh capture stayed at 0 quarantines, 414 strict allows,
27 exact live-object rewrites, 147 exact lifecycle claim summaries, 39
stream-probe quickbar summaries, and 1 committed quickbar summary. Semantic
item-proof state now logs whether exact Feature-25 refs were already backed by
item materialization before the Feature-25 proof is inserted. In this capture,
the generic live-object exact trace counted 17 first-list refs and 1 second-list
ref as materialized, but the item-specific semantic trace counted 17 first-list
and 21 second-list refs as deferred item refs. The committed quickbar still has
0 item buttons, so the next useful capture remains an item-bearing
`SetAllButtons` stream that can prove or disprove relying on deferred
Feature-25 refs for compact item-slot emission.

Update as of 2026-07-02 17:23 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 2h15m old at gate time, gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-registry-context-automation-20260702-171938`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
exact lifecycle claims, 39 stream-probe quickbar summaries, and 1 committed
quickbar summary. Proxy2 now logs the semantic registry item-proof context
beside registry-backed `GuiQuickbar_SetAllButtons` materialization, and the
replay summary exports those counters. This replay recorded 1 committed
registry-context summary, 0 stream-probe registry-context summaries, 0
committed quickbar item buttons, and 0 active/materialized/Feature-25 item refs
in the registry at committed rewrite time. The next useful capture remains an
item-bearing `SetAllButtons` stream with non-empty registry item context.

Update as of 2026-07-02 18:16 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 3 hours old at gate time, gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-split-context-automation-20260702-1816`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
exact lifecycle claims, 39 stream-probe quickbar summaries, 39 stream-probe
registry-context summaries, and 1 committed quickbar summary. Split-time
`GuiQuickbar` probes now receive the same registry-backed materialization
context used by committed rewrites, so the replay harness can report registry
item context before a stream is finally claimed. This capture still has 0
committed or stream-probe item buttons and 0 active/materialized/Feature-25 item
refs at quickbar probe/rewrite time; the next useful capture remains an
item-bearing `SetAllButtons` stream with non-empty registry item context.

Update as of 2026-07-02 19:12 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 4 hours old at gate time, gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-proof-summary-automation-20260702-191159`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
exact lifecycle claims, 39 stream-probe quickbar summaries, 39 stream-probe
registry-context summaries, and 1 committed quickbar summary. Proxy2 now emits
unique direct item-proof objects, unique Feature-25 item-proof objects, and
their compact item-emission proof union in the quickbar registry-context trace;
the replay summary exports committed and stream-probe max counters for those
fields. This capture still has 0 quickbar item buttons and 0 compact
item-emission proof objects at quickbar probe/rewrite time, so the next useful
capture remains an item-bearing `SetAllButtons` stream with nonzero proof
context.

Update as of 2026-07-02 20:15 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 5 hours old at replay time, gameplay reached. Strict
replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-ui-context-automation-20260702-2007`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
registry-context summary. Committed quickbar semantic state now records the
registry item-proof context alongside the exact slot profile; this capture
still recorded 36 slots, 29 blanks, 5 spells, 2 general buttons, 0 item slots,
and 0 compact item-emission proof objects. The next useful capture remains an
item-bearing `SetAllButtons` stream with nonzero committed proof context.

Update as of 2026-07-02 21:19 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 6 hours old at replay time, and gameplay reached. Strict
replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-proof-partition-automation-20260702-2119`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
registry-context summary. Quickbar registry-context traces and
`replay-summary.json` now expose direct-only, Feature-25-only, and shared
compact item-emission proof object counters. This capture still has 0 quickbar
item buttons and all three partition counters remain 0 at quickbar
probe/rewrite time, so the next useful capture remains an item-bearing
`SetAllButtons` stream with nonzero partitioned proof context.

Update as of 2026-07-02 22:17 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 7 hours old at replay time, and gameplay reached. Strict
replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-prior-context-automation-20260702-2218`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe quickbar summaries, and 1 committed quickbar
summary. Semantic quickbar state now records and summarizes the last relevant
inventory item context before committed quickbar profiles. In this capture the
committed quickbar still occurs before the later retained Feature-25 item
context, so `QuickbarSemanticPriorItemContextKnown=0`, all prior proof counters
are 0, and the next useful capture remains a later item-bearing
`SetAllButtons` after those Feature-25 refs.

Update as of 2026-07-02 23:19 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 8 hours old at replay time, and gameplay reached. Strict
replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-post-context-automation-20260702-2319`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
quickbar summary. Semantic quickbar state now separately records item context
that appears after a committed quickbar. This capture still has 0 quickbar item
buttons, but the new post-context summary reports 37 post-quickbar updates and
5 compact item-emission proof objects, all Feature-25-only. The next useful
capture remains a later item-bearing `SetAllButtons` after those post-quickbar
Feature-25 refs.

Update as of 2026-07-03 00:18 +10: live-data gate reused the same
gameplay-reaching HG capture
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest packet about 9 hours old at gate time, and gameplay reached. Strict
replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-previous-post-context-automation-20260703-0018`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
quickbar summary. Semantic quickbar state now snapshots the previous
post-quickbar item-context window when a later committed quickbar arrives, and
the replay summary exports previous-post counters. This capture still has one
committed quickbar, so previous-post counters stay 0 while post-context remains
37 updates and 5 compact item-emission proof objects, all Feature-25-only. The
next useful capture remains an item-bearing later `SetAllButtons` after those
post-quickbar Feature-25 refs.

Update as of 2026-07-03 01:13 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
`2026-07-03T01:09:21+10:00`, the newest gameplay packet was about 10 hours old
and gameplay had been reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-proof-class-automation-20260703-0113`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe quickbar summaries, 39 stream-probe
registry-context summaries, and 1 committed quickbar summary. The committed
quickbar still has 0 item buttons; post-quickbar context remains 37 updates and
5 compact item-emission proof objects, all Feature-25-only. The proxy2 writer
now keeps compact quickbar item emission bounded to registry-state proof
classes, so `ExplicitSelfMaterialization` cannot satisfy compact byte-owned
item slots. The next useful capture remains a later item-bearing
`GuiQuickbar_SetAllButtons` after those Feature-25 refs.

Update as of 2026-07-03 02:18 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest gameplay packet about 11 hours old, and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-best-context-automation-20260703-0218`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
quickbar summary. Semantic committed-quickbar traces now export the best
available item-proof context at commit time and its source. In this capture the
single committed quickbar still occurs before item proof, so
`QuickbarSemanticBestItemContextKnown=0`; post-quickbar context still reaches 5
compact item-emission proof objects, all Feature-25-only. The next useful
capture remains a later item-bearing `GuiQuickbar_SetAllButtons` after those
Feature-25 refs.

Update as of 2026-07-03 03:18 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest gameplay packet about 12 hours old, and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-pending-refresh-automation-20260703-031344`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
quickbar summary. Semantic quickbar state now explicitly records whether
post-committed compact item proof is pending a later item-bearing quickbar;
this capture has one committed quickbar before item proof, so the pre-commit
pending counter is 0 while post-context pending is 37 updates and 5 compact
proof objects, all Feature-25-only. The next useful capture remains a later
item-bearing `GuiQuickbar_SetAllButtons` after those pending Feature-25 refs.

Update as of 2026-07-03 04:20 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; newest gameplay
packet was about 13 hours old, and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-refresh-outcome-automation-20260703-0418`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
quickbar summary. Semantic committed-quickbar traces now export pending-refresh
outcomes: no pending window, pending but still blank, or pending emitted item
slots. This capture still reports only
`QuickbarSemanticPendingItemRefreshOutcomeNoPending=1` before the post-quickbar
Feature-25 proof window; no later blank or item-slot refresh outcome exists yet.
Post-context remains 37 updates and 5 compact proof objects, all
Feature-25-only. The next useful capture remains a later committed
`GuiQuickbar_SetAllButtons` after those pending Feature-25 refs.

Update as of 2026-07-03 05:17 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
`2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
newest gameplay packet about 14 hours old, and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-pending-proof-class-automation-20260703-051647`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
quickbar summary. Semantic quickbar traces now export the pending refresh proof
class. This capture has one no-pending committed quickbar, then 37 post-context
pending updates, all `feature25_only`, reaching 5 compact item-emission proof
objects and 0 direct/shared proof objects. The next useful capture remains a
later committed `GuiQuickbar_SetAllButtons` after those pending Feature-25 refs.

Update as of 2026-07-03 06:21 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at gate time the
newest gameplay packet was about 15 hours old and gameplay reached. Strict
replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-unresolved-refresh-automation-20260703-062111`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, 39 stream-probe registry-context summaries, and 1 committed
quickbar summary. Semantic quickbar state now counts verified events while a
post-committed item refresh remains pending; this replay reported
`QuickbarSemanticPostItemRefreshPendingEvents=265`, all after Feature-25-only
compact item proof and with no later committed quickbar. The next useful
capture remains a later committed `GuiQuickbar_SetAllButtons` after that
pending window, or harness/client instrumentation that deliberately provokes
that refresh.

Update as of 2026-07-03 07:19 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-event-breakdown-automation-20260703-071923`
against the same fresh gameplay capture stayed at 0 quarantines, 414 strict
allows, 27 exact live-object rewrites, 147 lifecycle claims, 39 stream-probe
registry-context summaries, and 1 committed quickbar summary. Semantic
quickbar state now buckets verified traffic while a post-committed item refresh
remains pending. The pending Feature-25-only window still has no later
committed quickbar or item buttons, and spans 265 verified events: 127
live-object, 0 quickbar, 0 area, 0 inventory, 1 client input, 4 chat, and 133
other. The next useful step is harness/client control that deliberately
provokes a later committed `GuiQuickbar_SetAllButtons` after this pending
window.

## Successful live HG capture contract

A successful live HG capture requires all of the following:

- Run from the populated checkout, currently
  `D:\Codex Projects\NWN EE Bridge`; fail visibly if `.git`, `Cargo.toml`, or
  `proxy2` is missing.
- Build the Diamond probe successfully in Release mode.
- Launch `tools\test-diamond-client-capture.ps1` against server `213` with
  account `5` and a timestamped `C:\nwnbridge\<descriptive-run>` run root.
- Use the established Diamond profile files under `C:\NWN\Config` and the
  Diamond install under `C:\NWN\NWN Diamond`.
- Reach the real HG endpoint for server `213` (`158.69.144.21:5133`) and get
  past BN/login/vault traffic into character/module selection and gameplay.
- Write a probe log plus packet files under the run root, then record the run
  root, log path, packet directory, packet count, furthest stage, timestamp,
  and whether gameplay was reached.

"Reached gameplay" means the capture advanced beyond BN/login/vault traffic and
character/module selection into an area/gameplay state with gameplay packet
evidence, such as area/module load completion, live-object traffic, or another
clearly documented in-world signal. BN/login/vault-only traffic is useful for
debugging, but it is not gameplay evidence for live-object or proxy-completion
work.

## Known harness issues

| Symptom | Likely cause | Response |
| --- | --- | --- |
| Automation starts in an empty Google Drive folder | Wrong cwd | Switch to `D:\Codex Projects\NWN EE Bridge` and fail visibly if the populated checkout is absent. |
| Packet dumps stop at BN/login/vault traffic | Harness did not reach character/module/gameplay | Treat as a harness blocker, record the stage, and fix or instrument the connection path before unrelated proxy work. |
| Capture reaches `BNVR A` and one `P/01/03` response, but never sends client `P/11/01` | Driver fell back to native DirectConnect after missing or discarding the server-list path | Keep using the server-list DirectConnect path; if Diamond's app-state server-list slot is empty, retry with the remembered `SERVERLIST_PANEL` from the constructor hook before native fallback. |
| `PRE_PLAYMOD` selection fires with `entries=0 count=0` | Auto-character path is too early or lacks refresh/retry | Add wait/refresh/retry instrumentation and rerun until the character list is populated or a new blocker is proven. |
| Player-password prompt or native connect overlay appears | Harness regressed to the wrong login path or password handling | Keep the old driver connect path; do not pass native `+password`; seed the player password internally with default `A`. |
| No probe log or packet directory is written | Probe build/injection/run-root setup failed | Rebuild the probe, check run-root permissions, and verify the Diamond process was injected before calling the run useful. |
| HG endpoint is unreachable or the server is down | External live-server blocker | Record the exact network/server failure and retry later; do not claim fresh gameplay evidence. |
| Strict replay fails before launch with `Access is denied` while replacing `target\debug\hgbridge_proxy2.exe` | A stale replay proxy is still holding the debug executable | List `hgbridge_proxy2.exe` processes, stop only the stale debug replay process, or pass `-ProxyExe` with an isolated build output. Leave unrelated live/public proxy processes alone. |
| Strict replay reaches only part of a long capture before the automation timeout, often during `drain dummy server` | Empty UDP receive waits are too expensive for 3k+ packet captures | Use `-DrainReceiveTimeoutMilliseconds 5` or another bounded value for automation replays; keep the default higher value for manual diagnosis when delayed UDP output is under investigation. |

Rules:

- Do not change harness launch, auto-connect, password, or auto-character logic
  in the same commit as proxy packet/resource translation work.
- Default harness runs should continue to use the old internal driver connect
  path, not native `+connect`.
- Driver-only harness runs should not pass native `+password`; the bridge seeds
  the EE player-password state internally. The default player password is `A`.
- The default automated character remains `starcore-druid60` on player account
  `starcore5`.
- When harness code changes, run a focused harness baseline before resuming
  proxy packet work. At minimum, confirm the client reaches area loading through
  the proxy without a player-password prompt or failed native connect overlay.
- If proxy work appears broken, reproduce once with an unchanged harness before
  editing harness code. This keeps packet regressions and harness regressions
  separable.

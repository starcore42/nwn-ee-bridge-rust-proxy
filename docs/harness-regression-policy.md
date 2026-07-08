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

Latest known live HG proxy status, as of 2026-07-09 04:57 +10: the freshest
gameplay-reaching proxy harness is
`C:\nwnbridge\codex-live-claim-neighborhood-inventory-20260709-045231\harness-proxy-20260709-045344`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, observed
`BNK3` after deferred `BNK2`, reached gameplay through `Module_Loaded`,
`Area_ClientArea`, proxy-generated `Area_AreaLoaded`, the post-area hold gate
opening, held post-area packet release, and sustained `GameObjUpdate_LiveObject`
traffic. It wrote `quickbar-item-refresh-hint.json` through about
`2026-07-09T04:57:12+10:00` and produced no quarantine directory. The forced
inventory action did not expose a server `Inventory` handoff this time; it
settled at `inventory_equipment_bridge_output_status="awaiting_client_gui_writer"`
with `inventory_equipment_bridge_output_last_decision_reason="deferred_client_gui"`,
`inventory_equipment_bridge_output_requires_client_gui_writer=true`, 5
`ClientGuiInventory` handoff events, 1 ready `ClientGuiInventory` handoff, 0
server `Inventory` handoffs, candidate `0x80015211`, and a status/self
client-GUI claim object `0x7F000000`. No synthetic inventory output was queued.

As of 2026-07-09 05:12 +10, proxy2 has exact decompile-backed
`ClientGuiInventory` EE payload builders for status and select-panel claims and
exports a non-emitting ClientGui writer plan in quickbar hints plus replay
summaries. The current-player inventory status plan builds exact payload
`700D010B0000000000007F90`; select-panel 3 builds `700D02080000000390`.
Emission remains disabled with `client_gui_inventory_bridge_timing_unproven`
until proxy-owned insertion timing is bounded. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-client-gui-writer-plan-20260709-050757` over
the 164-packet Diamond autoplay baseline reported 304 strict allow decisions, 0
strict quarantines, no quarantine directory, and 0 live-object terminal
residuals. The next production target is to implement bounded proxy-owned
ClientGui status emission timing for the proven current-player inventory
payload and verify it on live HG; if server `Inventory` traffic returns first,
continue the claim-neighborhood provenance path instead.

Previous live HG proxy status, as of 2026-07-08 23:17 +10: the
gameplay-reaching proxy harness was
`C:\nwnbridge\codex-live-pending-server-inventory-replay-rerun-20260708-231340\harness-proxy-20260708-231358`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, observed
`BNK3` after deferred `BNK2`, reached gameplay through `Module_Loaded`,
`Area_ClientArea`, proxy-generated `Area_AreaLoaded`, the post-area hold gate
opening, held post-area packet release, and sustained `GameObjUpdate_LiveObject`
traffic. It wrote `quickbar-item-refresh-hint.json` through
`2026-07-08T23:17:18+10:00` and `proxy.structured.log` through
`2026-07-08T23:17:24+10:00`, and produced no quarantine directory. The final
hint reported candidate `0x80015302` from active-object/direct proof, 18
direct item proof objects, 2 Feature-25 item proof objects, 18 ready compact
item objects, `inventory_equipment_handoff_ready=true`, one server
`Inventory` event, one ready server `Inventory` event, one bridge state update,
and `inventory_equipment_bridge_output_status="blocked_candidate_mismatch"`.
The ready candidate `0x80015302` did not match the parsed server-Inventory
claim object `0x800153B2`, so no synthetic `Inventory` output was queued.

This run followed the fresh live-data gate capture
`C:\nwnbridge\codex-live-current-confirm-20260708-224952\harness-proxy-20260708-225100`,
which also reached gameplay with no quarantine directory. That earlier settled
hint at `2026-07-08T22:54:40+10:00` showed one server `Inventory` handoff
before ready direct/materialized item state had been retained: later item
context had candidate `0x80015247`, 18 ready direct objects, and 2 deferred
Feature-25-only objects, but bridge output remained
`awaiting_bridge_state_update`. The current production slice retains such a
blocked server-Inventory claim and consumes it once when later item context
becomes ready. A first post-fix probe
`C:\nwnbridge\codex-live-pending-server-inventory-replay-20260708-230630\harness-proxy-20260708-230637`
failed before gameplay with server `BNCR` detail 6
(`observed-hg-rapid-reconnect-or-name-reservation`); after stopping the stale
client/proxy and waiting for the HG reservation cooldown, the 23:13 rerun above
reached gameplay.

As of 2026-07-08 23:20 +10, proxy2 also retains pending server-Inventory
handoff claims that were blocked only because direct/materialized item state
was not ready yet, replays the claim exactly once when a later live-object item
context becomes ready, and runs the bridge-output decider after every verified
server `M` packet so live-object-created bridge state can flush without
waiting for another `Inventory` packet. This does not change the
decompile-backed `Inventory` reader/writer shape; it only removes the timing
dependency between verified server `Inventory` and later verified item-state
evidence.
Focused state coverage proves the retained claim is consumed once and drains
into a server-Inventory bridge state update. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-pending-server-inventory-replay-20260708-2321`
over the 164-packet Diamond autoplay baseline reported 304 strict translation
decisions, 0 strict quarantines, no quarantine directory, and 0 live-object
terminal residuals; the Feature-25-only baseline still had one blocked
server-Inventory handoff, zero ready handoffs, zero bridge state updates, and
`inventory_equipment_bridge_output_status="awaiting_bridge_state_update"`.
The next production target is the candidate/claim association: prove why live
server `Inventory` can carry object `0x800153B2` while the retained ready item
candidate is `0x80015302`, then fix the shared state association before adding
any ClientGui inventory writer.

Previous live HG proxy status, as of 2026-07-07 21:05 +10: the
gameplay-reaching proxy harness is
`C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, proxy-generated
`Area_AreaLoaded`, the post-area hold gate opening, held post-area packet
release, and sustained `GameObjUpdate_LiveObject` traffic. It wrote
`quickbar-item-refresh-hint.json` and `proxy.structured.log` through
`2026-07-07T21:05:54+10:00` and produced no quarantine directory. The final
pending hint proved the current build's per-consumer inventory/equipment
handoff buckets on live HG traffic: 19 handoff events, 19 ready events, 0
blocked-without-ready events, 1 ready-with-deferred-Feature-25 event, 18
`ClientGuiInventory` events/ready events, and 1 server `Inventory` event/ready
event. The same hint reported candidate `0x80015386` from active-object
direct-only proof, 66 direct item proof objects, 2 Feature-25 item proof
objects, 66 compact-emission ready objects, 2 deferred Feature-25-only
objects, 6 Feature-25 reference records, 6 deferred item-ref mentions, 0
materialized item-ref mentions,
`inventory_feature25_materialization_outcome="all_item_refs_deferred"`,
`inventory_feature25_handoff_outcome="all_item_refs_deferred_with_ready_item_state"`,
`inventory_equipment_handoff_ready=true`, and
`inventory_equipment_handoff_outcome="ready_item_state_with_deferred_feature25_refs"`.
No generated client action was dispatched because the preserved active item
quickbar use-count state still mismatched the selected candidate. The next
implementation target is bounded bridge/writer behavior that uses the retained
ready direct item state for inventory/equipment UI consumers while keeping
later deferred Feature-25-only references reference-only.

As of 2026-07-07 21:04 +10, proxy2 also exports per-consumer
inventory/equipment handoff counters in pending and idle
`quickbar-item-refresh-hint.json` plus the Diamond replay summary. Focused
state tests prove both the idle and pending hint JSON carry the aggregate
handoff counters and the `ClientGuiInventory`/server `Inventory` splits, and
PowerShell replay parsing now surfaces matching summary fields.

As of 2026-07-07 22:56 +10, proxy2 also exports an explicit
`inventory_equipment_bridge_handoff_*` plan in pending and idle
`quickbar-item-refresh-hint.json` plus replay summaries. The plan is derived
only from the last retained ready inventory/equipment handoff snapshot: it is
`emit_ready_item_state` when direct/materialized compact item state has a
bridge candidate, and `none` when the evidence is Feature-25-only/deferred.
Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-plan-20260707-225132`
over 164 Diamond autoplay packets reported 304 strict allow decisions, 0
strict quarantines, 0 quarantine files, and 0 live-object terminal residuals;
the baseline correctly kept
`QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffAction=none` after one
blocked server-inventory handoff with deferred Feature-25-only evidence. The
next production target is the bounded writer/bridge consumer that uses
`emit_ready_item_state` live snapshots for `ClientGuiInventory`/server
`Inventory` while keeping later deferred Feature-25-only refs reference-only.

As of 2026-07-08 00:55 +10, proxy2 also records one-shot
`InventoryEquipmentHandoffBridgeEmission` records from ready bridge plans.
Pending/idle `quickbar-item-refresh-hint.json`, reducer diagnostics, and the
Diamond replay summary now expose the emission count plus the last emitted
consumer, event index, candidate object, and candidate source. Bounded strict
replay
`C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-emission-20260708-0055`
over the same 164 Diamond autoplay packets kept strict translation enabled,
produced no quarantine directory, saw 1 blocked server-inventory handoff, and
reported 0 bridge emissions because the baseline evidence was Feature-25-only.
The next production target remains the writer/bridge consumer, now consuming
these emitted ready item-state records rather than re-deriving handoff state.

As of 2026-07-08 02:57 +10, proxy2 also drains bridge emissions into
EE-facing `InventoryEquipmentBridgeStateUpdate` records. The drain is
idempotent by emission index and only accepts `emit_ready_item_state` plans with
a direct/materialized candidate; deferred Feature-25-only refs remain
reference-only and cannot create state updates. Pending/idle
`quickbar-item-refresh-hint.json`, reducer diagnostics, and replay summaries
now expose the state-update count and last drained candidate. Bounded strict
replay
`C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-drain-20260708-025233`
over the same 164 Diamond autoplay packets reported 304 strict allow decisions,
0 quarantine files, 0 live-object terminal residuals, 1 blocked
server-inventory handoff, 0 bridge emissions, and 0 bridge state updates. The
next production target is the concrete EE inventory/equipment writer output
from these drained ready item-state updates.

As of 2026-07-08 05:04 +10, proxy2 also builds and queues exact EE-facing
`Inventory` equip/cancel output from drained ready server-Inventory bridge
state updates. The queue path requires a parsed server `Inventory` claim, a
matching direct/materialized item-state candidate, and a payload that validates
through the strict inventory parser before inserting one proxy-owned reliable
server `M` frame after the triggering packet; `ClientGuiInventory` handoffs
remain state-only until their writer shape is proven. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-writer-20260708-0506-altports240`
over the same 164 Diamond autoplay packets used alternate local ports because
Windows denied the default replay listen port. It reported 304 strict allow
decisions, 0 strict quarantines, 0 quarantine files, 0 live-object terminal
residuals, 1 blocked server-inventory handoff, 0 ready handoffs, 0 bridge
emissions, and 0 bridge state updates. The next live HG run should confirm
whether real ready server-Inventory traffic queues the exact `Inventory` output
and whether any remaining visible equipment divergence belongs to a separate
ClientGui inventory writer.

As of 2026-07-08 06:56 +10, proxy2 also exports inventory/equipment bridge
output queue counters in `quickbar-item-refresh-hint.json` and the Diamond
replay summary. The fields include queued packet count, client-GUI deferrals,
missing-claim deferrals, candidate/claim mismatch blocks, and the last queued
synthetic `Inventory` metadata. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-summary-20260708-0648`
over the same 164 Diamond autoplay packets reported 303 strict allow decisions,
0 strict quarantines, 0 quarantine files, 0 live-object terminal residuals, 1
blocked server-inventory handoff, 0 ready handoffs, 0 bridge state updates, and
`inventory_equipment_bridge_output_queued_packets=0`. The next live HG run
should inspect this field directly; a zero value with ready server-Inventory
handoffs means use the new deferral/mismatch buckets before implementing any
ClientGui inventory writer.

As of 2026-07-08 08:56 +10, proxy2 also makes inventory/equipment bridge-output
decisions idempotent per drained state update and exports the last
decision/deferred/block update indexes. This keeps live deferral/mismatch
counters from growing repeatedly for the same immutable handoff update. Bounded
strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-decision-20260708-085235`
over the same 164 Diamond autoplay packets reported 304 strict allow decisions,
0 strict quarantines, 0 quarantine files, 0 live-object terminal residuals, 1
blocked server-inventory handoff, 0 ready handoffs, 0 bridge state updates, and
`inventory_equipment_bridge_output_queued_packets=0`. The next live HG run
should inspect the queued/deferral/mismatch buckets together with the last
decision indexes before deciding on server-Inventory claim repair versus a
separately proven ClientGui inventory writer.

As of 2026-07-08 11:00 +10, proxy2 also exports a typed last bridge-output
decision snapshot in `quickbar-item-refresh-hint.json` and replay summaries.
The fields include decision-known, reason, consumer, emission/event indexes,
ready candidate object/proof/source, and parsed server-Inventory claim object,
minor, result, and equip slot. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-decision-detail-20260708-105538`
over the same 164 Diamond autoplay packets reported 304 strict allow decisions,
0 strict quarantines, 0 quarantine files, and 0 live-object terminal residuals.
The Feature-25-only baseline had no drained ready update, so the new fields
correctly reported
`inventory_equipment_bridge_output_last_decision_known=false` and
`inventory_equipment_bridge_output_last_decision_reason="none"`. The next live
HG run should inspect the last-decision reason and candidate-vs-claim ids
beside the existing queue/deferral/mismatch counters before choosing
server-Inventory claim repair or a separately proven ClientGui inventory writer.

As of 2026-07-08 13:01 +10, proxy2 also exports a derived
`inventory_equipment_bridge_output_status` plus
`inventory_equipment_bridge_output_requires_client_gui_writer` in
`quickbar-item-refresh-hint.json` and replay summaries. The status gives the
next live run a single first-pass classifier: queued Inventory output wins over
server-Inventory candidate mismatch, missing claim, and client-GUI writer
deferral. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-status-20260708-1249`
over the same 164 Diamond autoplay packets passed with no quarantine files and
reported `inventory_equipment_bridge_output_status="awaiting_bridge_state_update"`
because the baseline has 1 blocked Feature-25-only server-Inventory handoff and
0 ready handoffs. The next live HG run should inspect this status before
choosing server-Inventory claim repair versus a separately proven ClientGui
inventory writer.

As of 2026-07-08 14:58 +10, proxy2 records non-server
inventory/equipment bridge-output decisions as soon as verified
`ClientGuiInventory` traffic creates a ready bridge state update. This removes
the old timing dependency where the quickbar hint could keep reporting
`awaiting_bridge_state_update` until a later server `Inventory` packet happened
to run the output decider. The server writer gate is unchanged: only a
server-Inventory update with a parsed matching claim can queue an exact EE
`Inventory` frame; ClientGuiInventory remains a writer gap until its packet
shape is separately proven. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-client-gui-bridge-decision-20260708-1458`
over 164 Diamond autoplay packet files passed with 304 strict allows, 0 strict
quarantines, 0 quarantine files, and
`inventory_equipment_bridge_output_status="awaiting_bridge_state_update"` on
the Feature-25-only baseline. The next live HG run should read
`inventory_equipment_bridge_output_status` first: `awaiting_client_gui_writer`
now means a real ClientGui ready handoff was classified immediately, not just
after a later server-Inventory trigger.

As of 2026-07-08 19:00 +10, proxy2 also carries the exact verified
`ClientGuiInventory` claim summary through the inventory/equipment bridge
decision path and exposes it in `quickbar-item-refresh-hint.json` and replay
summaries. The fields record whether a client-GUI claim was present, the claim
kind (`status` or `select_panel`), object id, selected panel,
player-inventory-gui flag, and self-object rewrite flag. This is state
propagation only: the server `Inventory` writer gate remains unchanged, and no
ClientGui writer is emitted until its packet shape is separately proven.
Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-client-gui-claim-detail-20260708-1900` over
the same 164 Diamond autoplay packet files passed with 304 strict allows, 0
strict quarantines, 0 quarantine files, and 0 live-object terminal residuals.
The Feature-25-only baseline still reported
`inventory_equipment_bridge_output_status="awaiting_bridge_state_update"` and
no last decision or client-GUI claim, as expected. The next live HG run should
use these claim fields if the status reaches `awaiting_client_gui_writer`.

As of 2026-07-08 21:05 +10, proxy2 also exports the ready direct/materialized
object count and deferred Feature-25-only object count stored on the typed last
bridge-output decision snapshot. These fields appear in
`quickbar-item-refresh-hint.json` and replay summaries as
`inventory_equipment_bridge_output_last_decision_ready_objects` and
`inventory_equipment_bridge_output_last_decision_deferred_feature25_only_objects`.
Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-bridge-decision-ready-context-built-20260708-210200`
over the same 164 Diamond autoplay packet files passed with strict translation,
0 quarantine files, and 0 live-object terminal residuals. The Feature-25-only
baseline had no drained ready bridge update, so it kept
`inventory_equipment_bridge_output_status="awaiting_bridge_state_update"` and
reported decision ready/deferred counts of 0/0. The 2026-07-07 21:05 live HG
capture was about 23h42m old at the start of this run; the next run should
refresh live HG evidence before using these fields to choose server-Inventory
claim repair versus a separately proven ClientGui inventory writer.

Previous live HG proxy status, as of 2026-07-07 16:49 +10: the
gameplay-reaching proxy harness was
`C:\nwnbridge\codex-live-bnk3-stall-diagnostic-20260707-164655\harness-proxy-20260707-164703`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
`GameObjUpdate_LiveObject` traffic, wrote `quickbar-item-refresh-hint.json` at
`2026-07-07T16:49:38+10:00`, left `proxy.structured.log` active through
`2026-07-07T16:49:38+10:00`, and produced no quarantine directory. The same
run logged `observed EE BNK3 after deferred BNK2` with `elapsed_ms=106`, so the
prior fresh-run BNK2/no-BNK3 crash did not reproduce. The final hint resolved
by prior quickbar use-count state with
`no_hint_reason="post_context_resolved_by_prior_quickbar_use_count_state"`,
`post_committed_item_refresh_resolution="resolved_by_prior_quickbar_use_count_state"`,
candidate `0x80015219` from active-object/direct-only proof, 18 direct item
proof objects, 2 Feature-25 item proof objects, 18 compact-emission ready
objects, 2 deferred Feature-25-only objects, 7 Feature-25 reference records, 7
deferred item-ref mentions, 0 materialized item-ref mentions,
`inventory_feature25_materialization_outcome="all_item_refs_deferred"`,
`inventory_feature25_handoff_outcome="all_item_refs_deferred_with_ready_item_state"`,
and
`inventory_equipment_handoff_outcome="ready_item_state_with_deferred_feature25_refs"`.

As of 2026-07-07 19:00 +10, proxy2 also consumes inventory/equipment handoff
readiness in shared UI state. Verified `Inventory` and `ClientGuiInventory`
events now increment handoff counters, consume the best retained
direct/materialized item context, keep deferred Feature-25-only refs
reference-only, and write the last handoff snapshot into idle
`quickbar-item-refresh-hint.json` plus the Diamond replay summary. Bounded
strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-equipment-handoff-consumer-20260707-185513`
over the 2026-07-03 Diamond autoplay packet set processed 164 packet files with
strict translation, 304 allow decisions, 0 strict quarantines, and 0 quarantine
files. That replay baseline had 1 inventory/equipment handoff event but no
ready direct/materialized item state, so it correctly stayed blocked with
`inventory_equipment_handoff_outcome="feature25_refs_without_ready_item_state"`.
The 2026-07-07 21:04 live HG harness above confirmed this build still reaches
gameplay and real `ClientGuiInventory`/`Inventory` traffic increments the
per-consumer ready buckets against retained ready direct item state.

Previous live HG proxy status, as of 2026-07-07 12:58 +10: the
gameplay-reaching proxy harness was
`C:\nwnbridge\codex-live-feature25-handoff-outcome-20260707-20260707-125516\harness-proxy-20260707-125522`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
`GameObjUpdate_LiveObject` traffic, wrote `quickbar-item-refresh-hint.json` at
`2026-07-07T12:58:45+10:00`, left `proxy.structured.log` active through
`2026-07-07T12:58:49+10:00`, and produced no quarantine directory. The final
hint resolved by prior quickbar use-count state with
`no_hint_reason="post_context_resolved_by_prior_quickbar_use_count_state"`,
`post_committed_item_refresh_resolution="resolved_by_prior_quickbar_use_count_state"`,
candidate `0x80015899` from active-object/direct-only proof, 18 direct item
proof objects, 2 Feature-25 item proof objects, 18 compact-emission ready
objects, 2 deferred Feature-25-only objects, 17 Feature-25 reference records,
17 deferred item-ref mentions, 0 materialized item-ref mentions,
`inventory_feature25_materialization_outcome="all_item_refs_deferred"`, and
`inventory_feature25_handoff_outcome="all_item_refs_deferred_with_ready_item_state"`.

As of 2026-07-07 14:56 +10, proxy2 also reports
`inventory_equipment_handoff_ready` and
`inventory_equipment_handoff_outcome` in semantic traces, pending/idle
`quickbar-item-refresh-hint.json`, and the Diamond replay summary. These fields
generalize the Feature-25 handoff classifier for the inventory/equipment UI:
direct or materialized compact item state is ready for handoff even when
Feature-25 item refs are all deferred, while Feature-25 reference-only evidence
is not ready. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-inventory-equipment-handoff-bounded-20260707-145448`
over the 2026-07-03 Diamond autoplay packet set processed 164 packet files with
strict translation, 304 allow decisions, 0 strict quarantines, and 0 quarantine
files. Its final hint reported `pending_item_refresh=false`,
`no_hint_reason="post_context_without_compact_item_proof"`,
`inventory_equipment_handoff_ready=false`,
`inventory_equipment_handoff_outcome="feature25_refs_without_ready_item_state"`,
0 ready compact item objects, and 6 deferred Feature-25-only objects. The next
live HG harness should confirm this build still reaches gameplay and should use
the new ready/outcome fields to choose the shared inventory/equipment UI
handoff rule instead of materializing deferred Feature-25 refs.

As of 2026-07-07 11:02 +10, proxy2 separates deferred Feature-25 refs from
emission-ready compact quickbar item proof. The semantic registry still reports
the union as `compact_item_emission_proof_objects` for diagnostics and
candidate tracking, but `compact_item_emission_ready_objects` and
`compact_item_emission_ready_candidate` now require direct/materialized item
state. Feature-25-only refs stay in
`compact_item_emission_deferred_feature25_only_objects`, do not open a pending
quickbar item-refresh window, and do not produce a harness hint. Bounded strict
replay
`C:\nwnbridge\codex-proxy2-replay-feature25-ready-split-20260707-105736` over
the 2026-07-03 Diamond autoplay packet set processed 164 packet files with
strict translation, 304 allow decisions, 0 strict quarantines, and 0 quarantine
files. Its final hint reported `pending_item_refresh=false`,
`no_hint_reason="post_context_without_compact_item_proof"`, diagnostic
candidate `0x80015DAA` from `feature25_second_list`, 0 ready compact emission
objects, and 6 deferred Feature-25-only objects; semantic post-context likewise
reported 23 Feature-25-only candidate selections, 0 Feature-25-only proof-class
refreshes, 0 ready objects, and 6 deferred objects. The next live HG harness
should confirm the ready/deferred split preserves the current
prior-quickbar-use-count no-action path, then continue the inventory/equipment
UI handoff audit.

As of 2026-07-07 12:58 +10, proxy2 also reports
`inventory_feature25_handoff_outcome` in semantic traces, pending/idle
`quickbar-item-refresh-hint.json`, and the Diamond replay summary. This field
combines the Feature-25 materialization outcome with whether separate direct or
materialized item state is ready for compact quickbar/UI handoff. Bounded
strict replay
`C:\nwnbridge\codex-proxy2-replay-feature25-handoff-outcome-20260707-124920`
over the 2026-07-03 Diamond autoplay packet set processed 164 packet files with
strict translation, 304 allow decisions, 0 strict quarantines, and 0 quarantine
files; its final hint reported
`inventory_feature25_handoff_outcome="all_item_refs_deferred_without_ready_item_state"`.
The fresh live HG run above reported
`inventory_feature25_handoff_outcome="all_item_refs_deferred_with_ready_item_state"`,
which preserves the prior-use-count no-action path while distinguishing ready
direct item state from deferred Feature-25-only references.

Previous live HG proxy status, as of 2026-07-07 04:42 +10: the
gameplay-reaching proxy harness is
`C:\nwnbridge\codex-live-inventory-feature25-current-20260707-043430\harness-proxy-20260707-043444`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
`GameObjUpdate_LiveObject` traffic, wrote `quickbar-item-refresh-hint.json` at
`2026-07-07T04:42:08+10:00`, left `proxy.stdout.log` at
`2026-07-07T04:42:08+10:00`, and produced no quarantine directory. Candidate
`0x80015270` resolved by prior quickbar use-count state with
`no_hint_reason="post_context_resolved_by_prior_quickbar_use_count_state"` and
`post_committed_item_refresh_resolution="resolved_by_prior_quickbar_use_count_state"`;
no generated client action was dispatched. The live per-bucket counters showed
21 quickbar item buttons preserved by explicit self materialization, 42
Feature-25 reference records, 21 first-list deferred item-ref mentions, 21
second-list deferred item-ref mentions, 0 Feature-25 materialized mentions, and
0 cleared inventory item ids.

As of 2026-07-07 04:54 +10, proxy2 also derives aggregate Feature-25 item-ref
mention totals and `inventory_feature25_materialization_outcome` in pending and
idle `quickbar-item-refresh-hint.json` output. Semantic trace logs include the
same aggregate/outcome values when retaining quickbar item context, and the
replay summary parser exports them under
`QuickbarItemRefreshHintInventoryFeature25*`. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-feature25-outcome-20260707-045300` over the
2026-07-03 Diamond autoplay packet set used 164 packet files, strict
translation, 304 allow decisions, 0 strict quarantines, and 0 quarantine files;
the pending Diamond path reported 23 Feature-25 reference records, 27 item-ref
mentions, 0 materialized mentions, 27 deferred mentions, and
`inventory_feature25_materialization_outcome="all_item_refs_deferred"`. The
later 08:53 run used this all-deferred baseline to keep deferred Feature-25
refs reference-only for compact quickbar emission; the remaining live target is
to confirm later inventory/equipment UI handoff behavior.

Previous live HG proxy status, as of 2026-07-07 00:33 +10: the
gameplay-reaching proxy harness is
`C:\nwnbridge\codex-live-stream-materialization-current-20260707-003039\harness-proxy-20260707-003052`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
`GameObjUpdate_LiveObject` traffic, wrote `quickbar-item-refresh-hint.json` at
`2026-07-07T00:33:48.7458235+10:00`, and produced no quarantine directory. The
final hint showed candidate `0x80015D81` from active-object/direct-only proof,
but the first preserved active quickbar item was `0x80015D89` in slot 0 with a
durable typed item-button `G Q` row; the candidate had no matching use-count
state row. This is a harness-driving state handoff issue, not a new packet
bit-shape proof.

As of 2026-07-07 00:50 +10, proxy2 also writes the first-preserved-active
quickbar item `G Q` use-count state into pending and idle
`quickbar-item-refresh-hint.json` output, and the replay summary parser exports
the normalized
`QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountState*` fields.
When the pending candidate differs from the preserved active item, the candidate
lacks matching slot state, and the preserved active item has a matching
item-button use-count row, the harness hint now suppresses generated client
actions with
`preserved_active_item_quickbar_use_count_state_candidate_mismatch`. Bounded
strict replay
`C:\nwnbridge\codex-proxy2-replay-preserved-active-use-count-state-20260707-005028`
over the 2026-07-03 Diamond autoplay packet set used 164 packet files, strict
translation, 304 allow decisions, 0 strict quarantines, and 0 quarantine files;
the replay path had no preserved-active row, so the new fields exported
`known=false` and did not suppress. The next live HG run should confirm the
00:33 mismatch now suppresses action dispatch before chasing the next visible
inventory/equipment or live-object UI state gap.

Previous live HG proxy status, as of 2026-07-06 20:32 +10: the
gameplay-reaching proxy harness was
`C:\nwnbridge\codex-live-prior-gq-state-handoff-current-20260706-202809\harness-proxy-20260706-202815`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
`GameObjUpdate_LiveObject` traffic, wrote `quickbar-item-refresh-hint.json` at
`2026-07-06T20:32:10+10:00`, and produced no quarantine directory. Candidate
`0x80015CCF` came from active-object/direct-only proof and matched durable
typed `G Q` item-use-count state for quickbar slot 0/button 1/property index
255/use count 1. The final hint reported `pending_item_refresh=false` and
`no_hint_reason="post_context_resolved_by_prior_quickbar_use_count_state"`, and
no `UseItem` subtype-low client action was observed or injected.

As of 2026-07-06 20:40 +10, proxy2 also writes
`post_committed_item_refresh_resolution` into pending and idle
`quickbar-item-refresh-hint.json` output, and the replay summary parser exports
it as `QuickbarItemRefreshHintPostCommittedItemRefreshResolution`. The field is
the machine-readable summary for the post-committed quickbar item-refresh state:
`pending`, `resolved_by_server_quickbar_use_count`,
`resolved_by_prior_quickbar_use_count_state`, or `none`. Keep the older
booleans for compatibility, but prefer this field when comparing current live
and replay artifacts. Strict replay
`C:\nwnbridge\codex-proxy2-replay-resolution-field-pending-20260706-204746`
confirmed the field on the current pending replay path
(`QuickbarItemRefreshHintPostCommittedItemRefreshResolution=pending`, 164 packet
files, strict translation, zero quarantines).

As of 2026-07-06 22:49 +10, proxy2 also writes the stream-probe quickbar item
materialization proof/missing-state counters into pending and idle
`quickbar-item-refresh-hint.json` output, and the replay summary parser exports
them as `QuickbarItemRefreshHintStreamProbe*` fields. These counters come from
the typed quickbar writer's existing item materialization decision path and
separate preserved active-object/Feature-25 proofs from unknown, item-delete
cleared, and area-reset cleared missing-state rejects. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-stream-materialization-counters-bounded-20260706-2250`
over the 2026-07-03 Diamond autoplay packet set used 164 packet files, strict
translation, 304 allow decisions, 0 strict quarantines, and 0 quarantine files;
the replay's pending feature-25-only candidate had zero stream-probe
preserved/rejected item-object counters, as expected for that replay path. The
next live HG run should use these fields to decide whether remaining visible
inventory/equipment divergence is caused by absent item materialization proof,
a cleared item id, or a later UI state handoff rather than by another quickbar
action probe.

Previous live HG proxy status, as of 2026-07-06 16:44 +10: the
gameplay-reaching proxy harness was
`C:\nwnbridge\codex-live-coalesced-continuation-fix-20260706-164042\harness-proxy-20260706-164049`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
`GameObjUpdate_LiveObject` traffic, wrote `quickbar-item-refresh-hint.json` at
`2026-07-06T16:44:44+10:00`, and produced no quarantine directory. Candidate
`0x800155A9` came from `active_object` / `direct_only` proof, matched the
preserved active-property quickbar item in quickbar slot 0, and matched durable
typed `G Q` item-use-count state for slot 0/button 1/property index 255/use
count 1. The first client action matched the subtype-low `UseItem`, and HG
returned 0 full quickbar, 0 post-action `G Q`, and 0 candidate active-property
uses/full responses after the action.

As of 2026-07-06 18:50 +10, proxy2 resolves that no-server-response branch
from prior durable typed `G Q` item-use-count state instead of asking the EE
client to generate another action probe. When the pending candidate, preserved
active item signature, preserved slot, and item button type match the durable
row, the semantic state records
`pending_refresh_resolved_by_use_count_state`, clears the pending hint,
reports `post_context_resolved_by_prior_quickbar_use_count_state`, and
suppresses generated client action hints with
`matching_quickbar_use_count_state`. Strict replay
`C:\nwnbridge\codex-proxy2-replay-prior-gq-state-handoff-20260706-184640`
against the 2026-07-03 Diamond autoplay capture stayed at 164 packet files,
304 strict allows, 0 strict quarantines, and 0 quarantine files; that replay has
no candidate durable use-count row, so the new resolved-by-use-count-state
counter is expected to stay 0 there. The next live HG run should confirm the
final `quickbar-item-refresh-hint.json` lands in the prior-state no-hint branch
and that the harness no longer dispatches the subtype-low `UseItem` probe for
this already-known active item state.

As of 2026-07-06 16:45 +10, proxy2 also protects coalesced zlib stream tails
from false high-level ownership. A current-code live probe
`C:\nwnbridge\codex-live-use-count-state-current-20260706-162740\harness-proxy-20260706-162752`
reached gameplay but emitted five identical 241-byte
`unclaimed-unknown-high-level` quarantine files for an inflated gameplay stream
tail that the splitter had already classified as an incomplete/non-header
continuation. The fixed coalesced rewrite path now checks for a single
incomplete stream unit before high-level parse fallback, keeping those payloads
on the stream-continuation path. Patched live verification produced no
quarantine directory. Strict replay
`C:\nwnbridge\codex-proxy2-replay-coalesced-continuation-fix-20260706-164526`
against the 2026-07-03 Diamond autoplay capture stayed at 164 packet files,
304 strict allows, 0 strict quarantines, and 0 quarantine files.

As of 2026-07-06 14:45 +10, proxy2 keeps a durable semantic table of verified
typed live-object `G Q` item-use-count rows keyed by slot/button/object/property
and writes candidate state evidence into active and idle
`quickbar-item-refresh-hint.json` files:
`quickbar_item_use_count_state_rows`,
`quickbar_item_use_count_updates_observed`, and the
`candidate_quickbar_item_use_count_state_*` row/slot-relation fields. The
replay summary exports the same fields. The current live result above confirms
the active item row is available when the final hint lands in the
no-server-response branch. The current production path now consumes that
durable typed `G Q` row as the generalized EE client/visible quickbar state
handoff.

As of 2026-07-05 12:33 +10, proxy2 also writes
`pending_item_refresh_recommended_action_outcome` into quickbar item-refresh
hints and semantic traces, and the replay summary exports it as
`QuickbarItemRefreshHintRecommendedActionOutcome`. The 12:17 live capture
predates that JSON field but its existing first-action and follow-up counters
derive as `recommended_use_object_no_server_quickbar`. Strict replay
`C:\nwnbridge\codex-proxy2-replay-recommended-outcome-20260705-123353` against
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
stayed at 164 packet files, 304 strict allow decisions, 0 quarantine
decisions/artifacts, and exported
`QuickbarItemRefreshHintRecommendedActionOutcome=awaiting_client_action` for
the replay's no-client-action pending window.

As of 2026-07-05 14:33 +10, the same 12:17/12:24 gameplay-reaching live HG
capture remained fresh, so no new live capture was required for the UseItem
classifier slice. Proxy2 now records UseItem-specific parsed fields in
quickbar action details and harness hints:
`first_client_action_use_item_known`,
`first_client_action_use_item_active_property_subtype`,
`first_client_action_use_item_has_optional_byte`,
`first_client_action_use_item_has_target_object`,
`first_client_action_use_item_target_object_id_hex`,
`first_client_action_use_item_target_is_self_or_legacy_self`,
`first_client_action_use_item_has_position`, and
`first_client_action_matches_recommended_client_use_item`. Recommended-action
outcomes now distinguish `recommended_use_item_no_server_quickbar` from
`recommended_use_item_observed_server_quickbar`. The next live active-property
probe should compare these fields against HG follow-up traffic before changing
the generated action rule, especially the meaning of the UseItem subtype byte.

As of 2026-07-05 16:31 +10, the same 12:17/12:24 gameplay-reaching live HG
capture remained fresh, so no new live capture was required for the
first-property subtype-low UseItem diagnostic. Proxy2 now writes
`recommended_use_item_first_property_subtype_low_*` fields into pending
quickbar item-refresh hints when the first preserved active item matches the
pending candidate and has a first active property. The generated diagnostic
payload keeps the decompile-backed `Input_UseItem` reader order:
`OBJECTID`, active-property byte, optional-byte BOOL, optional-target
BOOL/object, optional-position BOOL/vector. The 12:17 live hint gives a
dispatchable example for candidate `0x80015678`, with first property subtype
`0x020D`, low byte `0x0D`, and generated payload
`70060910000000785601800DFDFFFFFFC8`. Strict replay
`C:\nwnbridge\codex-proxy2-replay-useitem-subtype-low-retry-20260705-163118`
against
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
stayed at 164 packet files, 304 strict allow decisions, 0 quarantine
decisions/artifacts, and correctly reported the subtype-low payload as
unavailable for replay candidate `0x80015DAA` because no preserved active item
matched that candidate. The EE bridge validates this hinted packet before
dispatch and the PowerShell harness exposes it through
`-AutoQuickbarItemRefreshUseItemSubtypeLow`, gated by driver-only mode and
`HG_BRIDGE_AUTO_QUICKBAR_ITEM_REFRESH_USEITEM_SUBTYPE_LOW=1`. To run the live
probe after building the bridge and proxy target, use:

```powershell
.\tools\test-hg-bridge.ps1 -Server 213 -AutoQuickbarItemRefreshUseItemSubtypeLow -SeedNwsyncClientCache -SkipAssets -SkipBuild -ProxyLogRoot C:\nwnbridge\<descriptive-run>
```

Treat success as gameplay reached plus a final `quickbar-item-refresh-hint.json`
with
`first_client_action_match_class="recommended_use_item_first_property_subtype_low"`,
then inspect whether HG emits any server quickbar follow-up. The 2026-07-05
18:39 live run reached this state with 0 quarantines and 0 server quickbar
events, so future work should trace original-client active-property state
semantics instead of cycling exact generated probe payloads.

As of 2026-07-05 04:41 +10, proxy2 also writes first-preserved active-item
signature fields into quickbar item-refresh hints and unresolved traces. The
fields are:
`first_preserved_active_item_known`,
`first_preserved_active_item_matches_candidate`,
`first_preserved_active_item_object_id_hex`,
`first_preserved_active_item_base_item_hex`,
`first_preserved_active_item_appearance_type`,
`first_preserved_active_item_property_count`,
`first_preserved_active_item_first_property`,
`first_preserved_active_item_first_property_subtype`,
`first_preserved_active_item_state_mask_hex`, and
`first_preserved_active_item_value_mask_hex`. Proxy2 also classifies the first
client action with `first_client_action_matches_preserved_active_item` and
`first_client_action_match_class` (`awaiting_client_action`, `target_unknown`,
`other_object`, `candidate_object`, `preserved_active_item`,
`recommended_set_button`, or `recommended_gui_event_notify`). Strict rebuilt
replay
`C:\nwnbridge\codex-proxy2-replay-action-match-class-rebuilt-20260705-0441`
stayed at 164 packet files, 304 strict allows, and 0 quarantine artifacts; its
pending feature-25-only hint exposed the new fields with
`first_client_action_match_class="awaiting_client_action"`. The next live
GUI-event/action probe should use these fields as primary evidence for whether
the first action corresponds only to the candidate, to the preserved active
item, or to one of the exact generated probe shapes before changing the
active-property action/state translator rule.

As of 2026-07-05 06:22 +10, proxy2 also writes a decompile-backed
`Input_UseObject` probe into pending quickbar item-refresh hints. The generated
payload uses family/minor `70 06 0B`, declared byte count `0x0B`, the pending
candidate object id, and two final fragment BOOLs in the EE/legacy reader
order: `mark_inventory_gui_state=false` then `schedule_script_event=false`.
Strict replay
`C:\nwnbridge\codex-proxy2-replay-useobject-hint-20260705-061927` stayed at
164 packet files, 304 strict allows, and 0 quarantine artifacts, and emitted
`recommended_client_use_object_payload_hex=70060B0B000000AA5D0180A0` for
candidate `0x80015DAA`. The EE bridge validates this hinted packet before
dispatch and the PowerShell harness exposes it through
`-AutoQuickbarItemRefreshUseObject`, gated by driver-only mode and
`HG_BRIDGE_AUTO_QUICKBAR_ITEM_REFRESH_USEOBJECT=1`. To run the live probe after
building the bridge, use:

```powershell
.\tools\test-hg-bridge.ps1 -Server 213 -AutoQuickbarItemRefreshUseObject -SeedNwsyncClientCache -SkipAssets -SkipBuild -ProxyLogRoot C:\nwnbridge\<descriptive-run>
```

Treat success as gameplay reached plus a final `quickbar-item-refresh-hint.json`
with `pending_item_refresh=true` and
`first_client_action_match_class="recommended_use_object"`, then inspect whether
HG emits any server quickbar follow-up.

As of 2026-07-04 09:43 +10, proxy2 also observes consumed EE-only
`GuiEvent_Notify` client payloads semantically while still forwarding only an
empty Diamond/1.69 compatibility carrier. Pending quickbar item-refresh traces,
`quickbar-item-refresh-hint.json`, and replay summaries now expose
`client_gui_event_events_since_pending_refresh`,
`client_gui_event_events_after_first_client_action`, and
`client_gui_event_notify` first-follow-up/first-client-action buckets. Strict
replay `C:\nwnbridge\codex-proxy2-replay-client-gui-event-20260704-0940`
against the current Diamond capture stayed at 164 packet files, 304 strict
allows, 0 strict quarantines, and 0 quarantine files; the new GUI-event fields
were present and zero for that replay's still-`awaiting_client_action` pending
window. The fresh SetButton live probe above also exposed the fields with zero
GUI-event counts, so the next live radial/menu probe should treat them as the
primary evidence for whether the original client action after the pending item
proof is a GUI/radial event rather than another quickbar SetButton or UseItem
shape.

As of 2026-07-04 10:30 +10, proxy2 also writes a bounded recommended
`ClientGuiEvent/Notify` radial probe into `quickbar-item-refresh-hint.json`
when a pending quickbar item refresh has a candidate object id. The EE bridge
validates that hinted `70 35 01` payload before dispatch and the PowerShell
harness exposes it through `-AutoQuickbarItemRefreshGuiEventNotify`, gated by
driver-only mode and
`HG_BRIDGE_AUTO_QUICKBAR_ITEM_REFRESH_GUI_EVENT_NOTIFY=1`. To run the next
live radial/menu probe after building the bridge, use:

```powershell
.\tools\test-hg-bridge.ps1 -Server 213 -AutoQuickbarItemRefreshGuiEventNotify -SeedNwsyncClientCache -SkipAssets -SkipBuild -ProxyLogRoot C:\nwnbridge\<descriptive-run>
```

Treat success as gameplay reached plus a matched
`first_client_action="client_gui_event_notify"` in the final
`quickbar-item-refresh-hint.json`. The 2026-07-05 04:12 live run reached that
point and additionally proved the generated GUI event targeted both the
candidate and the preserved active-property quickbar item, so the remaining
failure mode is no server quickbar follow-up after the exact matched GUI event.
Treat that as the next action-family/state issue rather than a connection
blocker while `Area_ClientArea` and live-object traffic continue.

As of 2026-07-04 14:29 +10, the 11:50 pre-gameplay GUI-event notify blocker is
resolved by the shared Rust `Device_AdvertiseProperty` classifier. The earlier
failure trail was: run
`C:\nwnbridge\codex-live-gui-event-notify-20260704-113400\harness-proxy-20260704-113405`
selected an older repo debug proxy, reached module load, then quarantined
strict `GameObjUpdate_LiveObject` and `Area_ClientArea` payloads. The harness
resolver now selects the newest compatible `hgbridge_proxy2.exe` by
`LastWriteTime` after checking each candidate for the current hint CLI, so a
fresh `C:\nwnbridge\cargo-target` build is not shadowed by an older repo debug
binary during `-SkipBuild` runs. Retry run
`C:\nwnbridge\codex-live-gui-event-notify-newest-proxy-retry-20260704-114234\harness-proxy-20260704-114239`
used `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe` and passed
BNK/BNCS/BNVR, character list, login, `Module_Info`, and
`CNWCModule::LoadModuleResources`, but did not reach `Module_Loaded`,
`Area_ClientArea`, live-object traffic, or GUI-event dispatch by the run
cutoff. It wrote no quarantine files and the hint stayed
`pending_item_refresh=false` with `no_committed_quickbar_profile`. Treat this
as historical evidence only. Fresh rerun
`C:\nwnbridge\codex-live-device-property-classifier-gui-event-20260704-142731\harness-proxy-20260704-142740`
consumed 70 `Device_AdvertiseProperty` frames, reached gameplay, logged no
client high-level M-frame quarantines, and moved the active blocker to
quickbar stream-probe profiles that are verified but not committed. The
2026-07-04 16:22 follow-up added an exact stream-probe profile promotion path
and reached the GUI-event notify action path; keep the 14:27 run as historical
connection-blocker evidence.

As of 2026-07-04 05:32 +10, proxy2 also writes server-to-client and
client-to-server direction totals for pending quickbar item-refresh windows
into semantic traces, `quickbar-item-refresh-hint.json`, and replay summaries.
Strict replay
`C:\nwnbridge\codex-proxy2-replay-direction-counters-20260704-0532` against
the current Diamond HG gameplay capture stayed at 164 packet files, 304 strict
allows, 0 strict quarantines, and 0 quarantine files. The replay hint for
feature-25-only candidate `0x80015DAA` reported 190 post-proof events split 96
server-to-client and 94 client-to-server while still
`awaiting_client_action`.

Previous live HG proxy status, as of 2026-07-04 00:36 +10: the
gameplay-reaching proxy harness is
`C:\nwnbridge\codex-live-quickbar-setbutton-driver-20260704-003119\harness-proxy-20260704-003123`.
It reached gameplay through `Area_ClientArea` and sustained
`GameObjUpdate_LiveObject` traffic, wrote `quickbar-item-refresh-hint.json`,
and had no quarantine artifact files. The bridge DLL dispatched one validated
`GuiQuickbar_SetButton` item action for quickbar item-refresh candidate
`0x80016A0F` at `2026-07-04 00:33:10 +10`, using payload
`701E021200000000010F6A0180FFFFFFFF0060`. The proxy hint recorded
`first_client_action="client_quickbar_item_set_button"`,
`first_client_action_matches_candidate=true`, 353 verified events after that
first client action, 113 live-object events after it, and 0 server quickbar
events. The immediate next harness/protocol target is original-client
active-property item action semantics and timing beyond UseItem versus
SetButton dispatch.

As of 2026-07-04 01:35 +10, proxy2 also writes
`pending_item_refresh_action_outcome` into semantic traces and
`quickbar-item-refresh-hint.json`. Strict replay
`C:\nwnbridge\codex-proxy2-replay-action-outcome-20260704-0138` against the
current Diamond HG gameplay capture stayed at 164 packet files, 304 strict
allows, 0 strict quarantines, and 0 quarantine files. The replay hint ended
`awaiting_client_action` for feature-25-only candidate `0x80015DAA`; the latest
live SetButton probe above should read as
`candidate_client_action_no_server_quickbar` because the matched client action
was observed but no server quickbar followed.

As of 2026-07-04 02:40 +10, proxy2 also writes
`first_client_action_timing` and
`followup_events_before_first_client_action` into the same hint and semantic
trace path. Strict replay
`C:\nwnbridge\codex-proxy2-replay-action-timing-20260704-023643` and parser
check `C:\nwnbridge\codex-proxy2-replay-action-timing-summary-20260704-024005`
stayed at 164 packet files, 304 strict allows, 0 strict quarantines, and 0
quarantine files. The replay hint remained `awaiting_client_action` for
feature-25-only candidate `0x80015DAA`, proving the new fields are ready for
the next live SetButton/UseItem timing probe.

To send the proxy-recommended quickbar SetButton action from the EE driver,
use:

```powershell
.\tools\test-hg-bridge.ps1 -Server 213 -AutoQuickbarItemRefreshSetButton -SeedNwsyncClientCache -SkipAssets -SkipBuild -ProxyLogRoot C:\nwnbridge\<descriptive-run>
```

Latest known live HG proxy status, as of 2026-07-03 22:34 +10: the current
gameplay-reaching proxy harness is
`C:\nwnbridge\codex-live-useitem-self-target-hint-20260703-223120\harness-proxy-20260703-223124`.
It reached gameplay through `Area_ClientArea` and live-object traffic, wrote
`quickbar-item-refresh-hint.json`, and had 0 quarantine files. The run
committed the 36-slot/18-item quickbar profile, dispatched a matched
`Input_UseItem` for quickbar item-refresh candidate `0x80016691`, and proxy2
validated/rewrite-claimed the self-targeted payload
`700609100000009166018000FDFFFFFFC8` with the EE self target rewritten to
Diamond's legacy invalid/self target. The final hint recorded 151 verified
events after that first client action: 52 live-object, 1 inventory, 1 chat, 97
other, and 0 server/client quickbar events. The immediate next
harness/protocol target is active-property item client-action semantics and
timing, including quickbar set-button versus radial/UseItem behavior, not
another proof that the driver can send a valid UseItem payload.

As of 2026-07-03 23:35 +10, proxy2 also writes a decompile-backed
`GuiQuickbar_SetButton` candidate action into `quickbar-item-refresh-hint.json`.
The hint includes payload availability, hex bytes, target slot, slot source,
button type, item object id, int parameter, and target-object presence. Strict
replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-setbutton-hint-20260703-233507`
against the current Diamond capture stayed at 0 quarantines and produced
`recommended_client_quickbar_set_button_payload_hex=701E02120000000701AA5D0180FFFFFFFF0060`
for candidate `0x80015DAA`, slot 7, source `first_blank_committed_slot`. Next
harness action: add an opt-in driver path that sends this SetButton payload
from the hint file, then run a live HG probe and compare the post-action
quickbar/server counters with the UseItem-only probes above.

Latest known live HG status, as of 2026-07-03 15:29 +10: the current
gameplay-reaching Diamond capture is
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516`, with packet dumps
under `diamond-client-packets`, probe log `diamond-client-probe.log`, 164
packet files, and packet window
`2026-07-03T15:16:25.8610376+10:00 -> 2026-07-03T15:19:28.1192675+10:00`.
Gameplay was reached through tempclient BIC/PRE_PLAYMOD auto-play and repeated
HG live-object traffic; at the 2026-07-03 15:29 +10 check, the newest packet
was about 10 minutes old. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-useitem-driver-20260703-1530`
against this capture reported 164 packet files, 304 strict allows, 0 strict
quarantines, 0 semantic quarantine matches, and 0 quarantine files. Its
`quickbar-item-refresh-hint.json` was pending for candidate `0x80015DAA`
(`feature25_second_list`, Feature-25-only) with
`recommended_use_item_payload_hex=7006090C000000AA5D018000C0`.

As of 2026-07-03 15:35 +10, `tools\test-hg-bridge.ps1` has an opt-in
`-AutoQuickbarItemRefreshUseItem` live-driver path. It exports
`HG_BRIDGE_AUTO_QUICKBAR_ITEM_REFRESH_USEITEM=1`, wires
`HG_BRIDGE_QUICKBAR_ITEM_REFRESH_HINT_PATH` to the proxy2 hint file, and the
bridge DLL polls that file from the driver-only server-message hook. The bridge
validates the full high-level `70 06 09` `Input_UseItem` shape against the
decompile-backed reader order before sending it once through
`CNWMessage::SendPlayerToServerMessage`. A bounded live probe
`C:\nwnbridge\codex-live-quickbar-useitem-driver-20260703-1535\harness-proxy-20260703-153052`
reached gameplay through proxy2 (`Area_ClientArea` observations at
2026-07-03 15:31:51 and 15:32:22 +10). That live path did not write a pending
hint and no UseItem dispatch fired; proxy logs instead showed stream-probe
quickbar item candidates (`item_buttons_seen=1`, compact source) without a
committed item-preservation proof. Next useful harness action: make the live
probe summarize hint absence versus committed/pending quickbar state, then
drive a post-proof item action only when proxy2 actually emits the pending
hint.

As of 2026-07-03 16:26 +10, proxy2 writes
`quickbar-item-refresh-hint.json` even when no actionable quickbar item-refresh
hint exists. In that case the file has `pending_item_refresh=false`,
`no_hint_reason`, and committed/post-context counters so the live harness can
distinguish no committed quickbar profile, missing post-commit item context,
pending proof without a candidate, cleared proof, or no compact item proof.
Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-idle-hint-automation-20260703-1626`
against the current Diamond capture stayed at 0 quarantines and 304 strict
allows, and still ended with the expected pending candidate `0x80015DAA`. The
next live auto-UseItem probe should use the negative hint reason if the pending
hint is absent.

As of 2026-07-03 17:38 +10, live auto-UseItem probe
`C:\nwnbridge\codex-live-quickbar-idle-hint-rerun-20260703-1718\harness-proxy-20260703-171923`
reached gameplay but still had no committed quickbar profile. The hint file
reported `pending_item_refresh=false` and previously surfaced only
`no_committed_quickbar_profile`, while proxy logs showed stream-probe
`GuiQuickbar_SetAllButtons` records with compact item candidates. Proxy2 now
records those stream-probe summaries into semantic UI state. Post-code live
probe
`C:\nwnbridge\codex-live-quickbar-stream-probe-hint-20260703-1745\harness-proxy-20260703-173957`
reached gameplay and reported
`stream_probe_quickbar_item_candidates_without_committed_profile` with
stream-probe item-button/proof counters. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-stream-probe-hint-automation-20260703-1740`
against the current Diamond capture stayed at 0 quarantines and 304 strict
allows. If a future live probe reports this stream-probe no-hint reason, treat
the next harness/proxy target as quickbar stream commitment, not UseItem
injection.

As of 2026-07-03 18:41 +10, the quickbar stream commitment target has a
production fix: the buffered quickbar stream flush now observes the verified
`GuiQuickbar_SetAllButtons` payload through the semantic UI observer after the
rewritten frames are built. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-stream-commit-observe-20260703-184037`
against the current Diamond capture stayed at 0 quarantines, 304 strict
allows, one committed quickbar semantic profile, 39 stream-probe summaries, and
a pending hint for candidate `0x80015DAA` with recommended UseItem payload
`7006090C000000AA5D018000C0`. The next live auto-UseItem probe should verify
whether HG now emits a pending hint instead of
`stream_probe_quickbar_item_candidates_without_committed_profile`; if it does,
drive the recommended UseItem payload and inspect the following committed
quickbar state.

As of 2026-07-03 19:40 +10, fresh live probe
`C:\nwnbridge\harness-proxy-20260703-191931` reached gameplay but still ended
with `stream_probe_quickbar_item_candidates_without_committed_profile`. Proxy2
now splits focused quickbar streams by trying normal CNW-declared quickbar
endpoints before the zero-declared legacy-prefix fallback scan; strict replay
`C:\nwnbridge\codex-replay-declared-first-20260703-1933` against the current
Diamond capture stayed at 0 quarantines and produced a pending UseItem hint for
candidate `0x80015DAA`. A fresh live auto-UseItem probe
`C:\nwnbridge\harness-proxy-20260703-193410` reached gameplay, committed the
36-slot `GuiQuickbar_SetAllButtons` profile (`old_declared=1321`,
`read_size=1314`, `fragment_size=19`, 18 item buttons preserved), then wrote a
stable pending hint for candidate `0x8001612E` with proof `active_object`,
source `direct_only`, and
`recommended_use_item_payload_hex=7006090C0000002E61018000C0`. During the
observed wait window the proxy log still showed no client `Input_UseItem` and
the hint kept `first_client_action="none"`. The next harness target is the
driver-side poll/send path for this ready hint, not proxy-side quickbar
commitment.

As of 2026-07-03 20:29 +10, the driver-side poll/send path is active in
driver-only mode. The bridge DLL now calls
`TryDispatchQuickbarItemRefreshUseItem` from
`HookedServerToPlayerMessageDriverOnly`, matching the non-driver hook. Fresh
live probe
`C:\nwnbridge\codex-live-quickbar-useitem-driverhook-20260703-202458\harness-proxy-20260703-202501`
reached gameplay and wrote a pending hint for candidate `0x800162A4` with
recommended payload `7006090C000000A462018000C0`. The bridge log shows
`quickbar item-refresh UseItem dispatch #1` at
`2026-07-03 20:26:21 +10`; proxy2 then validated and forwarded
`Input_UseItem` for object `0x800162A4`, and the hint recorded
`first_client_action="client_input_use_item"` with
`first_client_action_matches_candidate=true`. The remaining harness/protocol
question is why no server quickbar refresh followed the matched UseItem action
in the observed window (`quickbar_events_since_pending_refresh=0`).

As of 2026-07-03 21:33 +10, proxy2 writes post-action pending-refresh counters
to the live hint and replay summaries. The hint now exposes
`first_event_after_client_action`, `events_after_first_client_action`, and
after-action family buckets. Strict replay
`C:\nwnbridge\codex-proxy2-replay-post-useitem-response-counters-20260703-2132`
against the current Diamond capture stayed at 0 quarantines and 304 strict
allows. Fresh live probe
`C:\nwnbridge\codex-live-post-useitem-response-counters-20260703-2145\harness-proxy-20260703-213130`
reached gameplay, matched and forwarded candidate `0x800164E0`
(`7006090C000000E064018000C0`), then observed no quickbar refresh across 97
post-UseItem events. Future probes should use these counters to distinguish
server response traffic from missing or mistimed client action traffic.

As of 2026-07-03 22:34 +10, proxy2 recommends a target-present UseItem shape
for quickbar item-refresh hints. The target is EE's self sentinel
`0xFFFFFFFD`, which the client-input translator rewrites to Diamond's
`0x7F000000` legacy invalid/self target before forwarding to HG. Strict replay
`C:\nwnbridge\codex-proxy2-replay-useitem-self-target-hint-20260703-222818`
against the current Diamond capture stayed at 0 quarantines and wrote payload
`70060910000000AA5D018000FDFFFFFFC8` for candidate `0x80015DAA`. Fresh live
probe
`C:\nwnbridge\codex-live-useitem-self-target-hint-20260703-223120\harness-proxy-20260703-223124`
reached gameplay and dispatched the self-targeted candidate `0x80016691`; HG
continued sending live-object/inventory/chat/other traffic but still sent no
quickbar refresh after 151 post-action events.

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

Update as of 2026-07-03 08:24 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-client-action-buckets-automation-20260703-0813`
against the same fresh gameplay capture stayed at 0 quarantines, 414 strict
allows, 27 exact live-object rewrites, 147 lifecycle claims, 39 stream-probe
registry-context summaries, and 1 committed quickbar summary. Semantic
pending-refresh diagnostics now export exact client-action buckets from the
verified `ClientInput` and `ClientQuickbar` parsers. The pending
Feature-25-only window still has no later committed quickbar or item buttons;
it reports 265 verified events, 127 live-object, 0 server quickbar, 0
inventory, 1 client input, 0 client UseItem, 0 client UseObject, 0 client
ChangeDoorState, 1 other client input (`Input_WalkToWaypoint`), 0 client
quickbar SetButton, 4 chat, and 133 other. The capture also has two client
`GuiQuickbar_SetButton` actions before the pending item-proof window. The next
useful harness action is to deliberately provoke UseItem or item-bearing client
quickbar SetButton after the pending Feature-25-only proof appears, then check
whether HG emits a later committed `GuiQuickbar_SetAllButtons`.

Update as of 2026-07-03 09:29 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-first-trigger-automation-20260703-0929`
against the same fresh gameplay capture stayed at 0 quarantines with 289 strict
allows, 19 exact live-object rewrites, 93 lifecycle claims, 39 stream-probe
registry-context summaries, and 1 committed quickbar summary. Semantic
pending-refresh diagnostics now export the first follow-up event after the
proof-opening row and the first client action after the pending window opens.
This replay still has 0 quickbar item buttons, 0 post-proof UseItem, and 0
post-proof item `GuiQuickbar_SetButton`; post-context first-follow-up evidence
was mostly live-object traffic (`first_followup_live_object=21`), and the only
first client actions were generic input (`first_client_action_other_input=2`).
The next useful harness action remains a deliberate post-proof UseItem or
item-bearing client quickbar SetButton, now with first-trigger counters to
verify the action landed in the correct pending window.

Update as of 2026-07-03 10:38 +10: strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-action-detail-automation-20260703-1038`
against the same fresh gameplay capture stayed at 0 quarantines, 414 strict
allows, 27 exact live-object rewrites, 147 lifecycle claims, 39 stream-probe
registry-context summaries, and 1 committed quickbar summary. Client
`GuiQuickbar_SetButton` item claims now retain item/target object ids, and
pending-refresh semantic traces retain the first client action's object id,
slot, button type, and body kind. The pending Feature-25-only window still has
0 post-proof UseItem and 0 item SetButton actions; the new detail counters show
only generic input with object id `2147497163`, slot/button zero, and body kind
`none`. The next useful harness action remains deliberately provoking a
post-proof UseItem or item-bearing client quickbar SetButton, then checking
whether HG emits a later committed item-bearing `GuiQuickbar_SetAllButtons`.

Update as of 2026-07-03 11:28 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; the newest gameplay
packet was about 20h04m old and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-candidate-automation-20260703-112533`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, and 1 committed quickbar summary. Semantic item-context
traces now expose a deterministic compact item-emission candidate id, source,
and proof for post-quickbar and pending-refresh windows. This replay reports
37 post-context candidate observations, max object id `2147574964`, all
Feature-25-only proof (`34` first-list and `3` second-list observations), with
0 post-proof UseItem/item SetButton actions and 0 committed quickbar item
buttons. The next useful harness action is to deliberately drive UseItem or an
item-bearing client quickbar SetButton using the surfaced candidate after the
post-proof window opens.

Update as of 2026-07-03 12:26 +10: live-data gate reused
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; the newest gameplay
packet was about 21h05m old and gameplay reached. Strict replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-action-candidate-match-automation-20260703-122155`
stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites, 147
lifecycle claims, and 1 committed quickbar summary. Pending-refresh diagnostics
now export whether the first post-proof client action targets the deterministic
compact item-emission candidate. This capture still has 37 post-quickbar
pending updates and 5 compact item-emission proof objects for candidate
`2147574964`; the only first client actions with candidate context were generic
input against object `2147497163`, so `matches_candidate=false` for all 4
candidate-known samples. The next useful harness action is to drive UseItem or
an item-bearing client quickbar SetButton specifically against candidate
`2147574964` after the post-proof window opens.

Update as of 2026-07-04 18:54 +10: live-data gate reused the gameplay-reaching
proxy harness
`C:\nwnbridge\codex-live-stream-probe-commit-gui-event-20260704-162250\harness-proxy-20260704-162301`;
`quickbar-item-refresh-hint.json` was written at
`2026-07-04T16:27:55+10:00`, about 1h40m old at the gate, and gameplay was
reached through module load, area load, live-object traffic, and the GUI-event
notify path. Proxy2 now exports the first client `GuiEvent_Notify` event A/B,
declared bytes, trailing fragment bytes, vector-present flag, and raw vector
bits in the pending quickbar item-refresh hint. The next live GUI-event or
active-property probe should inspect those `first_client_action_gui_event_*`
fields before deciding whether the missing HG quickbar refresh is caused by the
event ids, payload body, vector branch, timing, or a different action family.
Strict replay
`C:\nwnbridge\codex-proxy2-replay-gui-event-shape-20260704-1855` against
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
stayed at 164 packet files, 304 strict allows, and 0 quarantines.

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
| Live HG receives raw `BNK2` but no `BNK3`, `BNK4`, or `BNCS`; driver log has no `NonWindow` BNK2 begin/result and EE writes a fresh `nwmain-crash-*.nwcrash.txt` | Intermittent EE crypto handoff stall/crash before `HandleBNK2Message` processes the deferred BNK2, or a stale client/proxy state that makes the BNK2 handler unsafe | Stop stale `nwmain`/`hgbridge_proxy2` processes, rerun with `HG_BRIDGE_DRIVER_ONLY_TRACE_BNK_HANDLERS=1`, and inspect proxy `observed EE BNK3 after deferred BNK2` versus `EE crypto handshake stalled after BNK2; no BNK3 received` alongside driver `NonWindow` BNK2 rows. The 2026-07-07 16:37 failure was followed by a 16:47 retry that observed BNK3 after 106ms and reached gameplay. |
| `BNK3`/`BNK4`/`BNCS` succeed, then proxy logs `server BNCR reject result parsed` with `detail=6` and `detail_hint="observed-hg-rapid-reconnect-or-name-reservation"` before the client sends `BNDM` | HG still has a rapid-reconnect or player-name/session reservation for the account/character, usually after a live harness rerun too soon after stopping the previous client | Do not count the failed artifact as gameplay evidence. Stop stale `nwmain` and `hgbridge_proxy2`, wait 2-5 minutes for the HG reservation to clear, and rerun the same harness command. The 2026-07-08 23:06 run failed this way and the 23:13 rerun reached gameplay after cooldown. |
| Capture reaches `BNVR A` and one `P/01/03` response, but never sends client `P/11/01` | Driver fell back to native DirectConnect after missing or discarding the server-list path | Keep using the server-list DirectConnect path; if Diamond's app-state server-list slot is empty, retry with the remembered `SERVERLIST_PANEL` from the constructor hook before native fallback. |
| `PRE_PLAYMOD` selection fires with `entries=0 count=0` | Auto-character path is too early or lacks refresh/retry | Add wait/refresh/retry instrumentation and rerun until the character list is populated or a new blocker is proven. |
| Player-password prompt or native connect overlay appears | Harness regressed to the wrong login path or password handling | Keep the old driver connect path; do not pass native `+password`; seed the player password internally with default `A`. |
| No probe log or packet directory is written | Probe build/injection/run-root setup failed | Rebuild the probe, check run-root permissions, and verify the Diamond process was injected before calling the run useful. |
| HG endpoint is unreachable or the server is down | External live-server blocker | Record the exact network/server failure and retry later; do not claim fresh gameplay evidence. |
| Strict replay fails before launch with `Access is denied` while replacing `target\debug\hgbridge_proxy2.exe` | A stale replay proxy is still holding the debug executable | List `hgbridge_proxy2.exe` processes, stop only the stale debug replay process, or pass `-ProxyExe` with an isolated build output. Leave unrelated live/public proxy processes alone. |
| Strict replay reaches only part of a long capture before the automation timeout, often during `drain dummy server` | Empty UDP receive waits are too expensive for 3k+ packet captures | Use `-DrainReceiveTimeoutMilliseconds 5` or another bounded value for automation replays; keep the default higher value for manual diagnosis when delayed UDP output is under investigation. |
| Strict replay proxy exits before packet replay with `Access is denied. (os error 10013)` while binding the default listen endpoint, such as `127.0.0.1:55121` | Local port reservation, policy, or a stale process owns the default proxy listen port | Retry with an explicit free port pair, for example `-ListenPort 56121 -ServerPort 56133` or `-ListenPort 56221 -ServerPort 56233`, and keep `-DrainReceiveTimeoutMilliseconds 5` for automation replays. The 2026-07-08 inventory/equipment writer replay passed on alternate ports after the default port was denied. |
| Live HG reaches gameplay but writes identical `unclaimed-unknown-high-level` quarantine files for payloads that logs call incomplete/non-header stream continuations | A coalesced zlib stream tail is being passed to high-level packet ownership instead of the stream-continuation path | Fixed 2026-07-06 by classifying single incomplete inflated stream units before high-level parse fallback. If this recurs, inspect `coalesced` stream-continuation handling and require a no-quarantine live rerun before new packet-family work. |
| Live wrapper proxy exits with `unexpected argument --quickbar-item-refresh-hint` before EE launch, or `-SkipBuild` uses an older proxy than the one just built | The wrapper selected a stale proxy2 executable before a fresher compatible build | Use the resolver that checks `--help` for the hint flag, skips stale candidates, selects the newest compatible executable by `LastWriteTime`, rejects stale explicit paths, and honors `-SkipBuild` when no compatible binary exists. |
| GUI-event notify probe reaches BNK/BNCS/character list/login/`Module_Info` and `LoadModuleResources`, but not `Module_Loaded`, `Area_ClientArea`, live-object traffic, or GUI-event dispatch | Historical proxy/module-load handoff blocker: Rust was parsing the EE `Device_AdvertiseProperty` name length where the CNW declared read-buffer length lives | Use the shared `translate::client_device` classifier. Fresh 2026-07-04 14:27 rerun consumed 70 device-property frames and reached gameplay; if this recurs, verify those logs before unrelated action-family work. |
| GUI-event notify probe reaches gameplay but final hint says `stream_probe_quickbar_item_candidates_without_committed_profile` | Proxy2 can parse stream-probe `GuiQuickbar_SetAllButtons` candidates, but semantic state has no committed quickbar profile/candidate | Inspect quickbar stream commitment and profile promotion before injecting GUI-event/UseItem actions. The 2026-07-04 16:22 run added a guarded promotion path; if this recurs, confirm whether `promoted_committed_profile=true` is absent and whether normal `GuiQuickbar` proof was also absent. |
| Subtype-low UseItem probe reaches gameplay and stream-probe quickbar summaries show preserved item buttons, but the hint stays `stream_probe_quickbar_item_candidates_without_committed_profile` or `no_post_committed_item_context` | A focused quickbar stream path observed the profile but did not promote the completed stream-probe slot profile into committed quickbar semantic state | Confirm whether `quickbar_stream` logged `promoted_committed_profile=true`. If absent, fix the stream-probe promotion path before rerunning action-family probes. The 2026-07-05 18:39 rerun confirms the focused stream path can now commit profiles and progress to a pending item-refresh candidate. |
| GUI-event notify probe reaches gameplay, final hint has `first_client_action="client_gui_event_notify"` and `first_client_action_matches_candidate=true`, but `quickbar_events_after_first_client_action=0` and `server_quickbar_item_use_count_events_after_first_client_action=0` | The hinted GUI-event payload lands, but it is not sufficient to make HG emit the original item-refresh quickbar update as either full `GuiQuickbar` or live-object `GQ` item-use-count rows | Trace original-client active-property action semantics/timing before changing broad translation rules. Compare event id/body/vector/timing against Diamond/EE decompiles and live client action captures. |
| UseObject probe reaches gameplay, final hint has `first_client_action_match_class="recommended_use_object"`, but `quickbar_events_after_first_client_action=0` and `server_quickbar_item_use_count_events_after_first_client_action=0` | The bounded `Input_UseObject` payload lands, but it is not sufficient to make HG emit the original item-refresh quickbar update as either full `GuiQuickbar` or live-object `GQ` item-use-count rows | Stop retesting exact probe identity. Trace original-client active-property item action/state semantics beyond SetButton, GuiEvent_Notify, UseItem, and UseObject before changing broad translation rules. |
| UseItem subtype-low probe reaches gameplay, final hint has `first_client_action_match_class="recommended_use_item_first_property_subtype_low"`, but `quickbar_events_after_first_client_action=0` and `server_quickbar_item_use_count_events_after_first_client_action=0` | The decompile-ordered subtype-low `Input_UseItem` payload lands, but it is not sufficient to make HG emit the original item-refresh quickbar update as either full `GuiQuickbar` or live-object `GQ` item-use-count rows | Stop retesting exact probe identity. Trace original-client active-property item action/state semantics beyond SetButton, GuiEvent_Notify, UseObject, zero-byte UseItem, and subtype-low UseItem before changing broad translation rules. |
| Live auto-UseItem hint reports `stream_probe_quickbar_item_candidates_without_committed_profile` | Proxy2 can parse stream-probe `GuiQuickbar_SetAllButtons` candidates, but no accepted committed quickbar profile has reached semantic state | Inspect splitter/stream commitment and quickbar buffering before trying to inject UseItem; the driver should wait for a pending hint or a committed profile. |

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

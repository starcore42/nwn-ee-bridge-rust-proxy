# Active proxy2 issues

This is the working active-issues document for recurring proxy2 development.
Use it to leave concise notes on unresolved generalized protocol/state issues,
suspected packet families, evidence gathered, and next verification steps.
When an issue is confirmed fixed, mark it crossed off with the confirming
evidence and date or remove it from the active list. Specific modules, assets,
resrefs, captures, and chapters belong here only as evidence for a broader rule,
not as standalone workaround targets.

## 2026-06-25 manual automation/code review correction

- 2026-06-28 automation instruction refresh: every recurring run must check
  the newest real live HG harness capture first. Only a capture that reached
  gameplay and is no more than 24 hours old counts as current evidence. If the
  latest gameplay-reaching capture is stale or missing, run a fresh live HG
  capture before ordinary proxy work. If the previous capture did not reach
  gameplay, fix the harness/server-connection blocker first, update
  `docs/harness-regression-policy.md`, and rerun.
- 2026-07-12 preserved active-slot GQ coverage: proxy2 now derives candidate
  selection and harness diagnostics from one typed 36-slot coverage rule. A
  preserved active slot is satisfied only by durable `GQ` state with the same
  wire slot, object id, and item button type; pending and idle hints expose the
  ordered matching and missing slot sets. The selector walks that same missing
  set in wire order and still requires independent item-readiness proof before
  choosing a candidate.

  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-active-gq-coverage-20260712-0320`
  processed 164 packet files with 304 strict allows, zero strict quarantines or
  files, and zero live-object terminal residuals. Fresh HG capture
  `C:\nwnbridge\codex-live-active-gq-coverage-20260712-0300\harness-proxy-20260712-025724`
  reached module/area/live-object gameplay and remained at zero quarantines
  through `2026-07-12T03:00:16+10:00`. At quickbar commit its 21 retained
  active slots were all missing GQ state. After HG completed gameplay state,
  all 21 exact slots moved to matching GQ coverage; 66 item objects were ready,
  52 ClientGui items materialized, and one confirmed Inventory replay was
  dispatched. Suppressing the redundant automatic `UseItem` was therefore
  correct for this character. The next action capture requires a different
  module/character state where the coverage remains nonempty after inventory
  materialization; do not keep forcing actions against this fully satisfied
  21-slot profile.
- 2026-07-12 alternate-account live gate and password-flow ownership: the HG
  harness now binds proxy2 and the launcher to the selected Diamond account
  before proxy startup, and the bridge prefers that selected profile name over
  stale native app-manager state. For characters behind HG's spoken-password
  gate, successful `Module_Loaded` provides an opt-in fallback send only when
  the earlier prompt detector made no attempt.

  The first account-1 entry exposed an 81-byte `ClientSideMessage_Feedback`
  `0x12/0x0B`, id `0x5E`. EE `CNWSMessage::SendServerToPlayerCCMessage` case 11
  writes WORD id, OBJECTID, bounded `CExoString`, then a build-gated BOOL;
  `CNWCCMessageData::GetInteger` returns zero for the legacy message's absent
  integer. Proxy2 now accepts only that exact read boundary and canonicalizes
  the source three-header-bit fragment to one false BOOL. The opt-in client
  password send then exposed client `Chat_Talk`: EE sender `sub_1407BA7E0`
  creates `text_len + 4`, writes exactly one 32-bit-length `CExoString`, and
  sends `0x09/0x01` without fragment data bits. A separate `ClientChat` family
  now owns exactly that source shape; shifted string or fragment cursors are
  rejected.

  Fresh HG capture
  `C:\nwnbridge\codex-live-account1-owned-talk-20260712-1325\harness-proxy-20260712-132508`
  used account 1 and `starcore-stormre`, strictly allowed the password talk as
  `ClientChat`, reached `Module_Loaded`, `Area_ClientArea`, `Area_AreaLoaded`,
  sustained `GameObjUpdate_LiveObject`, and committed a 36-slot/14-item
  quickbar through `2026-07-12T13:27:05+10:00`. All 14 slots eventually gained
  matching GQ state, so the actionable-missing set correctly remained empty.
  ~~The later `U/5 0x4408` record was rejected after its five effect rows were
  translated because the transport boundary scanner split inside their inserted
  EE identity maps. Resolved in code 2026-07-12; direct live recurrence remains
  pending.~~ Diamond `sub_44ADD0` and EE `sub_140781E80` prove the ordered body:
  `0x0008` WORD count plus typed effect rows, `0x0400` four WORD scalars, then
  `0x4000` seven CNW BOOLs without more read bytes. The typed transport owner now
  keeps that full byte span together and leaves exact validation to advance the
  inherited fragment cursor `3 -> 10`. The private 962-byte live regression
  rewrites all five rows, then exact-claims the following `I/0xD5FF` state through
  bit 153. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-u5-4408-boundary-retry-20260712-1530`
  processed 164 files with 304 strict allows, zero quarantines/files, and zero
  terminal live-object residuals.

  Fresh verification capture
  `C:\nwnbridge\codex-live-u5-4408-boundary-20260712-1512\harness-proxy-20260712-151148`
  reached module/area/live-object gameplay and strictly owned the password talk,
  but `0x4408` did not recur before shutdown. It exposed two new generalized
  live-object blockers instead: a 1,987-byte top-level `G I A` inventory stream
  rejected at the first GUI boundary, and an 88-byte `U/5` current-player update
  with mask `0x0000004F` rejected at its typed record boundary. Preserve both as
  next regression seeds; do not treat the run as clean-zero-quarantine evidence.

  ~~The 1,987-byte top-level `G I/R A` stream was rejected because the earlier
  add-map transport walker entered a fragment-proven GUI item body and treated
  active-property bytes as a top-level creature add. Resolved in code
  2026-07-12; direct live recurrence remains pending.~~ Diamond `sub_4589A0`
  and EE `sub_1407B3F30` prove that the GUI prefix hands the whole nested object
  to the shared item-create reader. The add-map walker now carries that focused
  row end and exact Diamond fragment cursor, and hard-stops rather than scanning
  inside an unproven GUI row. A fixture-free active-property lookalike regression
  passes through the production adapter. The original 30-row live stream also
  rewrites from 1,987 to 2,401 bytes and exact-claims all 30 item-create rows,
  consuming 200 GUI fragment bits. The independent `U/5 0x0000004F` record is
  now the first unresolved quarantine from this capture.

  Fresh current-code HG capture
  `C:\nwnbridge\codex-live-gui-add-boundary-20260712-171117\harness-proxy-20260712-171118`
  reached `Module_Loaded`, `Area_ClientArea`, proxy-generated
  `Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject` gameplay through
  `2026-07-12T17:13:44+10:00`. It wrote no quarantine files, but it repeatedly
  rejected one three-span `PlayerList_Add` / `PlayerList_All` / `Chat_Talk`
  coalesced datagram as `coalesced-record-proof-invalid`. The focused chat
  claimant saw the exact declared string boundary and one fragment byte, but
  rejected nonzero unused bits below the three-bit CNW fragment header. The
  `G I/R A` and `U/5 0x0000004F` shapes did not recur before this earlier
  blocker, so neither gains direct live confirmation from the run. Before
  changing chat ownership, trace Diamond `GetWriteMessage` and the EE fragment
  reader to prove whether unused tail bits are ignored; if they are, canonicalize
  only those unused bits before coalesced typed proof rather than relaxing the
  exact chat body or broad coalesced validation.
- 2026-07-12 active quickbar-slot diagnostics and Chat_Talk ownership: proxy2
  now writes the exact count and ordered slot array for all retained
  decompile-owned 36-slot active-item signatures into both pending and idle
  `quickbar-item-refresh-hint.json` states. Fresh HG gameplay capture
  `C:\nwnbridge\codex-live-active-slot-hint-20260712-0052\harness-proxy-20260712-005140`
  proved 21 signatures at slots
  `[0,1,5,10,11,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35]`, 52
  materialized items, one dispatched confirmed Inventory replay, and prior-GQ
  suppression for slot-0 candidate `0x800162EE`. That run also reduced two
  quarantined `0x09/0x01` talk messages to the generalized original-client
  shape: speaker OBJECTID, bounded `CExoString`, exact declared-byte handoff,
  and one fragment byte containing only the three-bit CNW header.

  Diamond `CNWSMessage::SendServerToPlayerChatMessage` helper `0x0043D9A0`
  proves that channel 1 writes the object id and string, performs no fragment
  field write, and sends family/minor `0x09/0x01`. Proxy2 now claims only that
  exact `Chat_Talk` shape and rejects trailing read bytes or fragment data bits.
  Focused chat tests and strict replay
  `C:\nwnbridge\codex-proxy2-replay-chat-talk-active-slots-20260712-0102`
  passed; the replay processed 164 packets with 304 strict allows, zero
  quarantines, and zero live-object terminal residuals. The first verification
  retry stalled after BNK2; a traced retry observed BNK3 after 70 ms and fresh
  capture
  `C:\nwnbridge\codex-live-chat-talk-active-slots-retry-20260712-0107\harness-proxy-20260712-010711`
  reached module/area/live-object gameplay, reproduced the 21-slot set,
  materialized 52 items, dispatched one Inventory replay, and remained at zero
  quarantine files through `2026-07-12T01:09:41+10:00`. The exact talk message
  did not recur in the retry, so the next live recurrence remains the direct
  production proof for `Chat_Talk`; the quickbar action target remains a
  preserved active slot whose matching durable GQ row is absent.
- 2026-07-11 unresolved preserved quickbar-item selection: proxy2 now retains
  the active-item signature at every decompile-owned 36-slot
  `GuiQuickbar_SetAllButtons` position. Post-materialization action probes walk
  those slots in wire order and choose the first independently ready item that
  lacks a matching durable `GQ` use-count row; the candidate's own active
  property drives subtype-low `UseItem` construction and suppression. If all
  preserved active items are already satisfied, the prior-GQ resolver closes
  the window without sending a redundant action. Regression coverage proves a
  satisfied slot 0 advances to unresolved slot 1. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-unsatisfied-preserved-item-20260711-2320`
  processed 164 packets with 304 strict allows, zero quarantines, and zero
  live-object terminal residuals.

  Fresh HG capture
  `C:\nwnbridge\codex-live-unsatisfied-preserved-item-20260711-2305\harness-proxy-20260711-230422`
  reached `Module_Loaded`, `Area_ClientArea`, proxy-generated
  `Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject` through
  `2026-07-11T23:07:12+10:00` with zero quarantine files. It committed 36
  slots/21 items, materialized 52 ClientGui items, dispatched the confirmed
  inventory replay, and retained slot-0 candidate `0x80015866`; HG had already
  supplied its exact button-type-1 `GQ` row, so no `UseItem` was sent. Next
  production path: expose the preserved active-signature count/slot set in the
  harness hint and capture a module/character state with a preserved active
  item whose `GQ` row is genuinely absent, then use that real EE action/response
  window to implement the first proven engine-facing mismatch.
- ~~2026-07-11 ClientGui status response-window over-attribution: resolved and
  live-confirmed 2026-07-11.~~ Capture
  `C:\nwnbridge\codex-live-item-action-current-20260711-0442\harness-proxy-20260711-044124`
  reached gameplay with zero quarantine, but after its second proxy-owned
  `ClientGuiInventory_Status` request the bridge attributed 49 later
  `GameObjUpdate_LiveObject` packets to that request even though server sequence
  58 had already returned the matching 52-item materialized set. The retained
  terminal response stayed correct, but the open-ended window polluted response
  counts and allowed unrelated later object state to replace the last response.

  Proxy2 now closes each queued status response window when the first
  materialized response proves the queued candidate, ignores later live-object
  traffic for that update, and reopens attribution when a newer bridge update
  queues another status request. Fresh HG capture
  `C:\nwnbridge\codex-live-clientgui-response-window-20260711-205328\harness-proxy-20260711-205330`
  reached `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject` through `2026-07-11T20:56:23+10:00` with zero
  quarantine files. Its one status request completed on the first matching
  51-item materialized response (`server_sequence=48`); after more than a minute
  of later gameplay the final hint still reported exactly one response packet,
  one materialized packet, and `matches_queued_status_candidate`. Next
  production path: capture a real EE action for a preserved materialized item
  in a run where no genuine `GQ` row has already satisfied the refresh, then
  implement the first proven shared quickbar or engine-facing state mismatch.
- 2026-07-11 quickbar-relevant materialized candidate selection: fresh live HG
  capture
  `C:\nwnbridge\codex-live-quickbar-preferred-candidate-20260711-004730\harness-proxy-20260711-004731`
  reached `Module_Loaded`, `Area_ClientArea`, sustained live-object gameplay,
  materialized 51 inventory items, committed a 36-slot/21-item quickbar, and
  produced zero quarantine files through `2026-07-11T00:50:04+10:00`. The old
  post-quickbar candidate rule chose the numerically lowest ready inventory
  object; in the preceding clean capture that was `0x80016045` while the first
  preserved quickbar item was `0x8001604D`, leaving an unrelated false pending
  refresh. Proxy2 now prefers the first preserved quickbar item only when the
  object registry independently proves that exact object is ready, then falls
  back to the existing direct/shared/Feature-25 proof order. The fresh run
  proves the quickbar candidate became preserved slot-0 item `0x80016172`, while
  the separate generic inventory bridge candidate remained `0x8001616A`.

  ~~The same run exposed a generalized reliable-replay state issue: cached
  retransmission of one split quickbar/Inventory unit reapplied semantic events
  and proxy-owned side effects. Resolved and live-confirmed 2026-07-11.~~
  Coalesced direct and deflated records now retain typed replay entries keyed by
  source sequence, record offset, and exact gameplay bytes. Retransmits refresh
  their ACK/packetized transport fields but do not re-enter the semantic reducer
  or bridge-output scheduler; coalesced split output is no longer parsed a
  second time as if its compressed bytes were high-level payloads.

  Fresh HG capture
  `C:\nwnbridge\codex-live-coalesced-side-effects-20260711-025757\harness-proxy-20260711-025759`
  reached `Module_Loaded`, `Area_ClientArea`, and sustained live-object gameplay
  through `2026-07-11T03:01:07+10:00` with zero quarantine files. It exercised
  29 direct and 18 deflated typed-cache replays. The repeated burst contained
  exactly one semantic server `Inventory_Equip` claim instead of repeatedly
  advancing that claim; the earlier false 18-quickbar-event result did not
  recur. One genuine live-object `GQ` use-count row resolved preserved slot-0
  candidate `0x800180D3` before the opt-in UseItem could dispatch, so this is
  clean refresh/replay evidence rather than an item-action result. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-coalesced-side-effects-20260711-025602`
  processed 164 packets with 304 strict allows, zero strict quarantines, zero
  quarantine files, and zero live-object terminal residuals. Next production
  path: capture a real EE item action after materialization when no genuine GQ
  row has already satisfied the refresh, then implement the first proven shared
  quickbar or engine-facing mismatch instead of forcing a redundant action.
- ~~2026-07-10 post-inventory full-appearance visible-equipment quarantine:
  resolved and live-confirmed 2026-07-10.~~ The original gameplay capture
  `C:\nwnbridge\codex-live-mainloop-inventory-proof-20260710-204026\harness-proxy-20260710-204028`
  produced one unique 417-byte P/5 stream containing a current-player inventory
  header, creature add, full appearance with five dummy and three nested item
  rows, and following `U/5 0x3967`. The generalized defect was byte-side, not a
  borrowed fragment bit or missing property value: the second item's bare-inline
  name `Militia Shield` is followed by cost DWORD `0x00000032`, whose printable
  low byte was greedily consumed as a trailing `2`. The parser now tests bounded
  printable endpoints longest-first and accepts only the endpoint whose complete
  decompile-backed active-property suffix is exact.

  Diamond `sub_451020` and EE `sub_14076BD30` prove the bit handoff. Starting at
  P/5 cursor 6, the creature locstring pair consumes 3 bits; the three item-name
  branches then consume 6, 6, and 7 Diamond bits and land the following U/5
  exactly at cursor 28. EE inserts one active-property BOOL per item, so the
  appearance span widens from 22 to 25 bits without borrowing from U/5. The
  private 417-byte regression seed
  `proxy2/fixtures/live_object/hg_live_inventory_town_watch_20260710_legacy.bin`
  now exact-translates and claims.

  Fresh HG capture
  `C:\nwnbridge\codex-live-visible-equipment-cost-boundary-20260710-231503\harness-proxy-20260710-231505`
  reached `Module_Loaded`, `Area_ClientArea`, sustained live-object gameplay,
  and the forced inventory refresh. HG materialized 52 item rows, proxy2
  released and transport-dispatched exactly one retained Inventory replay, the
  final hint reports `client_gui_status_inventory_replay_dispatched`, and the
  visible-equipment stream reached exact EE shape. Artifacts continued through
  `2026-07-10T23:18:17+10:00` with zero quarantine files. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-visible-equipment-cost-boundary-20260710-231327`
  processed all 164 baseline packets with strict translation, zero strict
  quarantines, zero quarantine artifacts, and zero live-object terminal
  residuals.
- 2026-07-10 PlayerList inline-name repair and game-thread delayed action:
  the live-data gate used
  `C:\nwnbridge\codex-live-confirmed-inventory-replay-manual-20260710-170009\harness-proxy-20260710-170013`
  (`proxy.stdout.log` through `2026-07-10T17:03:49+10:00`, about 1h36m old
  at the `18:39+10:00` gate; gameplay reached). Reduction of both quarantined
  430-byte `PlayerList_All` units found the same generalized legacy defect:
  three of six rows retained a zero player-name CExoString length followed by
  printable name bytes and then the same row's creature object id. Diamond
  client `sub_453BD0` and EE
  `CNWSMessage::SendServerToPlayerPlayerList_All` both prove the order
  player id, player object, DM BOOL, player-name CExoString, has-creature BOOL,
  optional EE identity, and creature body. The captured fragment
  `84 44 44 4B` is exactly the 28 MSB-first header/BOOL bits for six rows.
  Proxy2 now repairs only the zero-length/name/repeated-object-id boundary,
  then requires the complete typed body and fragment claim. Both real units
  rewrite from 430 to 460 bytes with 6 EE identity insertions, 3 name-length
  repairs, 1 existing locstring repair, 28 consumed bits, and no fragment
  rewrite or residual. The driver also retains the scheduling CNWMessage and
  services a due harness-only inventory action from EE's client internal main
  loop on the game thread instead of depending on a later server dispatch.
  Fresh live capture
  `C:\nwnbridge\codex-live-mainloop-playerlist-20260710-1903\harness-proxy-20260710-190221`
  reached `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject` through
  `2026-07-10T19:04:41+10:00` with zero quarantine. That session emitted only
  one-row PlayerList traffic and no `Party_GetList`, so it did not schedule or
  exercise the delayed main-loop action. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-playerlist-mainloop-20260710-1911`
  processed the 164-packet Diamond baseline with 304 strict allows, 0 strict
  quarantines, 0 quarantine files, and 0 live-object terminal residuals.
  Active next verification: capture a recurrence of the six-row PlayerList and
  a run that emits `Party_GetList`; require a `source=client main loop`
  dispatch, real `ClientGuiInventory`, one confirmed Inventory replay, and
  zero quarantine.
- 2026-07-10 confirmed Inventory replay after ClientGui materialization:
  live-data gate reused the clean gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-clientgui-refresh-confirmed-current-20260710-0710\harness-proxy-20260710-070818`
  (`proxy.structured.log` through `2026-07-10T07:12:44+10:00`, about 9h22m
  old at the `2026-07-10T16:35+10:00` gate; gameplay reached; no quarantine).
  Proxy2 now retains an original server `Inventory_Equip` claim while its
  unknown item triggers the proxy-owned `ClientGuiInventory_Status` fallback,
  and replays that exact EE Inventory result only after one associated
  materialized response proves both the queued current-item candidate and the
  original claim object. The replay follows the complete response frame batch,
  uses the decompile-backed EE Inventory writer/validator order (object id and
  equip slot in the read buffer, result BOOL in the MSB fragment), and is
  idempotent per bridge update. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-confirmed-inventory-replay-current-20260710-165225`
  processed 164 packet files with 304 strict allows, 0 strict quarantines,
  no quarantine directory, and 0 live-object terminal residuals; the baseline
  does not contain the live ClientGui fallback, so its new counters correctly
  remain zero. Fresh live probes
  `C:\nwnbridge\codex-live-confirmed-inventory-replay-20260710-165429\harness-proxy-20260710-165434`
  and
  `C:\nwnbridge\codex-live-confirmed-inventory-replay-manual-20260710-170009\harness-proxy-20260710-170013`
  both reached gameplay, but did not exercise the replay. The retry scheduled
  auto-inventory from `Party_GetList` at `17:01:34+10:00` with a 25-second
  delay, then received no later dispatch callback on which the driver could
  service the due action before disconnect. Both probes also quarantined an
  unclaimed 430-byte, six-entry `PlayerList_All` stream unit (`P 0A 01`,
  declared length `0x01AA`). Active next paths: make delayed auto-inventory
  fire from a reliable post-area callback/timer rather than waiting
  indefinitely for another client dispatch; reduce the new PlayerList shape
  to a typed, decompile-backed locstring/cursor variant; then rerun live and
  require
  `inventory_equipment_bridge_output_status="client_gui_status_inventory_replay_queued"`,
  exactly one confirmed replay packet, and no quarantine.
- 2026-07-10 ClientGui status refresh confirmation: live-data gate reused the
  gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-clientgui-fallback-current-20260710-031303\harness-proxy-20260710-031307`
  (`quickbar-item-refresh-hint.json` through `2026-07-10T03:17:07+10:00`,
  about 3h43m old at the `2026-07-10T06:59:59+10:00` gate; gameplay reached;
  no quarantine directory). Proxy2 now promotes a materialized
  `ClientGuiInventory_Status` response whose item-id set contains the queued
  status candidate to first-class bridge output status
  `client_gui_status_refresh_confirmed`, and writes
  `inventory_equipment_bridge_output_client_gui_status_refresh_confirmed` into
  quickbar hints and replay summaries. Fresh live HG forced-inventory probe
  `C:\nwnbridge\codex-live-clientgui-refresh-confirmed-current-20260710-0710\harness-proxy-20260710-070818`
  reached gameplay, wrote artifacts through `2026-07-10T07:12:44+10:00`,
  produced no quarantine directory, and confirmed
  `inventory_equipment_bridge_output_status="client_gui_status_refresh_confirmed"`,
  1 queued proxy-owned `ClientGuiInventory_Status`, response outcome
  `materialized_items`, best association `matches_queued_status_candidate`,
  `...matches_queued_status_candidate=true`,
  `...materialized_item_object_ids_contain_queued_candidate=true`, and 52 best
  materialized item ids. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-clientgui-refresh-confirmed-20260710-0720`
  over the 164-packet Diamond autoplay baseline ran with 304 strict allows,
  0 strict quarantines, 0 quarantine files, and 0 live-object terminal
  residuals; that replay baseline does not exercise the live status path and
  therefore remains at `awaiting_bridge_state_update`. Active next path: use
  the confirmed status refresh as the proof seed for the generalized inventory
  UI refresh or visible-equipment output rule, then prove whether the EE client
  actually updates inventory/equipment state after the confirmed refresh.
- 2026-07-10 ClientGui status response materialized-set association:
  live-data gate reused the gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-clientgui-fallback-current-20260710-031303\harness-proxy-20260710-031307`
  (`proxy.stdout.log` through `2026-07-10T03:17:07+10:00`, about 1h42m
  old at gate; gameplay reached; no quarantine directory). That capture queued
  one proxy-owned `ClientGuiInventory_Status` request for ready candidate
  `0x8001538E`; the best materialized response reported compact current
  candidate `0x80015386` and candidate delta `-8`, but also materialized 52
  item object ids. Proxy2 now carries the exact materialized item-id set
  through the live-object reducer, records first/last/min/max ids and whether
  the set contains the queued ClientGui status candidate, and classifies a
  response as `matches_queued_status_candidate` when the materialized set
  contains that queued candidate even if the compact current candidate differs.
  Focused tests
  `cargo test -q -p hgbridge-proxy2 client_gui_status_response -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2 inventory_equipment -- --nocapture`
  passed; strict replay
  `C:\nwnbridge\codex-proxy2-replay-clientgui-materialized-association-20260710-051009`
  over the 164-packet Diamond autoplay baseline ran with strict translation,
  0 quarantine files, and 0 live-object terminal residuals. Active next path:
  run a fresh delayed forced-inventory live HG probe on this build and confirm
  `...materialized_item_object_ids_contain_queued_candidate=true` plus
  `matches_queued_status_candidate`; then implement the generalized inventory
  UI refresh or visible-equipment output rule.
- 2026-07-10 unknown server Inventory claim fallback: live-data gate reused the
  gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-deflated-clientgui-hook-20260710-010951\harness-proxy-20260710-010955`
  (`proxy.structured.log` through `2026-07-10T01:11:58+10:00`, about 1h45m
  old at gate; gameplay reached; no quarantine directory). That capture showed
  the generalized server-Inventory candidate/claim mismatch that blocked the
  proxy-owned ClientGui status path. Proxy2 now treats an unproven server
  Inventory claim mismatch as a request to refresh current-player ClientGui
  inventory state instead of emitting an EE Inventory packet for the unknown
  object: it queues an exact `ClientGuiInventory_Status` payload
  `700D010B0000000000007F90`, retains both the original server Inventory claim
  and the synthetic current-player ClientGui claim in bridge state, and leaves
  the old blocked mismatch behavior in place when no latest client sequence is
  available. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-clientgui-fallback-20260710-030823`
  over the 164-packet Diamond autoplay baseline ran with 304 strict allows,
  0 strict quarantines, no quarantine directory, and 0 live-object terminal
  residuals. Post-change live HG probe
  `C:\nwnbridge\codex-live-inventory-clientgui-fallback-current-20260710-031303\harness-proxy-20260710-031307`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`,
  proxy-generated `Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject`;
  produced no quarantine directory; queued 1 proxy-owned
  `ClientGuiInventory_Status` for unknown server Inventory claim `0x8001543E`
  versus ready candidate `0x8001538E`; and observed 85 post-status live-object
  responses including 1 live-GUI/materialized-item response. This is the live
  proof that the fallback reaches the materialized ClientGui path; the returned
  response/candidate mismatch is now handled by the materialized-set
  association slice above.
- 2026-07-10 ClientGui status deflated-response attribution hook: live-data
  gate reused the gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-client-gui-status-association-current-20260709-225811\harness-proxy-20260709-225914`
  (`proxy.structured.log` through `2026-07-09T23:01:47+10:00`, about 1h56m
  old at gate; gameplay reached; no quarantine directory). That capture proved
  the active attribution gap: proxy2 queued 17 proxy-owned
  `ClientGuiInventory_Status` requests for candidate `0x80015379`; the exact
  live-object validator saw one deflated response payload with
  `live_gui_records=1` and `materialized_item_object_ids=21`, but the bridge
  response counters only retained later generic live-object-only follow-ups.
  Proxy2 now records proxy-owned ClientGui status live-object responses
  immediately after deflated M reassembly semantic observation, before the
  payload is recompressed and emitted, using the reassembly first sequence and
  last ACK sequence as the response key. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-deflated-clientgui-hook-20260710-010459`
  over the 164-packet Diamond autoplay baseline ran with 304 strict allows,
  0 strict quarantines, no quarantine directory, and 0 live-object terminal
  residuals. Post-change live HG probe
  `C:\nwnbridge\codex-live-deflated-clientgui-hook-20260710-010951\harness-proxy-20260710-010955`
  reached gameplay, sustained live-object traffic, produced no quarantine
  directory, and wrote artifacts through `2026-07-10T01:11:58+10:00`, but did
  not exercise the status-response hook because this run stopped at
  `inventory_equipment_bridge_output_status="blocked_candidate_mismatch"` and
  queued 0 proxy-owned `ClientGuiInventory_Status` requests. Active next path:
  resolved by the 2026-07-10 unknown server Inventory claim fallback above; the
  remaining active target is response/candidate association and the final
  generalized inventory UI refresh or visible-equipment output rule.
- 2026-07-09 ClientGui status response live retest: fresh delayed
  forced-inventory HG proxy capture
  `C:\nwnbridge\codex-live-client-gui-status-association-current-20260709-225811\harness-proxy-20260709-225914`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`,
  proxy-generated `Area_AreaLoaded`, and sustained live-object traffic, wrote
  artifacts through `2026-07-09T23:01:47+10:00`, and produced no quarantine
  directory. It queued 17 proxy-owned `ClientGuiInventory_Status` requests for
  candidate `0x80015379` and observed 18 post-status live-object responses, all
  `live_object_only` with 0 counted live-GUI/materialized-item response
  packets. The exact-validator log did see a separate
  `live_gui_records=1` / `materialized_item_object_ids=21` live-object packet
  between the first and later status bursts, but the bridge did not classify it
  as a proxy-owned status response. Proxy2 now tie-breaks equal-strength
  retained ClientGui status responses by latest queued update/ACK/ready count,
  so repeated live-object-only status responses keep the final hint associated
  with the latest matching candidate instead of an obsolete
  `queued_update_mismatch`. Active next path: wire/split-response attribution
  for the live-GUI/materialized live-object packet, then decide whether the
  ClientGui inventory UI needs a separate EE-facing visible-equipment/status
  output rule.
- 2026-07-09 ClientGui status response association: live-data gate reused the
  gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-c008-delayed-inventory-confirm-20260709-185755\harness-proxy-20260709-185759`
  (`quickbar-item-refresh-hint.json` and logs through
  `2026-07-09T19:01:19+10:00`, about 1h55m old at the 20:56 +10 gate;
  gameplay reached; no quarantine directory). Proxy2 now preserves the ready
  item-state candidate on each queued proxy-owned
  `ClientGuiInventory_Status` request and classifies the best retained
  materialized ClientGui status response against that queued request rather
  than against a later server-Inventory decision. New hint/replay fields expose
  the queued status candidate, ready counts,
  `inventory_equipment_bridge_output_best_client_gui_status_response_association`,
  a match BOOL, and the candidate delta. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-gui-response-association-20260709-2104`
  over the 164-packet Diamond autoplay baseline ran with 304 strict allows,
  0 strict quarantines, no quarantine directory, and inactive ClientGui
  association defaults. Superseded by the 2026-07-10 materialized-set
  association slice above; the remaining active proof is a fresh live HG run on
  that build and then the concrete inventory UI refresh or visible-equipment
  output rule.
- 2026-07-09 ClientGui status response best-evidence tracking: live-data gate
  reused the gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-current-live-object-diagnostics-20260709-125914\harness-proxy-20260709-125919`
  (`proxy.structured.log` through `2026-07-09T13:01:31+10:00`, about 5h55m
  old at the 18:56 +10 gate; gameplay reached; no quarantine directory). A
  fresh delayed forced-inventory confirmation run
  `C:\nwnbridge\codex-live-c008-delayed-inventory-confirm-20260709-185755\harness-proxy-20260709-185759`
  reached gameplay, produced 20 ready `ClientGuiInventory` handoffs, queued 20
  proxy-owned `ClientGuiInventory_Status` packets, observed 52 live-object
  responses after those requests, including 9 live-GUI/materialized-item
  response packets, and produced no quarantine files. This confirms the prior
  seq51 `U/5` mask `0x0000_C008` live-object quarantine did not recur on the
  delayed inventory path. Proxy2 now preserves the strongest observed
  proxy-owned ClientGui status response separately from the latest live-object
  follow-up and exports
  `inventory_equipment_bridge_output_client_gui_status_response_outcome` plus
  `inventory_equipment_bridge_output_best_client_gui_status_response_*` fields
  in hints/replay summaries, so later generic movement/status packets cannot
  erase materialized live-GUI response evidence. Active next path: use the best
  materialized ClientGui status response to associate returned live-GUI item
  state with the bridge candidate and decide the next inventory UI refresh or
  visible-equipment output rule.
- 2026-07-09 C008 live-object status/self repair: live-data gate reused the
  gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-current-live-object-diagnostics-20260709-125914\harness-proxy-20260709-125919`
  (`proxy.stdout.log` through `2026-07-09T13:01:31+10:00`, about 3h56m old at
  the 16:57 +10 gate; gameplay reached; no quarantine directory). The delayed
  forced-inventory seq51 516-byte candidate reduced to a first reject row
  `U/5` object `0xFFFFFFDE`, mask `0x0000_C008`, split at 12 bytes with
  `bit_cursor=3`: the boundary scanner was treating embedded status-effect
  `A` rows as top-level live-object boundaries. Proxy2 now models C008 as the
  C408 status/self suffix family without the 0x0400 four-WORD scalar branch:
  count-derived legacy scan floor, EE-shaped byte boundary after status-effect
  identity maps, exact ten fragment-BOOL suffix validation, and generic
  compact-row identity-map insertion before claim. Focused tests
  `cargo test -q -p hgbridge-proxy2 creature_c008 -- --nocapture` and
  `cargo test -q -p hgbridge-proxy2 creature_status_effect -- --nocapture`
  passed; bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-c008-status-self-20260709-171155` over the
  2026-07-03 Diamond autoplay packet stream ran with strict translation, 0
  quarantine files, and 0 live-object terminal residuals. The 18:57 +10
  delayed forced-inventory confirmation above reached the same inventory path
  with no quarantine files, so the seq51 C008 blocker is confirmed fixed as of
  2026-07-09. Active next path moved to ClientGui inventory
  response/candidate association work.
- 2026-07-09 live-object strict-family diagnostic follow-up: live-data gate
  first found the gameplay-reaching forced-inventory capture
  `C:\nwnbridge\codex-live-client-gui-status-delayed-inventory-20260709-105516\harness-proxy-20260709-105528`
  current (`proxy.structured.log` through `2026-07-09T10:58:21+10:00`, about
  2h old at gate). It reached gameplay but quarantined server seq51
  `P/05/01` (`live-object-unclaimed-strict-family`, declared `0x013D`) after a
  516-byte `live-object-semantic-candidate-rejected-exact-validator` dump. A
  fresh current-code diagnostic run
  `C:\nwnbridge\codex-live-current-live-object-diagnostics-20260709-125914\harness-proxy-20260709-125919`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`,
  proxy-generated `Area_AreaLoaded`, held-packet release, and sustained
  live-object/quickbar traffic with no quarantine directory, but
  `AutoOpenInventory` did not produce `ClientGuiInventory` events before the
  client/proxy stopped advancing; use it as freshness evidence only. Proxy2 now
  logs declared/read/fragment lengths, exact-claim reject stage/cursor, and
  declared-repair candidates for intermediate semantic rewrite candidates that
  fail the exact validator. Active next path: reproduce the delayed-inventory
  seq51 shape or replay its dumped 516-byte candidate and fix the live-object
  translator/declared-window rule before continuing ClientGui writer behavior.
- 2026-07-09 reject-row preview diagnostic: live-data gate reused current
  gameplay-reaching evidence
  `C:\nwnbridge\codex-live-current-live-object-diagnostics-20260709-125914\harness-proxy-20260709-125919`
  (`proxy.structured.log` through `2026-07-09T13:01:31+10:00`, about 1h55m
  old at gate, gameplay reached, no quarantine directory). Proxy2 now threads
  the exact reject-row preview through `claim_payload_diagnostics` and both
  live-object reject log sites: reject window length, opcode/ascii, object
  type, object id, and the first WORD/DWORD after the object id. This does not
  relax strict validation or accept new rows; it only makes the next replay/live
  failure identify which `A/D/G/P/U/W` row and mask-like header failed. Bounded
  strict replay
  `C:\nwnbridge\codex-proxy2-replay-reject-record-preview-20260709-1505`
  processed the 164-packet Diamond autoplay baseline with 304 strict allows, 0
  strict quarantines, 0 quarantine files, and 0 live-object terminal residuals.
  The next run used those reject fields to implement the C008 status/self
  cursor repair above; keep this entry as the diagnostic evidence trail.
- 2026-07-07 BNK2 stall diagnostic and inventory/equipment handoff live
  confirmation: live-data gate first found the gameplay-reaching HG proxy
  capture
  `C:\nwnbridge\codex-live-feature25-handoff-outcome-20260707-20260707-125516\harness-proxy-20260707-125522`
  current (`proxy.structured.log` last write
  `2026-07-07T12:58:49+10:00`, about 3h36m old at gate). The first fresh
  current-build harness
  `C:\nwnbridge\codex-live-inventory-equipment-handoff-current-20260707-163707\harness-proxy-20260707-163721`
  failed before gameplay: EE received raw `BNK2` at `2026-07-07T16:37:38+10:00`
  but never entered the `NonWindow`/`HandleBNK2Message` path, emitted no BNK3,
  and wrote crash report
  `C:\Users\User\Documents\Neverwinter Nights\crashreport\nwmain-crash-1783406258.nwcrash.txt`.
  Proxy2 now records deferred BNK2 handoff progress in production diagnostics:
  successful `BNK3` observation, handshake restart before BNK3, or a 2s
  `BNK3` stall warning. Retry harness
  `C:\nwnbridge\codex-live-bnk3-stall-diagnostic-20260707-164655\harness-proxy-20260707-164703`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, produced no quarantine directory, and logged
  `observed EE BNK3 after deferred BNK2` after 106ms. Its final
  `quickbar-item-refresh-hint.json` at `2026-07-07T16:49:38+10:00` resolved by
  prior quickbar use-count state with
  `inventory_equipment_handoff_ready=true`,
  `inventory_equipment_handoff_outcome="ready_item_state_with_deferred_feature25_refs"`,
  18 ready direct item-proof objects, and 2 deferred Feature-25-only objects.
  The 2026-07-07 consumer slice below now owns this handoff readiness in shared
  UI state. If BNK2/no-BNK3 recurs, use the new proxy BNK diagnostics plus the
  opt-in driver BNK handler trace before unrelated packet work.
- 2026-07-07 inventory/equipment handoff consumer buckets live confirmation:
  live-data gate reused the fresh gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-bnk3-stall-diagnostic-20260707-164655\harness-proxy-20260707-164703`
  (`quickbar-item-refresh-hint.json` `2026-07-07T16:49:38+10:00`, about
  3h47m old at gate). The current-code live HG harness
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, proxy-generated
  `Area_AreaLoaded`, the post-area hold gate opening, held post-area packet
  release, and sustained `GameObjUpdate_LiveObject` traffic with no quarantine
  directory. Its final pending `quickbar-item-refresh-hint.json` at
  `2026-07-07T21:04:06+10:00` proved the new consumer buckets on live HG
  traffic: 19 handoff events, 19 ready events, 0 blocked-without-ready events,
  1 ready-with-deferred-Feature-25 event, 18 `ClientGuiInventory`
  events/ready events, and 1 server `Inventory` event/ready event. The same
  hint reported candidate `0x80015386` from active-object/direct-only proof,
  66 direct item proof objects, 2 Feature-25 item proof objects, 66
  compact-emission ready objects, 2 deferred Feature-25-only objects, 6
  deferred Feature-25 item-ref mentions, and
  `inventory_equipment_handoff_outcome="ready_item_state_with_deferred_feature25_refs"`.
  Proxy2 now exports the per-consumer handoff counters in pending and idle
  `quickbar-item-refresh-hint.json` plus the Diamond replay summary. The
  bridge-plan slice below is the follow-up that turns those counters into an
  explicit writer-facing handoff decision.
- 2026-07-07 inventory/equipment bridge handoff plan: live-data gate reused the
  current gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`quickbar-item-refresh-hint.json` and `proxy.structured.log`
  `2026-07-07T21:05:54+10:00`, about 1h31m old at gate). It reached
  `Module_Loaded`, `Area_ClientArea`, proxy-generated `Area_AreaLoaded`, held
  post-area packet release, and sustained `GameObjUpdate_LiveObject` traffic
  with no quarantine directory. Proxy2 now derives an explicit
  `inventory_equipment_bridge_handoff_*` plan from the last retained ready
  inventory/equipment handoff snapshot and exports it in pending/idle
  `quickbar-item-refresh-hint.json` plus replay summaries. The plan is
  `emit_ready_item_state` only when ready direct/materialized compact item state
  has a bridge candidate; deferred Feature-25-only refs remain blocked as
  reference-only evidence and do not produce a bridge handoff. Bounded strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-plan-20260707-225132`
  processed 164 Diamond autoplay packets with strict translation, 304 allow
  decisions, 0 strict quarantines, 0 quarantine files, and 0 live-object
  terminal residuals. The Diamond baseline correctly exported
  `inventory_equipment_bridge_handoff_action="none"` after one blocked
  server-inventory handoff with Feature-25-only evidence. Active next path:
  implement the bounded writer/bridge consumer that uses
  `emit_ready_item_state` live snapshots for `ClientGuiInventory`/server
  `Inventory` without materializing deferred Feature-25 references.
- 2026-07-08 inventory/equipment bridge handoff emission: live-data gate reused
  the same current gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`quickbar-item-refresh-hint.json` and `proxy.structured.log`
  `2026-07-07T21:05:54+10:00`, about 3h34m old at gate). It reached
  gameplay through module load, area load, held-packet release, and sustained
  live-object traffic with no quarantine directory, so no fresh live harness
  run was required. Proxy2 now records a one-shot
  `InventoryEquipmentHandoffBridgeEmission` whenever a verified
  `Inventory`/`ClientGuiInventory` handoff plan is `emit_ready_item_state`
  with a direct/materialized candidate; blocked deferred Feature-25-only refs
  remain reference-only and do not emit. Pending/idle
  `quickbar-item-refresh-hint.json`, reducer diagnostics, and the Diamond
  replay summary now expose emission counts and the last emitted consumer,
  event index, candidate object, and candidate source. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-emission-20260708-0055`
  processed 164 Diamond autoplay packets with strict translation, no quarantine
  directory, 1 blocked server-inventory handoff, and 0 bridge emissions, proving
  the Feature-25-only baseline still does not synthesize item state. Active
  next path: implement the writer/bridge consumer that drains these ready
  emissions into the EE-facing inventory/equipment state update.
- 2026-07-08 inventory/equipment bridge handoff drain: live-data gate reused the
  current gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`quickbar-item-refresh-hint.json` and `proxy.structured.log`
  `2026-07-07T21:05:54+10:00`, about 5h35m old at gate). It reached gameplay
  through module load, area load, held-packet release, and sustained
  live-object traffic with no quarantine directory, so no fresh live harness
  run was required. Proxy2 now drains each one-shot
  `InventoryEquipmentHandoffBridgeEmission` into an EE-facing
  `InventoryEquipmentBridgeStateUpdate` exactly once, keyed by emission index,
  and continues to reject Feature-25-only/deferred evidence before state update
  creation. Pending/idle hints, reducer diagnostics, and replay summaries expose
  state-update counts and the last drained candidate. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-drain-20260708-025233`
  processed 164 Diamond autoplay packets with strict translation, 304 allow
  decisions, 0 quarantine files, and 0 live-object terminal residuals; the
  Feature-25-only baseline reported 1 blocked server-inventory handoff, 0 bridge
  emissions, and 0 bridge state updates. Active next path: wire these drained
  ready item-state updates into the concrete EE inventory/equipment writer
  output and confirm with a fresh live HG harness if the current capture is
  stale.
- 2026-07-08 inventory/equipment bridge writer output: live-data gate reused the
  current gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`quickbar-item-refresh-hint.json` and `proxy.structured.log`
  `2026-07-07T21:05:54+10:00`, about 7h35m old at gate). It reached gameplay
  through module load, area load, held-packet release, and sustained
  live-object traffic with no quarantine directory, so no fresh live harness
  run was required. Proxy2 now parses server `Inventory` equip/cancel result,
  object id, and equip slot into semantic handoff state; builds exact
  EE-shaped `Inventory` payloads through the typed inventory writer; and queues
  one proxy-owned reliable server `M` frame after the triggering server
  `Inventory` packet only when the drained bridge update is server-Inventory
  owned, has a matching ready direct/materialized item-state candidate, and
  validates back through the strict inventory parser. `ClientGuiInventory`
  handoffs remain state-only until a decompile-backed GUI inventory writer is
  proven. Focused tests cover exact EE payload construction, server-Inventory
  queueing/sequence shift, and ClientGui state-only suppression. Bounded strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-equipment-bridge-writer-20260708-0506-altports240`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, 0 quarantine files, and 0
  live-object terminal residuals; the Feature-25-only baseline stayed blocked
  with 1 server-Inventory handoff, 0 ready handoffs, 0 bridge emissions, and 0
  bridge state updates. The default replay port was denied by Windows
  (`127.0.0.1:55121`, `os error 10013`), so the passing replay used
  `-ListenPort 56221 -ServerPort 56233`. Active next path: run fresh live HG
  confirmation when the 2026-07-07 21:05 capture becomes stale, then inspect
  whether real ready server-Inventory traffic produces the queued exact
  `Inventory` output and whether remaining visible equipment divergence needs
  a separate ClientGui inventory writer.
- 2026-07-08 inventory/equipment bridge output observability: live-data gate
  reused the current gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`proxy.stdout.log` `2026-07-07T21:05:54+10:00`, about 9h36m old at gate).
  It reached gameplay and had no quarantine directory, so the strongest
  reference for this instrumentation slice was the deterministic Diamond
  autoplay replay. Proxy2 now records inventory/equipment bridge output queue
  counters, safe deferral buckets, and last queued synthetic `Inventory`
  metadata in the m-frame bridge state; `quickbar-item-refresh-hint.json` and
  the replay summary export those fields. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-summary-20260708-0648`
  processed 164 Diamond autoplay packet files with strict translation, 303
  allow decisions, 0 strict quarantines, 0 quarantine files, and 0 live-object
  terminal residuals. The Feature-25-only baseline still reported 1 blocked
  server-Inventory handoff, 0 ready handoffs, 0 bridge state updates, and
  `inventory_equipment_bridge_output_queued_packets=0`. Active next path: run a
  fresh live HG confirmation when the 2026-07-07 21:05 capture becomes stale
  and verify whether real ready server-Inventory traffic increments
  `inventory_equipment_bridge_output_queued_packets`; if not, use the new
  deferral/mismatch buckets to choose between server-Inventory claim repair and
  a separately proven ClientGui inventory writer.
- 2026-07-08 inventory/equipment bridge output decision idempotence: live-data
  gate reused the same gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`proxy.stdout.log` `2026-07-07T21:05:54+10:00`, about 11h36m old at gate).
  It reached gameplay and had no quarantine directory, so the strongest
  reference for this state-machine slice was the deterministic Diamond autoplay
  replay. Proxy2 now makes one bridge-output decision per drained
  inventory/equipment state update, preventing repeated client-GUI deferral,
  missing-claim deferral, or candidate-mismatch block counts from accumulating
  on later frames for the same immutable update. Pending/idle
  `quickbar-item-refresh-hint.json` and replay summaries also export the last
  decision/deferred/block update indexes. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-decision-20260708-085235`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, 0 quarantine files, and 0 live-object
  terminal residuals; the Feature-25-only baseline still reported 1 blocked
  server-Inventory handoff, 0 ready handoffs, 0 bridge state updates, and
  `inventory_equipment_bridge_output_queued_packets=0`. Active next path: when
  the 2026-07-07 21:05 live capture is stale, run fresh HG confirmation and use
  the idempotent decision indexes plus queued/deferral/mismatch buckets to
  choose between server-Inventory claim repair and a separately proven
  ClientGui inventory writer.
- 2026-07-08 inventory/equipment bridge output decision detail: live-data gate
  reused the same gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`proxy.stdout.log` `2026-07-07T21:05:54+10:00`, about 13h37m old at gate).
  It reached gameplay and had no quarantine directory, so no fresh live harness
  was required. Proxy2 now records the last bridge-output decision as a typed
  production state snapshot, including reason, consumer, emission/event
  indexes, ready candidate object/proof/source, and parsed server-Inventory
  claim details. `quickbar-item-refresh-hint.json` and the replay summary now
  expose those fields beside the existing queue/deferral/mismatch counters.
  Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-decision-detail-20260708-105538`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, 0 quarantine files, and 0 live-object
  terminal residuals. The Feature-25-only baseline had 0 queued bridge output
  and no drained ready update, so
  `inventory_equipment_bridge_output_last_decision_known=false` and
  `inventory_equipment_bridge_output_last_decision_reason="none"` are the
  expected baseline. Active next path: run fresh live HG confirmation when the
  2026-07-07 21:05 capture is stale, then use last-decision reason plus
  candidate-vs-claim object ids to choose server-Inventory claim repair or a
  separately proven ClientGui inventory writer.
- 2026-07-08 inventory/equipment bridge output status: live-data gate reused
  the same gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`quickbar-item-refresh-hint.json` and `proxy.structured.log`
  `2026-07-07T21:05:54+10:00`, about 15h39m old at gate). It reached gameplay
  through module load, area load, held-packet release, and sustained
  `GameObjUpdate_LiveObject` traffic with no quarantine directory, so no fresh
  live harness was required. Proxy2 now derives
  `inventory_equipment_bridge_output_status` from production bridge-output
  counters with queued output taking precedence over server-Inventory candidate
  mismatch, missing claim, and client-GUI writer deferral, plus
  `inventory_equipment_bridge_output_requires_client_gui_writer` for the pure
  client-GUI gap. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-bridge-output-status-20260708-1249`
  processed the 164-packet Diamond autoplay baseline with strict translation
  and no quarantine files, and reported
  `inventory_equipment_bridge_output_status="awaiting_bridge_state_update"`
  with 1 blocked handoff and 0 ready handoffs, matching the Feature-25-only
  baseline. Active next path: when live capture freshness expires, run current
  HG confirmation and use this status first; `queued_inventory_output` means
  inspect visible state after the synthetic Inventory packet,
  `blocked_candidate_mismatch` or `deferred_missing_claim` means repair the
  server-Inventory claim path, and `awaiting_client_gui_writer` means implement
  the separately proven ClientGui inventory writer.
- 2026-07-08 ClientGui inventory bridge-output decision timing: live-data gate
  reused the same gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`proxy.structured.log` `2026-07-07T21:05:54+10:00`, about 17h40m old at
  gate). It reached gameplay and had no quarantine directory, so no fresh live
  harness was required. Proxy2 now records the non-server
  inventory/equipment bridge-output decision immediately after a verified
  `ClientGuiInventory` semantic observation, instead of waiting for a later
  server `Inventory` packet to call the output decider. The exact server
  `Inventory` writer gate is unchanged: only server-Inventory updates with a
  matching parsed claim can queue a synthetic EE `Inventory` frame. Bounded
  strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-gui-bridge-decision-20260708-1458`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, 0 quarantine files, and 0 live-object
  terminal residuals; the Feature-25-only baseline still reported
  `inventory_equipment_bridge_output_status="awaiting_bridge_state_update"`
  because it has no ready ClientGui handoff. Active next path remains fresh HG
  confirmation once the 2026-07-07 21:05 capture is stale, then follow the
  bridge-output status classifier.
- 2026-07-08 ClientGui inventory bridge-output claim detail: live-data gate
  reused the same gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`quickbar-item-refresh-hint.json` and `proxy.structured.log`
  `2026-07-07T21:05:54+10:00`, about 21h43m old at gate). It reached
  gameplay and had no quarantine directory, so no fresh live harness was
  required. Proxy2 now retains the exact verified `ClientGuiInventory` claim
  summary through semantic inventory/equipment handoff snapshots, bridge plans,
  drained bridge state updates, bridge-output decisions, pending/idle
  `quickbar-item-refresh-hint.json`, and replay summaries. This does not add a
  new packet writer or relax validation; it preserves the already verified
  client-GUI claim shape so the next live run can distinguish status/self-object
  traffic from select-panel GUI traffic when
  `inventory_equipment_bridge_output_status="awaiting_client_gui_writer"`.
  Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-gui-claim-detail-20260708-1900`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, 0 quarantine files, and 0 live-object
  terminal residuals. The Feature-25-only baseline still reported
  `inventory_equipment_bridge_output_status="awaiting_bridge_state_update"`,
  `inventory_equipment_bridge_output_last_decision_known=false`, and
  `inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_known=false`.
  Active next path: run fresh HG confirmation once the 2026-07-07 21:05 capture
  is stale and use the retained client-GUI claim details if live traffic reaches
  the ClientGui writer-gap status.
- 2026-07-08 inventory/equipment bridge-output decision ready context:
  live-data gate reused the same gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-handoff-consumer-buckets-current-20260707-210130\harness-proxy-20260707-210133`
  (`quickbar-item-refresh-hint.json`, `proxy.structured.log`, and
  `proxy.stdout.log` `2026-07-07T21:05:54+10:00`, about 23h42m old at gate).
  It reached gameplay through module load, area load, held-packet release, and
  sustained `GameObjUpdate_LiveObject` traffic with no quarantine directory, so
  no fresh live harness was required at run start. Proxy2 now preserves the
  ready direct/materialized object count and deferred Feature-25-only object
  count on the typed bridge-output decision snapshot and exports them in
  pending/idle `quickbar-item-refresh-hint.json` plus replay summaries. Bounded
  strict replay
  `C:\nwnbridge\codex-proxy2-replay-bridge-decision-ready-context-built-20260708-210200`
  processed 164 Diamond autoplay packet files with strict translation, 0
  quarantine files, and 0 live-object terminal residuals. The Feature-25-only
  baseline remained
  `inventory_equipment_bridge_output_status="awaiting_bridge_state_update"`
  with no last bridge-output decision, so the new ready/deferred decision
  counts correctly reported 0/0. Active next path: the live HG evidence is now
  stale or nearly stale; run fresh HG confirmation and use bridge-output status
  plus the last-decision ready/deferred counts to choose server-Inventory claim
  repair versus a separately proven ClientGui inventory writer.
- 2026-07-08 pending server-Inventory handoff replay and fresh HG confirmation:
  live-data gate found the 2026-07-07 21:05 gameplay-reaching HG proxy capture
  stale at about 25h42m old, so fresh live evidence was required. Fresh
  current-build harness
  `C:\nwnbridge\codex-live-current-confirm-20260708-224952\harness-proxy-20260708-225100`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, proxy-generated
  `Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject` traffic with no
  quarantine directory. Its settled hint at `2026-07-08T22:54:40+10:00`
  showed a real timing gap: one server `Inventory` handoff happened before
  ready direct/materialized item state was retained
  (`inventory_equipment_handoff_server_inventory_events=1`,
  ready events `0`), while later post-context state had candidate
  `0x80015247` with 18 ready direct objects and 2 deferred Feature-25-only
  objects, leaving bridge output at
  `inventory_equipment_bridge_output_status="awaiting_bridge_state_update"`.
  Proxy2 now retains a blocked server-Inventory claim in shared UI state,
  consumes it once when later item context becomes handoff-ready, and attempts
  the idempotent inventory/equipment bridge-output queue after every verified
  server `M` packet so live-object-created bridge state can flush without
  waiting for another `Inventory` packet. Focused state coverage proves the
  retained claim is consumed exactly once and drains into a server-Inventory
  bridge state update. The first post-fix live probe
  `C:\nwnbridge\codex-live-pending-server-inventory-replay-20260708-230630\harness-proxy-20260708-230637`
  failed before gameplay with server `BNCR` detail 6
  (`observed-hg-rapid-reconnect-or-name-reservation`), was documented as a
  transient harness/server issue, and was retried after a cooldown. Rerun
  `C:\nwnbridge\codex-live-pending-server-inventory-replay-rerun-20260708-231340\harness-proxy-20260708-231358`
  reached gameplay, wrote no quarantine directory, and settled at
  `2026-07-08T23:17:18+10:00` with server `Inventory` ready events `1/1`, one
  bridge state update, and
  `inventory_equipment_bridge_output_status="blocked_candidate_mismatch"`:
  ready candidate `0x80015302` did not match parsed server-Inventory claim
  `0x800153B2`, so no synthetic `Inventory` output was queued. Bounded strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-pending-server-inventory-replay-20260708-2321`
  processed the 164-packet Diamond autoplay baseline with 304 strict decisions,
  0 strict quarantines, no quarantine directory, and 0 live-object terminal
  residuals; the Feature-25-only baseline remained correctly blocked without a
  bridge state update. Active next path: prove why live server `Inventory`
  claims can name a different object than the ready direct/materialized item
  candidate, then fix the shared claim/candidate association before any
  ClientGui writer work.
- 2026-07-09 server-Inventory claim-status gate and diagnostics: live-data gate
  reused the current gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-pending-server-inventory-replay-rerun-20260708-231340\harness-proxy-20260708-231358`
  (`proxy.structured.log` last write `2026-07-08T23:17:37+10:00`, about 1h32m
  old at gate). It reached `Module_Loaded`, `Area_ClientArea`,
  proxy-generated `Area_AreaLoaded`, held-packet release, and sustained
  `GameObjUpdate_LiveObject` with no quarantine directory, so the run used that
  fresh live evidence plus deterministic Diamond replay. Proxy2 now allows a
  parsed server `Inventory` claim object to differ from the retained ready
  candidate only when the claim object is independently
  `InventoryItemObjectStatus::Proven`; the exact EE `Inventory` writer still
  emits the server claim object, result, and equip slot and still blocks
  unproven mismatches. The typed bridge-output decision now snapshots both the
  candidate and server-claim inventory object statuses/proofs, and
  `quickbar-item-refresh-hint.json` plus the replay summary expose those fields
  for the next live run. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-claim-status-inventory-output-20260709-0200`
  processed the 164-packet Diamond autoplay baseline with 304 strict allow
  decisions, 0 strict quarantines, no quarantine directory, and 0 live-object
  terminal residuals. Fresh live confirmation with the conservative claim gate
  `C:\nwnbridge\codex-live-known-claim-inventory-output-20260709-0110\harness-proxy-20260709-010013`
  reached gameplay with no quarantine, but still blocked output:
  parsed server claim `0x80016E36` differed from candidate `0x80016D85`; the
  claim id appeared only in the `Inventory_Equip` decision log, while the
  candidate id was the quickbar-materialized item. Active next path: rerun live
  with the new claim-status fields, then trace the server `Inventory` claim
  object provenance/owner semantics before relaxing the writer or starting a
  ClientGui writer.
- 2026-07-09 server-Inventory claim-neighborhood diagnostics: live-data gate
  first found the gameplay-reaching capture
  `C:\nwnbridge\codex-live-known-claim-inventory-output-20260709-0110\harness-proxy-20260709-010013`
  current (`quickbar-item-refresh-hint.json` last write
  `2026-07-09T01:05:19+10:00`, about 1h47m old at gate). A fresh
  current-code no-inventory probe
  `C:\nwnbridge\codex-live-claim-status-current-20260709-025219\harness-proxy-20260709-025325`
  reached gameplay with no quarantine but did not drive the inventory handoff.
  The forced-inventory probe
  `C:\nwnbridge\codex-live-claim-status-inventory-20260709-025758\harness-proxy-20260709-025805`
  reached gameplay, wrote `proxy.structured.log` through
  `2026-07-09T03:05:58+10:00`, produced no quarantine-like files, and settled
  at `inventory_equipment_bridge_output_status="blocked_candidate_mismatch"`:
  ready candidate `0x80016EFA` was proven from active/direct item state, while
  parsed server `Inventory` claim `0x80016FAA` remained
  `InventoryItemObjectStatus::Unknown`. Proxy2 now snapshots the nearest lower,
  higher, and closest proven direct/materialized item object around the parsed
  server claim on the typed bridge-output decision, exports those fields in
  `quickbar-item-refresh-hint.json` and replay summaries, and logs the closest
  proven neighbor when blocking the mismatch. The writer gate remains
  conservative and still emits no synthetic `Inventory` packet for unproven
  claim/candidate mismatches. That forced-inventory rerun is recorded in the
  ClientGui writer-plan slice below; server-Inventory claim provenance remains
  the fallback path if live traffic returns to server `Inventory` first.
- 2026-07-09 ClientGui inventory writer-plan scaffold: live-data gate first
  found the gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-claim-status-inventory-20260709-025758\harness-proxy-20260709-025805`
  current (`quickbar-item-refresh-hint.json`/`proxy.structured.log` through
  `2026-07-09T03:05:58+10:00`, about 1h45m old at gate). A fresh
  forced-inventory probe
  `C:\nwnbridge\codex-live-claim-neighborhood-inventory-20260709-045231\harness-proxy-20260709-045344`
  reached gameplay, wrote `quickbar-item-refresh-hint.json` through about
  `2026-07-09T04:57:12+10:00`, and produced no quarantine directory. This run
  did not expose a server `Inventory` handoff; instead it settled at
  `inventory_equipment_bridge_output_status="awaiting_client_gui_writer"` with
  `inventory_equipment_bridge_output_last_decision_reason="deferred_client_gui"`,
  `inventory_equipment_bridge_output_requires_client_gui_writer=true`, 5
  `ClientGuiInventory` handoff events, 1 ready `ClientGuiInventory` handoff, 0
  server `Inventory` handoffs, candidate `0x80015211`, and a status/self
  client-GUI claim object `0x7F000000`. Proxy2 now has decompile-backed
  `ClientGuiInventory` EE payload builders for status and select-panel claims
  and exports a non-emitting writer plan in quickbar hints and replay summaries.
  The current-player inventory status plan builds exact payload
  `700D010B0000000000007F90`; select-panel 3 builds
  `700D02080000000390`. Emission intentionally remains disabled with
  `client_gui_inventory_bridge_timing_unproven` until the proxy-owned insertion
  timing is bounded. Strict Diamond replay
  `C:\nwnbridge\codex-proxy2-replay-client-gui-writer-plan-20260709-050757`
  processed the 164-packet autoplay baseline with 304 strict allow decisions, 0
  strict quarantines, no quarantine directory, and 0 live-object terminal
  residuals; the baseline correctly reported no writer plan because it only has
  a blocked Feature-25-only server-Inventory handoff. Active next path:
  implement a bounded proxy-owned ClientGui status emission timing for the
  proven current-player inventory payload and verify it on live HG; if server
  `Inventory` traffic returns first, continue the claim-neighborhood provenance
  path instead.
- 2026-07-09 ClientGui inventory current-player status output: live-data gate
  reused the current gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-claim-neighborhood-inventory-20260709-045231\harness-proxy-20260709-045344`
  (`quickbar-item-refresh-hint.json`/`proxy.structured.log` through
  `2026-07-09T04:57:12+10:00`, about 1h55m old at gate). It reached gameplay,
  sustained `GameObjUpdate_LiveObject`, and produced no quarantine directory,
  so the run used that fresh live ClientGui writer-gap evidence plus the
  deterministic Diamond replay. Proxy2 now queues one proxy-owned
  client-to-server reliable `ClientGuiInventory_Status` request for a proven
  current-player inventory claim (`0x7F000000`) once client sequence state is
  available, validates the exact EE payload
  `700D010B0000000000007F90` through the typed ClientGui parser, records a
  client sequence shift, and exports queued ClientGui status metadata in
  `quickbar-item-refresh-hint.json` and replay summaries. Select-panel claims,
  non-current-player status claims, and missing client sequence state remain
  deferred. Focused tests cover exact status queueing, idempotence, select-panel
  deferral, the ClientGui parser, and the queued-status hint fields. Bounded
  strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-gui-status-output-20260709-071534`
  used alternate ports `-ListenPort 40021 -ServerPort 40033`, processed 164
  Diamond autoplay packets with strict translation, 0 quarantine files, and 0
  live-object terminal residuals. That replay source did not contain a ready
  ClientGui inventory handoff, so the queued ClientGui status counters
  correctly stayed at 0. Active next path: run a fresh live HG forced-inventory
  probe before this capture ages past 24h and verify whether
  `inventory_equipment_bridge_output_queued_client_gui_status_packets` becomes
  nonzero and whether HG answers with the expected inventory/UI refresh stream.
- 2026-07-09 ClientGui status live-object response tracking: live-data gate
  first confirmed a fresh gameplay-reaching HG forced-inventory capture at
  `C:\nwnbridge\codex-live-client-gui-status-output-20260709-085527\harness-proxy-20260709-085537`
  (`proxy.structured.log` through `2026-07-09T08:58:14+10:00`, no
  quarantine directory). That run reached gameplay and showed the concrete
  sequence this slice models: real `ClientGuiInventory_Status` traffic,
  proxy-owned queued current-player status output
  (`700D010B0000000000007F90`, object `0x7F000000`, synthetic sequence 82),
  then HG answering with `GameObjUpdate_LiveObjectCombinedRecords` carrying
  51 live-GUI records, 348 live-GUI fragment bits, and 51 materialized item
  object ids. Proxy2 now carries those decompile-backed live-GUI counters from
  the verified live-object claim into semantic state, snapshots the latest
  live-object inventory materialization summary, records live-object packets
  observed after a queued ClientGui status request, and exports the response
  counters/last-response fields in `quickbar-item-refresh-hint.json` and the
  Diamond replay summary. A current-code rerun
  `C:\nwnbridge\codex-live-client-gui-status-response-20260709-091913\harness-proxy-20260709-091918`
  reached gameplay through `Area_ClientArea`, proxy-generated
  `Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject` through
  `2026-07-09T09:21:38+10:00` with no quarantine directory, but the EE client
  exited before the auto-inventory step produced any `ClientGuiInventory`
  event, so its response counters correctly stayed at 0. Focused tests cover
  live-object response recording after queued ClientGui status and hint
  serialization of the response fields. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-gui-status-response-20260709-091641`
  ran the Diamond autoplay baseline with strict translation and alternate
  ports `-ListenPort 56321 -ServerPort 56333`; that baseline still has no
  ready ClientGui handoff, so the queued/response counters remain inactive
  there. Active next path: rerun live HG forced-inventory with manual or
  delayed inventory opening to exercise the current-code JSON response fields,
  then use the recorded live-GUI/materialized-item response as the seed for the
  next inventory UI refresh/visible-equipment bridge decision.
- 2026-07-09 delayed forced-inventory live-object quarantine diagnostics:
  live-data gate first found the gameplay-reaching
  `C:\nwnbridge\codex-live-client-gui-status-response-20260709-091913\harness-proxy-20260709-091918`
  capture current (`quickbar-item-refresh-hint.json` and logs through
  `2026-07-09T09:21:38+10:00`, about 1h33m old at gate, no quarantine
  directory). The strongest reference setup was a fresh live HG delayed
  inventory-open probe because the current active path needed real
  `ClientGuiInventory`/server `Inventory` timing. The probe
  `C:\nwnbridge\codex-live-client-gui-status-delayed-inventory-20260709-105516\harness-proxy-20260709-105528`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, proxy-generated
  `Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject`, but it produced
  two identical `live-object-unclaimed-strict-family` quarantine artifacts for
  server seq51 at `2026-07-09T10:58:21+10:00`:
  `quarantine\live-object-unclaimed-strict-family-GameObjUpdate_LiveObject-seq51-frames1-1783558701965.bin`
  and the timestamp-only duplicate. The final hint reported two
  `ClientGuiInventory` handoffs blocked before ready item context, then one
  ready server `Inventory` handoff with
  `inventory_equipment_bridge_output_status="blocked_candidate_mismatch"`.
  The ready candidate was `0x80015854` with active-object/direct proof; the
  parsed server `Inventory` claim was unknown object `0x80015977`, closest
  proven item `0x800158CD` at distance 170. The quarantined `P/05/01` payload
  is 322 bytes with declared window `0x013D`; do not relax live-object strict
  validation until its exact EE/Diamond record and fragment cursor are proven.
  Proxy2 now logs structured final live-object quarantine diagnostics from the
  focused claim parser: declared/read/fragment lengths, decoded fragment bit
  count, claim reject stage/cursor, declared-repair candidate counts, and the
  first capacity-plausible repair candidate. Verification: `cargo fmt --all`,
  `cargo test -q -p hgbridge-proxy2
  live_object_claim_diagnostics_reports_declared_window_reject -- --nocapture`,
  and `cargo check -q -p hgbridge-proxy2`. Active next path: run the HG delayed
  inventory probe on the diagnostic build or replay the seq51 payload through a
  focused harness, then implement the exact live-object translator/declared
  repair rule indicated by the claim reject fields before resuming ClientGui
  inventory writer work.
- 2026-07-07 inventory/equipment handoff consumer state: live-data gate reused
  the fresh gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-bnk3-stall-diagnostic-20260707-164655\harness-proxy-20260707-164703`
  (`quickbar-item-refresh-hint.json` `2026-07-07T16:49:38+10:00`, about
  1h47m old at gate). It reached `Module_Loaded`, `Area_ClientArea`, and
  sustained `GameObjUpdate_LiveObject` traffic with no quarantine directory.
  The strongest reference setup for this slice was the Diamond autoplay replay
  after the fresh live gate, because the code only consumes already-verified
  semantic item state and the replay gives deterministic strict-regression
  evidence. Proxy2 now observes verified `Inventory` and `ClientGuiInventory`
  events as inventory/equipment handoff consumers, consumes the best retained
  direct/materialized item context without materializing deferred
  Feature-25-only refs, and exports aggregate/per-consumer counters in pending
  and idle `quickbar-item-refresh-hint.json` plus replay summaries. Idle hints
  also retain the last handoff snapshot.
  Focused state/reducer tests prove ready direct item state is consumed while
  deferred Feature-25-only refs remain reference-only. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-equipment-handoff-consumer-20260707-185513`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, and 0 quarantine files; this Diamond
  baseline had 1 handoff event and correctly classified it as blocked without
  ready state:
  `inventory_equipment_handoff_outcome="feature25_refs_without_ready_item_state"`.
  The 2026-07-07 21:04 live HG harness above confirmed real
  `ClientGuiInventory`/`Inventory` events increment the ready buckets against
  retained ready direct item state. If visible inventory/equipment still
  diverges, implement the bounded bridge/writer behavior from those consumer
  snapshots.
- 2026-07-07 inventory/equipment handoff readiness classifier: live-data gate
  reused the fresh gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-feature25-handoff-outcome-20260707-20260707-125516\harness-proxy-20260707-125522`
  (`quickbar-item-refresh-hint.json` `2026-07-07T12:58:45+10:00`,
  about 1h36m old at gate). It reached `Module_Loaded`, `Area_ClientArea`,
  and sustained `GameObjUpdate_LiveObject` traffic with no quarantine
  directory. Proxy2 now reports
  `inventory_equipment_handoff_ready` and
  `inventory_equipment_handoff_outcome` beside the Feature-25 materialization
  and handoff outcome fields in semantic traces, pending/idle
  `quickbar-item-refresh-hint.json`, and replay summaries. The rule treats
  direct/materialized compact item state as ready for the inventory/equipment
  UI handoff even when all Feature-25 item refs remain deferred, while keeping
  pure Feature-25 reference-only evidence classified as not ready. Bounded
  strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-equipment-handoff-bounded-20260707-145448`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, and 0 quarantine files; its final hint
  reported `inventory_equipment_handoff_ready=false`,
  `inventory_equipment_handoff_outcome="feature25_refs_without_ready_item_state"`,
  `inventory_feature25_handoff_outcome="all_item_refs_deferred_without_ready_item_state"`,
  0 ready compact item objects, and 6 deferred Feature-25-only objects. Active
  next path: run the next live HG harness on this build and use the new
  ready/outcome fields to audit the visible inventory/equipment UI handoff; if
  live still reports ready direct item state with deferred Feature-25 refs,
  implement the shared UI handoff consumer instead of materializing deferred
  Feature-25 references.
- 2026-07-07 Feature-25 handoff outcome classifier: live-data gate reused the
  fresh gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-feature25-u5-boundary-fix-20260707-071504\harness-proxy-20260707-071516`
  (`quickbar-item-refresh-hint.json`
  `2026-07-07T07:17:18.6853537+10:00`, about 5h17m old at gate). It reached
  `Module_Loaded`, `Area_ClientArea`, and sustained `GameObjUpdate_LiveObject`
  traffic with no quarantine directory. Proxy2 now reports
  `inventory_feature25_handoff_outcome` alongside the materialization outcome
  in semantic traces, pending/idle `quickbar-item-refresh-hint.json`, and replay
  summaries. The classifier keeps the all-deferred Feature-25 reference-only
  state separate from the live HG state where direct/materialized item proof is
  already ready for quickbar/UI handoff. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-feature25-handoff-outcome-20260707-124920`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, and 0 quarantine files; its final hint
  reported
  `inventory_feature25_handoff_outcome="all_item_refs_deferred_without_ready_item_state"`
  with 0 ready compact item objects and 6 deferred Feature-25-only objects.
  Fresh live HG confirmation
  `C:\nwnbridge\codex-live-feature25-handoff-outcome-20260707-20260707-125516\harness-proxy-20260707-125522`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, wrote `quickbar-item-refresh-hint.json` at
  `2026-07-07T12:58:45+10:00`, produced no quarantine directory, and reported
  `inventory_feature25_handoff_outcome="all_item_refs_deferred_with_ready_item_state"`
  with 18 ready direct item-proof objects and 2 deferred Feature-25-only
  objects while still resolving by prior quickbar use-count state. Active next
  path: use this classifier to drive the next inventory/equipment UI audit and
  implement the shared handoff rule that consumes ready item state without
  materializing all-deferred Feature-25 refs.
- 2026-07-07 deferred Feature-25 ready/emission split: live-data gate reused
  the fresh gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-feature25-u5-boundary-fix-20260707-071504\harness-proxy-20260707-071516`
  (`quickbar-item-refresh-hint.json`
  `2026-07-07T07:17:18.6853537+10:00`, about 3h16m old at gate). It reached
  `Module_Loaded`, `Area_ClientArea`, and sustained `GameObjUpdate_LiveObject`
  traffic with no quarantine directory; the hint still resolved by prior
  quickbar use-count state and reported 7 deferred Feature-25 item-ref
  mentions, 0 materialized mentions, and
  `inventory_feature25_materialization_outcome="all_item_refs_deferred"`.
  Proxy2 now splits diagnostic compact item refs from EE quickbar emission-ready
  item proof: `compact_item_emission_proof_objects` keeps direct plus
  Feature-25 refs for tracing and candidate selection, while
  `compact_item_emission_ready_objects` and
  `compact_item_emission_ready_candidate` require direct/materialized item
  state. Deferred Feature-25-only refs remain visible as
  `compact_item_emission_deferred_feature25_only_objects`, but they no longer
  open a pending quickbar item-refresh window or produce a harness action hint.
  Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-feature25-ready-split-20260707-105736`
  processed 164 Diamond autoplay packet files with strict translation, 304
  allow decisions, 0 strict quarantines, and 0 quarantine files; final summary
  had `pending_item_refresh=false`,
  `no_hint_reason="post_context_without_compact_item_proof"`, diagnostic
  candidate `0x80015DAA` from `feature25_second_list`, 0 ready objects, and 6
  deferred Feature-25-only objects. Active next path: run the next live HG
  harness on this ready/deferred split and confirm the prior quickbar use-count
  no-action path still holds before changing inventory/equipment UI handoff
  rules.
- 2026-07-07 deferred Feature-25 refs are reference-only for compact quickbar
  emission: live-data gate reused the fresh gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-feature25-u5-boundary-fix-20260707-071504\harness-proxy-20260707-071516`
  (`quickbar-item-refresh-hint.json`
  `2026-07-07T07:17:18.6853537+10:00`, about 1h15m old at gate). It reached
  `Module_Loaded`, `Area_ClientArea`, and sustained `GameObjUpdate_LiveObject`
  traffic with no quarantine directory; the hint had 7 Feature-25 reference
  records, 7 deferred item-ref mentions, 0 materialized item-ref mentions, and
  `inventory_feature25_materialization_outcome="all_item_refs_deferred"`.
  Proxy2 now keeps unmaterialized Feature-25 first/second/legacy-tail object
  ids as `DeferredFeature25` status for diagnostics and candidate tracking, but
  `inventory_item_object_proof` and the quickbar writer do not treat them as
  item materialization proof. Compact quickbar item emission with only deferred
  Feature-25 evidence now rejects with `MissingStateProof` and records the
  Feature-25 bucket in missing-state diagnostics. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-deferred-feature25-reference-only-20260707-0854`
  used alternate ports 59121/59133 because Windows refused the default replay
  port 55121, processed 164 packet files with strict translation, 304 allow
  decisions, 0 strict quarantines, and 0 quarantine files; the pending Diamond
  path still reports Feature-25 candidate counts as diagnostics, with 23
  feature25-only candidate selections and 15 second-list unmaterialized refs.
  Active next path: run the next live HG harness on this build and confirm any
  later item-bearing `SetAllButtons` path no longer materializes all-deferred
  Feature-25 refs, then continue the inventory/equipment UI handoff audit.
- 2026-07-07 `P/5` appearance followed by `U/5 0x3967` boundary fix:
  live-data gate found the gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-inventory-feature25-current-20260707-043430\harness-proxy-20260707-043444`
  fresh (final hint `2026-07-07T04:42:08+10:00`, about 1h51m old at
  gate). A fresh pre-fix harness
  `C:\nwnbridge\codex-live-feature25-outcome-current-20260707-063346\harness-proxy-20260707-063402`
  reached gameplay but quarantined seq48 as
  `live-object-unclaimed-strict-family-GameObjUpdate_LiveObject-seq48-frames1-1783370159548.bin`
  after an exact I/0x2000 Feature-25 row, legacy `A/5`, full `P/5`
  appearance/name row, and following `U/5 0x3967` row. Proxy2 now tries later
  boundary-looking record ends only for the decompile-audited `U/5 0x3967`
  tail class and requires the focused creature-update exact/legacy cursor proof
  before accepting the end; other creature update masks keep the historical
  first-boundary behavior. Patched live verification
  `C:\nwnbridge\codex-live-feature25-u5-boundary-fix-20260707-071504\harness-proxy-20260707-071516`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, wrote `quickbar-item-refresh-hint.json` at
  `2026-07-07T07:17:18.6853537+10:00`, and produced no quarantine directory.
  Active next path: with this live-object quarantine cleared, continue the
  Feature-25 all-deferred inventory/equipment materialization decision and keep
  the older broad live-object fixture failure cluster as separate debt.
- 2026-07-07 inventory Feature-25 materialization outcome: live-data gate found
  gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-preserved-active-mismatch-confirm-20260707-023305\harness-proxy-20260707-023309`
  fresh (`proxy.stdout.log` last write `2026-07-07T02:48:23+10:00`, about
  1h44m old at gate), with no quarantine directory. A fresh current-code
  confirmation harness
  `C:\nwnbridge\codex-live-inventory-feature25-current-20260707-043430\harness-proxy-20260707-043444`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, wrote `quickbar-item-refresh-hint.json` at
  `2026-07-07T04:42:08+10:00`, and produced no quarantine directory. It
  resolved by prior quickbar use-count state for candidate `0x80015270`, with
  21 quickbar item buttons preserved by explicit self materialization, 42
  Feature-25 reference records, 21 first-list deferred item-ref mentions, 21
  second-list deferred item-ref mentions, 0 Feature-25 materialized mentions,
  and 0 cleared inventory item ids. Proxy2 now derives aggregate
  `InventoryItemContextSummary` Feature-25 item-ref mention totals and an
  `inventory_feature25_materialization_outcome` for pending and idle
  `quickbar-item-refresh-hint.json` output, semantic trace logs, and replay
  summaries. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-feature25-outcome-20260707-045300`
  used 164 packet files, strict translation, 304 allow decisions, 0 strict
  quarantines, and 0 quarantine files; its pending Diamond path reported 23
  Feature-25 reference records, 27 item-ref mentions, 0 materialized mentions,
  27 deferred mentions, and
  `inventory_feature25_materialization_outcome="all_item_refs_deferred"`.
  The later 08:53 run used this baseline to keep deferred Feature-25 refs
  reference-only for compact quickbar emission while leaving inventory/equipment
  UI handoff as the next live-confirmation target.
- 2026-07-07 preserved-active quickbar `G Q` mismatch guard: live-data gate
  found the gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-prior-gq-state-handoff-current-20260706-202809\harness-proxy-20260706-202815`
  fresh (`quickbar-item-refresh-hint.json` last write
  `2026-07-06T20:32:10+10:00`, about 4h old at gate), then a fresh current-code
  live harness
  `C:\nwnbridge\codex-live-stream-materialization-current-20260707-003039\harness-proxy-20260707-003052`
  reached gameplay, wrote `quickbar-item-refresh-hint.json` at
  `2026-07-07T00:33:48.7458235+10:00`, and produced no quarantine directory.
  Live HG was the strongest reference setup because the issue is a live
  harness-driving state handoff after stream-probe materialization. The final
  hint showed candidate `0x80015D81` from active-object/direct-only proof, but
  the first preserved active quickbar item was `0x80015D89` in slot 0 and had a
  durable typed item-button `G Q` row; the candidate had no matching use-count
  state row. Proxy2 now carries that first-preserved-active-item use-count row
  through pending/idle quickbar item-refresh hints and replay summaries, and it
  suppresses generated client actions with
  `preserved_active_item_quickbar_use_count_state_candidate_mismatch` when the
  candidate lacks matching slot state but the preserved active item has it.
  Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-preserved-active-use-count-state-20260707-005028`
  used 164 packet files, strict translation, 304 allow decisions, 0 strict
  quarantines, and 0 quarantine files; this replay path has no preserved-active
  row, so the new fields correctly exported `known=false` and did not suppress.
  Active next path: rerun live HG on this build and confirm the final hint now
  reports the preserved-active mismatch suppression with no generated client
  action, then inspect the next visible inventory/equipment or live-object UI
  divergence.
- 2026-07-06 stream-probe item materialization counters: live-data gate used
  the gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-prior-gq-state-handoff-current-20260706-202809\harness-proxy-20260706-202815`;
  `quickbar-item-refresh-hint.json` last write was
  `2026-07-06T20:32:10+10:00`, about 1h56m old at gate. Gameplay reached
  through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, with no quarantine directory. Current work used
  the Diamond autoplay replay as the strongest reference setup because the live
  gate was fresh and the slice only surfaces already-owned quickbar/inventory
  materialization decisions. Proxy2 now carries the quickbar writer's
  stream-probe item materialization proof and missing-state counters into
  pending and idle `quickbar-item-refresh-hint.json` output, and the replay
  summary parser exports the same `QuickbarItemRefreshHintStreamProbe*`
  counters. Bounded strict replay
  `C:\nwnbridge\codex-proxy2-replay-stream-materialization-counters-bounded-20260706-2250`
  used 164 packet files, strict translation, 304 allow decisions, 0 strict
  quarantines, and 0 quarantine files. The replay's pending feature-25-only
  candidate `0x80015DAA` correctly reported zero stream-probe preserved/rejected
  item-object counters because that capture has no stream-probe preserved item
  profile in the pending hint path, while semantic post-context still carried 6
  Feature-25 second-list refs. Active next path: run a fresh live HG harness on
  this build and compare the new stream-probe preserved/rejected counters
  against visible inventory/equipment state after the prior-`G Q` quickbar
  resolution.
- 2026-07-06 current-code prior `G Q` state live confirmation: live-data gate
  found gameplay-reaching HG proxy capture
  `C:\nwnbridge\codex-live-coalesced-continuation-fix-20260706-164042\harness-proxy-20260706-164049`
  fresh (`quickbar-item-refresh-hint.json` last write
  `2026-07-06T16:44:44+10:00`; about 3.7h old at gate). A fresh current-code
  harness
  `C:\nwnbridge\codex-live-prior-gq-state-handoff-current-20260706-202809\harness-proxy-20260706-202815`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, wrote the final hint at
  `2026-07-06T20:32:10+10:00`, and produced no quarantine directory. The final
  hint has `pending_item_refresh=false`,
  `no_hint_reason="post_context_resolved_by_prior_quickbar_use_count_state"`,
  candidate `0x80015CCF`, and a matching durable typed `G Q` state row for
  quickbar slot 0/button 1/property index 255/use count 1. No client action was
  observed or injected. Proxy2 now also serializes
  `post_committed_item_refresh_resolution` in pending and idle quickbar
  item-refresh hints, and the replay summary parser exports it, so future
  live/replay artifacts can distinguish `pending`, server-`G Q` resolution,
  prior-state resolution, and no-resolution states without interpreting the
  older boolean pair. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-resolution-field-pending-20260706-204746`
  confirmed the new summary field on a pending hint
  (`QuickbarItemRefreshHintPostCommittedItemRefreshResolution=pending`, 164
  packet files, strict translation, zero quarantines). Active next path: use the
  fresh no-action resolution as the baseline for the next visible gameplay
  state gap, especially inventory/equipment or remaining live-object UI state
  that still diverges after quickbar active-property use counts are owned.
- 2026-07-06 prior durable `G Q` quickbar state handoff: live-data gate reused
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-live-coalesced-continuation-fix-20260706-164042\harness-proxy-20260706-164049`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-06T16:44:44+10:00`; about 1h41m old at gate). Gameplay reached
  through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, with no quarantine directory. The live hint
  proved candidate `0x800155A9` already matched durable typed `G Q`
  item-use-count state for quickbar slot 0/button 1/property index 255/use
  count 1, and HG sent no quickbar/property response after the generated
  subtype-low `UseItem`. Proxy2 now resolves the pending post-committed
  item-refresh window from that prior durable state when it matches the
  candidate, preserved active item signature, slot, and item button type. It
  records `pending_item_refresh_outcome="pending_refresh_resolved_by_use_count_state"`,
  clears the active pending hint, reports
  `no_hint_reason="post_context_resolved_by_prior_quickbar_use_count_state"`,
  and suppresses generated client actions with
  `recommended_client_action_suppressed_reason="matching_quickbar_use_count_state"`.
  Replay summaries now export
  `QuickbarSemanticPendingItemRefreshOutcomeResolvedByUseCountState`. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-prior-gq-state-handoff-20260706-184640`
  over the 2026-07-03 Diamond autoplay capture stayed at 164 packet files, 304
  strict allows, 0 strict quarantines, and 0 quarantine files; that replay has
  no candidate durable use-count state, so the new outcome counter was 0 as
  expected. Active next path: run a fresh live HG harness on this build and
  confirm the final hint/no-hint reason resolves by prior quickbar use-count
  state with no generated subtype-low `UseItem` dispatch.
- 2026-07-06 coalesced zlib stream continuation gate: live-data gate found
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-live-gq-slot-relation-current-20260706-142738\harness-proxy-20260706-142747`
  fresh (`quickbar-item-refresh-hint.json` last write
  `2026-07-06T14:45:21+10:00`; about 1h40m old at gate). A current-code live
  probe
  `C:\nwnbridge\codex-live-use-count-state-current-20260706-162740\harness-proxy-20260706-162752`
  reached gameplay but produced five identical 241-byte
  `unclaimed-unknown-high-level` quarantine files. Each payload was an
  inflated gameplay stream tail beginning `70 5E 51 F2 6E 9E F9 CF`, and the
  proxy log had already classified it as a single incomplete/non-header stream
  continuation. Proxy2 now tests inflated coalesced payloads with the shared
  gameplay-stream splitter before high-level parse fallback, so a single
  incomplete continuation/pending-fragment/unknown stream unit stays on the
  stream-continuation path instead of masquerading as a new high-level family.
  Patched live verification
  `C:\nwnbridge\codex-live-coalesced-continuation-fix-20260706-164042\harness-proxy-20260706-164049`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, wrote the final hint at
  `2026-07-06T16:44:44+10:00`, and produced no quarantine directory. The final
  hint still shows the active quickbar issue: candidate `0x800155A9` matched
  durable `G Q` state for quickbar slot 0/button 1/property index 255/use
  count 1, the first client action matched the subtype-low `UseItem`, and HG
  sent 0 full quickbar, 0 post-action `G Q`, and 0 candidate active-property
  responses. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-coalesced-continuation-fix-20260706-164526`
  over the 2026-07-03 Diamond autoplay capture stayed at 164 packet files, 304
  strict allows, 0 strict quarantines, and 0 quarantine files. The follow-up
  handoff slice above generalizes the durable typed `G Q` use-count row into
  the EE client/visible quickbar state rule.
- 2026-07-06 durable `G Q` quickbar use-count state: live-data gate found
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-live-gq-resolution-current-20260706-102406\harness-proxy-20260706-102502`
  fresh, then a current-code live harness
  `C:\nwnbridge\codex-live-gq-slot-relation-current-20260706-142738\harness-proxy-20260706-142747`
  also reached gameplay, wrote `quickbar-item-refresh-hint.json` at
  `2026-07-06T14:41:03+10:00`, and produced no quarantine artifacts. The final
  live hint selected candidate `0x80015AE3`, matched the first preserved active
  item in quickbar slot 0, dispatched the subtype-low `UseItem`, and again saw
  no server quickbar, no `G Q`, and no active-property response after the
  action. Proxy2 now keeps a durable semantic table of verified typed
  live-object `G Q` item-use-count rows keyed by slot/button/object/property,
  exposes the candidate table row in active and idle quickbar item-refresh
  hints, and exports the same fields in replay summaries. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-use-count-state-20260706-144554` over the
  2026-07-03 Diamond autoplay capture stayed at 164 packet files, 304 strict
  allows, 0 strict quarantines, and 0 quarantine files; its pending hint
  correctly reported 0 durable use-count state rows and
  `candidate_quickbar_item_use_count_state_slot_relation="no_candidate_use_count_row"`.
  Active next path: rerun live HG after this state-table slice and compare
  whether any prior durable `G Q` row exists for the selected active item when
  the final hint lands in the no-server-response branch.
- 2026-07-06 pre-action `G Q` quickbar state handoff: live-data gate reused
  the gameplay-reaching HG proxy harness
  `C:\nwnbridge\codex-live-preaction-gq-suppression-20260706-070013\harness-proxy-20260706-070018`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-06T07:02:35+10:00`; about 1h20m old at gate). Gameplay was reached
  through `Module_Loaded`, `Area_ClientArea`, and sustained live-object
  traffic, with no quarantine directory. The capture showed four typed
  candidate live-object `G Q` item-use-count rows before any client action.
  Proxy2 now treats that verified server row as the generalized semantic
  resolution for the pending post-committed quickbar item-refresh window:
  it snapshots the pending counters/first-row timing into the last committed
  quickbar summary, records
  `pending_item_refresh_outcome="pending_refresh_observed_use_count_rows"`,
  clears the active pending hint, and reports
  `no_hint_reason="post_context_resolved_by_server_quickbar_use_count"`.
  No packet bit reader/writer changed in this slice; it consumes the existing
  typed `G Q` rows from the decompile-backed live-object parser. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-gq-resolution-20260706-083719`
  over the 2026-07-03 Diamond autoplay capture stayed at 164 packet files, 304
  strict allows, 0 strict quarantines, and 0 quarantine files; that capture has
  no candidate `G Q` row in the pending window, so
  `QuickbarSemanticPendingItemRefreshOutcomeObservedUseCountRows=0` is
  expected. Replay summaries now count that outcome for captures that do have
  the resolved row. Next verification path: run the live HG harness on the
  current build and confirm the final hint resolves by server quickbar use-count
  rows without dispatching another generated active-property action.
- 2026-07-06 current-code `G Q` resolution rerun: fresh live HG harness
  `C:\nwnbridge\codex-live-gq-resolution-current-20260706-102406\harness-proxy-20260706-102502`
  reached gameplay, wrote the final quickbar item-refresh hint at
  `2026-07-06T10:28:23+10:00`, and produced no quarantine artifacts. This run
  did not reproduce the earlier pre-action `G Q` row: candidate `0x80015A29`
  dispatched the matched subtype-low `UseItem`, and HG returned 0 full quickbar,
  0 `G Q`, and 0 candidate active-property uses/full responses after 167
  post-action events. Proxy2 now carries the parsed quickbar slot of the first
  preserved active item into quickbar rewrite summaries, semantic harness hints,
  and replay summaries so the next live comparison can check whether `G Q` rows
  line up with the actual active quickbar slot before changing action/state
  rules. Next path: trace original client handling of active-property use-count
  rows and why live HG sometimes emits pre-action `G Q` and sometimes no
  quickbar/property response.
- 2026-07-06 `G Q` slot-relation discriminator: live-data gate reused
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-gq-resolution-current-20260706-102406\harness-proxy-20260706-102502`
  (`quickbar-item-refresh-hint.json` last write `2026-07-06T10:28:23+10:00`;
  about two hours old at the gate). Gameplay reached through `Module_Loaded`,
  `Area_ClientArea`, and sustained live-object traffic, with no quarantine
  artifacts. Proxy2 now derives
  `first_server_quickbar_item_use_count_candidate_row_slot_relation` and a
  boolean slot-match flag between a typed candidate live-object `G Q` use-count
  row and the first preserved active quickbar item slot. The fields are present
  in both pending hints and idle hints after a server-`G Q` resolution, so a
  successful pre-action row no longer clears the evidence needed for the next
  live comparison. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-gq-slot-relation-20260706-1240` over the
  2026-07-03 Diamond autoplay capture stayed at 164 packet files, 304 strict
  allows, 0 strict quarantines, and 0 quarantine files; that replay correctly
  reported `no_candidate_use_count_row` because it has no candidate `G Q` row.
  Active next path: run the live HG harness on current code and compare
  `matches_preserved_active_item_slot` versus
  `differs_from_preserved_active_item_slot` when HG emits the candidate `G Q`
  row.
- 2026-07-06 pre-action `G Q` quickbar response suppression: live-data gate
  first reused the gameplay-reaching HG proxy harness
  `C:\nwnbridge\codex-live-active-property-outcome-20260706-022124\harness-proxy-20260706-022134`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-06T02:26:01+10:00`; about 3h56m old at gate). A current-code probe
  before the implementation,
  `C:\nwnbridge\codex-live-useitem-subtype-low-row-current-20260706-062327\harness-proxy-20260706-062336`,
  reached gameplay and showed the same pattern: a server live-object `G Q`
  item-use-count row for the candidate existed before the injected subtype-low
  `UseItem`, while no quickbar response followed the injected action. Proxy2
  now classifies any pre-first-client-action server quickbar response as
  `server_quickbar_response_before_first_client_action` /
  `server_quickbar_response_before_recommended_action`, exports
  `recommended_client_action_should_dispatch=false` plus the suppression
  reason in `quickbar-item-refresh-hint.json`, and the EE bridge/harness honors
  that hint for all generated quickbar item-refresh probes. Fresh verification
  harness
  `C:\nwnbridge\codex-live-preaction-gq-suppression-20260706-070013\harness-proxy-20260706-070018`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, wrote the final hint at
  `2026-07-06T07:02:35+10:00`, produced no quarantine directory, selected
  candidate `0x8001596F`, recorded four candidate `G Q` rows before any client
  action and zero after, and left `first_client_action="none"`. The driver log
  confirmed the generated subtype-low action was skipped with
  `recommended client action suppressed by proxy hint:
  server_quickbar_response_before_first_client_action`. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-preaction-gq-suppression-20260706-070434`
  over the 2026-07-03 Diamond autoplay capture stayed at 164 packet files, 304
  strict allows, 0 strict quarantines, and 0 quarantine files. Active next path:
  trace and implement the generalized EE client handling/state handoff for the
  pre-action live-object `G Q` quickbar use-count row so that the visible
  quickbar/active-property state matches original gameplay, rather than adding
  another generated action identity probe.
- 2026-07-06 server quickbar `G Q` row timing details: live-data gate reused
  the gameplay-reaching HG proxy harness
  `C:\nwnbridge\codex-live-active-property-outcome-20260706-022124\harness-proxy-20260706-022134`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-06T02:26:01+10:00`; about 1h56m old at gate). Gameplay was reached
  through `Module_Loaded`, `Area_ClientArea`, and sustained live-object
  traffic, with no quarantine artifacts. The current production slice records
  the first typed live-object `G Q` use-count row for the pending quickbar item
  candidate overall, before the first client action, and after the first client
  action; `quickbar-item-refresh-hint.json` and replay summaries now expose
  the row `slot`, `button_type`, object id, active-property index, use count,
  and timing. Active next path: rerun the live subtype-low active-property
  probe with these row fields visible, then use the pre-action candidate row to
  prove the original-client active-property state handoff before changing the
  generalized translator/action rule.
- 2026-07-06 server quickbar response timing diagnostics: live-data gate first
  used gameplay-reaching HG proxy harness
  `C:\nwnbridge\codex-live-useitem-subtype-low-after-stream-promote-20260705-183917\harness-proxy-20260705-183926`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T18:43:53+10:00`; about 7.6h old at gate). Fresh current-code
  subtype-low probe
  `C:\nwnbridge\codex-live-active-property-outcome-20260706-022124\harness-proxy-20260706-022134`
  reached gameplay, wrote `quickbar-item-refresh-hint.json` at
  `2026-07-06T02:26:01+10:00`, and produced no quarantine artifacts. Candidate
  `0x80015D4C` matched the preserved active-property quickbar item and the
  first client action was
  `first_client_action_match_class="recommended_use_item_first_property_subtype_low"`.
  After 293 post-action events (167 server-to-client, 89 live-object), HG sent
  0 full `GuiQuickbar`, 0 live-object `GQ`, and 0 candidate `0x18/0x01` or
  `0x18/0x02` active-property responses. The full pending window did contain
  one live-object `GQ` event with four candidate rows before the injected
  client action, so proxy2 now derives
  `pending_item_refresh_server_quickbar_response_timing` and explicit
  `*_before_first_client_action` response counters in the hint JSON, plus replay
  summary readback for the timing field. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-server-response-timing-20260706-023636`
  over the 2026-07-03 Diamond autoplay capture stayed at 164 packet files, 304
  strict allows, and 0 quarantines, with
  `QuickbarItemRefreshHintServerQuickbarResponseTiming=awaiting_client_action`.
  Active next path: trace why the original client receives/uses the pre-action
  `GQ` refresh before our generated action, then implement the generalized
  active-property state handoff instead of another exact action payload probe.
- 2026-07-06 active-property response outcome diagnostics: live-data gate used
  gameplay-reaching HG proxy harness
  `C:\nwnbridge\codex-live-useitem-subtype-low-after-stream-promote-20260705-183917\harness-proxy-20260705-183926`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T18:43:53+10:00`; about 5h52m old at gate). Gameplay was
  reached through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, with no quarantine artifacts, so no fresh live
  run was required. Proxy2 now exposes a bounded
  `pending_item_refresh_active_property_outcome` plus candidate-specific
  `0x18/0x01` uses-delta and `0x18/0x02` full-refresh counters/row totals for
  both the full pending window and the post-first-client-action window in
  semantic traces, `quickbar-item-refresh-hint.json`, and replay summaries.
  This is an implementation-enabling diagnostic slice: it does not change the
  generated SetButton, GuiEvent_Notify, UseObject, zero-byte UseItem, or
  subtype-low UseItem actions. Active next path: run the next live
  subtype-low/active-property probe and use
  `pending_item_refresh_active_property_outcome` to distinguish no HG
  active-property response from candidate `uses`, candidate `full`, or
  candidate `uses+full` responses before changing the generalized
  active-property action/state handoff.
- 2026-07-05 active item property update packet ownership: live-data gate reused
  the gameplay-reaching HG proxy harness
  `C:\nwnbridge\codex-live-useitem-subtype-low-after-stream-promote-20260705-183917\harness-proxy-20260705-183926`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T18:43:53+10:00`; about 3h35m old at gate). Gameplay was
  reached through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, with no quarantine artifacts. EE decompile
  evidence for `CNWSItem::UpdateUsedActiveProperties` shows active-property
  server refresh state is emitted as high-level family `0x18`: minor `0x01`
  writes OBJECTID, used mask, changed-uses mask, and one use-count byte for
  each changed mask bit; minor `0x02` writes OBJECTID, active-property count,
  7-byte property rows, used mask, `0xFF`, and eight use-count bytes. Proxy2
  now owns those exact no-BOOL CNW cursor shapes as
  `ItemUpdate_ActiveProperties`, registers them in strict/server dispatch and
  the gameplay splitter, and exposes separate pending item-refresh hint
  counters for active-property events, uses/full events, and candidate-object
  hits. Active next path: run the next live subtype-low/active-property probe
  and check whether HG replies with `0x18/0x01` or `0x18/0x02`; if still zero,
  continue tracing the original-client active-property action/state handoff
  rather than retesting exact probe identity.
- 2026-07-05 live-object `GQ` quickbar item-use-count response tracking:
  live-data gate reused gameplay-reaching HG harness
  `C:\nwnbridge\codex-live-useitem-subtype-low-after-stream-promote-20260705-183917\harness-proxy-20260705-183926`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T18:43:53+10:00`; about 1h34m old at gate). Gameplay was
  reached through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, with no quarantine artifacts. EE server
  decompile evidence shows quickbar item use-count refreshes are emitted as
  live-object GUI `G Q` rows (`slot`, `button_type`, raw item object id,
  active-property index, `use_count`), not only as full `GuiQuickbar`
  packets. Proxy2 now exposes verified `GQ` rows as typed live-object
  quickbar item-use-count updates, counts them separately in pending
  item-refresh state and harness hints, and treats either full `GuiQuickbar` or
  verified `GQ` as an observed server quickbar response. The latest live
  capture still contained 0 such `GQ` rows after the matching subtype-low
  UseItem action, so the active implementation target remains the generalized
  original-client active-property action/state handoff.
- 2026-07-05 UseItem subtype-low dispatch live result: live-data gate used
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-useobject-after-current-creature-20260705-121704\harness-proxy-20260705-121714`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T12:24:46+10:00`; about 5h51m old at gate). Gameplay was reached
  and no quarantine artifacts were present. The bridge/harness now has an
  opt-in `HG_BRIDGE_AUTO_QUICKBAR_ITEM_REFRESH_USEITEM_SUBTYPE_LOW` /
  `-AutoQuickbarItemRefreshUseItemSubtypeLow` path that validates the hinted
  `Input_UseItem` high-level shape before dispatch: header `70 06 09`, exact
  declared byte count, `OBJECTID`, active-property byte, optional-byte BOOL,
  optional-target BOOL/object, optional-position BOOL/vector, and the final
  3-bit BOOL fragment. Production proxy2 also promotes completed
  quickbar stream-probe profiles from `quickbar_stream` summaries into the
  committed semantic quickbar state; the first live subtype-low attempt reached
  gameplay with no quarantines but exposed this missing promotion by reporting
  `stream_probe_quickbar_item_candidates_without_committed_profile`.
  Fresh current-code probe
  `C:\nwnbridge\codex-live-useitem-subtype-low-after-stream-promote-20260705-183917\harness-proxy-20260705-183926`
  reached gameplay, produced 0 quarantines, committed the stream-probe quickbar
  profile, selected candidate `0x80015989` from `active_object` / `direct_only`
  proof, generated payload `70060910000000895901800DFDFFFFFFC8`, and observed
  the first client action as `client_input_use_item` with
  `first_client_action_match_class="recommended_use_item_first_property_subtype_low"`.
  HG still emitted 0 server quickbar events after the matching action
  (`pending_item_refresh_recommended_action_outcome="recommended_use_item_first_property_subtype_low_no_server_quickbar"`).
  Active next path: stop cycling exact SetButton, GuiEvent_Notify, UseObject,
  zero-byte UseItem, and subtype-low UseItem identity probes. Trace the original
  Diamond/EE active-property action/state handoff that causes HG to schedule
  the quickbar refresh, then implement that generalized state/translator rule.
- 2026-07-05 UseItem first-property subtype-low diagnostic: live-data gate
  reused gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-useobject-after-current-creature-20260705-121704\harness-proxy-20260705-121714`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T12:24:46+10:00`; about 3h50m old at the gate). Gameplay was
  reached through `Module_Loaded`, `Area_ClientArea`, and sustained
  `GameObjUpdate_LiveObject`, with no quarantine artifacts. The live hint
  showed candidate `0x80015678` matching the preserved active-property quickbar
  item, first property `15`, subtype `0x020D`, and cost-table value `13`.
  EE decompile evidence for group input case 9 reads `OBJECTID`, the
  active-property byte, optional-byte BOOL, optional-target BOOL/object, and
  optional-position BOOL/vector before calling server `UseItem`, so the
  generated packet cursor is byte/bit-order backed but the byte's gameplay
  meaning remains diagnostic. Proxy2 now emits
  `recommended_use_item_first_property_subtype_low_*` hint fields only when the
  first preserved active item matches the pending candidate, using the low byte
  of that first property subtype; it also classifies observed first actions as
  `recommended_use_item_first_property_subtype_low` and reports dedicated
  no-server-quickbar/observed-server-quickbar outcomes. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-useitem-subtype-low-retry-20260705-163118`
  against
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  completed with 164 packet files, 304 strict allow decisions, 0 quarantine
  decisions/artifacts, and correctly left the subtype-low payload unavailable
  for replay candidate `0x80015DAA` because no preserved active item matched.
  Resolved 2026-07-05: the guarded bridge/harness dispatch path now sends the
  subtype-low UseItem probe, and live HG confirmed the payload is observed as a
  matching first client action but is still insufficient to trigger a server
  quickbar refresh.
- 2026-07-05 UseItem active-property action classifier: live-data gate reused
  the gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-useobject-after-current-creature-20260705-121704\harness-proxy-20260705-121714`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T12:24:46+10:00`; about 1.8h old at the gate and about 2.2h old
  when this slice was documented). Gameplay was reached through sustained
  live-object traffic and no quarantine artifacts were present, so no fresh
  live run was required. Proxy2 now carries parsed `Input_UseItem`
  active-property subtype, optional-byte, target-object, and position presence
  into semantic quickbar action details and pending item-refresh hints. The
  hint classifier recognizes the exact generated UseItem probe shape as
  `recommended_use_item` only when it targets the pending candidate, preserves
  subtype byte `0`, omits the optional byte and position, and targets either
  EE self `0xFFFF_FFFD` or legacy self `0xFFFF_FFFF`. This does not claim
  UseItem is the final HG action fix; it removes the diagnostic blind spot so
  the next live/decompile pass can prove whether the active-property subtype
  byte is a constant zero, a quickbar/property index, or another original-client
  state field before changing outbound action generation.
- 2026-07-05 UseObject active-item probe scaffold: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-active-item-signature-current-20260705-041228\harness-proxy-20260705-041233`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T04:17:44+10:00`; about 1.9h old at the gate). Gameplay was
  reached through live-object traffic and no quarantine artifacts were present.
  Because exact `GuiEvent_Notify` delivery to the preserved active quickbar
  item still produced no server quickbar, proxy2 now builds a decompile-backed
  `Input_UseObject` (`70 06 0B`) candidate action for the same pending item.
  The builder writes the raw object id followed by the two EE/legacy
  server-reader BOOLs in order, currently both false, and self-validates
  through the focused `client_input` parser. Pending quickbar hints now expose
  `recommended_client_use_object_*` fields and classify an observed first
  action as `recommended_use_object` only when kind, object id, and both BOOLs
  exactly match. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-useobject-hint-20260705-061927` against
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  completed with 164 packet files, 304 strict allow decisions, 0 quarantine
  decisions/artifacts, and emitted
  `recommended_client_use_object_payload_hex=70060B0B000000AA5D0180A0` for
  candidate `0x80015DAA`. Resolved 2026-07-05: the bridge/harness now has an
  opt-in `HG_BRIDGE_AUTO_QUICKBAR_ITEM_REFRESH_USEOBJECT` /
  `-AutoQuickbarItemRefreshUseObject` dispatch path that validates the hinted
  `Input_UseObject` packet before sending it through
  `CNWMessage::SendPlayerToServerMessage`. Reference setup: live HG
  driver-only harness, because the unresolved question is HG's response to the
  generated client action. Fresh probe
  `C:\nwnbridge\codex-live-useobject-driver-retry-20260705-0827\harness-proxy-20260705-082506`
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, and
  `GameObjUpdate_LiveObject` traffic, but no UseObject was dispatched because
  the hint stayed `pending_item_refresh=false` with
  `no_hint_reason="no_committed_quickbar_profile"`. The same run quarantined a
  517-byte `GameObjUpdate_LiveObject` payload as
  `live-object-unclaimed-strict-family` after exact record-boundary validation
  rejected the intermediate rewrite. Resolved 2026-07-05: the current-creature
  full `P/5` appearance now proves the promoted `100` CNW header as a bounded
  fence before the direct-name selector, accepts sentinel id `0xFFFF_FFF8`,
  and owns all eight counted visible-equipment rows through the following
  `U/5` boundary instead of splitting on the embedded item `A` rows or the
  printable `W` inside "Wrap of the Dark Prince". Private live regression
  coverage uses the quarantined seq28 payload and proves the appearance record
  ends at live offset 407. Fresh current-code UseObject rerun
  `C:\nwnbridge\codex-live-useobject-after-current-creature-20260705-121704\harness-proxy-20260705-121714`
  selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
  gameplay, wrote its final hint at `2026-07-05T12:24:46+10:00`, and produced
  no quarantine artifacts. The committed quickbar profile recovered; candidate
  `0x80015678` was `active_object` / `direct_only`, matched the preserved
  active-property quickbar item, and the bridge dispatched the validated
  `Input_UseObject` payload `70060B0B00000078560180A0`. The first client
  action was `client_input_use_object`, matched the recommended UseObject probe,
  but HG still sent 0 server quickbar events after 583 post-action events
  (181 live-object, 1 inventory, 1 chat). Proxy2 now also classifies the
  specifically recommended action family outcome as
  `pending_item_refresh_recommended_action_outcome`; this live result derives
  as `recommended_use_object_no_server_quickbar`. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-recommended-outcome-20260705-123353`
  against
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  completed with 164 packet files, 304 strict allow decisions, 0 quarantine
  decisions/artifacts, and exported
  `QuickbarItemRefreshHintRecommendedActionOutcome=awaiting_client_action` for
  the replay's no-client-action pending window. Active next path: stop cycling
  exact SetButton, GuiEvent_Notify, UseItem, and UseObject probe identity; trace
  original Diamond/EE active-property item action/state semantics and implement
  the generalized rule that differs from these exact payload probes.
- 2026-07-05 active-item action match-class slice: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-gui-event-shape-match-20260705-002118\harness-proxy-20260705-002126`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T00:26:33+10:00`; about 3h44m old at the gate, still under
  24h). Fresh current-code GUI-event probe
  `C:\nwnbridge\codex-live-active-item-signature-current-20260705-041228\harness-proxy-20260705-041233`
  reached gameplay, wrote its final hint at `2026-07-05T04:17:34+10:00`, and
  produced no quarantine artifacts. The first preserved active item
  `0x80015219` matched the pending candidate, and the first client action was
  the generated `GuiEvent_Notify` probe matching the candidate, but HG still
  sent 0 server quickbar events after 319 post-action events / 100 live-object
  events. Proxy2 now classifies first client-action match strength in hint JSON
  and semantic traces with
  `first_client_action_matches_preserved_active_item` and
  `first_client_action_match_class`. Strict rebuilt replay
  `C:\nwnbridge\codex-proxy2-replay-action-match-class-rebuilt-20260705-0441`
  against
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  completed with 164 packet files, 304 strict allow decisions, 0 quarantine
  decisions/artifacts, and replay hint class `awaiting_client_action`. Active
  next path: stop retesting target identity and bounded payload shape; trace
  the original Diamond/EE active-property item action/state semantics that
  differ from the exact GUI-event probe.
- 2026-07-05 active-item signature instrumentation slice: live-data gate used
  the gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-gui-event-shape-match-20260705-002118\harness-proxy-20260705-002126`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-05T00:26:33+10:00`; about 1h45m old at the gate, still under
  24h). No fresh live run was required. Proxy2 now carries the first accepted
  quickbar active-item signature from the verified `GuiQuickbar_SetAllButtons`
  rewrite summary into stream-probe semantic state, unresolved traces, idle
  hints, and pending `quickbar-item-refresh-hint.json`. The signature records
  object id, base item, appearance type, active-property count, first
  property/subtype/cost-table/param, armor/name flags, and state/value masks,
  plus whether the signature matches the pending item-refresh candidate. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-active-item-signature-20260705-022017`
  against
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  completed with 164 packet files, 304 strict allow decisions, 0 quarantine
  decisions/artifacts, and a pending feature-25-only candidate `0x80015DAA`.
  The replay hint included the new fields with
  `first_preserved_active_item_known=false`, proving the field path is present
  even when the replay's pending candidate is not backed by a preserved
  active-item quickbar body. Active next path: run the next live GUI-event
  probe with these fields and compare whether HG's live candidate matches the
  preserved active-property quickbar item before changing the generalized
  active-property action/state rule.
- 2026-07-05 GUI-event probe shape-match live evidence: live-data gate used
  the gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-stream-probe-commit-gui-event-20260704-162250\harness-proxy-20260704-162301`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-04T16:27:55+10:00`; about 7h43m old at the gate, still under
  24h). Proxy2 now compares the first quickbar item-refresh client action
  against the generated SetButton and `GuiEvent_Notify` probe shapes and
  exports
  `first_client_action_matches_recommended_client_quickbar_set_button` plus
  `first_client_action_matches_recommended_client_gui_event_notify` in the
  harness hint and unresolved trace. Fresh live rerun
  `C:\nwnbridge\codex-live-gui-event-shape-match-20260705-002118\harness-proxy-20260705-002126`
  selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
  gameplay, dispatched candidate `0x80015B11`, and wrote the final hint at
  `2026-07-05T00:26:07+10:00` with no quarantine files. The first client
  action was `client_gui_event_notify`, matched the candidate, had event
  words `0x0011/0x0000`, declared 27 bytes, one trailing fragment byte, a zero
  vector, and
  `first_client_action_matches_recommended_client_gui_event_notify=true`.
  After the exact matched probe HG still sent 363 post-action events
  (198 server-to-client, 165 client-to-server, 111 live-object, 1 inventory,
  1 chat) and 0 server quickbar events. Strict replay of
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  through the same proxy build completed with 164 packet files, 304 strict
  allow decisions, and 0 quarantine decisions/artifacts; replay did not contain
  the live client GUI-event follow-up and remains a packet-shape regression
  check rather than proof that the live action is sufficient. Active next path:
  stop treating the bounded GUI-event payload/driver delivery as the suspect;
  trace the original Diamond/EE active-property item action semantics and
  implement the next generalized client action/state rule that differs from the
  exact `GuiEvent_Notify` probe.
- 2026-07-04 device-property classifier live rerun: live-data gate first used
  the gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-client-gui-event-current-20260704-094122\harness-proxy-20260704-094127`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-04T09:43:40+10:00`; about 4h23m old at the gate, still under 24h).
  Because the previous GUI-event notify run failed before gameplay, proxy2 now
  claims EE `Device_AdvertiseProperty` (`70 36 01`) with the CNW declared
  read-buffer length at payload offset 3 and the `CExoString` property name at
  offset 7. Fresh live rerun
  `C:\nwnbridge\codex-live-device-property-classifier-gui-event-20260704-142731\harness-proxy-20260704-142740`
  selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, wrote
  `quickbar-item-refresh-hint.json` at `2026-07-04T14:28:57+10:00`, and
  reached gameplay through `Module_Loaded`, `Area_ClientArea`, synthetic
  `Area_AreaLoaded`, exact `GameObjUpdate_LiveObject`, and quickbar stream
  probe evidence. The run consumed 70 `Device_AdvertiseProperty` frames as
  proxy-owned EE-only M payloads, logged 0 `client high-level M frame
  quarantined` lines, and wrote no quarantine artifact files. Remaining issue:
  `-AutoQuickbarItemRefreshGuiEventNotify` still had no dispatchable candidate
  because the final hint was
  `stream_probe_quickbar_item_candidates_without_committed_profile` with
  `committed_quickbar_seen=false`. Resolved 2026-07-04 by carrying the exact
  validated quickbar slot profile on stream-probe rewrite summaries and adding
  a guarded semantic-state promotion path for exact stream probes that do not
  already carry a normal `GuiQuickbar` proof.
- 2026-07-04 quickbar stream-profile follow-up: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-device-property-classifier-gui-event-20260704-142731\harness-proxy-20260704-142740`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-04T14:28:57+10:00`; about 1h40m old at the gate, still under 24h).
  Proxy2 now records `validated_slot_profile` in `GuiQuickbar_SetAllButtons`
  rewrite summaries and can promote exact stream-probe profiles into committed
  quickbar semantic state when the verified proof does not already include
  `GuiQuickbar`. Fresh live rerun
  `C:\nwnbridge\codex-live-stream-probe-commit-gui-event-20260704-162250\harness-proxy-20260704-162301`
  selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
  gameplay through `Module_Loaded`, two `Area_ClientArea` observations, live
  object traffic, and the GUI-event notify path, and wrote
  `quickbar-item-refresh-hint.json` at `2026-07-04T16:27:55+10:00`. This live
  run logged `validated_slot_profile=true` for the stream probe, but
  `promoted_committed_profile=false` because the same window also carried a
  normal committed `GuiQuickbar` slot profile. The final hint now has
  `pending_item_refresh=true`, candidate `0x80015219` with
  `candidate_proof="active_object"` / `candidate_source="direct_only"`,
  matched `first_client_action="client_gui_event_notify"`, and
  `first_client_action_matches_candidate=true`; after that action HG produced
  147 server-to-client, 136 client-to-server, 91 live-object, 1 inventory, and
  0 quickbar follow-up events. No client high-level M-frame quarantines or
  quarantine artifacts were observed. Active next path: trace the original
  client active-property action semantics/timing for this item refresh, because
  the bounded `GuiEvent_Notify` probe lands but HG does not emit a server
  quickbar refresh.
- 2026-07-04 GUI-event action-shape diagnostic slice: live-data gate reused
  the gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-stream-probe-commit-gui-event-20260704-162250\harness-proxy-20260704-162301`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-04T16:27:55+10:00`; about 1h40m old at the gate, still under
  24h). Proxy2 now retains the first client `GuiEvent_Notify` shape in the
  pending quickbar item-refresh action detail and exports event A/B, declared
  bytes, trailing fragment bytes, vector presence, and raw vector bits through
  `quickbar-item-refresh-hint.json` and unresolved semantic traces. Reference
  setup: fresh live HG evidence is still the authoritative source for the
  missing item-refresh action, while the bounded radial notify payload remains
  guarded by the typed client GUI-event builder. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-gui-event-shape-20260704-1855` against
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  stayed at 164 packet files, 304 strict allows, and 0 quarantines. Active next
  path: run the next live GUI-event/active-property probe with these fields,
  then compare the exact event ids, body length, vector branch, and timing
  against Diamond/EE decompiles before changing any broad active-item
  translator rule.
- 2026-07-04 GUI-event live probe blocker: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-client-gui-event-current-20260704-094122\harness-proxy-20260704-094127`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-04T09:43:40+10:00`; about 1h49m old at the gate, still under 24h).
  The first `-AutoQuickbarItemRefreshGuiEventNotify` run selected a stale repo
  debug proxy and reached module load before strict `LiveObject` and
  `Area_ClientArea` quarantines. The harness resolver now chooses the newest
  compatible proxy2 executable among repo and `C:\nwnbridge\cargo-target`
  builds, making `-SkipBuild` use the freshly built proxy instead of silently
  preferring an older repo debug binary. Retry run
  `C:\nwnbridge\codex-live-gui-event-notify-newest-proxy-retry-20260704-114234\harness-proxy-20260704-114239`
  selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe` and passed
  BNK/BNCS/BNVR, character list, login, `Module_Info`, and
  `CNWCModule::LoadModuleResources`, but did not reach `Module_Loaded`,
  `Area_ClientArea`, live-object traffic, or GUI-event dispatch by the run
  cutoff. No quarantine files were written; the hint stayed
  `pending_item_refresh=false` with `no_committed_quickbar_profile`. The proxy
  logged repeated pre-gameplay client high-level unknown M-frame quarantines
  matching the EE `Device_AdvertiseProperty` window seen in the bridge log.
  Resolved 2026-07-04 by the shared `Device_AdvertiseProperty` classifier
  described above; keep this paragraph as the failure-mode trail.
- 2026-07-04 quickbar GUI-event notify probe slice: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-client-gui-event-current-20260704-094122\harness-proxy-20260704-094127`
  (`quickbar-item-refresh-hint.json` last write
  `2026-07-04T09:43:40+10:00`; about 47 minutes old at the start-of-run gate,
  still under 24h). Gameplay reached `Area_ClientArea` and sustained
  live-object traffic with no quarantine files. The hint remained
  `candidate_client_action_no_server_quickbar` after matched SetButton
  candidate `0x8001620D`, with 48 server-to-client, 39 client-to-server, 33
  live-object, 1 inventory, 1 chat, 0 quickbar, and 0
  `client_gui_event_notify` events after the pending proof. Proxy2 now builds
  and self-validates a bounded `ClientGuiEvent/Notify` radial probe payload
  (`70 35 01`, declared 27-byte EE vector form, event words `0x0011/0x0000`,
  candidate object id, zero vector, final fragment cursor `0x60`) and exports
  it in `quickbar-item-refresh-hint.json`. The EE bridge and harness now have
  opt-in `HG_BRIDGE_AUTO_QUICKBAR_ITEM_REFRESH_GUI_EVENT_NOTIFY` /
  `-AutoQuickbarItemRefreshGuiEventNotify` dispatch for that exact hinted
  payload. Reference setup: fresh live HG SetButton evidence for the unresolved
  action gap plus strict replay/unit checks for the writer/hint shape. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-gui-event-probe-20260704-1047` stayed at
  164 packet files, 304 strict allows, 0 strict quarantines, and 0 quarantine
  files; its hint exported
  `recommended_client_gui_event_notify_payload_hex=7035011B00000011000000AA5D018000000000000000000000000060`
  for candidate `0x80015DAA`. Next production path: run live HG with
  `-AutoQuickbarItemRefreshGuiEventNotify`, then inspect whether HG records
  `first_client_action="client_gui_event_notify"` and emits a server quickbar
  refresh before changing the active item action translator rule.
- 2026-07-04 consumed ClientGuiEvent semantic-observation slice: live-data
  gate used the gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-direction-counters-setbutton-20260704-0535\harness-proxy-20260704-053414`
  (`quickbar-item-refresh-hint.json` last write `2026-07-04T05:39:44+10:00`;
  about 50 minutes old at the start-of-run gate, still under 24h). Gameplay
  was reached through `Area_ClientArea` and sustained live-object traffic; the
  hint remained `candidate_client_action_no_server_quickbar` after matched
  SetButton candidate `0x800153FD`. Proxy2 now keeps the original verified
  `GuiEvent_Notify` payload as a semantic observation even when the M-frame
  compatibility layer consumes it into an empty Diamond/1.69 carrier. The
  pending quickbar item-refresh window now counts `client_gui_event_notify`,
  can record it as the first client action with object-id/candidate matching,
  and the replay/hint summaries expose the new GUI-event buckets. Reference
  setup: fresh live HG proxy evidence for the current SetButton gap plus unit
  replay-style regressions for the generalized client-frame and semantic-state
  rules. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-gui-event-20260704-0940` stayed at
  164 packet files, 304 strict allows, 0 strict quarantines, 0 quarantine
  files, and exported the new GUI-event summary fields with zero counts for
  the Diamond replay's still-`awaiting_client_action` pending window. Fresh
  post-build live probe
  `C:\nwnbridge\codex-live-client-gui-event-current-20260704-094122\harness-proxy-20260704-094127`
  reached gameplay and wrote `quickbar-item-refresh-hint.json` at
  `2026-07-04T09:43:34+10:00` with no quarantine files. It observed matched
  SetButton candidate `0x8001620D`, `client_quickbar_item_set_button`,
  47 server-to-client and 37 client-to-server events in the pending window, 44
  server-to-client and 35 client-to-server after the first client action, 31
  live-object events after the action, 0 quickbar events, and 0
  `client_gui_event_notify` events. Next
  production path: run the live radial/menu action probe and check
  `client_gui_event_events_since_pending_refresh` and
  `first_client_action="client_gui_event_notify"` before changing the active
  item action translator rule.
- 2026-07-04 quickbar direction-counter and live SetButton probe: live-data
  gate first checked the gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-quickbar-setbutton-driver-20260704-003119\harness-proxy-20260704-003123`
  (proxy log last write `2026-07-04T00:36:53.3320829+10:00`, about 4.9h old
  at gate time), so ordinary production work could continue. Proxy2 now records
  server-to-client and client-to-server event totals for pending quickbar
  item-refresh windows in semantic traces, `quickbar-item-refresh-hint.json`,
  and replay summaries. Reference setup: strict replay of Diamond HG gameplay
  traffic for repeatable legacy pending-window evidence, plus a fresh live HG
  EE driver-only SetButton probe because the open question is HG's live
  response after a matched client action. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-direction-counters-20260704-0532` stayed at
  164 packet files, 304 strict allows, 0 strict quarantines, and 0 quarantine
  files; its pending hint for `0x80015DAA` reported 190 post-proof events split
  96 server-to-client and 94 client-to-server, still awaiting client action.
  Fresh live probe
  `C:\nwnbridge\codex-live-direction-counters-setbutton-20260704-0535\harness-proxy-20260704-053414`
  reached gameplay through `Area_ClientArea`/live-object traffic, dispatched
  matched candidate `0x800153FD` with SetButton payload
  `701E02120000000501FD530180FFFFFFFF0060`, and ended with
  `candidate_client_action_no_server_quickbar`: 365 post-action events, 187
  server-to-client, 178 client-to-server, 114 live-object, 1 inventory, 1 chat,
  and 0 quickbar events. No quarantine artifact files were written, but the
  run logged 70 pre-gameplay unclaimed client high-level frame isolations before
  `Area_ClientArea`; track that separately as a generalized client-frame
  ownership gap, not as a harness connection blocker. Next production path:
  compare Diamond/EE active-property item radial/action semantics and decide
  whether the missing behavior is a radial/menu `ClientGuiEvent`/`ClientInput`
  sequence or another client state transition, since HG is continuing to send
  server traffic after the current SetButton but no quickbar refresh.
- 2026-07-04 live SetButton driver probe: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-useitem-self-target-hint-20260703-223120\harness-proxy-20260703-223124`
  (log last write `2026-07-03T22:34:14.7839098+10:00`, gameplay reached, about
  1.8h old at the gate). The bridge/harness now has an opt-in
  `-AutoQuickbarItemRefreshSetButton` path that validates the full high-level
  `70 1E 02` `GuiQuickbar_SetButton` item payload from proxy2's hint file and
  sends it through `CNWMessage::SendPlayerToServerMessage`. Reference setup:
  fresh live HG via the EE driver-only harness, because the question is HG's
  live response to the SetButton action after previous UseItem probes produced
  no quickbar refresh. Fresh live probe
  `C:\nwnbridge\codex-live-quickbar-setbutton-driver-20260704-003119\harness-proxy-20260704-003123`
  reached gameplay, committed an item-bearing quickbar profile, and dispatched
  candidate `0x80016A0F` with payload
  `701E021200000000010F6A0180FFFFFFFF0060`. The hint recorded
  `first_client_action="client_quickbar_item_set_button"`,
  `first_client_action_matches_candidate=true`, and 353 events after that
  action, including 113 live-object events, 1 inventory event, 1 chat event,
  and 0 server quickbar events. Next production path: compare original
  Diamond/EE active-property item radial/quickbar action semantics and timing;
  the driver can now send the typed SetButton action, so the remaining gap is
  likely a missing action-family/state rule rather than payload dispatch.
- 2026-07-04 quickbar item-refresh action-outcome slice: live-data gate used
  the gameplay-reaching live HG proxy harness
  `C:\nwnbridge\codex-live-quickbar-setbutton-driver-20260704-003119\harness-proxy-20260704-003123`
  (proxy log last write `2026-07-04T00:36:53+10:00`, about 50 minutes old at
  gate time). Gameplay was reached and no quarantine artifact directory was
  present. Proxy2 now classifies the pending quickbar item-refresh client
  response as `awaiting_client_action`, `first_client_action_target_unknown`,
  `first_client_action_targets_other_object`,
  `candidate_client_action_no_server_quickbar`, or
  `candidate_client_action_observed_server_quickbar`, and writes that outcome
  into semantic traces plus `quickbar-item-refresh-hint.json`. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-action-outcome-20260704-0138` against the
  current Diamond HG gameplay capture verified 164 packet files, 304 strict
  allows, 0 strict quarantines, 0 quarantine files, and a pending
  `awaiting_client_action` hint for feature-25-only candidate `0x80015DAA`.
  The latest live SetButton probe would classify as
  `candidate_client_action_no_server_quickbar`: the client targeted the
  candidate, but no server quickbar followed. Next production path: use this
  outcome in the next live probe, then implement or correct the generalized
  active-property item radial/action timing rule instead of adding another
  one-off payload guess.
- 2026-07-04 quickbar client-action timing slice: live-data gate used the same
  gameplay-reaching live HG proxy harness
  `C:\nwnbridge\codex-live-quickbar-setbutton-driver-20260704-003119\harness-proxy-20260704-003123`
  (proxy log last write `2026-07-04T00:36:53+10:00`, about 1h50m old at gate
  time), with no quarantine artifacts. Proxy2 now classifies the first
  item-refresh client action as `awaiting_client_action`,
  `immediate_after_proof`, or `delayed_after_pending_followup`, and records
  `followup_events_before_first_client_action` in semantic traces plus
  `quickbar-item-refresh-hint.json`; the replay harness exports both summary
  fields. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-action-timing-20260704-023643` and parser
  check `C:\nwnbridge\codex-proxy2-replay-action-timing-summary-20260704-024005`
  stayed at 164 packet files, 304 strict allows, 0 strict quarantines, and 0
  quarantine files. The Diamond replay remains `awaiting_client_action` for
  feature-25-only candidate `0x80015DAA`. Next production path: run the live
  SetButton/UseItem driver probes with these timing fields, then compare the
  first client action's delay against Diamond/EE active-property item action
  semantics before changing the translator action rule.
- 2026-07-03 ClientQuickbar SetButton hint slice: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-useitem-self-target-hint-20260703-223120\harness-proxy-20260703-223124`
  (log last write `2026-07-03T22:34:14.7839098+10:00`, gameplay reached, about
  0.8h old at the gate). Because the latest live UseItem probe still recorded
  0 quickbar events after the matched self-targeted UseItem, proxy2 now builds
  an exact `GuiQuickbar_SetButton` item payload from the decompile-backed
  client quickbar parser, records the first blank/item committed quickbar slot,
  and writes a `recommended_client_quickbar_set_button_*` action alongside the
  existing UseItem hint. Reference setup: the fresh live HG capture establishes
  the behavior gap, and strict Diamond-capture replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-setbutton-hint-20260703-233507`
  verified 164 packet files, 304 strict allows, 0 quarantines, and hint payload
  `701E02120000000701AA5D0180FFFFFFFF0060` for candidate `0x80015DAA` using
  slot 7 from `first_blank_committed_slot`. Next production path: teach the EE
  driver/harness to dispatch this SetButton payload, then run a fresh live HG
  probe and compare post-action quickbar/server traffic against the current
  UseItem-only result.
- 2026-07-03 self-target UseItem hint slice: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-post-useitem-response-counters-20260703-2145\harness-proxy-20260703-213130`
  (log last write `2026-07-03T21:33:37+10:00`, gameplay reached, under 24h).
  Proxy2 now emits the recommended quickbar item-refresh `Input_UseItem` with a
  target-present EE self sentinel (`0xFFFFFFFD`) so the existing client-input
  rewrite maps it to Diamond's legacy invalid/self target (`0x7F000000`).
  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-useitem-self-target-hint-20260703-222818`
  stayed at 164 packet files, 304 strict allows, 0 quarantines, and wrote
  candidate `0x80015DAA` with payload
  `70060910000000AA5D018000FDFFFFFFC8`. Fresh live probe
  `C:\nwnbridge\codex-live-useitem-self-target-hint-20260703-223120\harness-proxy-20260703-223124`
  reached gameplay, committed the 36-slot/18-item quickbar profile, dispatched
  a matched candidate `0x80016691`, and proxy2 validated/rewrite-claimed
  `Input_UseItem` (`700609100000009166018000FDFFFFFFC8`) with
  `rewritten_self_object_id=true`. The final hint still recorded 0 quickbar
  events after 151 post-action events (52 live-object, 1 inventory, 1 chat, 97
  other). Next production path: compare original Diamond/EE active-property
  item quickbar/radial action semantics and timing, including whether HG needs
  a quickbar set-button action or a different target/object branch instead of
  another `Input_UseItem` shape tweak.
- 2026-07-03 post-UseItem response-counter slice: live-data gate used the
  gameplay-reaching proxy harness
  `C:\nwnbridge\codex-live-quickbar-useitem-driverhook-20260703-202458\harness-proxy-20260703-202501`
  (log last write `2026-07-03T20:27:30+10:00`, gameplay reached, under 24h).
  Proxy2 now tracks pending quickbar item-refresh traffic after the first
  client action separately from the whole post-proof window, and the
  `quickbar-item-refresh-hint.json` plus replay summaries expose
  `events_after_first_client_action`, `first_event_after_client_action`, and
  after-action family buckets. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-post-useitem-response-counters-20260703-2132`
  stayed at 164 packet files, 304 strict allows, and 0 quarantines. Fresh live
  probe
  `C:\nwnbridge\codex-live-post-useitem-response-counters-20260703-2145\harness-proxy-20260703-213130`
  reached gameplay, dispatched a matched `Input_UseItem` for candidate
  `0x800164E0`, and ended with 97 verified events after that client action:
  32 live-object, 1 inventory, 1 chat, 63 other, and 0 server/client quickbar.
  Next production path: compare Diamond/EE quickbar or radial use semantics for
  active-property items and adjust the harness/proxy action shape or timing
  before trying another item-refresh trigger.
- 2026-07-03 driver-only quickbar UseItem hook slice: live-data gate reused
  gameplay-reaching Diamond HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  (164 packets, newest gameplay packet about 5h01m old at gate time). The
  proxy-side pending hint from the previous live run was ready, but the C++
  bridge only called `TryDispatchQuickbarItemRefreshUseItem` from the
  non-driver server-message hook. The driver-only server-message hook now polls
  the same hint path. Fresh live probe
  `C:\nwnbridge\codex-live-quickbar-useitem-driverhook-20260703-202458\harness-proxy-20260703-202501`
  reached gameplay, committed the 36-slot/18-item quickbar profile, dispatched
  `Input_UseItem` once for candidate `0x800162A4` at
  `2026-07-03 20:26:21 +10`, and proxy2 validated/forwarded that
  `ClientInput` payload (`7006090C000000A462018000C0`). The hint recorded
  `first_client_action="client_input_use_item"` and
  `first_client_action_matches_candidate=true`. Remaining issue: no server
  quickbar refresh followed that action in the observed window
  (`quickbar_events_since_pending_refresh=0`), so the next production path is
  HG response/state parity after UseItem: confirm whether the active-property
  item expects different target flags/timing or a different original-client
  quickbar action.
- 2026-07-03 declared quickbar split ordering slice: live-data gate used
  gameplay-reaching Diamond HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516\diamond-client-packets`
  (packet window
  `2026-07-03T15:16:25.8610376+10:00 -> 2026-07-03T15:19:28.1192675+10:00`,
  164 packets). Fresh live proxy probe
  `C:\nwnbridge\harness-proxy-20260703-191931` reached gameplay but still
  ended with `stream_probe_quickbar_item_candidates_without_committed_profile`;
  the root cause was the focused quickbar stream splitter mixing normal
  CNW-declared candidate endpoints with zero-declared legacy-prefix fallback
  endpoints. Proxy2 now tries declared endpoints first and uses the legacy
  prefix scan only as fallback, with a regression proving the live-shaped
  `GuiQuickbar_SetAllButtons` owns the full `old_declared=1321`,
  `read_size=1314`, `fragment_size=19` payload before a following status
  packet. Strict replay
  `C:\nwnbridge\codex-replay-declared-first-20260703-1933` stayed at 0
  quarantines and produced a pending UseItem hint. Fresh live probe
  `C:\nwnbridge\harness-proxy-20260703-193410` reached gameplay, committed the
  36-slot quickbar profile with 18 item buttons and produced a stable pending
  hint for `0x8001612E` (`active_object`, direct-only), but the EE driver did
  not emit the recommended `Input_UseItem` during the wait window. Next
  production path: fix or instrument the driver-side hint consumption/send
  path now that proxy2 emits the live pending hint.
- 2026-07-02 live-data gate refresh: stale prior gameplay capture forced a new
  HG Diamond run
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`, packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  219 packet files, gameplay reached. Strict replay first exposed sequence 179
  as a mixed inventory Feature-25/placeable add/terminal all-bits `U/09`
  placeable update stream with six leftover legacy packed-name fragment bits.
  Production tail9 rewriting now removes exactly those six bits only when the
  terminal bounded name payload is proven; confirming replay
  `C:\nwnbridge\codex-proxy2-replay-fresh-live-fixed-20260702-153427` stayed at
  0 quarantines, 414 strict allows, 27 exact live-object rewrites, and 147
  exact lifecycle claim summaries. Next production path: use this fresh capture
  for item-bearing quickbar materialization pressure or the next live-object
  exact-shape gap while it remains fresh.
- 2026-07-02 follow-up Feature-25 materialization-state slice: live-data gate
  reused the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`, still fresh at
  run time. Semantic item-proof state now records materialized vs deferred
  Feature-25 refs before inserting the Feature-25 proof itself. Confirming
  replay
  `C:\nwnbridge\codex-proxy2-replay-feature25-materialization-state-automation-20260702-1605`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  and 147 exact lifecycle claims. The existing generic live-object trace still
  counted 17 first-list refs and 1 second-list ref as materialized, but the new
  item-specific semantic trace counted all 17 first-list and 21 second-list
  refs as deferred item refs. Next production path: capture or replay an
  item-bearing `SetAllButtons` stream and decide whether compact item emission
  may rely on deferred Feature-25 refs, rather than generic owner
  materialization.
- 2026-07-02 quickbar registry-context slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`, packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  still fresh at about 2h15m old, and gameplay reached. Proxy2 now summarizes
  typed inventory item proof context from `ObjectRegistry` and logs it beside
  registry-backed `GuiQuickbar_SetAllButtons` materialization rewrites; the
  replay harness exports committed/probe registry-context counters. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-registry-context-automation-20260702-171938`
  stayed at 0 quarantines, 414 strict allows, 27 live-object exact rewrites,
  147 lifecycle claims, 39 quickbar probes, 1 committed quickbar summary, and
  1 registry-context summary. The committed quickbar still had 0 item buttons,
  and the registry context at rewrite time had 0 active/materialized/Feature-25
  item refs, so this capture still cannot decide deferred Feature-25 item-slot
  emission. Next production path: obtain an item-bearing `SetAllButtons` with
  non-empty registry context, or drive a local/live harness action that
  produces one.
- 2026-07-02 quickbar split-probe registry-context slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`, with the newest
  gameplay packet still under 24 hours old. Split-time `GuiQuickbar` probes now
  receive the same registry-backed materialization context as committed
  rewrites, and the context trace is emitted by the quickbar facade for both
  committed and stream-probe roles. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-split-context-automation-20260702-1816`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe quickbar summaries, 39 stream-probe
  registry-context summaries, and 1 committed quickbar summary. Both committed
  and probe paths still saw 0 item buttons and 0 active/materialized/Feature-25
  registry item refs, so the next production path remains driving or capturing
  an item-bearing `SetAllButtons` stream with non-empty registry item context.
- 2026-07-02 quickbar proof-summary slice: live-data gate reused the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; newest packet was
  about 4 hours old at gate time. The semantic inventory-item context now
  reports unique direct item-proof objects, unique Feature-25 item-proof
  objects, and their compact item-emission proof union through the quickbar
  registry-context trace and replay summary. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-proof-summary-automation-20260702-191159`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed registry-context summary. Both committed and stream-probe
  `compact_item_emission_proof_objects` counters were 0, matching the 0 quickbar
  item buttons in this capture. Next production path: drive or capture an
  item-bearing `SetAllButtons` stream where these proof-union counters are
  nonzero before widening compact item-slot emission.
- 2026-07-02 quickbar UI-state context slice: live-data gate reused the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  newest packet about 5 hours old at replay time, and gameplay reached.
  Committed `GuiQuickbar` semantic events now snapshot the registry item-proof
  context into `UiState` beside the exact slot profile, while placeholder
  frames leave both committed snapshots intact. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-ui-context-automation-20260702-2007`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed registry-context/profile summary. The semantic committed profile
  line recorded 36 slots, 29 blanks, 5 spells, 2 general buttons, 0 item slots,
  and 0 compact item-emission proof objects. Next production path: capture or
  drive an item-bearing `SetAllButtons` where the committed UI snapshot has
  nonzero compact item proof.
- 2026-07-02 quickbar proof-partition slice: live-data gate reused the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  newest packet about 6 hours old at replay time, and gameplay reached.
  `ObjectRegistry` now partitions compact quickbar item-proof context into
  direct-only, Feature-25-only, and shared direct+Feature-25 object counts,
  while keeping the existing compact item emission policy unchanged. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-proof-partition-automation-20260702-2119`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed registry-context summary. This capture still has 0 quickbar item
  buttons and 0 direct-only/Feature-25-only/shared compact proof objects at
  quickbar probe/rewrite time. Next production path: drive or capture an
  item-bearing `SetAllButtons` and use the partition counters to decide whether
  Feature-25-only proof is sufficient for compact item-slot emission.
- 2026-07-02 quickbar prior-context slice: live-data gate reused the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; newest packet was
  about 7 hours old at replay time. The semantic reducer now retains the last
  proof-bearing or cleared inventory item context before a committed
  `GuiQuickbar_SetAllButtons`, and the replay summary exports those prior
  context fields. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-prior-context-automation-20260702-2218`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe quickbar summaries, and 1 committed
  quickbar summary. This capture's committed quickbar still had 0 item buttons
  and `QuickbarSemanticPriorItemContextKnown=0`; later live-object processing
  retained deferred Feature-25 item context after the quickbar, reaching 5
  compact proof objects. Next production path: capture or drive a later
  item-bearing `SetAllButtons` after those retained Feature-25 refs, then use
  the prior/current context fields to decide compact item emission policy.
- 2026-07-02 quickbar post-context slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`, with the newest
  gameplay packet about 8 hours old. Semantic UI state now opens a fresh
  post-context window after each committed `GuiQuickbar_SetAllButtons`, then
  retains and traces later verified inventory item context separately from the
  prior/current quickbar snapshots. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-post-context-automation-20260702-2319`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has 0 quickbar item buttons
  and 0 prior/current quickbar proof objects, but the new post-context summary
  proves 37 updates after the committed quickbar, reaching 5 compact item
  emission proof objects, all Feature-25-only. Next production path: capture
  or drive a later item-bearing `SetAllButtons` after these post-quickbar
  Feature-25 refs, then decide whether Feature-25-only proof is sufficient for
  compact item-slot emission.
- 2026-07-03 quickbar previous-post-context slice: live-data gate reused the
  same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  newest packet about 9 hours old at gate time, and gameplay reached.
  Committed `GuiQuickbar_SetAllButtons` semantic state now snapshots the
  previous post-quickbar item-context window before resetting it for the newly
  committed profile, and the replay summary exports those previous-post
  counters. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-previous-post-context-automation-20260703-0018`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has only one committed
  quickbar, so previous-post counters remain 0 while the current post-context
  still reaches 5 compact item-emission proof objects, all Feature-25-only.
  Next production path: capture or drive a later item-bearing
  `GuiQuickbar_SetAllButtons` after post-quickbar Feature-25 refs, then use
  prior/current/previous-post/post context fields to decide compact item-slot
  emission policy.
- 2026-07-03 quickbar proof-class slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
  `2026-07-03T01:09:21+10:00`, the newest gameplay packet was about 10 hours
  old and gameplay had been reached. The quickbar writer now treats
  `ExplicitSelfMaterialization` as valid only for explicit type-1 item bodies;
  compact byte-owned item slots must receive registry state proof from active
  item objects, GUI item-create, or inventory Feature-25 refs. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-proof-class-automation-20260703-0113`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe quickbar summaries, and 1 committed
  quickbar summary. This capture still has 0 item buttons; post-quickbar
  context still reaches 5 compact item-emission proof objects, all
  Feature-25-only. Next production path: capture or drive a later item-bearing
  `GuiQuickbar_SetAllButtons` after those Feature-25 refs and verify compact
  item emission consumes only registry-state proof classes.
- 2026-07-03 quickbar best-context slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  newest gameplay packet about 11 hours old at gate time, and gameplay reached.
  Committed `GuiQuickbar_SetAllButtons` semantic state now snapshots the best
  available item-proof context at commit time: current registry evidence first,
  then previous post-quickbar window, then older prior context, with current
  cleared-state evidence overriding stale proof. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-best-context-automation-20260703-0218`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has a single committed
  quickbar before item proof, so `QuickbarSemanticBestItemContextKnown=0`;
  later post-context still reaches 5 compact proof objects, all Feature-25-only.
  Next production path: capture or drive a later item-bearing committed
  quickbar and compare accepted/rejected item decisions with the new
  best-context source and counters.
- 2026-07-03 quickbar pending-refresh slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  newest gameplay packet about 12 hours old, and gameplay reached. Semantic
  quickbar state now marks compact item proof that arrives after a committed
  `GuiQuickbar_SetAllButtons` as pending a later item-bearing refresh, resets
  that pending window on the next committed quickbar, and cancels it if a later
  cleared item context supersedes the proof. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-pending-refresh-automation-20260703-031344`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has no item-bearing quickbar,
  so `QuickbarSemanticPendingItemRefreshBeforeCommit=0`, but it now reports
  `QuickbarSemanticPostItemRefreshPending=37` and
  `QuickbarSemanticPostItemRefreshPendingUpdates=37` while the post-context
  proof window reaches 5 compact item-emission proof objects, all
  Feature-25-only. Next production path: capture or drive a later item-bearing
  committed quickbar after this pending window, then compare accepted/rejected
  item decisions with current/best/pending context counters.
- 2026-07-03 quickbar pending-refresh outcome slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; newest gameplay
  packet was about 13 hours old at gate time and gameplay reached. Semantic
  committed-quickbar state now records whether a pending post-quickbar item
  refresh had no pending window, arrived but stayed blank, or emitted item
  slots. The replay summary exports those outcomes. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-refresh-outcome-automation-20260703-0418`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has only the first
  no-pending committed quickbar
  (`QuickbarSemanticPendingItemRefreshOutcomeNoPending=1`) before the
  post-quickbar Feature-25 window; no later blank or item-slot outcome is
  present. Post-context still reaches 37 updates and 5 compact item-emission
  proof objects, all Feature-25-only. Next production path: capture or drive a
  later committed `GuiQuickbar_SetAllButtons` after the pending Feature-25-only
  proof window, then use the outcome counter to distinguish an emitted item
  refresh from a later still-blank profile.
- 2026-07-03 quickbar pending proof-class slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`,
  newest gameplay packet about 14 hours old at gate time, and gameplay reached.
  Semantic quickbar state now snapshots the pending post-quickbar compact item
  proof class as `direct_only`, `feature25_only`, `shared`, or `mixed`, and the
  replay summary exports both committed and post-context proof-class counters.
  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-pending-proof-class-automation-20260703-051647`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has one no-pending committed
  quickbar (`pending_item_refresh_proof_class="none"`); all 37 later pending
  post-context updates are `feature25_only`, with 5 compact item-emission proof
  objects and 0 direct/shared proof objects. Next production path: capture or
  drive a later committed `GuiQuickbar_SetAllButtons` after this
  Feature-25-only pending window, then use the proof-class and outcome counters
  to decide whether Feature-25-only proof can safely emit compact item slots.
- 2026-07-03 quickbar pending event-window slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
  `2026-07-03T06:12:14.9858139+10:00` the newest gameplay packet was about
  15 hours old and gameplay reached. Semantic quickbar state now counts
  verified events that pass while a post-committed-quickbar item refresh is
  pending, snapshots that count into the next committed quickbar, and exposes
  an unresolved pending-refresh summary for graceful session shutdown. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-unresolved-refresh-automation-20260703-062111`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has one no-pending committed
  quickbar and no later item-bearing refresh; after the Feature-25-only
  post-context window, `QuickbarSemanticPostItemRefreshPendingEvents=265`,
  proving substantial later verified traffic without a second committed
  quickbar. Next production path: drive or capture a later committed
  `GuiQuickbar_SetAllButtons` after the Feature-25-only pending window, or
  instrument the harness/client action needed to provoke that refresh.
- 2026-07-03 quickbar pending event-breakdown slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`, packet window
  `2026-07-02T15:05:09.9590892+10:00 -> 2026-07-02T15:09:59.0156462+10:00`;
  at `2026-07-03T07:13:01+10:00` the newest gameplay packet was about 16
  hours old and gameplay reached. Semantic quickbar state now buckets pending
  post-committed item-refresh traffic by verified family, and the replay
  summary exports those buckets for committed, post-context, and unresolved
  pending windows. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-event-breakdown-automation-20260703-071923`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. This capture still has no later committed
  quickbar or item buttons; the pending Feature-25-only post-context window
  spans 265 verified events: 127 live-object, 0 quickbar, 0 area, 0 inventory,
  1 client input, 4 chat, and 133 other. It still reaches 5 compact
  item-emission proof objects, all Feature-25-only. Next production path:
  drive or instrument a client/harness action that should provoke a committed
  `GuiQuickbar_SetAllButtons` refresh after this pending window, then compare
  the accepted/rejected item decision counters against these event buckets.
- 2026-07-03 quickbar client-action bucket slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
  `2026-07-03T08:13:49+10:00`, the newest gameplay packet was about 17 hours
  old and gameplay reached. Semantic pending-refresh state now carries exact
  client action claims from the decompile-backed `ClientInput` and
  `ClientQuickbar` parsers, splitting client input into UseItem, UseObject,
  ChangeDoorState, and other, and splitting client quickbar SetButton into
  item vs other. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-client-action-buckets-automation-20260703-0813`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. The pending Feature-25-only post-context window
  still spans 265 verified events and 5 compact item-emission proof objects,
  but the single client input in that window is now proven to be
  `Input_WalkToWaypoint` (`client_input_other=1`), with 0 UseItem, 0
  ClientQuickbar SetButton, and 0 item SetButton events. Two client
  `GuiQuickbar_SetButton` actions were observed before the pending window, so
  the next useful harness work is specifically to provoke UseItem or
  item-bearing client quickbar SetButton after the pending Feature-25-only item
  proof window, then look for the later committed server `SetAllButtons`.
- 2026-07-03 quickbar first-trigger slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
  `2026-07-03T09:13:21+10:00`, the newest gameplay packet was about 18 hours
  old and gameplay reached. Semantic pending-refresh state now records the
  first follow-up event after the proof-opening row and the first client action
  after the pending window opens, while preserving the existing aggregate event
  counters. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-first-trigger-automation-20260703-0929`
  stayed at 0 quarantines. This capture still has 0 quickbar item buttons and
  no post-proof UseItem or item-bearing client `GuiQuickbar_SetButton`; the
  post-context first-follow-up evidence is mostly live-object traffic
  (`first_followup_live_object=21`), and the only first client actions were
  generic input (`first_client_action_other_input=2`). Next production path:
  adjust harness/client control to deliberately perform UseItem or item-bearing
  quickbar SetButton after Feature-25-only proof opens, then use the new
  first-trigger counters to decide whether HG emits a later committed
  `SetAllButtons`.
- 2026-07-03 quickbar first-action detail slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
  `2026-07-03T10:16:02+10:00`, the newest gameplay packet was about 19h06m old
  and gameplay reached. Client `GuiQuickbar_SetButton` claims now expose the
  item object id and optional target object id for decompile-backed type-1 item
  bodies, and semantic pending-refresh state snapshots the first post-proof
  client action's object id, quickbar slot, button type, and body kind. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-action-detail-automation-20260703-1038`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, 39 stream-probe registry-context summaries, and 1
  committed quickbar summary. The pending Feature-25-only post-context window
  still has 0 UseItem and 0 item SetButton actions; the new detail fields prove
  the only post-proof client action details were generic input with object id
  `2147497163`, slot/button/body-kind all zero/none. Next production path:
  update harness/client control to deliberately perform post-proof UseItem or
  item-bearing client quickbar SetButton, then check whether HG emits a later
  committed item-bearing `GuiQuickbar_SetAllButtons`.
- 2026-07-03 quickbar compact-candidate slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
  `2026-07-03T11:14:36+10:00`, the newest gameplay packet was about 20h04m old
  and gameplay reached. Semantic item context now exposes a deterministic
  compact item-emission candidate object id, source, and proof from the same
  direct/Feature-25 proof sets used by the quickbar policy, and pending/post
  quickbar traces plus the replay summary export those fields. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-candidate-automation-20260703-112533`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, and 1 committed quickbar summary. The replay reported
  37 post-context candidate observations, candidate object id `2147574964`,
  all Feature-25-only proof, with 34 first-list and 3 second-list proof labels.
  It still has 0 post-proof UseItem/item SetButton actions and 0 committed
  quickbar item buttons. Next production path: use this candidate signal to
  drive a post-proof UseItem or item-bearing client quickbar SetButton in the
  harness, then verify whether HG emits a later committed item-bearing
  `GuiQuickbar_SetAllButtons`.
- 2026-07-03 quickbar client-action candidate-match slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at the gate, the
  newest gameplay packet was about 21h05m old and gameplay reached. Pending
  refresh client-action diagnostics now compare the first post-proof client
  action object id against the deterministic compact item-emission candidate.
  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-action-candidate-match-automation-20260703-122155`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, and 1 committed quickbar summary. This capture still
  has 37 post-quickbar pending updates and 5 compact item-emission proof
  objects for candidate `2147574964`, but the first recorded post-proof client
  actions were generic input against object `2147497163`; all 4 candidate-known
  action samples therefore had `matches_candidate=false`. Next production path:
  drive UseItem or an item-bearing client quickbar SetButton specifically
  against candidate `2147574964` after the Feature-25-only post-proof window
  opens, then check whether HG emits a later item-bearing committed
  `GuiQuickbar_SetAllButtons`.
- 2026-07-03 quickbar harness-hint slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at the gate, the
  newest gameplay packet was about 22h05m old and gameplay reached. Proxy2 now
  accepts `--quickbar-item-refresh-hint` and writes a polling JSON hint from
  verified semantic state whenever a pending post-quickbar item refresh has a
  compact item-emission candidate. The replay and live HG harness scripts wire
  the hint path into proxy2, and replay summaries export the parsed hint fields.
  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-hint-automation-20260703-132844`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, and 1 committed quickbar summary. The hint file was
  emitted with `pending_item_refresh=true`, final candidate `2147574946`
  (`feature25_second_list`, Feature-25-only), while earlier hint updates mostly
  tracked candidate `2147574964`. Next production path: make the live harness
  consume this hint after the post-proof window opens and drive UseItem or an
  item-bearing client quickbar SetButton against the current hinted candidate.
- 2026-07-03 quickbar UseItem payload hint slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260702-1504`; at
  `2026-07-03T14:15:19+10:00`, the newest gameplay packet was about 23h05m old
  and gameplay reached. Proxy2 now builds a decompile-backed minimal
  `Input_UseItem` payload for the current hinted compact item-emission
  candidate, using the case `0x09` reader order `OBJECTID, BYTE,
  BOOL+optional BYTE, BOOL+optional OBJECTID, BOOL+optional position` and exact
  CNW final-fragment bit packing. The quickbar item-refresh hint JSON exposes
  `recommended_use_item_payload_hex` and the associated item id/branch fields.
  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-useitem-payload-hint-automation-20260703-1420`
  stayed at 0 quarantines, 414 strict allows, 27 exact live-object rewrites,
  147 lifecycle claims, and 1 committed quickbar summary. The emitted hint was
  pending for candidate `2147574946` (`0x800164A2`,
  `feature25_second_list`, Feature-25-only) with payload
  `7006090C000000A264018000C0`. Next production path: make the live harness or
  proxy-side client-action driver consume `recommended_use_item_payload_hex`
  after the post-proof window opens, or drive the equivalent item
  `GuiQuickbar_SetButton`, then verify whether HG emits a later item-bearing
  committed `GuiQuickbar_SetAllButtons`.
- 2026-07-03 quickbar UseItem driver slice: live-data gate refreshed Diamond HG
  evidence with
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516`; 164 packet files,
  window `2026-07-03T15:16:25.8610376+10:00 ->
  2026-07-03T15:19:28.1192675+10:00`, gameplay reached through tempclient
  BIC/PRE_PLAYMOD and repeated live-object traffic. The bridge DLL now has an
  opt-in driver-only poller for `quickbar-item-refresh-hint.json`; when
  `pending_item_refresh` and `recommended_use_item_payload_available` are true
  it validates the full `70 06 09` `Input_UseItem` payload shape and sends the
  decompile-backed payload once through `CNWMessage::SendPlayerToServerMessage`.
  `tools\test-hg-bridge.ps1` exposes this as
  `-AutoQuickbarItemRefreshUseItem` and now skips stale proxy2 binaries that do
  not support `--quickbar-item-refresh-hint`. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-useitem-driver-20260703-1530`
  stayed at 0 quarantines and emitted a pending hint for candidate `0x80015DAA`
  with payload `7006090C000000AA5D018000C0`. Bounded live proxy probe
  `C:\nwnbridge\codex-live-quickbar-useitem-driver-20260703-1535\harness-proxy-20260703-153052`
  reached gameplay, but the live proxy path did not write the pending hint and
  no UseItem dispatch fired; stream-probe quickbar logs showed compact-source
  item candidates (`item_buttons_seen=1`) without committed preservation.
  Next production path: make the live harness/proxy summary classify why the
  live path has item candidates but no pending hint, then drive the UseItem
  action only when the pending hint is actually emitted.
- 2026-07-03 quickbar idle-hint classification slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516`; at
  `2026-07-03T16:15:49+10:00`, the newest gameplay packet was about 56 minutes
  old and gameplay reached. Proxy2 now writes the quickbar item-refresh hint
  file even when no actionable pending hint exists, with a structured
  `no_hint_reason` and committed/post-context counters; the replay summary
  exports that reason, and the bridge driver includes it in its skip reason
  when `pending_item_refresh=false`. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-idle-hint-automation-20260703-1626`
  stayed at 0 quarantines, 304 strict allows, and emitted the expected pending
  hint for candidate `0x80015DAA` (`feature25_second_list`, Feature-25-only)
  with payload `7006090C000000AA5D018000C0`. Next production path: rerun the
  live auto-UseItem probe and use the new `no_hint_reason` file/log field to
  distinguish missing committed quickbar state, missing post-commit item proof,
  and pending proof without a driveable candidate.
- 2026-07-03 quickbar stream-probe idle classification slice: live-data gate
  reused `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516`; at
  `2026-07-03T17:17:04+10:00`, the newest gameplay packet was about 1h58m old
  and gameplay reached. Live auto-UseItem probe
  `C:\nwnbridge\codex-live-quickbar-idle-hint-rerun-20260703-1718\harness-proxy-20260703-171923`
  reached gameplay but wrote an idle hint with
  `no_hint_reason=no_committed_quickbar_profile`; proxy logs showed repeated
  stream-probe `GuiQuickbar_SetAllButtons` candidates with compact item buttons
  (`item_buttons_seen=3`, `item_buttons_source_compact=3`,
  `item_buttons_rejected_missing_state_proof=3`) and no committed quickbar
  profile. Proxy2 now records stream-probe quickbar summaries into semantic UI
  state and exposes the sharper idle reason
  `stream_probe_quickbar_item_candidates_without_committed_profile`, plus
  stream-probe item-button/proof counters, without changing quickbar emission
  policy. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-stream-probe-hint-automation-20260703-1740`
  stayed at 0 quarantines, 304 strict allows, 39 stream-probe quickbar
  summaries, and the expected pending hint for candidate `0x80015DAA`.
  Post-code live probe
  `C:\nwnbridge\codex-live-quickbar-stream-probe-hint-20260703-1745\harness-proxy-20260703-173957`
  reached gameplay and wrote
  `no_hint_reason=stream_probe_quickbar_item_candidates_without_committed_profile`
  with one stream-probe summary, 36 owned slots, and 18 preserved explicit item
  buttons but no committed quickbar profile. Next production path: determine
  why live stream-probe quickbar units never become a committed profile, then
  either repair the splitter/stream commitment rule or drive the client action
  only after a committed profile exists.
- 2026-07-03 quickbar stream-flush semantic commit slice: live-data gate reused
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260703-1516`; at
  `2026-07-03T18:27:44+10:00`, the newest gameplay packet was about 3h09m old
  and gameplay reached. The buffered quickbar stream flush now observes the
  verified `GuiQuickbar_SetAllButtons` payload through the normal semantic UI
  observer after the rewritten frames are built, so streamed committed quickbar
  payloads update `last_committed_quickbar_profile` and refresh the
  item-refresh hint state. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-stream-commit-observe-20260703-184037`
  stayed at 0 quarantines, 304 strict allows, one committed quickbar semantic
  profile, 39 stream-probe summaries, and a pending hint for candidate
  `0x80015DAA` (`feature25_second_list`, Feature-25-only) with payload
  `7006090C000000AA5D018000C0`. Next production path: rerun the live
  auto-UseItem probe with this commit; if live still reports no committed
  quickbar profile, inspect why the stream flush is not reaching the verified
  commit observer, otherwise drive the emitted UseItem payload and watch for a
  later item-bearing committed `GuiQuickbar_SetAllButtons`.
- The recurring automation/project workspace must use the populated checkout at
  `D:\Codex Projects\NWN EE Bridge`. Future runs must start there and fail
  visibly if `Cargo.toml`, `.git`, or `proxy2` are missing.
- Recent `A/09` fixed-width custom-carrier work contains real implementation,
  including source-carried pre-add custom carrier synthesis, but too many
  consecutive runs ended with packet bytes unchanged and "next replay" notes.
  Do not add another carrier counter-only slice until a fresh replay or harness
  blocker is recorded.
- Manual Diamond live-capture review:
  `C:\nwnbridge\codex-review-diamond-client-20260625-174949` built the probe,
  launched Diamond against HG server `213` (`158.69.144.21:5133`), accepted
  `BNVR A`, sent startup/login `M...` frames, received vault/module-list data,
  and wrote 24 packet dumps. The unattended run stalled before gameplay because
  the auto-character path tried PRE_PLAYMOD selection while the entry list was
  still empty (`entries=0 count=0`). Treat this as the next harness production
  target if gameplay replay cannot be obtained manually.
- Immediate automation target: while the 2026-07-03 live HG capture remains
  fresh, rerun the live auto-UseItem probe with the quickbar stream-flush
  semantic commit fix. If live still reports
  `stream_probe_quickbar_item_candidates_without_committed_profile`, inspect
  the stream flush/verified commit observer path. Once a pending hint is
  emitted, use its `recommended_use_item_payload_hex` for a post-proof UseItem,
  or drive an item-bearing client quickbar SetButton, then check whether HG
  emits a later item-bearing committed `GuiQuickbar_SetAllButtons`.
- 2026-06-28 live-data gate satisfied by
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260628-000537`: probe window
  `2026-06-28 00:05:38.124 -> 00:07:37.612`, 119 packet files, gameplay reached
  through module/resource load, area load, and repeated live-object/status
  traffic. Follow-up production slice moved `P/04/01 Area_ClientArea` ownership
  into the focused gameplay-stream splitter, using the existing exact EE
  LoadArea proof/legacy rewrite as the owner; replay
  `C:\nwnbridge\codex-proxy2-replay-area-stream-automation-20260628-190900`
  stayed at 0 quarantines and 214 strict allows. Next concrete path: inspect
  residual gameplay pressure from `P/05` live-object/add/update or the next
  high-frequency focused family in the same fresh capture, rather than adding
  more carrier counters.
- 2026-06-28 follow-up replay
  `C:\nwnbridge\codex-proxy2-replay-baseline-automation-1950-20260628-195131`
  against the same fresh gameplay capture stayed at 0 quarantines, 214 strict
  allows, and no live-object/carrier residuals. Current replay pressure is still
  `Journal_Updated` plus `P/05` live-object, both already focused, so the
  production slice hardened a replay-adjacent strict gap: `P/17/03
  Sound_Object_Stop` now has focused gameplay-stream ownership and rejects
  shifted fragment tails before following high-level packets. This capture does
  not contain Sound packets; next live capture with Sound traffic should confirm
  the focused owner in replay.
- 2026-06-28 follow-up module-resource stream hardening: the same fresh
  gameplay capture exercises the early `P/01/03 ServerStatus_ModuleResources`
  transition through Module_Info/resource-load replay. `P/01/03` now has
  focused gameplay-stream ownership through the existing decompile-backed
  `CNWCModule::LoadModuleResources` parser/writer, so shifted NWSync BOOL
  fragment tails cannot split before a following high-level status packet.
- 2026-06-28 follow-up status stream hardening: the same fresh gameplay replay
  repeatedly uses `P/01/01 ServerStatus_Status` as the boundary after module,
  loadbar, and synthetic area-load transitions. `P/01/01` now routes through
  the focused `server_status` owner before generic fixed-length splitting, so
  unowned slack after the empty status envelope keeps the stream pending
  instead of splitting a false boundary. Next production path: continue moving
  replay-exercised fixed/generic high-level families into exact focused stream
  owners, or return to `P/05`/`A/09` only when fresh replay shows residual
  carrier or live-object pressure again.
- 2026-06-28 follow-up login stream hardening: the same fresh gameplay replay
  exercises `P/02/05 Login_Confirm`, `P/02/0C Login_GetWaypoint`, and client
  `P/02/11 Login_ServerSubDirectoryCharacter`. `P/02` login/client-login rows
  now route through the existing decompile-backed focused owners before generic
  splitting, so no-body login signals and declared login rows reject unowned
  tail slack before a following high-level packet. Replay
  `C:\nwnbridge\codex-proxy2-replay-login-stream-automation-20260628-225950`
  stayed at 0 quarantines, 214 strict allows, and 0 fixed-width/live-object
  residuals. Next production path: continue focused stream ownership for any
  remaining replay-exercised generic boundary families, or return to
  `P/05`/`A/09` only if fresh replay pressure reappears.
- 2026-06-29 follow-up PlayModuleCharacterList stream hardening: at the live
  gate the latest gameplay-reaching HG capture was still
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260628-000537`, last packet
  `2026-06-28T00:07:37.5989693+10:00`, about 23h47m old, with gameplay reached.
  The same replay exercises client `P/31/01` start and `P/31/02` stop controls.
  `P/31/01..03` now route through the existing decompile-backed
  PlayModuleCharacterList focused owner before generic fixed/declared
  splitting, so no-body client controls and response fragment cursors reject
  unowned tail slack before following high-level packets. Replay
  `C:\nwnbridge\codex-proxy2-replay-play-module-stream-automation-20260629-0009`
  stayed at 0 quarantines, 214 strict allows, and 0 fixed-width/live-object
  residuals. This capture is now over 24h old after the run crossed midnight
  local time; the next automation run should refresh live HG gameplay evidence
  before ordinary proxy work.
- 2026-06-29 live-data gate refresh: stale prior gameplay capture forced a new
  HG Diamond capture. First run
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260629-005550` reached `BNVR A`
  and one `P/01/03` response but did not reach gameplay because the driver
  fell back from the server-list DirectConnect path to native DirectConnect and
  never sent the client `P/11/01` character-list startup packet. Production
  harness fix now reuses the remembered `SERVERLIST_PANEL` when Diamond's
  global app-state slot is empty, instead of treating the server-list path as
  absent. Confirming capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143` produced 151
  packet dumps, window `2026-06-29 01:01:44.364 -> 01:04:43.744 +10:00`, and
  reached gameplay with module/resource load, area/gameplay traffic, and 67
  live-object frames. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-fresh-live-20260629-20260629-010523` stayed
  at 0 quarantines, 280 strict allows, 67 captured live-object frames, 18 exact
  live-object shapes, and 0 fixed-width/live-object residuals. Next production
  path remains a replay-exercised packet-family stream-owner slice only when a
  fresh capture shows residual pressure; current fresh evidence has no
  quarantine/residual blocker.
- 2026-06-29 follow-up client stream-owner hardening: live-data gate used the
  same gameplay-reaching capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`, still fresh at
  run start. The replay exercises client `P/01/00 ServerStatus_0`,
  `P/03/02 Module_Loaded`, `P/04/03 Area_AreaLoaded`, and
  `P/1E/02 GuiQuickbar_SetButton`. Those rows now route through their existing
  focused client validators in the gameplay-stream splitter before generic
  fixed/declared splitting, so no-body client controls and quickbar fragment
  cursors reject unowned slack before following high-level client packets.
  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-stream-automation-20260629-020541`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: continue migrating replay-exercised generic boundaries into
  focused owners, or switch back to `P/05`/`A/09` only when fresh replay shows
  live-object or carrier residual pressure.
- 2026-06-29 follow-up focused server-family stream hardening: live-data gate
  used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`, still fresh
  at run start with last packet `2026-06-29 01:04:43 +10:00` (about 2 hours
  old). `P/03/0E Module_EndGame`, `P/04/06 Area_ChangeDayNight`, `P/10/*
  Camera`, `P/22/01 SafeProjectile`, `P/28/01..08 Ambient`, `P/30/01
  GuiTimingEvent`, and `P/33/* Cutscene` now route through their existing
  decompile-backed focused validators in the gameplay-stream splitter before
  generic declared splitting. Shifted fragment tails remain pending instead of
  forming false boundaries. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-focused-server-stream-automation-20260629-030715`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: add focused stream owners for the remaining dispatch-owned
  families that still split generically, or return to `P/05`/`A/09` only when
  fresh live replay shows residual pressure.
- 2026-06-29 follow-up Dialog/Area_VisualEffect stream hardening: live-data
  gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet dump
  timestamp `2026-06-29 01:04:43 +10:00`, about 3h05m old at verification
  time, with gameplay reached. `P/04/02 Area_VisualEffect` and
  `P/14/01..05 Dialog` now route through their existing decompile-backed
  focused validators in the gameplay-stream splitter before generic declared
  splitting. The new regressions cover the legacy Area_VisualEffect identity
  transform insertion path, direct-string Dialog_Entry fragment bits, client
  Dialog_Reply, Dialog_Close slack rejection, and shifted-tail rejection.
  Strict replay
  `C:\nwnbridge\codex-proxy2-replay-dialog-area-vfx-stream-automation-20260629-0417`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: continue moving remaining server-dispatch/client-dispatch
  exact validators into focused stream ownership, then switch back to
  `P/05`/`A/09` only if fresh live replay shows residual pressure.
- 2026-06-29 follow-up client-high stream hardening: live-data gate used the
  same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 4h03m old at verification time, with
  gameplay reached. Client `P/06` Input, `P/0D` GuiInventory, `P/15/01`
  GuiCharacterSheet, and consumed EE-only `P/35/01` GuiEvent now route through
  their existing decompile-backed focused validators in the gameplay-stream
  splitter before generic declared splitting. Shifted client fragment tails
  remain pending instead of splitting before a plausible following high-level
  status packet. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-client-high-stream-automation-20260629-0500`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: continue focused ownership for any remaining generic
  client/server families only if replay or local harness traffic exercises
  them; otherwise return to live-object/area static state work with fresh
  capture evidence.
- 2026-06-29 follow-up placeable alias lifecycle hardening: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 4h55m old at gate time, with gameplay
  reached. The semantic object registry now applies the existing
  compact/external placeable alias owner rule to active lifecycle lookup, so a
  compact `U/09`/`D/09` row can see the active external `A/09` owner before
  missing-object cleanup and delete handling. Focused regression
  `active_placeable_lifecycle_lookup_uses_compact_external_aliases` passed, and
  strict replay
  `C:\nwnbridge\codex-proxy2-replay-placeable-alias-lifecycle-automation-20260629-0600`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: continue live-object/area static state work, prioritizing
  any alias/lifecycle path that can strand compact and external ids as separate
  owners.
- 2026-06-29 follow-up untyped placeable-owner lifecycle hardening: live-data
  gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 5h55m old at gate time, with gameplay
  reached. Untyped active-object lifecycle lookup now shares the
  compact/external placeable alias owner rule used by typed `U/09`/`D/09`
  checks, so inventory-owner rows with object type `0` can see an active
  external placeable owner before missing-object cleanup. Focused regression
  `untyped_lifecycle_lookup_uses_placeable_compact_external_aliases` passed,
  and strict replay
  `C:\nwnbridge\codex-proxy2-replay-untyped-placeable-owner-automation-20260629-0703`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: continue live-object/area static state work, especially
  typed model/writer paths where area static context can prove generalized
  placeable state or appearance repair.
- 2026-06-29 follow-up untyped area/static conflict lookup hardening:
  live-data gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 6h55m old at gate time, with gameplay
  reached. Untyped live-object owner rows now share the compact/external
  placeable alias rule used by typed `U/09`/`D/09` rows when looking up
  unresolved area/static placeable conflicts, so inventory-owner diagnostics
  and repair-enabling summaries can see the external `A/09` owner instead of
  dropping conflict context. Focused regressions confirm untyped compact
  placeable ids resolve the conflict owner while compact creature ids and
  typed non-placeable rows do not. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-untyped-conflict-owner-automation-20260629-0805`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: continue area-static/live-object repair work where the
  conflict summaries can choose generalized placeable appearance/state repairs,
  or refresh live HG evidence first once this capture is older than 24 hours.
- 2026-06-29 follow-up delete lifecycle hardening: live-data gate used the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 7h55m old at gate time, with gameplay
  reached. `D/09` live-object deletion now clears object lifecycle facts
  (name, position, orientation, bounds, placeable appearance/state, and
  area/static conflict context) before an object id can be reused, preventing
  stale state from feeding later repair or diagnostic paths when a compact
  add omits those fields. Focused regression
  `delete_clears_lifecycle_fields_before_object_id_reuse` passed, and strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-clear-delete-lifecycle-automation-20260629-0900`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: continue area-static/live-object repair work where the
  current conflict summaries can choose generalized placeable appearance/state
  repairs, or refresh live HG evidence first once this capture is older than 24
  hours.
- 2026-06-29 follow-up record-level placeable appearance repair: live-data gate
  used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 9h05m old at gate time, with gameplay
  reached. The exact `U/09` door/placeable update reader proves the appearance
  WORD cursor before scale/state bits, so record-level area/static reconciliation
  now rewrites normal-to-normal placeable appearance words from a unique
  module-backed static row without moving bytes or fragment bits. Custom
  `>=0xFFFE` appearance/resref width changes still stay with the parent exact
  validating rewrite path and are intentionally skipped in the record-local
  helper. Focused regressions
  `exact_placeable_update_reconciliation_uses_verified_appearance_word_cursor`
  and
  `exact_placeable_update_reconciliation_skips_custom_appearance_width_changes`
  passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-normal-placeable-appearance-automation-20260629-1011`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact live-object shapes, and 0 fixed-width/live-object residuals. Next
  production path: extend the record-local path only after decompile-backed
  proof and exact validation for custom/resref insert/remove widths, or refresh
  live HG evidence first once this capture is older than 24 hours.
- 2026-06-29 follow-up custom/resref placeable appearance validation:
  live-data gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 9h56m old at gate time, with gameplay
  reached. The exact `U/09` door/placeable update parser now retains raw
  vector-orientation and scale/state fields, and the custom/resref appearance
  width-changing validator requires every non-appearance field plus state/next
  bit cursors to survive insert/remove rewrites. This hardens the existing
  module-backed custom TemplateResRef path against byte-plausible shifted
  scale/state tails. Focused regressions
  `exact_placeable_update_custom_resref_insert_preserves_scale_state_tail`
  and
  `exact_placeable_update_custom_resref_validator_rejects_shifted_scale_state_tail`
  passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-custom-placeable-scale-automation-20260629-1100`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact live-object/placeable state work where custom
  appearance or area/static repairs can shift following read-buffer fields,
  refreshing HG live evidence once this capture exceeds 24 hours.
- 2026-06-29 parser-owned `U/09` appearance branch model:
  live-data gate used the gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 10h56m old at gate time, with gameplay
  reached. The exact door/placeable update parser now carries the appearance
  WORD plus optional 16-byte CResRef as a typed verified field, so downstream
  exact reconciliation and record-local appearance repair no longer re-read
  that branch ad hoc after the parser has already proven EE/Diamond cursor
  order. Focused regressions
  `door_placeable_update_parser_retains_custom_appearance_resref` and
  `exact_placeable_update_reconciliation*` passed, `cargo check -p
  hgbridge-proxy2` passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-appearance-parser-model-automation-20260629-121115`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: use the parser-owned appearance branch to continue
  decompile-backed exact live-object/placeable state or custom appearance work,
  refreshing HG live evidence once this capture exceeds 24 hours.
- 2026-06-29 parser-owned `U/09` state branch model: live-data gate used the
  same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 11h57m old at gate time, with gameplay
  reached. The exact door/placeable update parser now carries the five legacy
  state bits plus EE's neutral state suffix as a typed verified field, so
  exact reconciliation and diagnostics consume parser-proven lock/visual bits
  instead of re-reading them from an ad hoc cursor after other branches have
  been validated. The custom/resref appearance validator also requires that
  parser-owned state branch to survive width-changing rewrites. Focused
  regressions
  `door_placeable_update_parser_retains_verified_state_branch`,
  `door_placeable_update_parser_retains_custom_appearance_resref`,
  `exact_placeable_update_reconciliation_uses_verified_vector_state_cursor`,
  and `exact_placeable_update_mentions_use_verified_vector_state_cursor`
  passed, `cargo check -p hgbridge-proxy2` passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-parser-owned-state-automation-20260629-135120`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: use the parser-owned appearance/state model to finish
  bounded exact placeable state writer/repair work, refreshing HG live evidence
  once this capture exceeds 24 hours.
- 2026-06-29 staged exact `U/09` state writer validation: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; last packet
  `2026-06-29 01:04:43 +10:00`, about 13 hours old at gate time, with gameplay
  reached. Exact placeable update state repair now stages lockable/locked bit
  edits, re-runs the EE door/placeable parser, and commits only if the same
  parser-owned state cursor, next cursor, legacy visual bits, and EE neutral
  suffix survive. The whole-payload exact appearance pass also refreshes the
  parser claim after same-row position/orientation/state edits, so its
  non-appearance preservation check compares against the current proven row
  instead of a stale pre-rewrite claim. Focused regression
  `exact_placeable_update_state_rewrite_requires_fresh_exact_claim` passed,
  the `exact_placeable_update_` subset passed, `cargo check -p
  hgbridge-proxy2` passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-staged-state-automation-20260629-1420`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: keep tightening exact placeable update repair validation for
  multi-field same-row edits, refreshing HG live evidence once this capture
  exceeds 24 hours.
- 2026-06-29 exact `U/09` scale/state cursor validation: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 14 hours old at gate
  time, with gameplay reached. The exact door/placeable update parser now
  carries the scale/state read offset, and custom/resref appearance validation
  requires that cursor to move by exactly the inserted or removed 16-byte
  CResRef width while preserving scale, generic state, fragment state, and next
  cursors. Focused parser/custom-resref tests passed, the `exact_placeable_update_`
  subset passed, `cargo check -p hgbridge-proxy2` passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-scale-state-cursor-automation-20260629-151334`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact placeable update multi-field validation and
  writer repair, refreshing HG live evidence once this capture exceeds 24 hours.
- 2026-06-29 exact `U/09` pre-appearance cursor validation: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 15 hours old at gate
  time, with gameplay reached. The custom/resref appearance validator now also
  requires the whole record end to shift by exactly the inserted or removed
  CResRef width, while position and scalar/vector orientation read offsets
  before the appearance branch remain fixed. This extends the existing
  decompile-backed order proof for Diamond `sub_467AE0` and EE
  `sub_14079C050`: position, orientation, appearance/resref, scale/state, then
  fragment state. Focused regressions
  `exact_placeable_update_appearance_validator_requires_record_end_shift` and
  `exact_placeable_update_appearance_validator_rejects_shifted_pre_appearance_offsets`
  passed, the `exact_placeable_update_` subset passed, `cargo check -p
  hgbridge-proxy2` passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-appearance-offset-validator-automation-20260629-1604`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact placeable update multi-field writer/validator
  repair, or refresh HG live evidence first once this capture exceeds 24 hours.
- 2026-06-29 exact `U/09` rewritten-field postcondition: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 16 hours old at gate
  time, with gameplay reached. Exact placeable update reconciliation now
  re-parses each rewritten row after same-row edits, including custom/resref
  appearance width changes, and requires parser-owned rewritten position,
  appearance/resref, scalar/vector orientation, and lock state fields to match
  the selected module-backed area/static row before counting the row as
  repaired. Focused regression
  `exact_placeable_update_field_postcondition_rejects_state_drift_after_custom_insert`
  passed, the `exact_placeable_update_` subset passed, `cargo check -p
  hgbridge-proxy2` passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-placeable-field-postcondition-automation-20260629-171317`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact placeable update writer repair using this
  final-row proof, or refresh HG live evidence first once this capture exceeds
  24 hours.
- 2026-06-29 exact `U/09` final-row scale/state preservation: live-data gate
  used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 17 hours old at gate
  time, with gameplay reached. Exact placeable update reconciliation now checks
  the final parser-owned row against the pre-appearance exact claim after
  position/orientation/state edits, so custom/resref appearance width changes
  can only move the scale/generic-state cursor by the inserted or removed
  CResRef width while preserving the parser-owned values and fragment cursors.
  Focused regressions
  `exact_placeable_update_field_postcondition_rejects_scale_state_drift_after_custom_insert`
  and `exact_placeable_update_field_postcondition_rejects_state_drift_after_custom_insert`
  passed, the `exact_placeable_update_` subset passed, `cargo check -p
  hgbridge-proxy2` passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-final-placeable-scale-postcondition-automation-20260629-181353`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact placeable update writer repair, or refresh HG
  live evidence first once this capture exceeds 24 hours.
- 2026-06-29 exact `U/09` appearance-branch byte-delta validation: live-data
  gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 18 hours old at gate
  time, with gameplay reached. Custom/resref appearance validation now requires
  the parser-owned appearance branch itself to widen or shrink by the claimed
  16-byte CResRef delta before accepting shifted later scale/state cursors.
  This prevents a byte-plausible final `U/09` row from relying only on record
  end and later-cursor alignment while the appearance branch remains
  impossible. Focused regressions
  `exact_placeable_update_field_postcondition_requires_appearance_branch_delta`,
  `exact_placeable_update_field_postcondition_rejects_scale_state_drift_after_custom_insert`,
  and `exact_placeable_update_appearance_validator*` passed, the
  `exact_placeable_update_` subset passed, `cargo check -p hgbridge-proxy2`
  passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-appearance-branch-delta-automation-20260629-1908`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact placeable update writer repair if more
  bounded cursor invariants remain; otherwise refresh HG live evidence first
  once this capture exceeds 24 hours.
- 2026-06-29 exact `U/09` update-state postcondition model: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 19h04m old at gate
  time, with gameplay reached. The final exact placeable update row
  postcondition now routes parser-owned state through the typed
  update-state-to-area-state model instead of duplicating lock-bit comparison in
  the parent validator. This keeps `U/09` state rows scoped to the decompiled
  lockable/locked bits, keeps `A/09` as the owner for use/trap state, and still
  rejects an impossible non-neutral EE state suffix before a row can count as
  repaired. Focused regression
  `verified_update_state_area_match_uses_observed_lock_state` passed, the
  `exact_placeable_update_` subset passed, `cargo check -p hgbridge-proxy2`
  passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-update-state-observed-postcondition-automation-20260629-2019`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact placeable update writer/postcondition repair
  only while this live capture remains fresh; otherwise refresh HG live evidence
  first.
- 2026-06-29 exact `U/09` sibling-state postcondition hardening: live-data gate
  used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 20h04m old at gate
  time, with gameplay reached. The final selected-row postcondition now also
  checks parser-owned `U/09` state bits that are present beside another repaired
  field when the module-backed area/static row has state, so an appearance-only
  repair cannot commit while preserving drifted lock bits or an impossible EE
  state suffix in the same exact row. Regression
  `exact_placeable_update_field_postcondition_rejects_state_drift_after_custom_insert`
  now covers the appearance-only sibling-state case, the
  `exact_placeable_update_` subset passed, `cargo check -p hgbridge-proxy2`
  passed, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-sibling-state-postcondition-automation-20260629-2119`
  stayed at 0 quarantines, 280 strict allows, 67 captured live-object frames,
  18 exact shape matches, and 0 fixed-width/live-object residuals. Next
  production path: continue exact placeable update writer/postcondition repair
  only if fresh evidence remains under 24 hours; otherwise refresh HG live
  gameplay evidence first.
- 2026-06-29 exact `U/09` sibling visual-field postcondition hardening:
  live-data gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 21h07m old at gate
  time, with gameplay reached. The final selected-row postcondition now checks
  present sibling position, orientation, and appearance branches whenever any
  exact placeable update field is repaired and the module-backed static row has
  a concrete target for that sibling. Rewritten fields still must match, while
  packet-authored siblings remain allowed when the static row lacks the
  required proof, such as missing TemplateResRef for a custom appearance or no
  area direction. Regression
  `exact_placeable_update_field_postcondition_rejects_present_visual_sibling_drift`
  covers position, orientation, and appearance drift in syntactically exact
  rows. Verification passed `cargo fmt --all --check`,
  `cargo test -q -p hgbridge-proxy2 exact_placeable_update_ -- --nocapture`,
  `cargo check -q -p hgbridge-proxy2`, `git diff --check`, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-sibling-visual-postcondition-automation-20260629-2208`
  with 0 quarantines, 280 strict allows, 67 captured live-object frames, 18
  exact shape matches, and 0 fixed-width/live-object residuals. Next production
  path: refresh HG live gameplay evidence first on the next run if this capture
  has exceeded 24 hours; otherwise continue exact placeable update
  writer/postcondition repair.
- 2026-06-29 exact `U/09` orientation-branch sibling proof hardening:
  live-data gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 22h05m old at gate
  time, with gameplay reached. The final selected-row postcondition now treats
  a present orientation sibling as requiring proof whenever the selected
  module-backed static row has any decompile-backed orientation target. A
  repaired exact row can no longer commit just because its scalar/vector branch
  differs from the only target the static row can prove. Regression
  `exact_placeable_update_field_postcondition_rejects_unproved_orientation_branch_sibling`
  covers a vector sibling beside an appearance repair when the selected row
  only proves scalar yaw. Verification passed `cargo fmt --all --check`,
  `cargo test -q -p hgbridge-proxy2 exact_placeable_update_ -- --nocapture`,
  `cargo check -q -p hgbridge-proxy2`, `git diff --check`, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-orientation-sibling-postcondition-automation-20260629-231933`
  with 0 quarantines, 280 strict allows, 67 captured live-object frames, 18
  exact shape matches, and 0 fixed-width/live-object residuals. Next production
  path: refresh HG live gameplay evidence first on the next run if this capture
  has exceeded 24 hours; otherwise continue exact placeable update
  writer/postcondition repair.
- 2026-06-30 exact `U/09` appearance-delta ownership hardening: live-data gate
  used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-serverlist-retry-20260629-010143`; packet window
  `2026-06-29 01:01:54.950 -> 01:04:43.126 +10:00`, about 23h05m old at gate
  time, with gameplay reached. The final exact-row postcondition now rejects a
  nonzero CResRef byte-shift unless the parser-owned appearance branch is the
  rewritten field, so later scale/state cursors cannot be accepted under an
  inconsistent same-row rewrite cause. Regression
  `exact_placeable_update_field_postcondition_requires_appearance_branch_delta`
  covers the byte-shifted tail without an appearance rewrite. Verification
  passed `cargo fmt --all --check`, the focused regression,
  `cargo test -q -p hgbridge-proxy2 exact_placeable_update_ -- --nocapture`,
  `cargo check -q -p hgbridge-proxy2`, `git diff --check`, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-final-appearance-delta-automation-20260630-0019`
  with 0 quarantines, 280 strict allows, 67 captured live-object frames, 18
  exact shape matches, and 0 fixed-width/live-object residuals. Next run should
  refresh live HG gameplay evidence first if this capture has exceeded 24 hours.
- 2026-06-30 `P/11/04 CharList_UpdateCharResponse` empty-cursor padding fix:
  live-data gate used gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 10h10m old at gate
  time, with gameplay reached. Replays of this fresh capture exposed two early
  quarantines on deflated `P/11/04` update responses whose byte-only BIC body
  ended with empty CNW cursor high bits but preserved low padding bits (`0x68`).
  The translator now treats a single trailing `0b011xxxxx` byte as the empty
  `GetWriteMessage` cursor and still rejects missing tails, non-empty cursor
  classes, and extra fragment bytes. Verification passed `cargo fmt --all
  --check`, `cargo test -q -p hgbridge-proxy2 char_list -- --nocapture`,
  `cargo check -q -p hgbridge-proxy2`, and strict replay
  `C:\nwnbridge\codex-proxy2-replay-charlist-update-tail-full-automation-20260630-1650`
  with 0 quarantines, 3,547 strict allows, 2,781 captured direct live-object
  frames, 445 exact shape matches, 10 area rewrites, and 0 fixed-width or
  live-object terminal residuals. Next production path: continue reducing the
  remaining live-object/area exact-shape gaps with this fresh capture as the
  regression seed until the next live-data refresh is due.
- 2026-06-30 live-object exact-claim coverage diagnostic: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 12h14m old at gate
  time, with gameplay reached. Production `proxy2` now logs compact typed
  `GameObjUpdate_LiveObject` exact-claim summaries after lifecycle proof, so
  replay can distinguish already-exact accepted payloads from rewrite-produced
  exact shapes. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-exact-claim-summary-automation-20260630-1844`
  used alternate local ports because an older debug proxy still held the
  default port, and completed with 0 quarantines, 3,547 strict allows, 445
  exact rewrite matches, 3,226 exact lifecycle claim summaries, 2,781 captured
  direct live-object frames, 10 area rewrites, and 0 fixed-width or live-object
  terminal residuals. Aggregated exact claims covered 5,106 typed records,
  including 463 adds, 21 update records, 438 deletes, 875 inventory records,
  443 creature appearances, 2,861 creature update records, 5 world-status
  records, 15 placeable-appearance mentions, and 30 placeable-state mentions.
  Next production path: use these exact-claim counters to target the still
  rewrite-dependent placeable add/update clusters and inventory/creature update
  families, rather than broadening validators without a typed counter.
- 2026-06-30 live-object exact-claim type classifier: live-data gate used the
  same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 13h12m old at gate
  time, with gameplay reached. Production `proxy2` exact-claim traces now
  classify object type, opcode/type, orientation source, and placeable
  appearance/state shape; the replay harness aggregates those fields into
  `replay-summary.json`. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-exact-claim-types-automation-20260630-193913`
  completed with 0 quarantines, 3,547 strict allows, 445 exact rewrite
  matches, 3,226 exact lifecycle claim summaries, 2,781 captured direct
  live-object frames, 10 area rewrites, and 0 fixed-width or live-object
  terminal residuals. Aggregated exact claims showed 4,186 creature mentions,
  2,862 creature update mentions, 2,840 creature position mentions, 0 creature
  orientation mentions, 30 placeable mentions (15 add/15 update, all scalar
  orientation and normal appearance), 10 door mentions, and 875 untyped
  inventory-owner mentions. Next production path: reduce the dominant creature
  update/position exact rows and untyped inventory-owner rows into bounded
  parser/validator invariants before widening any lower-volume placeable
  repair.
- 2026-06-30 parser-owned `U/05` creature update claim model: live-data gate
  used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 14h51m old at
  verification time, with gameplay reached. Exact live-object mentions now
  carry a `U/05` creature-update claim only after the existing decompile-backed
  final cursor walk succeeds and its bit cursor matches the accepted record.
  The claim records the raw update mask plus parser-owned position and
  scalar/vector orientation selector cursors, separating creature orientation
  proof from the door/placeable orientation registry field. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-creature-update-claim-automation-20260630-204326`
  completed with 0 quarantines, 3,547 strict allows, 445 exact rewrite
  matches, 3,226 exact lifecycle claim summaries, 2,781 captured direct
  live-object frames, 10 area rewrites, and 0 fixed-width or live-object
  terminal residuals. Aggregated exact claims showed 2,861 creature update
  claim mentions, including 2,840 parser-owned position claims, 2,840 scalar
  orientation selector claims, and 0 vector selector claims. Next production
  path: reduce the remaining 875 untyped inventory-owner rows into a bounded
  owner/opcode parser invariant, then use the typed creature claim mask counts
  to choose any remaining `U/05` branch-specific model work.
- 2026-06-30 parser-owned inventory owner claim model: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 15h13m old at
  gate time, with gameplay reached. Exact `I` inventory owner mentions now
  expose owner id, mask, fragment-bit count, and accepted bit-cursor bounds
  only after the decompile-backed inventory validator replays the record and
  lands on the same next cursor. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-owner-claim-automation-20260630-2225`
  completed with 0 quarantines, 3,547 strict allows, 445 exact rewrite
  matches, 3,226 exact lifecycle claim summaries, 2,781 captured direct
  live-object frames, 10 area rewrites, and 0 fixed-width or live-object
  terminal residuals. All 875 formerly untyped inventory-owner mentions now
  carry exact owner claims; the aggregate showed 1 `0xD5FF` mask and 874
  other masks, all with external-owner ids and no compact/sentinel owners.
  Next production path: break down the 874 non-`0xD5FF` inventory masks into
  bounded branch counters, then choose the next exact writer/parser slice from
  those mask families or the remaining `U/05` update-mask counts.
- 2026-06-30 inventory owner branch counters: live-data gate used the same
  gameplay-reaching HG capture, about 17h07m old at verification time. Exact
  `I` owner claims now carry a typed mask-branch set only after the inventory
  validator replays the accepted cursor, and replay aggregation reports each
  decompile-owned mask branch. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-inventory-branch-counters-automation-20260630-2311`
  completed with 0 quarantines, 3,547 strict allows, 445 exact rewrite
  matches, 3,226 exact lifecycle claim summaries, 2,781 captured direct
  live-object frames, 10 area rewrites, and 0 fixed-width/live-object terminal
  residuals. The 875 inventory-owner claims still had 1 `0xD5FF` mask and 874
  other masks; branch counters showed 870 Feature-25 `0x2000` mentions, with
  only low single-digit mentions for every other inventory branch. Next
  production path: reduce the dominant `0x2000` Feature-25 owner rows into a
  typed list-count/object-reference claim so the following writer/parser work
  can target the actual inventory pressure.
- 2026-06-30 typed inventory Feature-25 claims: live-data gate used the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 17h14m old at
  gate time, with gameplay reached. Exact `I` owner claims now expose the
  decompile-backed `0x2000` Feature-25 branch as first-list and second-list
  OBJECTID vectors, byte cursors, and the second-list fragment-bit span only
  after the inventory mask walker has replayed the accepted branch order and
  landed on the same next cursor. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-feature25-claim-automation-20260630-2344`
  completed with 0 quarantines, 3,547 strict allows, 445 exact rewrite
  matches, 3,226 exact lifecycle claim summaries, 2,781 captured direct
  live-object frames, 10 area rewrites, and 0 fixed-width/live-object terminal
  residuals. All 870 `0x2000` inventory-owner branch mentions now have typed
  Feature-25 claims: 437 first-list object refs, 442 second-list object refs,
  1,326 second-list BOOL bits, and 0 legacy-tail object refs in this live
  replay. Next production path: classify these Feature-25 object refs by
  owner/mask and materialization state, then implement the bounded inventory
  state/writer rule that consumes or validates the referenced objects instead
  of leaving them as diagnostics.
- 2026-07-01 inventory Feature-25 materialization classification: live-data
  gate used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 18h13m old at gate
  time, with gameplay reached. Exact live-object trace summaries now classify
  inventory Feature-25 claims by owner kind, specific mask, owner
  materialization, and first/second/legacy-tail object-reference
  materialization using the same semantic object-registry lifecycle lookup as
  live-object exact claim validation. The replay harness now exposes
  `-DrainReceiveTimeoutMilliseconds` so long fresh captures do not spend the
  run on empty UDP receive waits; the first two replay attempts proved the
  previous default could exceed automation limits. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-feature25-materialization-automation-20260701-0114`
  completed all 3,294 packets with 0 quarantines, 3,547 strict allows, 445
  exact rewrites, 3,226 exact lifecycle claim summaries, 2,781 direct
  live-object frames, 10 area rewrites, and 0 fixed-width/live-object terminal
  residuals. All 870 Feature-25 claims had external and materialized owners;
  869 were exact mask `0x2000` and 1 was another mask. First-list refs were
  fully materialized (437/437), while second-list refs were almost entirely
  not yet materialized at reference time (1/442 materialized, 441/442
  unmaterialized), with 0 legacy-tail refs. Next production path: implement a
  bounded inventory state/writer rule for second-list Feature-25 refs that
  treats those refs as deferred/reference-only inventory links rather than
  requiring immediate object materialization, with decompile-backed proof for
  the three BOOLs per second-list object.
- 2026-07-01 inventory Feature-25 deferred state rule: live-data gate used the
  same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 19h14m old at gate
  time, with gameplay reached. Exact `I/0x2000` Feature-25 claims now feed a
  typed semantic event and object-registry cache for first-list, second-list,
  and legacy-tail inventory item refs. The cache is deliberately separate from
  active object/materialized-item lifecycle proof, so the 441 deferred
  second-list refs in the fresh HG replay can support later item/quickbar
  context without making missing-object cleanup accept unmaterialized live
  updates. Quickbar materialization gateways now ask for known inventory item
  context instead of only active object ids. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-feature25-deferred-state-automation-20260701-0154`
  completed all 3,294 packets with 0 quarantines, 3,547 strict allows, 445
  exact rewrites, 3,226 exact lifecycle claim summaries, 2,781 direct
  live-object frames, 10 area rewrites, and 0 fixed-width/live-object terminal
  residuals. The replay preserved the prior evidence: 870 Feature-25 claims,
  437/437 materialized first-list refs, 1/442 materialized second-list refs,
  441/442 deferred second-list refs, 1,326 second-list BOOL bits, and 0
  legacy-tail refs. Next production path: use the new deferred inventory item
  context to narrow quickbar/inventory item writer decisions, while keeping
  compact or recovered quickbar item bodies blanked until their own
  decompile-backed EE materialization proof exists.
- 2026-07-01 state-backed compact quickbar item emission: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 20h17m old at gate
  time, with gameplay reached. The quickbar writer now distinguishes explicit
  type-1 item bodies, compact byte-owned bodies with a source type, and
  missing-source-type recovered bodies. Explicit type-1 bodies keep the EE
  `sub_14079DB00` self-materialization allowance; compact byte-owned bodies
  require registry proof from verified live-object, GUI item-create, or
  Feature-25 inventory refs before emission; missing-source-type recovery stays
  boundary-only and blanks. Focused tests prove unproven compact byte-owned
  items still blank, while state-proven compact byte-owned item ids emit typed
  EE quickbar item slots with exact validation. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-compact-state-automation-20260701-0245`
  completed all 3,294 packets with 0 quarantines, 3,547 strict allows, 445
  exact live-object rewrites, 3,226 exact lifecycle claim summaries, 2,781
  direct live-object frames, 10 area rewrites, and 0 fixed-width/live-object
  terminal residuals. Feature-25 evidence stayed stable: 870 claims, 437/437
  first-list refs materialized, 1/442 second-list refs materialized, 441/442
  deferred second-list refs, 1,326 second-list BOOL bits, and 0 legacy-tail
  refs. Next production path: use a live or replayed quickbar stream that
  follows verified Feature-25 refs to confirm whether real HG compact
  byte-owned item slots now stay visible, then continue bounded inventory/GUI
  item writer work from any remaining blanked state-proven slots.
- 2026-07-01 quickbar materialization provenance diagnostics: live-data gate
  used the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 21h17m old at gate
  time, with gameplay reached. The quickbar writer now reports the proof source
  for every emitted item object: explicit EE self-materialization, active
  registry state, Feature-25 first-list refs, Feature-25 second-list refs, or
  legacy-tail refs. The registry exposes those Feature-25 refs as distinct
  proof kinds, with active object state taking precedence over deferred
  references. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-proof-provenance-automation-20260701-0345`
  completed all 3,294 packets with 0 quarantines, 3,547 strict allows, 445
  exact live-object rewrites, 3,226 exact lifecycle claim summaries, 2,781
  direct live-object frames, 10 area rewrites, and 0 fixed-width/live-object
  terminal residuals. Feature-25 evidence stayed stable: 870 claims, 437/437
  first-list refs materialized, 1/442 second-list refs materialized, 441/442
  deferred second-list refs, 1,326 second-list BOOL bits, and 0 legacy-tail
  refs. This capture's quickbar traffic emitted no item objects, so every new
  quickbar item materialization provenance counter stayed at 0. Next production
  path: capture or replay a quickbar SetAllButtons stream that actually carries
  item slots after verified Feature-25 refs, then use the provenance counters to
  decide whether compact item emission should accept active-only, first-list,
  or second-list proof.
- 2026-07-01 quickbar item rejection diagnostics: live-data gate used the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 22h16m old at gate
  time, with gameplay reached. The quickbar writer now reports why parsed item
  slots are not emitted, separating explicit/compact/recovered source buckets
  from recovered-type, missing-source-type, no-present-item, invalid-object-id,
  missing-active-property, unsupported-appearance, appearance-shape, and
  missing-state-proof rejects. This keeps the decompile-backed policy strict
  while making the next item-slot capture actionable. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-reject-diagnostics-automation-20260701-043828`
  completed all 3,294 packets with 0 quarantines, 3,547 strict allows, 445
  exact live-object rewrites, 3,226 exact lifecycle claim summaries, 2,781
  direct live-object frames, 10 area rewrites, and 0 terminal residuals. The
  replay saw 40 quickbar rewrite summaries but still 0 item buttons, so the
  next production path remains obtaining a live or replayed `SetAllButtons`
  stream that carries item slots after verified Feature-25 refs; then use these
  buckets to decide the bounded compact-item proof rule.
- 2026-07-01 item-specific quickbar active proof: live-data gate used the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260630-041346`; packet window
  `2026-06-30 04:13:58.302 -> 06:13:42.862 +10:00`, about 23h16m old at gate
  time, with gameplay reached. The quickbar materialization gateway now treats
  active registry state as item proof only when the state is a materialized GUI
  item id or an active typed `0x06` item live-object. Broader lifecycle checks
  for inventory owners still use active creature/placeable ids and placeable
  aliases, but those ids can no longer satisfy quickbar item emission by
  accident. Feature-25 first/second/legacy-tail refs remain distinct deferred
  item-context proofs. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-item-proof-specific-automation-20260701-0558`
  completed all 3,294 packets with 0 quarantines, 3,547 strict allows, 445
  exact live-object rewrites, 3,226 exact lifecycle claim summaries, 2,781
  direct live-object frames, 10 area rewrites, 40 quickbar rewrite summaries,
  and 0 quickbar item buttons. Next production path is still to obtain a live
  or replayed `SetAllButtons` stream with real item slots after verified
  Feature-25 refs, then use the existing source/proof/rejection buckets to
  choose any bounded compact-item rule.
- 2026-07-01 item-delete quickbar proof lifecycle: live-data gate found the
  previous gameplay-reaching HG capture stale by about 18 minutes, so a fresh
  live HG Diamond capture was run:
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, 165 packet files, with
  gameplay reached through character/module entry and in-area HG messages. The
  fresh strict replay
  `C:\nwnbridge\codex-proxy2-replay-item-delete-proof-lifecycle-automation-20260701-0644`
  stayed at 0 quarantines, 217 strict allows, 79 direct live-object frames, 13
  exact live-object rewrites, 54 exact lifecycle claim summaries, 10 area
  rewrites, 40 quickbar rewrite summaries, 0 quickbar item buttons, and 0
  fixed-width/live-object residuals. Production slice: exact `D/06`
  live-object deletes now clear GUI-materialized and Feature-25 deferred
  quickbar item proof for the deleted object id, while leaving unrelated
  Feature-25 item refs intact. Focused regressions prove both materialized GUI
  item proof and second-list Feature-25 proof are removed on delete. Next
  production path remains obtaining or replaying a `SetAllButtons` stream with
  real item slots after verified Feature-25 refs; the stale-proof lifecycle
  guard should stay in place before accepting compact item-slot emission.
- 2026-07-01 quickbar blank-slot profile diagnostics: live-data gate used the
  same fresh gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, with gameplay reached.
  Production quickbar summaries now count true type-0 blank slots separately
  from item, spell, general, unsupported, and rejected item buckets, and the
  complete-slot stream gate treats type-0 slots as decompile-owned one-byte
  `SetAllButtons` records when a fragment-tail proof exists. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-slot-profile-automation-20260701-0747`
  stayed at 0 quarantines, 256 strict allows, 79 direct live-object frames, 16
  exact live-object rewrite matches, 77 exact lifecycle claim summaries, 10
  area rewrites, and 40 quickbar rewrite summaries. The current HG quickbar
  stream had 0 item buttons, 492 blank slots, 192 spells, 80 preserved general
  buttons, 0 general blanks, and 676 unsupported blanks across those 40
  summaries. Inventory Feature-25 evidence in the replay showed 18 typed
  claims, 9/9 first-list refs materialized, 1/13 second-list refs materialized,
  12/13 second-list refs unmaterialized, and 0 legacy-tail refs. Next
  production path remains obtaining or replaying a `SetAllButtons` stream with
  real item slots after verified Feature-25 refs; this slot profile proves the
  current fresh HG capture cannot decide compact item-slot emission.
- 2026-07-01 committed quickbar summary diagnostics: live-data gate used the
  same fresh gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about two hours old at
  verification time, with gameplay reached. The focused gameplay-stream
  splitter now calls explicit quickbar stream-probe rewrite helpers, and
  quickbar rewrite logs carry `trace_role` plus `committed` fields. The replay
  harness now aggregates only `committed=true` quickbar summaries, so
  speculative normalized split probes no longer inflate slot-preservation or
  blanking counters. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-committed-summary-automation-20260701-0849`
  stayed at 0 quarantines, 223 strict allows, 79 direct live-object frames, 14
  exact live-object rewrites, 60 exact lifecycle claim summaries, 10 area
  rewrites, and 1 committed quickbar rewrite summary. The actual committed
  SetAllButtons output had 0 item buttons, 29 blank slots, 5 spells, 2 preserved
  general buttons, 0 general blanks, and 0 unsupported blanks; the earlier 676
  unsupported blanks were stream-probe diagnostics, not sent proxy output. Next
  production path remains obtaining or replaying a `SetAllButtons` stream with
  real item slots after verified Feature-25 refs.
- 2026-07-01 quickbar missing-proof lifecycle diagnostics: live-data gate used
  the same fresh gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about three hours old at
  gate time, with gameplay reached. The semantic item-proof cache now records
  whether a previously proven inventory/GUI item id was cleared by exact
  `D/06` item delete or by `Area_ClientArea` reset, and quickbar summaries now
  split missing state proof into unknown, cleared-by-delete, and
  cleared-by-area-reset buckets. This does not relax compact item emission:
  only `Proven(...)` item status still permits an EE type-1 quickbar item slot.
  The replay harness now exports those buckets. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-clear-status-summary-automation-20260701-095000`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, and 1 committed quickbar rewrite summary. The committed quickbar
  still had 0 item buttons, 29 blank slots, 5 spells, 2 preserved general
  buttons, and all three missing-state detail buckets at 0 because this fresh
  capture carries no item slots. Feature-25 evidence in this shorter replay:
  24 claims, 12/12 first-list refs materialized, 1/16 second-list refs
  materialized, 15/16 second-list refs unmaterialized, 48 second-list BOOL bits,
  and 0 legacy-tail refs. Next production path remains obtaining or replaying a
  `SetAllButtons` stream with real item slots after verified Feature-25 refs;
  the new lifecycle buckets should then show whether blanked compact items were
  never proven, deliberately deleted, or area-reset before the quickbar arrived.
- 2026-07-01 quickbar missing-state item-object diagnostics: live-data gate used
  the same fresh gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about four hours old at
  gate time, with gameplay reached. Production quickbar summaries now count
  item-object statuses on `MissingStateProof` rejects: proven objects split by
  active/Feature-25 first/second/legacy-tail source, plus unknown,
  cleared-by-delete, and cleared-by-area-reset objects. This keeps compact item
  emission strict, but the next item-bearing `SetAllButtons` capture can show
  whether a whole item button lacked proof or only one primary/secondary item
  object did. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-object-status-automation-20260701-1036`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, and 1 committed quickbar rewrite summary. The committed quickbar
  still had 0 item buttons, 29 blank slots, 5 spells, 2 preserved general
  buttons, and all new rejected item-object status counters at 0 because this
  fresh capture carries no item slots. Next production path remains obtaining
  or replaying a `SetAllButtons` stream with real item slots after verified
  Feature-25 refs, then using the object-level counters to decide whether a
  bounded compact-item rule is justified.
- 2026-07-01 quickbar item-decision trace: live-data gate used the same fresh
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about five hours old at
  replay time, with gameplay reached. Production quickbar rewrite now emits a
  bounded item materialization decision trace for each parsed item button,
  including slot index, source shape, primary/secondary object ids, object
  shape status, registry proof/clear status, accepted flag, and reject reason.
  The replay harness exports committed item-decision trace counts. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-item-decision-automation-20260701-114413`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, and 1 committed quickbar rewrite summary. The committed quickbar
  still had 0 item buttons, so the new committed decision trace count was 0.
  Next production path remains obtaining or replaying an item-bearing
  `SetAllButtons` stream after verified Feature-25 refs; the decision trace
  should then identify the exact object id/proof source that accepts or blanks
  each compact item slot without relaxing the writer.
- 2026-07-01 quickbar item-shape gate unification: live-data gate used the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about six hours old at
  gate time, with gameplay reached. The quickbar writer now owns a single typed
  item-object shape classifier used by the emission gate, missing-state
  diagnostics, and item-decision trace labels, so `invalid_object_id`,
  `missing_active_properties`, `unsupported_appearance_type`, and
  `appearance_shape` cannot drift between accept/reject policy and trace output.
  Item-decision traces also include base item, appearance type/length, and
  active-property presence/count for primary and secondary objects. Strict
  replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-shape-status-automation-20260701-124219`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, and 1 committed quickbar rewrite summary. The committed quickbar
  still had 0 item buttons, so the next concrete step remains obtaining or
  replaying an item-bearing `SetAllButtons` stream after verified Feature-25
  refs; the unified shape/status fields should identify whether any rejected
  compact slot lacks object proof or fails the exact item body shape.
- 2026-07-01 quickbar materialization helper unification: live-data gate used
  the same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about 7h03m old at gate
  time, with gameplay reached. Direct `GuiQuickbar` dispatch and buffered
  quickbar zlib-stream handling now share one M-frame materialization helper
  for semantic item-proof status/proof mapping and normalized/simple rewrite
  retry order. Context-aware stream probes now keep registry proof while
  logging `trace_role=stream_probe committed=false`; the final emitted
  `SetAllButtons` rewrite remains `committed=true`. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-materialization-helper-automation-20260701-1350`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, and 1 committed quickbar rewrite summary. The replay log had 39
  stream-probe quickbar summaries and 1 committed quickbar summary. The
  committed quickbar still had 0 item buttons, 29 blank slots, 5 spell slots,
  and 2 preserved general buttons. Next production path remains obtaining or
  replaying an item-bearing `SetAllButtons` stream after verified Feature-25
  refs; this helper should keep direct and stream quickbar decisions identical
  once item slots appear.
- 2026-07-01 quickbar stream-probe counter split: live-data gate used the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about 8h03m old at gate
  time, with gameplay reached. Production quickbar rewrite summaries now expose
  `slot_records_owned`, the count of decompile-owned non-unsupported slot
  records in the parsed 36-slot body, and the replay harness exports
  committed-vs-stream-probe quickbar summary/decision counters. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-probe-counters-automation-20260701-1448`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, 39 stream-probe quickbar summaries, and 1 committed quickbar
  summary. Both probe and committed paths saw 0 quickbar item buttons; the
  committed rewrite owned all 36 slot records with 29 blank slots, 5 spells,
  and 2 preserved general buttons. Next production path remains obtaining or
  replaying an item-bearing `SetAllButtons` stream after verified Feature-25
  refs; the split counters should prove whether item slots are absent, only
  seen during buffering, or blanked by the committed writer.
- 2026-07-01 split-stream semantic shadow state: live-data gate used the same
  gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about 9h04m old at gate
  time, with gameplay reached. Split inflated server streams now translate each
  claimed high-level unit against a shadow copy of the semantic object registry
  plus the latest split-local area context, then observe the accepted unit into
  that shadow before translating the next unit. This prevents later same-buffer
  quickbar/live-object units from seeing stale item proof or stale area
  placeable context while leaving the real session state update to the normal
  accepted-payload reducer. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-split-shadow-state-automation-20260701-161120`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, 39 stream-probe quickbar summaries, and 1 committed quickbar
  summary. The committed quickbar still had 0 item buttons, 29 blank slots, 5
  spells, and 2 preserved general buttons. Next production path remains
  obtaining or replaying an item-bearing `SetAllButtons` stream after verified
  Feature-25 refs; the new split shadow state keeps any same-buffer lifecycle
  facts ordered correctly once such a stream appears.
- 2026-07-01 committed quickbar slot-profile state: live-data gate used the
  same gameplay-reaching HG capture
  `C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`; packet window
  `2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`, about 10h13m old at replay
  time, with gameplay reached. Exact EE `GuiQuickbar_SetAllButtons`
  validation now exposes a typed committed slot profile, and the semantic
  reducer stores/logs the last committed profile without letting placeholder
  quickbars replace it. Strict replay
  `C:\nwnbridge\codex-proxy2-replay-quickbar-slot-profile-state-automation-20260701-1649`
  stayed at 0 quarantines, 308 strict allows, 79 direct live-object frames, 19
  exact live-object rewrites, 98 exact lifecycle claim summaries, 10 area
  rewrites, 39 stream-probe quickbar summaries, and 1 committed quickbar
  summary. The committed semantic profile was 36 slots, 29 blanks, 5 spells, 2
  general buttons, 0 items, and 7 visible first-page slots. Next production
  path remains obtaining or replaying an item-bearing `SetAllButtons` stream
  after verified Feature-25 refs, where this state profile should distinguish
  accepted item slots from placeholder or probe-only traffic.

## Cross-cutting audit: Diamond/EE bit-order and cursor-shift correctness

Reported: 2026-05-25 during automation prompt review.

Symptoms:
- Some translated packets can look semantically plausible or pass exact
  byte-shape validation while still being wrong because Diamond and EE read or
  emit bits in a different order.
- A shifted bit cursor can make later appearance, trap/static, lock/useable,
  orientation, inventory/equipment, quickbar, or nested-object fields decode as
  believable but incorrect values.

Current status:
- Treat every previously accepted packet family, translator, repair, validator,
  and fixture as needing re-audit unless it already has explicit
  decompile-backed proof for Diamond and EE bit order and cursor movement.
- Do not sign off on a packet as correct based only on visible behavior,
  semantic-looking decoded values, an accepted capture, or an EE byte-shape
  validator pass.
- Required proof includes field sequence, bit widths, BOOL ordering, optional
  branch guards, inserted/removed bits, padding/alignment, variable-length
  string/locstring boundaries, nested object boundaries, signedness/endian/scale
  handling, and cursor reset or handoff points.
- When proof is missing, add a targeted audit note here, keep or reopen the
  issue, and convert any specific observed case into a generalized regression
  fixture only after the bit-level rule is confirmed from decompiles.
- 2026-05-25 quickbar split audit: tightened the cursor-derived
  `P/1E/01 GuiQuickbar_SetAllButtons` fallback so it follows the same
  decompile-owned boundary rule as the normal bounded tail scan. A source-only
  tail may no longer be discarded when the shifted cursor can prove only
  blank/general slots or unowned item candidates; at least one real item or
  spell slot must survive. Added a fixture-free public regression for the
  candidate-only tail case.
- 2026-05-25 `P/05/01` inventory/quickbar-link audit: rechecked the
  `I/0x2E01` large-equipment quickbar-link prefix against the Diamond
  `sub_455940` / EE `sub_1407B4F70` mask order. The parser must reject an
  absent pre-promotion interleaved tail, but the exact validator still has to
  accept `read_end == record_end` after those bytes are promoted into the CNW
  fragment bitstream. Added fixture-free tests for both sides of that contract
  and re-ran the captured `0x2E01` inventory/GQ cases.
- 2026-06-08 production hardcode guard audit: no packet behavior changed.
  Tightened the Rust production architecture guard so test-only fixture modules
  remain usable as evidence, while production translator/strict code is checked
  against current named capture/module terms such as CEP v2.2/v2.3 starter
  streams, Cormanthor/Chapter references, and Lance/Lute/Patron examples. This
  does not assign the active `U/10`/`A/6`/`U/6` bits; it prevents future fixes
  from encoding those evidence names instead of generalized packet/state rules.
- 2026-06-08 `P/05/01` creature `P -> U/5 0x3967` tail-repair audit: removed
  the transport helper's generic +/-16-bit source-tail scan. Tail repair now
  splices only the caller-computed original tail start, including tracked
  inserted/removed appearance bits, and rejects if the following focused
  creature-update validator does not accept that exact cursor. The Sooty Crow
  NPC fixture remains active evidence rather than a positive rewrite fixture:
  exact `original_tail=58` rejected, while the old window accepted
  `candidate_tail=56` and duplicated two earlier source bits. Do not restore a
  cursor window; prove the missing two-bit owner from Diamond/EE appearance
  decompiles or make the appearance rewrite report a decompile-backed bit delta.
- 2026-06-08 follow-up `P/05/01` full creature appearance bit-delta audit: no
  packet behavior changed. Debug proof for the Sooty Crow NPC span shows the
  failing `P/5` at offset 325 is a byte-only EE build widening:
  `source_bit_cursor=57`, direct CExoString name, `legacy_bits=1`,
  `ee_bits=1`, no nested `ee_name_bit_rewrites`, no EE fragment inserts, and
  no removed fragment bits before the following `U/5 0x3967` at offset 454.
  Added fixture-free coverage for the generalized shape with nonzero
  delete-only visible-equipment rows: Diamond and EE both consume only the
  direct creature-name selector while the EE widening is read-buffer-only. The
  old `candidate_tail=56` therefore duplicated pre-appearance/name-selector
  bits and remains invalid; the unresolved two-bit owner is outside this
  appearance bit-delta path.
- 2026-06-08 `U/5 0x3967` scalar-orientation cursor audit: no packet behavior
  changed. Added fixture-free coverage for the decompile-backed action-0
  `0x3967` row order: position owns two fragment bits, scalar orientation owns
  selector plus four residual bits at the exact caller cursor, then `0x0040`,
  identity, and associate suffix BOOLs follow. The same test rejects a
  two-bit-late cursor and a tail missing the two position-owned bits, with the
  caller cursor restored. This pins the Sooty offset-454 rejection as a real
  cursor-boundary problem, not a license to retry or borrow the two bits before
  the following `U/5`; the upstream source-tail owner remains unresolved.
- 2026-06-08 full-appearance following-`U/5` fence audit: packet behavior
  changed only by removing an unproven repair candidate. Following-side
  cross-record fences now require the same named CNW header bit shapes proven
  for preceding appearance fences (`CNW_FRAGMENT_HEADER_BITS` or
  `CNW_FRAGMENT_HEADER_BITS + 1`). The old
  `CNW_FRAGMENT_HEADER_BITS + LEGACY_UPDATE_POSITION_FRAGMENT_BITS` candidate
  could claim a following `U/5` row's two position-owned bits as appearance-side
  transport, so it is no longer offered. This keeps the Sooty exact-cursor
  rejection active evidence; it does not assign the unresolved upstream two-bit
  owner.
- 2026-06-08 Diamond reader dispatch audit for the same `P/5 -> U/5 0x3967`
  span: no packet behavior changed. Private fixture debug rerun accepted the
  creature appearance at offset 325 with `record_end=454`,
  `source_bit_cursor=57`, direct CExoString name, and translated
  `fragment_bits=1`; the exact following `U/5` starts at offset 454 with
  caller `bit_cursor=58`, rejects at `stage=orientation-scalar` after reaching
  `read_cursor=471 bit_cursor=61`, and finds no action-0 bridge follow-up.
  Diamond client decompile confirms the `P` and `U` records are separate outer
  submessages: `sub_44EF00` dispatches `P`/type 5 to `sub_448E30`, returns to
  the same outer loop, then dispatches `U`/type 5 through `sub_459700` to
  `sub_44ADD0`. The `U` body begins from the passed mask/object id; for mask
  `0x0001` it reads the three position values, then for mask `0x0002` it reads
  the orientation BOOL and scalar/vector branch. No decompile-backed
  inter-record BOOL or byte-free appearance-side owner has been found. Next
  target is the upstream live-object/fragment-storage source-tail handoff before
  the `P/5` record, not a retry window at the `U/5` boundary.
- 2026-06-08 upstream source-tail trace for the same span: no packet behavior
  changed. A focused private rerun shows the preceding creature-update span,
  after the earlier appearance rewrite, accepts `U/5 0x3967` at
  `offset=218`, `read_end=285`, `start_bit_cursor=44`, and
  `end_bit_cursor=57`; the next full creature appearance enters at
  `offset=325` with `bit_cursor=57`. The unresolved owner is therefore not the
  following `U/5` and not the `P/5` direct-name bit; it is the preceding
  interleaved fragment span currently promoted between that `U/5` read end and
  the next `P/5`. The open proof question is whether that adjacent source span
  is a fresh CNW fragment-storage blob with its own three-bit final-count
  header, as the current generic promoter assumes, or a continuation/list
  handoff with different cursor ownership. Added diagnostic logging for
  creature-update interleaved-span promotion so the next harness/live rerun can
  report `read_end`, `old_record_end`, `bits_promoted`, insertion cursor, and
  start/end cursor directly. Do not change the three-bit header handling or
  donate/borrow bits until decompile-backed source-writer/list-handoff proof or
  a compact-source capture assigns that span.
- 2026-06-08 follow-up diagnostic audit for the Sooty span: no packet behavior
  changed. The private negative fixture rerun with
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM_OWNER_OFFSET=218,325,454` still rejects the
  later `P/5` at `offset=325` and following `U/5 0x3967` at `offset=454`
  without any mutating `interleaved fragment span promoted` trace. The visible
  `offset=218` accepts are read-only appearance-fence probes, so they do not
  prove that the `285..325` upstream bytes are a fresh CNW storage blob. Added
  debug proof logging for the immutable creature-update interleaved-span
  verifier and pinned fixture-free coverage that creature-update span promotion
  inserts only payload bits after the three CNW final-count header bits. Next
  verification should instrument or reduce the actual `285..325` source span
  before assigning it as fresh header storage or continuation/list-handoff data.
- 2026-06-08 inter-record reduction for the same span: no packet behavior
  changed. Added debug-only record-candidate and span-owner scan reducers. A
  private rerun shows the earlier `P/5` at `offset=81` rewrites, then the
  following `U/5 0x3967` at `offset=218` is a real top-level record ending at
  `285` with `bit_cursor=44->57`; the bounded span-owner scan reports
  `accepted_read_end=None`. The bytes after that are not a fresh CNW storage
  blob: the walker sees a top-level `A/5` creature add at `offset=285` ending
  at `317`, and the EE visual-transform identity insert shifts the next `P/5`
  candidate to `offset=325` without advancing the fragment cursor. The later
  `P/5` consumes the direct CExoString selector (`57->58`), then the following
  `U/5 0x3967` at `offset=454` also has `accepted_read_end=None` and rejects.
  Next verification should audit the decompile-backed `U/5 0x3967` action-0
  bit cursor for the `offset=218` shape, plus the intervening `A/5`
  fragment-neutral add contract; do not treat `285..325` as donated CNW
  storage and do not restore cursor borrowing.
- 2026-06-08 offset-218 `U/5 0x3967` / intervening `A/5` contract audit: no
  packet behavior changed. Added generalized tests for the decompile-backed
  action-0 scalar orientation shape with the explicit false orientation-target
  guard; it advances the caller CNW cursor by 13 bits (`position`, scalar
  selector/residuals, target guard, `0x0040`, identity, associate). Added a
  fixture-free stream regression proving a legacy `A/5` creature add inserts
  only EE object visual-transform bytes before a following `U/5 0x3967`: no
  fragment bits are inserted, removed, promoted, consumed, or donated, and the
  packed CNW fragment tail remains byte-for-byte identical. The offset-454
  rejection remains unresolved; next verification should trace the earlier
  cursor owner before the later `P/5`/`U/5` pair, not the intervening `A/5` or
  the already pinned offset-218 action-0 row.
- 2026-06-08 later `U/5 0x3967` handoff regression: no packet behavior
  changed. Added a public fixture-free stream shaped as
  `U/5 0x3967 -> A/5 -> P/5 direct-name -> U/5 0x3967`. The exact stream
  claims after the normal `A/5` EE visual-transform byte insertion, with zero
  fragment bits inserted/removed/promoted, while a sibling missing the later
  `U/5` row's two position residual bits rejects and rolls back unchanged.
  This rules out a generalized cursor donation from the earlier `0x3967`
  action-0 row, the intervening creature add, or a name-only creature
  appearance selector. The private offset-454 full-appearance rejection remains
  active; next verification should focus on the full `P/5` appearance
  rewrite/tail provenance or a source-capture/decompile owner before that
  full appearance, not on borrowing the later `U/5` position bits.
- 2026-06-08 full-appearance stream handoff regression: no packet behavior
  changed. Added a public fixture-free stream shaped as full `P/5` creature
  appearance with direct creature name and a nested visible-equipment item add
  followed by `U/5 0x3967`. The exact stream rewrites and claims with the full
  appearance byte widening plus one EE active-property BOOL inserted inside the
  nested item row; the following `0x3967` bits remain unchanged. The sibling
  stream missing the following `U/5` row's two position residual bits rejects
  transactionally, so the full appearance rewrite/visible-equipment item path
  is not a generalized owner for those bits. The private offset-454 rejection
  remains active; next proof still needs source-capture/decompile ownership
  before the full appearance or a narrower full-appearance tail provenance rule.
- 2026-06-08 byte-only full-appearance tail-repair audit: no accepted packet
  behavior changed. A private Sooty debug rerun still shows the later full
  `P/5` at offset `325` entering with `bit_cursor=57`, consuming only the
  direct-name BOOL to reach `58`, and leaving the following `U/5 0x3967` at
  offset `454` rejected from exact `original_tail=58`. The appearance rewrite
  at this boundary inserted bytes only (`inserted_bits=0`, `removed_bits=0`),
  so proxy2 no longer arms the `P`-side fragment-tail splice for that byte-only
  widening. Normal following-`U/5` action repairs already run before any pending
  tail splice; a byte-only appearance therefore cannot be treated as a fragment
  cursor owner. The unresolved owner remains upstream of the full appearance or
  in a decompile-backed source-capture boundary, not in a no-op tail splice.
- 2026-06-08 byte-only full-appearance stream regression: no packet behavior
  changed. Added a public fixture-free stream shaped as full `P/5` creature
  appearance with direct creature name and delete-only visible-equipment rows
  followed by `U/5 0x3967`. The exact stream rewrites and claims with only EE
  full-appearance read-buffer widening (`bits_inserted=0`,
  `bits_removed=0`, no interleaved span promotion), and the packed CNW
  fragment bits remain `direct-name + following 0x3967` unchanged. A sibling
  missing the following `U/5` row's two position bits rejects
  transactionally and remains quarantinable. This rules out the byte-only
  full-appearance/delete-equipment path as a generalized owner; next proof
  should trace the bitstream provenance before the full appearance or obtain a
  source capture/decompile-backed boundary owner.
- 2026-06-08 creature-update suffix/top-level add boundary audit: narrowed the
  `U/5` interleaved-fragment promoter so a candidate suffix beginning with a
  decompile-shaped legacy/EE `A/5` creature add is kept as a top-level read
  buffer row, not stripped as CNW fragment storage just because byte `0x41`
  can decode as a plausible final-count header. Added fixture-free coverage for
  `U/5 0x3967 -> A/5`: the helper rejects promotion, the ordinary add
  translator inserts only EE visual-transform bytes, and the final stream
  exact-claims with zero interleaved bits promoted. The private Sooty diagnostic
  still rejects at the later full `P/5 -> U/5 0x3967` boundary, so this rules
  out one false storage-owner path without assigning the unresolved two bits.
- 2026-06-08 EE full-appearance candidate audit: no packet behavior changed.
  Added debug-only accepted-candidate logging for verified EE `P/5` appearance
  boundary selection, including the chosen end cursor and whether it was a
  generic boundary or focused following-creature-update proof. Added a
  fixture-free regression proving that after full appearance EE widening,
  `try_get_verified_ee_creature_appearance_record_end_and_cursor` lands exactly
  before the following `U/5 0x3967` and advances only the direct-name plus
  visible-equipment EE BOOLs. Removing the decompile-owned name selector bit
  leaves that boundary unclaimable. This pins the byte-plausible
  full-appearance candidate selection around the Sooty offset-325 trace but does
  not assign the unresolved two bits before offset 454.
- 2026-06-08 stale-declared `P/5 -> U/5 0x3967` split audit: tightened the
  appearance/update read-window helper so a byte-shaped same-object pair is not
  enough when the inherited CNW cursor is proven through the preceding
  appearance row. In that proven path, the terminal `U/5` row must validate from
  the exact cursor using the focused legacy/EE creature-update cursor advancer,
  and a fixture-free regression now rejects the same split when only the
  following `0x3967` row's two position residual bits are missing. Existing
  full-current-player/full-appearance stale-declared positives remain
  unsupported at this transport helper and rely on the later typed rewrite plus
  exact EE validator; keep the active issue open until decompiles or compact
  source captures assign that full-appearance tail provenance.
- 2026-06-08 stale-declared capacity follow-up for the same full-appearance
  handoff: tightened the broad read-prefix capacity preflight only for an
  adjacent same-object creature appearance followed by `U/5 0x3967` when the
  preceding `P/5` cursor can be proven. In that bounded shape, the capacity gate
  now reuses the focused legacy/EE update cursor advancers instead of a coarse
  0x3967 byte-length floor, so nested full-appearance active-property bits
  cannot masquerade as the update row's two missing position residual bits.
  Existing stale-declared positive streams with unmodeled full-current-player
  or other creature-update rows still fall through to the later typed rewrite
  and exact EE validator. The Sooty offset-454 owner remains unresolved; this
  proves only that transport capacity must not borrow across a proven adjacent
  full `P/5 -> U/5 0x3967` boundary.
- 2026-06-09 stale-declared adjacency audit for the same `P/5 -> U/5 0x3967`
  helper: tightened the transport-only pair proof to a proven immediately
  preceding same-object `P/5` record followed by terminal `U/5 mask=0x3967`.
  The older helper remembered any prior same-object creature appearance, which
  made the code broader than the decompile-backed adjacent-pair rule and could
  ignore an intervening top-level row while checking the terminal update cursor.
  Added a fixture-free `P/5 -> A/5 -> U/5 0x3967` negative proving even a
  fragment-neutral Diamond creature add breaks this stale-declared helper's
  adjacency proof; such streams must continue through the normal typed rewrite
  plus exact EE validator. This does not assign the unresolved Sooty two-bit
  owner.
- ~~2026-06-09 `U/5 0x3967` action-2 optional-float guard audit: tightened the
  typed repair so it first accepts already exact/interleaved-span-valid records
  and reports the rewritten BOOL bit when it really clears the Diamond
  action-followup optional-float guard. This prevents the alternate
  orientation-target candidate from clearing the following `0x0040` state BOOL
  on already valid action-2 rows whose optional-float guard is false. Verified
  with fixture-free true-guard and false-guard regressions plus focused `3967`
  and `live_object_update` tests.~~

Most likely areas to re-audit first:
- `P/04/01 Area_ClientArea`: static row bit/byte order, module-resource-backed
  repairs, trap/static flags, and orientation fields.
- `P/05/01 GameObjUpdate_LiveObject`: add/update records, appearance/model
  records, scalar/vector orientation branches, state flags, and nested fragment
  boundaries.
- Quickbar, inventory/equipment, and any packet family with nested
  variable-length objects or conditional BOOL gates.

## Visual/alignment regression: placeables and player model

Reported: 2026-05-24 during the recurring `continue-ee-bridge-dev` automation.

Symptoms:
- Some chests/placeables appear randomly rotated.
- Some non-trapped chests/placeables appear with trapped visuals.
- The player character model can appear as a bird even when the selected
  character should not be a bird.

Current status:
- Treat this as a suspected packet reader/writer alignment bug, not as a
  cosmetic asset issue.
- The source character seed is probably not the cause of the bird model:
  `C:\NWN\NWN Diamond\servervault\Starcore5\starcore-druid60.bic` and
  `C:\NWN\NWN Diamond\localvault\druid.bic` both report
  `Appearance_Type = 2`, `Race = 2`, `Gender = 1`, and `Phenotype = 0`.
  That points the player-model symptom toward live-object current-player
  add/appearance translation unless later BIC/CharList evidence proves
  otherwise.
- Do not promote new live-object or area fixtures as positive coverage when
  the only proof is that the EE byte-shape validator accepts them. These
  symptoms show that exact byte-shape can still be semantically wrong if a
  field cursor is shifted before appearance, trap/static state, orientation, or
  creature appearance/model fields.
- A 2026-05-24 local Chapter2 harness run
  `C:\nwnbridge\local-diamond-bridge-20260524-210621` reached gameplay with no
  root quarantine and produced accepted-live-object diagnostics, but those
  diagnostics were intentionally not promoted because of this report.
- 2026-05-24 follow-up: reduced the placeable visual symptoms to a generalized
  `P/04/01 Area_ClientArea` semantic row problem, not a byte-shape failure.
  The local `To Heir is Human` `cormanthor` compact fixture had exact EE
  cursor proof but decoded one static row as appearance 82, x=48.367825,
  y=4.26e-39, z approximately 0 while the module GIT proves y=43.604927.
  Implemented module-resource-backed GIT placeable parsing and bounded
  static-row repair:
  preserve packet object ids, rewrite appearance/position/bearing only when
  the local GIT static count equals the packet static count and each row
  uniquely matches by appearance plus at least two coordinates. Private tests
  prove this against `cormanthor` and parse Chapter2 `a08_barracks` as 43
  placeables/32 static/7 usable/0 trapped.
- A fresh 2026-05-24 local `To Heir is Human` bridge replay
  `C:\nwnbridge\local-diamond-bridge-20260524-221113` reached the named
  `cormanthor` `Area_ClientArea` path with 0-byte proxy stderr and no root
  quarantine. That named packet did not need static-row repair
  (`module_resource_static_placeable_repairs=0`), but the production hook now
  also resolves exact named area packets through the same module-backed static
  placeable guard before deciding no rewrite is required.
- `P/04/01 Area_ClientArea` static-row semantic drift is partially addressed
  for module-resource-backed compact and exact-named area packets. Keep this
  issue open until a visual replay confirms affected placeables and until
  `P/05/01 GameObjUpdate_LiveObject` current-player/placeable appearance
  records are audited for the player-model symptom.
- 2026-05-24 current-player `P/05/01` follow-up: reduced the stale
  accepted-live-object diagnostic to a generalized `P/5` full creature
  appearance cursor shift. The no-proof locstring parser could treat the first
  direct CExoString length/name as a plain locstring token, and the body parser
  could accept a shifted short/zero body branch before the real full body table
  plus visible-equipment count. Added writer-contract checks for the fixed
  nineteen-value full-body table branch and zero EE high bytes, plus a private
  Chapter2 regression proving current-player `Appearance_Type = 2`, selector
  `0x13`, and 8 visible-equipment records survive the Diamond-to-EE rewrite.
  The same pass fixed an appearance-adjacent `U/5 0x3967` stream consequence:
  after the full `P/5` rewrite, fragment-tail repair now composes the original
  tail splice with the existing decompile-backed action0 missing-damage and
  short-associate repairs before exact EE validation. The Dark Ranger seq15
  private fixture exact-claims this composed repair. Keep the broader issue
  open until visual replay confirms the player model and remaining placeable
  appearance/orientation symptoms.
- 2026-05-25 `P/05/01` placeable-add audit: replaced the literal add-record
  bit-copy table with a typed Diamond-to-EE state-bit mapper for `A/09`
  placeable add records. The mapper preserves the shared reputation/static,
  useable, trap-disarmable, lockable, locked, unknown sibling, and name-valid
  fields, inserts the EE optional-target guard bit instead of overwriting a
  shared legacy bit, clears stale optional-target-like bits when no guarded
  OBJECTID bytes are present, preserves real guarded OBJECTID tails, and forces
  the EE light guard false. New fixture-free semantic tests run through the
  exact EE add-fragment validator for both absent and present optional-target
  branches. Keep this issue open pending visual replay and remaining
  placeable-update/orientation audit.
- 2026-05-25 `P/05/01` placeable-update boundary audit: found and removed a
  scalar-only duplicate door/placeable `U` boundary helper in the live-object
  add-map pass. The shared transport boundary now follows the same EE reader
  order as the typed validator: position, scalar/vector orientation,
  appearance/resref, scale/state, and optional inline name; it accepts only
  candidate spans that land on a real stream boundary and rejects ambiguous
  scalar/vector byte shapes. Regression tests cover vector-orientation spans,
  opcode-looking appearance WORD bytes, and scalar+appearance candidate
  skipping. Verified with `cargo test -q -p hgbridge-proxy2
  live_object_update::boundary::tests:: -- --nocapture` and the existing
  `legacy_all_bits_placeable_update_preserves_orientation_mask_when_rewritten`
  regression. Keep this issue open pending visual replay and any remaining
  placeable state/update audit.
- 2026-05-25 `P/05/01` placeable state diagnostics: added Rust proxy2
  semantic logging at the typed placeable add/update boundary without changing
  packet shape. `A/09` add logs now expose the decompile-backed source state
  fields, consumed legacy optional gate, emitted EE optional-target gate, and
  neutral light gate when the full source bool span is proven; compact
  source-only add shapes remain explicitly unlabeled instead of inventing
  state. `U/09` update logs now print decoded source and emitted EE visual /
  visual-active / locked / lockable / visual-payload bits from the same
  state-block cursor used by the exact validator. Verified with
  `cargo test -q -p hgbridge-proxy2 placeable_add_semantic_tests -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 placeable_update -- --nocapture`, and
  `cargo check -q -p hgbridge-proxy2`. Next replay should compare these labels
  on visually bad placeables before changing mapping rules.
- 2026-05-25 `P/05/01` live-object OBJECTID scanner audit: centralized the
  known legacy live-object id namespace guard used by the live-object add,
  update boundary, creature/appearance, and inventory validators. This keeps
  the decompile-backed OBJECTID treatment as an opaque DWORD while sharing the
  proxy's false-positive guard for proven Diamond/HG namespaces, with the
  tighter inventory compact-id floor still explicit. The same pass fixed the
  low-compact placeable continuation probe, which had rejected every nonzero
  compact id before checking the following fragment/continuation proof. Verified
  with `cargo test -q -p hgbridge-proxy2 object_id -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 live_object_update::boundary::tests:: -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 placeable_add_semantic_tests -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 placeable_update -- --nocapture`, the two
  private Chapter2/Winds live-object fixture seeds, and
  `cargo check -q -p hgbridge-proxy2`. Keep this issue open until a local visual
  replay confirms the affected placeable/player appearances and the diagnostic
  labels match the intended Diamond state.
- 2026-05-25 local `To Heir is Human` replay
  `C:\nwnbridge\local-diamond-bridge-20260525-041906` reached `cormanthor`
  again with 0-byte proxy stderr and no quarantine files. The late screenshot
  shows the player rendered as a normal small humanoid, not the reported bird
  model. The repeated compact `A/09` + `U/09 0xFFFFFFF7 -> 0x37` placeable
  update was not a state-mapping mismatch: the byte reader proves scalar
  orientation while the legacy bit stream is already at the state block, so the
  source diagnostic must decode with inserted-orientation semantics instead of
  the raw sparse mask cursor. Fixed the shared `U/09` diagnostic cursor and
  added fixture-free tests proving compact inserted-scalar source state and
  normal preserved-scalar source state. Verified with
  `cargo test -q -p hgbridge-proxy2 source_state_diagnostic -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 placeable_update -- --nocapture`, and
  `cargo check -q -p hgbridge-proxy2`. Keep the broader issue open for a
  targeted visual comparison against an actually bad placeable/player capture;
  current local evidence no longer shows a source/EE state-label divergence.
- 2026-05-25 `P/05/01` compact placeable-add residue audit: generalized the
  compact `A/09` add-record rewrite so any bounded residual source-fragment
  count from 0 through the four Diamond compact tail BOOLs is drained before
  the EE direct-name guard/light bits are emitted. The prior accepted set
  covered zero, one, or full four-bit residue but skipped the structurally
  equivalent two/three-bit cases. Added fixture-free coverage that runs all
  five residue counts through the exact EE add-fragment validator. Verified
  with `cargo test -q -p hgbridge-proxy2 placeable_add_semantic_tests -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 compact_placeable_add -- --nocapture`,
  `cargo fmt --all --check`, and `cargo check -q -p hgbridge-proxy2`. Keep the
  broader issue open pending visual replay against a confirmed bad placeable.
- 2026-05-25 `P/04/01` -> `P/05/01` placeable state-context audit: carried
  uniquely matched module GIT static-placeable state into the reusable
  `AreaPlaceableContext` rows instead of leaving live-object overlap checks with
  only appearance/position evidence. Matching is still bounded to repaired or
  named module-backed static rows with unique appearance/position/bearing
  proof. `A/09` overlap diagnostics now print that module state and flag
  useable/trap-disarmable/lockable/locked mismatches against the decompile-backed
  add-record source bits without changing emitted packet state. Verified with
  `cargo test -q -p hgbridge-proxy2 placeable_add_semantic_tests -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 translate::area::tests::local_to_heir_compact_area_uses_module_resource_dimensions -- --nocapture`,
  `RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q -p hgbridge-proxy2 translate::area::tests::local_chapter2_no_name_area_resolves_fragmented_area_resref -- --nocapture`,
  `cargo fmt --all --check`, `git diff --check`, and
  `cargo check -q -p hgbridge-proxy2`. Keep the broader issue open pending
  visual replay against a confirmed bad placeable.
- 2026-05-25 `P/04/01` static direction audit: tightened the static-placeable
  direction normalizer so zero-length direction vectors no longer get converted
  to north. The decompiled static row supplies only `OBJECTID + appearance +
  position + direction vector`; a zero horizontal vector has no yaw to preserve,
  so it must either be repaired by unique module GIT bearing proof or fail the
  final EE static-row proof. Added fixture-free public regression coverage for
  zero-vector rejection and nonzero yaw preservation. Keep the broader issue
  open pending a visual replay against a confirmed bad static placeable.
- 2026-05-25 `P/05/01` inventory/quickbar-link audit: confirmed the
  `I/0x2E01` large-equipment interleaved-tail branch is a split/promotion
  boundary issue, not a byte-reader body. The prefix scanner rejects a missing
  bounded tail before promotion, while the post-promotion exact claim accepts
  the shortened read cursor. Added fixture-free coverage for the absent-tail
  prefix and the bounded 11-byte tail before `GQ`; verified the existing
  captured `0x2E01` Docks/Sooty inventory cases still claim after rewrite.
- 2026-05-25 `P/05/01` placeable-add repeat-pass guard audit: tightened the
  already-EE-shaped `A/09` guard repair so an inline CExoString using EE's
  `outer=true, inner=false` helper keeps the inner selector before the optional
  OBJECTID guard. Only the legacy `outer=true, inner=true` direct-name mismatch
  is collapsed to `outer=false`. Added fixture-free tests for both optional
  OBJECTID branches and exact post-repair add validation.
- 2026-05-25 `P/05/01` placeable-add guard coverage audit: no packet behavior
  changed, but the decompile-backed name/optional-target bit-order proof is now
  covered in the public test suite. Added fixture-free no-optional-OBJECTID
  guard regressions for both `outer=true, inner=false` inline helper names and
  `outer=true, inner=true` direct-name repair, and moved the inline/direct
  placeable name-mode tests out from the private-fixture gate while asserting
  the final exact EE add cursor. Verified with `cargo test -q -p
  hgbridge-proxy2 add_guard -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 placeable_name_mode_tests -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 placeable_add_semantic_tests -- --nocapture`, `cargo fmt
  --all --check`, `git diff --check`, and `cargo check -q -p hgbridge-proxy2`.
- 2026-06-23 follow-up exact `A/09` placeable-add TLK-name branch: exact EE
  placeable add validation now accepts the decompile-shaped locstring/token
  name path only when the fragment cursor has `outer=true, inner=true` and the
  read buffer at the name cursor is exactly `BYTE client-TLK selector + DWORD
  token`. The same validator still rejects a stale true inner bit over inline
  CExoString bytes, so the first post-name state BOOL cannot be borrowed as a
  name helper selector. Verified with focused `placeable_add`/`add_guard`/
  `placeable_name_mode_tests` filters and `cargo check -q -p hgbridge-proxy2`.
- 2026-05-25 `P/05/01` placeable-update forced-orientation diagnostic audit:
  no packet shape changed, but the source-state logger now decodes `U/09`
  state bits using the byte-proven forced scalar/vector branch width instead of
  reusing a stale selector bit. This matters when exact bytes prove one
  orientation branch while the fragment cursor still contains the opposite
  selector; otherwise visual/locked state diagnostics can be shifted while the
  emitted packet remains exact. Added fixture-free tests for forced scalar and
  forced vector source-state decoding. Verified with `cargo test -q -p
  hgbridge-proxy2 source_state_diagnostic -- --nocapture` and `cargo test -q
  -p hgbridge-proxy2 placeable_update -- --nocapture`.
- 2026-05-25 `P/05/01` placeable-update state-preservation audit: hardened the
  cloned `U/09` fragment-bit rewrite so source placeable state bits decoded at
  the byte-proven Diamond cursor must equal the emitted EE state bits after
  scalar insertion, forced scalar, or forced vector orientation repair. A
  mismatch now rejects the candidate instead of allowing a semantically shifted
  but byte-plausible update. Added fixture-free coverage for all three
  orientation repair paths. Verified with `cargo test -q -p hgbridge-proxy2
  source_state_diagnostic -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  placeable_update_bit_rewrite_preserves_state -- --nocapture`, and `cargo
  test -q -p hgbridge-proxy2 placeable_update -- --nocapture`. Keep the broader
  issue open pending a targeted visual comparison against a confirmed bad
  placeable/player capture.
- 2026-05-25 `P/04/01` static-placeable context-state audit: no packet behavior
  changed, but the module GIT state handoff now has public regression coverage
  for the decompiled two-triplet static row rule. A static row may inherit
  module useable/trap/lock state only when appearance plus at least two
  coordinates and the second direction triplet match the resource bearing;
  plausible appearance/position alone is not enough because EE and Diamond use
  that second triplet to derive static mesh yaw. Verified with `cargo test -q
  -p hgbridge-proxy2 module_context_state_requires_matching_static_direction_triplet
  -- --nocapture`. Keep the broader issue open pending visual replay against a
  confirmed bad static/live placeable capture.
- 2026-05-25 `P/04/01` static-placeable context uniqueness audit: no packet
  behavior changed, but public coverage now proves module trap/use/lock state is
  not inherited when two static GIT rows match the same appearance/position/
  direction proof or when the matching module row is not static. This keeps
  later `A/09` overlap diagnostics from treating ambiguous module state as a
  decompile-backed fact. Verified with `cargo test -q -p hgbridge-proxy2
  public_static_direction_tests -- --nocapture`. Keep the broader issue open
  pending visual replay against a confirmed bad static/live placeable capture.
- 2026-05-25 `P/04/01` static-placeable repair cursor audit: no packet behavior
  changed, but public fixture-free coverage now proves the module-resource row
  repair path can replace an EE-unsafe static direction vector with the unique
  GIT bearing while preserving the exact legacy post-tile cursor/count proof,
  and refuses to rewrite when two static GIT rows can match the same packet row.
  Verified with `cargo test -q -p hgbridge-proxy2 public_static_direction_tests
  -- --nocapture`. Keep the broader issue open pending visual replay against a
  confirmed bad static/live placeable capture.
- 2026-06-10 `P/04/01` static-placeable module-repair claim audit: hardened
  the module-resource repair path so the unique static GIT match is carried as
  an immutable row claim before any appearance/position/direction bytes are
  rewritten. The claim records the source row cursor, original appearance,
  placement triplet, direction triplet, matched module row, and match kind
  (`appearance+two coordinates` or `zero appearance+all coordinates`), then the
  writer revalidates that claim against the current source bytes before
  mutation. This keeps module-backed trap/use/lock context and static-row
  repairs tied to the exact decompiled row identity rather than a stale tuple.
  Verified with `cargo test -q -p hgbridge-proxy2 module_static --
  --nocapture`, `cargo fmt --all --check`, `git diff --check -- proxy2/src/translate/area.rs`,
  and `cargo check -q -p hgbridge-proxy2`. Keep the broader issue open pending
  visual replay against a confirmed bad static/live placeable capture.
- 2026-06-10 `P/04/01` static-placeable module-state context claim audit:
  tightened the production context collector so module-backed trap/use/lock
  state is exported only after the exact static list has a whole-list,
  one-to-one row claim and each claimed direction triplet matches the module
  bearing. Duplicate packet rows that can each match one module row still expose
  static context, but no longer inherit resource state without the list-level
  proof. Verified with `cargo test -q -p hgbridge-proxy2
  placeable_context_module_state_requires_list_level_static_claims --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 module_static --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2
  public_static_direction_tests -- --nocapture`. Keep the broader issue open
  pending visual replay against a confirmed bad static/live placeable capture.
- 2026-06-10 `P/04/01` named static resource proof audit: aligned the
  named-area module-resource selector with the same safe static-row predicate
  used by repair and context export. A named area packet can no longer select a
  local module resource from a static GIT row whose replacement geometry or
  bearing is outside the decompiled static-row value domain, even when
  appearance plus two coordinates match. Static row claims now also retain the
  packet object id and the context exporter rechecks the raw row claim before
  attaching module trap/use/lock state. Verified with `cargo test -q -p
  hgbridge-proxy2 named_static_resource_candidate -- --nocapture`, `cargo
  test -q -p hgbridge-proxy2 module_static -- --nocapture`, and `cargo test -q
  -p hgbridge-proxy2 public_static_direction_tests -- --nocapture`. Keep the
  broader issue open pending visual replay against a confirmed bad static/live
  placeable capture.
- 2026-06-10 `P/04/01` -> `P/05/01` placeable context provenance audit:
  promoted the area-context overlap API from untyped row references to
  light/static row-kind matches. Live-object `A/09` overlap diagnostics now
  report light and static area rows separately and only compare live add state
  against module trap/use/lock state on static-context rows. No emitted packet
  bytes changed; this keeps the next visual replay evidence tied to the
  decompiled light-list versus static-list owner instead of a generic
  area/static overlap. Verified with `cargo test -q -p hgbridge-proxy2
  placeable_context_uses_light_rows_before_static_rows -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2
  placeable_overlap_context_format_keeps_light_static_provenance --
  --nocapture`, `cargo fmt --all --check`, `git diff --check`, and `cargo
  check -q -p hgbridge-proxy2`. Keep the broader issue open pending visual
  replay against a confirmed bad static/live placeable capture.
- 2026-06-10 `P/04/01` -> `P/05/01` placeable context identity-confidence
  audit: no emitted packet bytes changed. Area placeable context rows now carry
  explicit object-id confidence (`unique`, area-object alias, duplicate, or
  both) after the exact post-tile cursor proof, and non-unique rows no longer
  export module trap/use/lock state into later live-object `A/09` mismatch
  diagnostics. Duplicate light/static rows remain visible as diagnostic
  context, but the bridge no longer treats their GIT state as a unique
  engine-state fact. Verified with non-incremental isolated-target `cargo
  check -q -p hgbridge-proxy2`, `cargo test -q -p hgbridge-proxy2
  placeable_context_ -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  placeable_overlap_context_format_keeps_light_static_provenance --
  --nocapture`, and `cargo fmt --all --check`. Keep the broader issue open
  pending visual replay against a confirmed bad static/live placeable capture.
- 2026-06-10 `P/04/01` -> `P/05/01` placeable update-context audit: no emitted
  packet bytes changed. The M-frame live-object update passes now thread the
  latest area placeable context into the `U/09` translator, and successful
  placeable update rewrites log light/static overlap rows, object-id confidence,
  source/EE state bits, and module-backed lockable/locked mismatches where a
  unique static row proves module state. This makes the next visual replay
  distinguish add-state drift from later update-state drift without treating
  GIT state as a rewrite authority. Verified with non-incremental isolated
  target `cargo test -q -p hgbridge-proxy2
  placeable_update_area_context_helpers_keep_identity_and_lock_proof --
  --nocapture` and `cargo test -q -p hgbridge-proxy2 placeable_update --
  --nocapture`. Keep the broader issue open pending visual replay against a
  confirmed bad static/live placeable capture.
- 2026-06-10 `P/04/01` -> `P/05/01` shared placeable overlap audit: no emitted
  packet bytes changed. `AreaPlaceableContext` now owns the shared overlap
  result, row formatting, and static module-state conflict helper used by both
  `A/09` add diagnostics and `U/09` update diagnostics. Exact object-id matching
  and legacy/external-id equivalence remain caller-selected, but light/static
  row provenance and module trap/use/lock mismatch formatting now follow one
  generalized state path. Verified with non-incremental isolated-target `cargo
  check -q -p hgbridge-proxy2`, `cargo test -q -p hgbridge-proxy2
  placeable_context_overlap_formats_rows_and_checks_static_module_state --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 placeable_context_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  placeable_overlap_context_format_keeps_light_static_provenance --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  placeable_update_area_context_helpers_keep_identity_and_lock_proof --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 placeable_update --
  --nocapture`, `cargo fmt --all --check`, and `git diff --check`. Next
  verification remains visual replay/local harness evidence to decide whether
  the remaining drift belongs to `A/09` add-state synthesis or `U/09` update
  translation.
- 2026-06-10 `P/04/01` -> `P/05/01` shared placeable observed-state audit: no
  emitted packet bytes changed. `AreaPlaceableContext` now owns the observed
  live-state model and static module-state conflict field reporting for
  placeable add/update diagnostics; `A/09` maps useable/trap-disarmable/
  lockable/locked bits into that model, while `U/09` maps only the
  decompile-backed lockable/locked update bits. The old per-family mismatch
  helpers were removed so future rewrite policy has one static/live state
  comparison path, and logs now include exact conflicting field names instead
  of only a boolean. Verified with non-incremental isolated-target `cargo
  check -q -p hgbridge-proxy2`, `cargo test -q -p hgbridge-proxy2
  placeable_context_overlap_formats_rows_and_checks_static_module_state --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  placeable_overlap_context_format_keeps_light_static_provenance --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  placeable_update_area_context_helpers_keep_identity_and_lock_proof --
  --nocapture`, `cargo fmt --all --check`, and `git diff --check`. Next
  verification remains local visual replay with these conflict-field labels to
  decide whether a generalized state rewrite is warranted for add synthesis or
  later update translation.
- 2026-06-10 `P/04/01` -> `P/05/01` verified placeable state-registry audit:
  production state machinery now exposes exact `A/09` useable/trap/lock state
  and exact `U/09` lock-state facts through `LiveObjectRecordMention`,
  semantic live-object events, and `ObjectRegistry` merge state. The `U/09`
  extractor requires the decompile-backed neutral sixth EE door/placeable state
  BOOL before publishing those facts. No emitted packet bytes changed. Verified
  with isolated-target `cargo test -q -p hgbridge-proxy2
  exact_placeable_add_and_update_mentions_expose_state_bits -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2
  verified_placeable_state_merges_add_and_update_facts -- --nocapture`,
  `cargo check -q -p hgbridge-proxy2`, and `cargo fmt --all --check`. Next
  verification remains local visual replay comparing semantic add/update state
  against area static context before choosing add synthesis or update
  translation changes.
- 2026-06-10 `P/04/01` -> `P/05/01` semantic static/live placeable conflict
  audit: no emitted packet bytes changed. Server M-frame semantic observation
  now passes the latest `AreaPlaceableContext` into the verified live-object
  reducer, and `ObjectRegistry` records per-placeable area/static overlaps,
  latest module-backed state conflict fields, and conflict counts after exact
  `A/09`/`U/09` state facts are merged. The match uses the shared compact vs
  EE external object-id equivalence rule, so a compact area static row and the
  corresponding EE `0x800000NN` live object are treated as the same protocol
  identity. Verified with isolated-target `cargo test -q -p hgbridge-proxy2
  area_context_conflicts_use_merged_verified_placeable_state -- --nocapture`
  and `cargo check -q -p hgbridge-proxy2`. Next verification remains local
  visual replay/harness logging to determine whether observed static/live drift
  is an `A/09` add-state synthesis problem or a later `U/09` update-state
  translation problem.
- 2026-06-10 follow-up `P/04/01` -> `P/05/01` unresolved static/live
  placeable state audit: no emitted packet bytes changed. `ObjectRegistry` now
  separates historical area/static conflict observations from currently
  unresolved conflicts, clears unresolved state on delete or a later verified
  matching `A/09`/`U/09` state, and exposes a compact-vs-EE-external-id aware
  lookup for future add/update rewrite policy. The live-object M-frame claim
  path now logs when a translated record still intersects an unresolved
  module-backed static conflict. Verified with isolated-target `cargo test -q
  -p hgbridge-proxy2 area_context_conflicts_use_merged_verified_placeable_state
  -- --nocapture`, `cargo check -q -p hgbridge-proxy2`, and `cargo fmt --all
  --check`. Next production path remains local visual replay using these
  unresolved-vs-resolved signals to decide whether a generalized fix belongs in
  `A/09` add-state synthesis or `U/09` update-state translation.
- 2026-06-10 follow-up `P/04/01` -> `P/05/01` initial placeable add-state
  synthesis: emitted `A/09` state bits can now be reconciled with a uniquely
  matched, module-backed static area row when the source add conflicts on
  useable, trap-disarmable, lockable, or locked state. The override is limited
  to the existing decompile-backed add-state BOOL writer, requires a static row
  with unique object-id confidence, refuses light/static aliases or duplicate
  static ids, and leaves `U/09` runtime update state diagnostic-only. Exact EE
  add validation still owns the final cursor. Verified with isolated-target
  `cargo test -q -p hgbridge-proxy2 placeable_add_rewrite_ -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 placeable_context_ -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2
  placeable_update_area_context_helpers_keep_identity_and_lock_proof --
  --nocapture`, `cargo check -q -p hgbridge-proxy2`, `cargo fmt --all
  --check`, and `git diff --check`. Next verification should run local visual
  replay against a confirmed bad static/live placeable to decide whether any
  remaining unresolved state belongs to later `U/09` update-state translation.
- 2026-06-10 follow-up `P/04/01` -> `P/05/01` placeable update-lock synthesis:
  emitted `U/09` lockable/locked state bits can now be reconciled with a
  uniquely matched, module-backed static area row. The override is limited to
  the decompile-backed EE door/placeable update state block (`position`,
  optional orientation, five Diamond state BOOLs, then EE's neutral sixth
  BOOL), changes only same-width lock BOOLs after exact cursor proof, applies
  both to already exact `U/09` rows and to rows that just passed the legacy
  update writer/validator path, and refuses light/static aliases or duplicate
  static ids. `U/09` still cannot synthesize useable or trap-disarmable state
  because those bits are not owned by the update record. Verified with
  isolated-target `cargo test -q -p hgbridge-proxy2 exact_placeable_update_
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  placeable_update_area_context_helpers_keep_identity_and_lock_proof --
  --nocapture`, `cargo check -q -p hgbridge-proxy2`, and `cargo fmt --all
  --check`. Next local visual replay should focus on any remaining
  static/live drift outside unique lock state: non-unique area identity,
  add-only use/trap bits, appearance/model, or orientation.
- 2026-06-10 follow-up `P/04/01` -> `P/05/01` placeable state-registry alias
  audit: semantic object registry conflict tracking now merges verified
  placeable live-object mentions by the same compact-vs-EE-external id
  equivalence used by the translator. An external `A/09` conflict and later
  compact `U/09` resolution now update the same active placeable state instead
  of creating parallel registry entries, so unresolved static/live diagnostics
  reflect the decompile-backed `AddExternalObject` identity rule. Verified with
  isolated-target `cargo test -q -p hgbridge-proxy2
  placeable_area_conflicts_resolve_across_compact_external_aliases --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  area_context_conflicts_use_merged_verified_placeable_state -- --nocapture`,
  `cargo check -q -p hgbridge-proxy2`, and `cargo fmt --all --check`. Next
  local visual replay should use this alias-coherent unresolved/resolved signal
  before adding any more add/update state synthesis.
- 2026-06-11 `P/04/01` -> `P/05/01` exact placeable add-state synthesis:
  exact EE-shaped `A/09` rows now share the unique module-backed static state
  reconciliation used by legacy add rewrites and exact `U/09` lock updates.
  The rewrite changes only the same-width add-state BOOLs that the decompiled
  placeable add reader owns (`useable`, `trap_disarmable`, `lockable`,
  `locked`), preserves static/plot and unknown sibling bits, and refuses
  duplicate or light/static-alias area identities. Verified with focused
  `exact_placeable_add`, `exact_placeable_update`, and `placeable_add_rewrite_`
  regressions plus `cargo fmt --all --check` and `git diff --check`.
- 2026-06-11 follow-up exact `A/09` add-layout audit: the exact placeable add
  byte layout is now a shared typed helper used by add validation, add cursor
  advancement, and area/static state reconciliation. The optional OBJECTID
  branch is tied to the same fragment BOOL that advances the cursor before the
  EE visual-transform identity map, so static-state reconciliation cannot use a
  different byte boundary than the exact validator. Verified with focused
  `exact_placeable_add` and `exact_placeable_update` regressions; next visual
  replay should still compare remaining static/live drift outside unique
  add-state and update-lock bits.
- 2026-06-11 follow-up exact `U/09` update state-cursor audit: the verified EE
  door/placeable update parser now exposes the state-bit cursor it consumed,
  and exact placeable update lock reconciliation uses that parser-owned cursor
  instead of independently rewalking position/orientation bits from the mask.
  Added vector-orientation coverage proving unique module-backed lock synthesis
  changes only the verified state block and leaves the vector selector, visual
  payload, neutral EE state suffix, and following bits untouched. Verified with
  focused `exact_placeable_update` and `placeable_update` regressions; next
  visual replay should still compare any remaining static/live drift outside
  unique add-state and update-lock bits.
- 2026-06-12 follow-up exact `A/09`/`U/09` state-cursor sharing: packet bytes
  and cursor ownership are unchanged. Exact placeable add validation,
  add-cursor advancement, placeable state mentions, and add-state
  reconciliation now share one typed fragment-layout helper that ties the
  post-name state cursor to the optional-OBJECTID guard and the neutral EE
  visual-transform guard. Exact placeable update mentions now reuse the
  verified EE parser's state cursor instead of independently rewalking
  position/orientation bits. Verified with focused add-layout/vector-update
  mention tests, `exact_placeable`, `placeable_add`, `placeable_update`, `cargo
  fmt --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`; next visual replay should still compare remaining
  static/live drift outside unique add-state and update-lock bits.
- 2026-06-12 follow-up exact `A/09` add-state claim: packet bytes and cursor
  ownership are unchanged. Add-state mention extraction and unique
  module-backed static reconciliation now share a typed exact-layout claim
  carrying the verified post-name state cursor, optional-OBJECTID guard,
  next cursor, and visual-transform map offset, so later diagnostics cannot
  drift back to a bare cursor rewalk. Verified with focused `exact_placeable`
  add/update and `placeable_add_rewrite_`/`placeable_update` regressions; next
  production path remains local visual replay for remaining placeable
  appearance/orientation/model drift.
- 2026-06-12 follow-up exact placeable appearance mention state: packet bytes
  and cursor ownership are unchanged. Verified `A/09` mentions now expose the
  parser-owned add-tail appearance WORD, and verified `U/09` mentions reuse the
  EE update parser's appearance byte offset, including the optional CResRef
  branch, before the semantic registry stores/clears that placeable appearance
  fact with object lifecycle state. Verified with focused appearance/state
  mention tests, the semantic registry appearance test, `exact_placeable`,
  `cargo fmt --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`; next production path remains local visual replay for
  remaining placeable orientation/model drift.
- 2026-06-12 follow-up exact static/live placeable appearance synthesis:
  emitted exact `A/09` add and `U/09` update appearance WORDs can now be
  reconciled with the uniquely matched, module-backed static area row that
  already authorizes state synthesis. The rewrite is same-width only, uses the
  verified add layout or EE update parser-owned appearance offset, refuses
  non-unique/light-static aliases and dynamic `0xFFFE+`/CResRef update rows, and
  reclaims the final exact payload after mutation. Verified with focused
  `exact_placeable_`, `placeable_context_`, and `placeable_update` regressions,
  `cargo fmt --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`; next local replay should focus on any remaining static/live
  drift outside same-width appearance and proven state bits, especially
  orientation.
- 2026-06-12 follow-up exact static/live placeable scalar-orientation
  synthesis: the verified EE door/placeable update parser now retains the
  parser-owned scalar orientation byte/bit cursor. Exact scalar `U/09` rows can
  reconcile that same-width 12-bit scalar with the uniquely matched,
  module-backed static row's decompiled direction bearing; vector-orientation
  rows remain diagnostic-only because they do not expose the same-width scalar
  field. Verified with focused `exact_placeable_` and orientation encoder
  regressions plus `cargo fmt --all --check`, `git diff --check`, and `cargo
  check -q -p hgbridge-proxy2`; next local replay should focus on any remaining
  placeable drift outside exact scalar orientation, same-width appearance, and
  proven state bits.
- 2026-06-12 follow-up exact `U/09` mention-claim sharing: packet bytes and
  cursor ownership are unchanged. Verified door/placeable update mention
  extraction now builds one exact EE parser claim per row and shares it across
  scalar orientation, placeable appearance, and placeable state facts, so the
  semantic registry cannot report those fields from three independent cursor
  walks. Verified with focused exact scalar/placeable update mention coverage
  plus `exact_placeable_` and `placeable_update` regressions; next local replay
  should still focus on remaining drift outside exact scalar orientation,
  same-width appearance, and proven state bits.
- 2026-06-12 follow-up exact `U/09` reconciliation-claim sharing: packet bytes
  and cursor ownership are unchanged. The exact postpass for module-backed
  placeable update reconciliation now parses one verified EE U/09 claim per row
  and shares it across same-width appearance, scalar orientation, and lock-state
  reconciliation instead of rewalking the row separately for each field.
  Verified with a combined exact-placeable update regression plus focused
  `exact_placeable_update_` coverage; next local replay should still focus on
  remaining drift outside exact scalar orientation, same-width appearance, and
  proven state bits.
- 2026-06-12 follow-up semantic orientation conflict state: packet bytes and
  cursor ownership are unchanged. The semantic object registry now compares
  verified scalar placeable orientation mentions against the uniquely matched,
  module-backed static area row's decompile-backed scalar bearing, tracks
  unresolved/resolved orientation conflicts separately from use/trap/lock state,
  and includes the orientation mismatch in the server-dispatch unresolved
  placeable-conflict diagnostic. Verified with focused `area_context_`,
  `placeable_area_conflicts_resolve_across_compact_external_aliases`,
  `exact_placeable_`, `placeable_update`, and `cargo check -q -p
  hgbridge-proxy2`; next local replay should use the combined state/orientation
  diagnostic to identify any remaining drift outside same-width scalar
  orientation, appearance, and proven state bits.
- 2026-06-12 follow-up exact vector orientation diagnostics: packet bytes and
  reconciliation behavior are unchanged. The verified EE door/placeable update
  parser now retains vector-branch read offset, bit cursor, and decoded
  `ReadFLOAT(-2,2,16)` components; exact `U/09` mentions expose a vector-sourced
  scalar-equivalent yaw to the semantic registry, so static/live orientation
  conflicts can be tracked or resolved for vector rows without pretending the
  row has a same-width scalar rewrite field. Verified with isolated-target
  focused exact-vector update and semantic orientation-conflict regressions plus
  `cargo check -q -p hgbridge-proxy2`; next local replay should compare any
  remaining placeable drift against scalar-vs-vector orientation source labels.
- 2026-06-12 follow-up vector orientation semantic payload: packet bytes and
  reconciliation behavior are unchanged. Exact live-object orientation mentions
  now carry parser-owned vector x/y/z components through the semantic reducer,
  and module-backed static orientation conflicts record whether the observed row
  came from the scalar or vector branch. This keeps replay diagnostics tied to
  the exact EE `U/09` parser while leaving vector-byte rewrite policy blocked on
  harness evidence. Verified with isolated-target focused exact-vector update
  and semantic orientation-conflict regressions, `cargo fmt --all --check`,
  `git diff --check`, and `cargo check -q -p hgbridge-proxy2`; next local
  replay should compare vector components, source labels, and module static
  direction before adding any vector rewrite rule.
- 2026-06-12 follow-up exact `A/09` add-claim sharing: packet bytes and cursor
  ownership are unchanged. Exact placeable add mention extraction and
  module-backed static add reconciliation now share one verified EE add claim
  carrying object id, layout, parser-owned appearance WORD, and add-state bits,
  so optional-object/visual-map cursor proof cannot diverge between appearance
  and state paths. Verified with focused exact add/update mention and add
  appearance/state reconciliation regressions plus `cargo fmt --all`,
  `git diff --check`, and `cargo check -q -p hgbridge-proxy2`; next local
  replay should still focus on remaining drift outside exact scalar/vector
  orientation diagnostics, same-width appearance, and proven state bits.
- 2026-06-12 follow-up unresolved placeable conflict diagnostics: packet bytes
  and reconciliation behavior are unchanged. The semantic object registry now
  exposes the active compact/external alias that owns an unresolved
  area/static state or orientation conflict, and server-dispatch replay
  diagnostics log that prior registry appearance/state/orientation beside the
  exact current live-object record offsets, fragment span, appearance, state,
  and scalar/vector orientation. Verified with focused alias-conflict
  regressions, `cargo fmt --all --check`, `git diff --check`, and
  `cargo check -q -p hgbridge-proxy2`; next local replay should compare these
  current-vs-prior fields before adding any vector rewrite rule.
- 2026-06-12 follow-up exact static/live placeable vector-orientation
  synthesis: exact `U/09` vector-orientation rows now reconcile the six
  parser-owned `ReadFLOAT(-2,2,16)` bytes with the uniquely matched,
  module-backed static row direction while preserving the vector selector bit,
  record length, fragment cursor, appearance/state offsets, and final exact EE
  claim. Scalar rows still use the existing same-width 12-bit scalar rewrite,
  and vector rewrite refuses rows without a normalized static direction. Verified
  with focused vector/exact-placeable/placeable-update regressions,
  `cargo fmt --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`; next local replay should compare remaining drift outside
  exact vector/scalar orientation, same-width appearance, and proven state bits.
- 2026-06-12 follow-up unresolved placeable appearance diagnostics: packet
  bytes and reconciliation behavior are unchanged. The semantic object registry
  now tracks verified live-object placeable appearance conflicts against the
  uniquely matched, module-backed static row appearance, carries unresolved
  conflicts across compact/external id aliases, clears them on delete, and logs
  appearance mismatches beside state/orientation in the live-object replay
  diagnostic. Verified with focused appearance/state/orientation conflict
  regressions, `cargo fmt --all --check`, `git diff --check`, and `cargo check
  -q -p hgbridge-proxy2`; next local replay should classify any remaining
  placeable drift as appearance, state, orientation, identity, or an unmodeled
  packet family before adding another rewrite rule.
- 2026-06-12 follow-up placeable identity-conflict diagnostics: packet bytes
  and reconciliation behavior are unchanged. Area/static overlaps that are not
  exactly one unique module-backed static row now produce a typed semantic
  identity conflict, survive compact/external id aliases, clear on delete or
  later unique proof, and appear in server-dispatch unresolved diagnostics
  beside appearance/state/orientation. Verified with incremental disabled
  because the local rustc incremental cache ICEs, using focused
  `area_context_tracks_placeable_identity_conflicts`, `area_context_`, and
  `cargo check -q`; next local replay should classify remaining placeable drift
  as identity versus appearance/state/orientation before changing writer bytes.
- 2026-06-12 follow-up placeable conflict snapshot diagnostics: packet bytes
  and reconciliation behavior are unchanged. The semantic registry now exposes
  one typed unresolved area/static placeable conflict snapshot per live-object
  record, preserving compact/external alias owner state and classifying the
  active mismatch as identity, appearance, state, and/or orientation for
  server-dispatch replay logs. Verified with incremental disabled using focused
  `area_context_tracks_`, `area_context_conflicts_use_merged_verified_placeable_state`,
  `cargo check -q`, `cargo fmt --all --check`, and `git diff --check`; next
  local replay should use the conflict class field to decide whether any
  remaining placeable drift needs a writer change or more decompile proof.
- 2026-06-12 follow-up static/live placeable reconciliation target: packet
  bytes and reconciliation policy are unchanged. Area placeable overlap now
  exposes a typed unique-module-backed-static versus identity-blocked target,
  and the exact `A/09`/`U/09` reconciliation pass uses that shared decision for
  appearance, state, scalar orientation, and vector orientation while logging
  identity-blocked skips. The pass now also reports exact add/update records
  examined separately from records rewritten, so replay diagnostics can tell
  whether a remaining conflict survived an inspected add row, update row, or an
  unmodeled identity case. Verified with incremental disabled using focused
  `placeable_context_overlap_formats_rows_and_checks_static_module_state`,
  `exact_placeable_`, `area_context_tracks_`, `cargo check -q`, `cargo fmt
  --all --check`, and `git diff --check`; next local replay should compare the
  new examined/rewrite counters with unresolved conflict classes before adding
  another writer rule.
- 2026-06-24 follow-up static/live placeable row provenance: packet bytes and
  reconciliation policy are unchanged. `AreaPlaceableContextRow` now carries
  the decompile-owned read-buffer span and inherited fragment-bit cursor for
  light/static placeable rows, and overlap diagnostics print those spans beside
  appearance/state/orientation context. This keeps future visual replay
  classification tied to exact row ownership instead of semantic-looking
  module matches. Verified with focused area context/placeable-update
  regressions, formatter, diff-check, and `cargo check -q -p hgbridge-proxy2`;
  next local replay should compare remaining placeable drift against the row
  `read`/`bits` spans before changing writer bytes.
- 2026-06-24 follow-up static/live placeable source-provenance gate: explicit
  `P/04` row spans now gate `P/05` exact static-row reconciliation. Light and
  static placeable rows remain read-buffer-only; an explicit malformed read
  width or nonzero fragment-bit delta makes that row unproven/identity-blocking
  instead of module-backed static evidence, while fixture-only zero spans remain
  accepted. Verified with isolated-target
  `explicit_placeable_row_provenance_gates_static_reconciliation`,
  `exact_placeable_update_position_ignores_invalid_area_row_source_provenance`,
  focused `placeable_context_`, focused `exact_placeable_update_position`,
  `cargo check -q -p hgbridge-proxy2`, formatter, and diff-check. Next replay
  should compare any remaining static/live placeable drift against rejected row
  provenance before adding a writer rule.
- 2026-06-24 follow-up static/live placeable source-provenance diagnostics:
  packet bytes and reconciliation policy are unchanged. The same `P/04` row
  gate now reports typed rejection evidence: row diagnostics print `source=ok`,
  `source=read-width=actual/expected`, or `source=fragment-bits=start..end`,
  and identity conflicts count module-unbacked, source-incompatible,
  read-width-mismatched, and fragment-owning static rows separately. Verified
  with focused provenance, placeable-context, exact `U/09`, semantic identity
  conflict, formatter, diff-check, and `cargo check -q -p hgbridge-proxy2`.
  Next replay should use those counters to decide whether any remaining
  static/live drift is a malformed `P/04` source row or a real `P/05` writer
  gap.
- 2026-06-24 follow-up static/live placeable base-identity provenance
  blockers: packet bytes and reconciliation policy are unchanged. Exact
  `P/05` placeable reconciliation now records the pre-resolution/base
  identity-blocked `P/04` source blockers into `LiveObjectUpdateRewriteSummary`
  for add and update rows, and emits them in a separate bridge debug summary so
  a malformed static row remains visible even when position or fixed-field
  proof selects a compatible row. Verified with focused exact `U/09`
  provenance and area row provenance tests, formatter, diff-check, and
  `cargo check -q -p hgbridge-proxy2`. Next replay should compare these base
  source blocker totals with final target/rewrite counters before adding any
  new `P/05` writer rule.
- 2026-06-24 follow-up static/live placeable source-blocked rewrite
  correlation: packet bytes and reconciliation policy are unchanged. Exact
  `P/05` placeable reconciliation now counts add/update field rewrites that
  were accepted only after a source-incompatible base `P/04` identity block was
  overridden by stronger `P/05` proof. The bridge debug summary now reports
  source-blocked add appearance/state rewrites and update position/appearance/
  orientation/state rewrites beside the base provenance blockers. Verified with
  the malformed-provenance exact `U/09` regression, formatter, diff-check, and
  `cargo check -q -p hgbridge-proxy2`. Next replay should use these paired
  counters to separate malformed area-row provenance from a real `P/05` writer
  gap before adding a new writer rule.
- 2026-06-24 follow-up static/live placeable source-blocked selection
  correlation: packet bytes and reconciliation policy are unchanged. Exact
  `P/05` placeable reconciliation now separately counts source-blocked base
  identity targets resolved by later fixed-field, position, add-output, and
  position-output-equivalent proof, so replay can pair malformed `P/04` row
  provenance with the specific `P/05` proof path that selected the compatible
  static row. Verified with the malformed-provenance exact `U/09` regression,
  formatter, diff-check, and `cargo check -q -p hgbridge-proxy2`. Next replay
  should compare source-blocked selected-target buckets against field rewrite
  buckets before implementing any new writer rule.
- 2026-06-24 follow-up static/live placeable source-blocked field
  correlation: packet bytes and reconciliation policy are unchanged. Exact
  `P/05` placeable reconciliation now reports row-level source-blocked selected
  targets with field rewrites separately from selected targets whose emitted
  fields were already byte/bit equivalent, so replay no longer has to infer
  that state by subtracting raw field rewrite counts from proof-path selection
  counts. Verified with the malformed-provenance exact `U/09` regression,
  focused `exact_placeable_`, diff-check, and `cargo check -q -p
  hgbridge-proxy2`. Next replay should use the new
  `*_field_rewrite_targets` versus `*_field_unchanged_targets` buckets to
  decide whether remaining source-blocked rows still need a writer rule.
- 2026-06-24 follow-up static/live placeable source-blocked provenance-target
  classification: packet bytes and reconciliation policy are unchanged. Exact
  `P/05` placeable reconciliation now pairs every source-blocked selected
  target with the `P/04` blocker class that made the base identity untrusted:
  malformed read-width provenance, fragment-owned row bits, or both. The bridge
  summary exposes add/update `*_source_blocked_read_mismatch_targets` and
  `*_source_blocked_fragment_owned_targets`, so replay can distinguish
  malformed area-row ownership from a live-object writer gap without deriving
  it from broader conflict counters. Verified with the malformed-provenance
  exact `U/09` regression; next replay should compare these blocker-class
  target buckets against the field-rewrite/unchanged buckets before adding a
  new `P/05` writer rule.
- 2026-06-24 follow-up static/live placeable source-blocked field-class
  correlation: packet bytes and reconciliation policy are unchanged. Exact
  `P/05` placeable reconciliation now emits read-width-mismatch and
  fragment-owned source-blocked targets split again by whether the selected
  `P/05` add/update row rewrote fields or was byte/bit equivalent. This ties the
  malformed `P/04` row ownership class to the actual writer outcome for the
  same selected target, instead of requiring replay to subtract unrelated
  aggregate counters. Verified with the malformed-provenance exact `U/09`
  regression, the helper-level field-correlation regression, and focused
  `exact_placeable_`; next replay can compare the class-specific rewrite versus
  unchanged buckets before adding a new `P/05` writer rule.
- 2026-06-24 follow-up static/live placeable combined source-blocker
  classification: packet bytes and reconciliation policy are unchanged. `P/04`
  placeable identity conflicts now count static rows whose explicit source
  provenance has both a malformed read-buffer width and owned fragment bits;
  `P/05` reconciliation summary/debug counters carry that row-level combined
  blocker through selected-target and rewrite/unchanged outcome buckets. This
  distinguishes one malformed area row from separate read-width and fragment
  blockers before any writer rule is added. Verified with focused area
  provenance, malformed `U/09`, and source-blocked field-correlation tests;
  next replay should compare combined-class counts against remaining source-
  blocked field rewrites before changing emitted packet behavior.
- 2026-06-24 follow-up static/live placeable custom-carrier source blockers:
  packet bytes and reconciliation policy are unchanged. Exact `A/09`
  fixed-width custom-carrier blockers now retain the same `P/04` source
  provenance class as base identity blockers: source-blocked, malformed
  read-width, fragment-owned, and combined malformed+fragment-owned. The
  live-object and M-frame debug summaries expose those buckets when a custom
  carrier is skipped because only fixed A/09 output is proven. Verified with
  focused fixed-output custom-carrier and source-blocked field-correlation
  tests; next replay should compare these carrier-source buckets against
  remaining source-blocked field rewrites before adding a custom carrier writer
  rule.
- 2026-06-24 follow-up static/live placeable custom-carrier field outcomes:
  packet bytes and reconciliation policy are unchanged. The source-blocked
  fixed-output custom-carrier skip now records whether the same selected A/09
  row rewrote fields or was field-unchanged, split by malformed read-width,
  fragment-owned, and combined source-provenance classes. Direct live-object,
  M-frame, and server-dispatch summaries carry the new buckets. Verified with
  focused helper, fixed-output carrier, exact-placeable, formatter,
  diff-check, and `cargo check -q -p hgbridge-proxy2`; next replay should
  compare source-blocked custom-carrier `field_rewrite` versus
  `field_unchanged` before adding a custom carrier writer rule.
- 2026-06-24 follow-up static/live placeable source-blocker model: packet
  bytes and reconciliation policy are unchanged. `P/04` static-placeable
  identity conflicts now expose a typed source-provenance blocker summary, and
  exact `P/05` add/update plus fixed-width custom-carrier counters consume that
  shared model instead of re-reading raw conflict fields. Verified with focused
  explicit P/04 provenance and live-object `source_blocked` tests; next replay
  should use the same blocker summary when deciding whether a custom-carrier
  skip is malformed P/04 ownership evidence or a real P/05 writer gap.
- 2026-06-24 follow-up static/live placeable custom-carrier disposition:
  packet bytes and reconciliation policy are unchanged. Shared live-object and
  M-frame summaries now expose fixed-width custom-carrier disposition counters:
  source-owned skips with malformed `P/04` provenance and unchanged `A/09`
  fields versus residual writer-gap candidates. Verified with focused
  source-blocker helper and fixed-output custom-carrier rewrite tests; next
  replay should inspect `writer_gap_candidates` before widening any custom
  carrier writer rule.
- 2026-06-24 follow-up static/live placeable custom-carrier writer-gap slots:
  packet bytes and reconciliation policy are unchanged. Residual fixed-width
  custom-carrier writer-gap candidates now carry selected carrier-slot counters
  for following normal/custom `U/09`, pre-add normal/custom `U/09`, and add-only
  gaps after malformed source-owned unchanged skips are excluded. Verified with
  focused writer-gap slot, source-blocker disposition, and fixed-output carrier
  tests; next replay should compare slot counters before adding any new
  `U/09` custom-carrier writer rule.
- 2026-06-24 follow-up static/live placeable custom-carrier source-blocked
  writer-gap slots: packet bytes and reconciliation policy are unchanged.
  Residual fixed-width custom-carrier writer-gap candidates now keep a row-level
  split for source-blocked field-rewrite slots versus source-unblocked slots,
  across following normal/custom `U/09`, pre-add normal/custom `U/09`, and
  add-only gaps. Direct live-object, M-frame, and server-dispatch traces expose
  the source-blocked slot counters, so the next replay can distinguish malformed
  `P/04` source provenance plus changed `A/09` fields from an unblocked `P/05`
  custom-carrier writer gap before widening any `U/09` custom-carrier rule.
- 2026-06-24 follow-up static/live placeable custom-carrier writer-gap model:
  packet bytes and reconciliation policy are unchanged. Direct live-object and
  M-frame summaries now derive a typed writer-gap slot summary with all,
  source-blocked, and source-unblocked buckets for following normal/custom
  `U/09`, pre-add normal/custom `U/09`, and add-only gaps. Direct and dispatch
  traces emit the explicit source-unblocked slots, so the next replay can
  compare unblocked slot pressure without subtracting aggregate counters before
  widening any custom-carrier writer rule.
- 2026-06-24 follow-up static/live placeable custom-carrier selected-slot
  model: packet bytes and reconciliation policy are unchanged. The direct
  `A/09` unproven custom-carrier skip path and residual writer-gap counters now
  use one typed selected-slot classifier for following normal/custom `U/09`,
  pre-add normal/custom `U/09`, and add-only rows, and the row trace emits the
  selected slot. Next replay should use the row-level slot plus source-blocked
  and source-unblocked aggregate buckets before changing any `U/09`
  custom-carrier writer behavior.
- 2026-06-24 follow-up static/live placeable custom-carrier source disposition:
  packet bytes and reconciliation policy are unchanged. Residual writer-gap
  recording now uses a typed source-disposition classifier instead of a loose
  boolean, splitting source-unblocked gaps, source-blocked rows with rewritten
  `A/09` fields, and source-blocked rows whose fields stayed unchanged. The
  last class remains suppressed as source-owned malformed `P/04` evidence, and
  row traces emit the chosen source disposition before any `U/09`
  custom-carrier writer rule is widened.
- 2026-06-24 follow-up static/live placeable custom-carrier source-owned slots:
  packet bytes and reconciliation policy are unchanged. Direct live-object and
  M-frame summaries now derive source-owned/field-unchanged selected-slot
  buckets beside writer-gap, source-blocked-field-rewrite, and source-unblocked
  buckets. Next replay can compare suppressed malformed `P/04` slot pressure
  against real `P/05` custom-carrier writer-gap slots before widening any
  `U/09` custom-carrier writer behavior.
- 2026-06-24 follow-up static/live placeable unproven-carrier row evidence:
  packet bytes and reconciliation policy are unchanged. The exact `A/09`
  custom-carrier skip path now records each unproven row through one typed
  selected-slot plus source-disposition evidence object, so skipped slots,
  source-owned rows, and writer-gap slots cannot drift while the writer rule
  remains gated. Next replay should inspect these row dispositions before
  widening any `U/09` custom-carrier emission.
- 2026-06-25 follow-up static/live placeable unproven-carrier row recorder:
  packet bytes and reconciliation policy are unchanged. The same typed row
  evidence object now owns the aggregate skipped-row count, malformed `P/04`
  source-provenance blocker counters, source field-outcome correlation, and
  selected-slot writer-gap counters. Next replay should compare row-level
  source disposition plus slot pressure before changing `U/09` custom-carrier
  emission.
- 2026-06-25 follow-up static/live placeable source-blocker slot matrix:
  packet bytes and reconciliation policy are unchanged. The shared `P/04`
  source-provenance blocker predicate now treats the combined read-width plus
  fragment-owned class as a blocker even when helper evidence carries only that
  combined class, and exact `A/09` unproven custom-carrier summaries now retain
  source-blocker class slots for source-blocked, read-mismatch, fragment-owned,
  and combined rows. Direct and M-frame diagnostics expose the typed matrix, so
  the next replay can compare slot pressure by malformed `P/04` class before
  widening any `U/09` custom-carrier writer behavior.
- 2026-06-25 follow-up static/live placeable source-unblocked writer-gap
  synthesis: source-unblocked fixed-width `A/09` unproven-carrier rows now feed
  the existing add-boundary synthetic `U/09` carrier path when the selected
  module row has a concrete `TemplateResRef` and the fixed-output proof has no
  divergent carrier output. Source-owned, source-blocked, missing-template, and
  divergent-output rows remain gated and reported through the typed synthesis
  gate. Next replay should inspect remaining blocked-source-provenance and
  divergent-output gate pressure before widening source-blocked carriers.
- 2026-06-25 follow-up static/live placeable unproven-carrier synthesis gate
  slots: packet bytes and synthesis policy are unchanged. The typed `A/09`
  unproven-carrier synthesis gate now records selected-slot matrices for
  eligible source-unblocked rows, source-owned blockers, source-provenance
  blockers, missing `TemplateResRef`, and divergent output. Direct live-object
  and M-frame/server-dispatch traces expose the matrix plus active
  source-provenance/divergent slot totals, so the next replay can compare the
  remaining blocked gates before widening any source-blocked carrier emission.
- 2026-06-25 follow-up static/live placeable source-provenance gate classes:
  packet bytes and synthesis policy are unchanged. The `A/09` unproven-carrier
  synthesis gate now splits blocked-source-provenance slots by malformed `P/04`
  read-width, fragment-owned, and combined blocker classes, and row traces emit
  the same class flags. Next replay should compare these class slots before
  deciding whether any source-blocked field-rewrite carrier can safely emit a
  synthetic `U/09` custom branch.
- 2026-06-25 follow-up static/live placeable source-trusted carrier synthesis:
  source-provenance-blocked `A/09` unproven-carrier rows now synthesize the
  add-boundary custom `U/09` carrier when the add has an in-place field rewrite
  and the selected row passes the source-trusted gate: concrete
  `TemplateResRef`, no divergent fixed-output carrier. Malformed `P/04` source
  blockers are still counted; missing-template and divergent-output rows remain
  blocked. Next replay should inspect any remaining blocked-source-provenance
  pressure by selected carrier slot.
- 2026-06-25 follow-up static/live placeable residual source-provenance gates:
  packet bytes and synthesis policy are unchanged. The `A/09` unproven-carrier
  gate now derives residual blocked-source-provenance slots after subtracting
  source-trusted eligible rows, so direct and M-frame diagnostics distinguish
  remaining missing-template/divergent-output blockers from already-synthesized
  source-trusted rows. Next replay should target only nonzero residual slots.
- 2026-06-25 follow-up static/live placeable residual source-provenance reasons:
  packet bytes and synthesis policy are unchanged. The `A/09` unproven-carrier
  residual source-provenance gate is now a typed summary that carries
  missing-`TemplateResRef` and divergent-output slots separately through direct
  and M-frame diagnostics, with the legacy aggregate preserved for compatibility.
  Next replay should target whichever residual reason has nonzero slots before
  any further `U/09` custom-carrier emission rule is widened.
- 2026-06-25 follow-up static/live placeable residual source-provenance blocker
  classes: packet bytes and synthesis policy are unchanged. Residual
  missing-`TemplateResRef` and divergent-output gates now retain the malformed
  `P/04` source-provenance class that produced them: read-width mismatch,
  fragment-owned, or the combined class. Direct live-object and
  M-frame/server-dispatch diagnostics expose these class slots so the next
  replay can distinguish malformed area-row ownership from real custom-carrier
  output ambiguity before widening any `U/09` carrier emission.
- 2026-06-25 follow-up pre-add custom carrier source resrefs: fixed-width
  `A/09` unproven-carrier rows with missing module `TemplateResRef` now
  synthesize the add-boundary custom `U/09` carrier only when the selected
  same-lifecycle pre-add custom `U/09` supplies a parser-owned source resref
  whose appearance matches the fixed-width add. Following custom rows still
  remain later packet-authored state, not fallback proof. Next replay should
  inspect any remaining residual missing-template or divergent-output slots by
  selected carrier scope before widening source-carried resrefs beyond this
  pre-add custom case.
- 2026-06-25 follow-up source-carried carrier gate: packet bytes and synthesis
  output are unchanged. The same pre-add custom source-carried `TemplateResRef`
  rule is now a first-class synthesis gate instead of a residual
  missing-template blocker, and source-provenance-blocked rows separately report
  the source-carried subslot while retaining malformed `P/04` blocker classes.
  Next replay should compare remaining true missing-template/divergent slots
  after source-carried rows are excluded.
- 2026-06-25 follow-up source-carried residual resolution: packet bytes and
  synthesis output are unchanged. The `A/09` unproven-carrier synthesis
  diagnostics now derive a shared resolution summary that splits source-carried
  synthesized slots, source-provenance synthesized slots, and the remaining
  missing-template/divergent residual slots. Direct live-object and
  M-frame/server-dispatch traces consume the same summary, so the next replay
  should use the source-carried synthesis-resolution event before widening any
  residual carrier rule.
- 2026-06-25 follow-up source-carried divergent carrier guard:
  source-carried pre-add custom `TemplateResRef` synthesis now compares the
  effective source-carried add output against concrete module carrier outputs
  in the same fixed-output proof set. Divergent source/module carriers remain
  blocked as divergent output even when the selected row lacks a module
  `TemplateResRef`; next replay should inspect remaining residual
  missing-template/divergent slots after excluding these guarded source-carried
  conflicts.
- 2026-06-25 follow-up source-carried divergent slot split: packet bytes and
  synthesis policy are unchanged. Source-carried pre-add custom
  `TemplateResRef` rows blocked by divergent module output now have separate
  gate and synthesis-resolution slot counters, including source-provenance
  blocked rows. Direct live-object and M-frame/server-dispatch diagnostics
  preserve the broad divergent counters while exposing the source-carried
  divergent sub-bucket. Next replay should compare that sub-bucket against the
  remaining residual missing-template/divergent slots before changing carrier
  emission.
- 2026-06-25 follow-up source-carried divergent residual split: packet bytes
  and synthesis policy are unchanged. The shared unproven-carrier synthesis
  resolution now splits source-carried divergent rows into source-unblocked and
  source-provenance components, then derives residual source-provenance slots
  after subtracting source-carried divergence. Direct live-object and
  M-frame/server-dispatch diagnostics expose the post-subtraction residual, so
  the next replay should target only true missing-template or non-source-carried
  divergent output before widening carrier emission.
- 2026-06-25 follow-up mixed source-carried carrier selection: fixed-width
  `A/09` rows missing module `TemplateResRef` now let a selected pre-add custom
  `U/09` source-carried resref drive add-boundary custom-carrier synthesis even
  when a following normal `U/09` remains later packet-authored state. This
  changes packet output for that generalized mixed-carrier shape by inserting
  the add-boundary custom `U/09` from the verified add cursor, while leaving
  the later normal update shifted but otherwise packet-authored. Verified with
  `exact_placeable_add_pre_add_source_carrier_survives_following_normal_update`;
  next replay should inspect only residual cases with no selected pre-add custom
  source resref or with concrete divergent custom output.
- 2026-06-25 follow-up source-carried blocker-class residual split: packet
  bytes and synthesis policy are unchanged. The shared `A/09` unproven-carrier
  synthesis resolution now retains malformed `P/04` source blocker classes for
  source-carried divergent rows separately, then exposes post-subtraction
  residual blocker classes in direct live-object and M-frame/server-dispatch
  diagnostics. Next replay should target only non-source-carried residual
  blocker classes before widening any carrier emission rule.
- 2026-06-12 follow-up exact placeable reconciliation summary diagnostics:
  packet bytes and reconciliation policy are unchanged. The exact `A/09`/`U/09`
  pass now records unique-module-backed, identity-blocked, no-overlap, and
  unique-unchanged target counts separately for add/update rows, counts
  custom/resref appearance fields that cannot be same-width static rewrites,
  emits a debug summary even when no exact-placeable rewrite is produced, and
  threads the same counters through the M-frame exact-rewrite dispatch log.
  Verified with focused `exact_placeable_`, `area_context_tracks_`,
  `placeable_context_`, `placeable_update`, `cargo check -q`, `cargo fmt
  --all --check`, and `git diff --check`; next local replay should compare
  these target/skipped counters with unresolved conflict snapshots to decide
  whether remaining placeable drift is an unimplemented writer rule or needs
  more decompile proof for custom/resref appearance handling.
- 2026-06-12 follow-up exact placeable conflict-counter correlation: packet
  bytes and reconciliation policy are unchanged. The semantic registry now
  aggregates unresolved area/static placeable conflicts by compact/external
  owner alias and class (`identity`, `appearance`, `state`, `orientation`, plus
  state subfields). M-frame finalization carries the bounded exact live-object
  rewrite summary through lifecycle proof and logs those unresolved conflict
  counters beside the exact `A/09`/`U/09` target/skipped counters, so the next
  local replay can compare a single final-payload summary instead of matching
  separate lines by hand. Verified with focused `area_context_conflict_summary`,
  `area_context_tracks_`, `exact_placeable_`, `placeable_update`,
  `placeable_context_`, `cargo check -q -p hgbridge-proxy2`, and
  `cargo fmt --all --check`; next local replay should classify remaining
  placeable drift from the combined target-vs-conflict counters before adding
  another writer rule.
- 2026-06-12 follow-up exact static/live placeable position synthesis: exact
  `U/09` position rows now expose the parser-owned six read-buffer bytes plus
  two Z residual bits from the verified EE/Diamond shared generic update
  reader. The exact area/static reconciliation pass rewrites those same-width
  X/Y/Z fields to the uniquely matched module-backed static row while
  preserving orientation/appearance/state cursors and final exact claim.
  Verified with focused exact-position, exact-placeable, placeable-update, and
  area-context regressions plus `cargo check -q -p hgbridge-proxy2`,
  `cargo fmt --all --check`, and `git diff --check`; next local replay should
  classify remaining drift outside exact position, vector/scalar orientation,
  same-width appearance, and proven state bits.
- 2026-06-12 follow-up semantic position conflict diagnostics: packet bytes and
  reconciliation policy are unchanged. Exact `U/09` position mentions now reuse
  the verified parser-owned position claim, and the semantic registry plus
  server-dispatch summaries track unresolved area/static position conflicts
  beside identity, appearance, state, and orientation. Verified with focused
  `area_context_`, `exact_placeable_`, and `placeable_update` regressions; next
  local replay should classify remaining drift outside exact position,
  vector/scalar orientation, same-width appearance, and proven state bits.
- 2026-06-12 follow-up current-record conflict progress diagnostics: packet
  bytes and reconciliation policy are unchanged. Server-dispatch unresolved
  area/static placeable diagnostics now classify the exact current `A/09`/`U/09`
  record as resolving, repeating, or leaving untouched each prior
  identity/appearance/state/orientation/position conflict, with owner-deduped
  aggregate counters beside the exact rewrite summary. Next local replay should
  use these fields to tell whether remaining visual drift persists after a
  resolving record or is simply waiting for a later exact record to carry the
  correcting module-backed facts.
- 2026-06-12 follow-up exact placeable field-rewrite counters: packet bytes
  and reconciliation policy are unchanged. Exact `A/09`/`U/09` reconciliation
  summaries now count rewritten fields separately (`add` appearance/state and
  `update` position/appearance/orientation/state) and thread those counts
  through the M-frame exact-rewrite log beside unresolved conflict and
  current-record progress counters. Verified with incremental disabled using
  focused `exact_placeable_`, `area_context_`, `placeable_update`, and
  `cargo check -q -p hgbridge-proxy2`; next local replay should compare
  field-rewrite counters against unresolved conflict classes before adding
  another writer rule.
- 2026-06-12 follow-up exact `U/09` CResRef appearance collapse: exact
  placeable update appearance reconciliation now handles a source
  `0xFFFE+ CResRef` branch when the unique module-backed static row proves a
  normal appearance WORD. The pass rewrites the WORD, removes the parser-owned
  16-byte CResRef payload, shifts later exact record offsets, updates declared
  lengths from the final exact claim, and still refuses module-backed custom
  appearance targets. Verified with incremental disabled using focused
  `exact_placeable_`, `area_context_`, `placeable_update`,
  `cargo check -q -p hgbridge-proxy2`, `cargo fmt --all --check`, and
  `git diff --check`; next local replay should compare remaining appearance
  conflicts now that source-custom update rows can shrink to static WORDs.
- 2026-06-13 follow-up exact `A/09` source-custom appearance collapse: exact
  placeable add appearance reconciliation now treats a source `0xFFFE+`
  appearance as the fixed-width add-tail WORD proven by the add parser, not a
  `U/09`-style CResRef branch. A unique module-backed normal static row can
  rewrite that WORD in place while preserving declared length, fragment cursor,
  add-state bits, and the EE visual-transform map; only module-backed custom
  targets remain counted as custom skipped. Verified with focused
  `exact_placeable_add_rewrites_custom_word_to_unique_area_static_word`,
  `exact_placeable_`, `placeable_update`, `cargo fmt --all --check`, and
  `cargo check -q -p hgbridge-proxy2`; next local replay should classify any
  remaining appearance conflicts as module-custom targets, identity/no-overlap,
  or an unmodeled packet family.
- 2026-06-13 follow-up unresolved placeable appearance classification: packet
  bytes and rewrite policy are unchanged. The semantic unresolved
  area/static-placeable conflict summary now splits appearance conflicts into
  module-custom targets, module-normal targets, and source-custom observed
  rows, and server-dispatch/exact-rewrite logs emit those counters beside the
  existing unresolved appearance totals. Verified with focused
  `area_context_conflict`; next local replay should use the new counters to
  decide whether remaining visual drift is an intentional module-custom skip,
  a normal target that never reached an exact `A/09`/`U/09` rewrite, or another
  packet family.
- 2026-06-13 follow-up exact placeable custom-counter split: packet bytes and
  rewrite policy are unchanged. Exact live-object `A/09`/`U/09` reconciliation
  summaries now split module-custom target skips from source-custom appearance
  collapses, with separate add/update counters threaded through the M-frame
  dispatch summary. The remaining module-custom target rewrite is blocked until
  decompile/capture proof identifies the exact CResRef source to emit for a
  module-backed static row; the current area context only carries the `0xFFFE+`
  appearance WORD. Verified with focused `exact_placeable_`; next local replay
  should compare module-custom skip counters against unresolved module-custom
  appearance conflicts before adding a custom-target writer.
- 2026-06-13 follow-up exact `U/09` module-custom TemplateResRef writer:
  module-backed static placeable context now carries the GIT `TemplateResRef`
  only after the same unique static-row, direction, and object-id proof used for
  state/orientation/position. Exact `U/09` appearance reconciliation can now
  widen a normal source WORD to EE's decompile-backed `0xFFFE+ WORD + CResRef`
  branch, or overwrite an existing custom branch, while shifting following
  record offsets and revalidating the final exact payload. Exact `A/09` custom
  targets remain fixed-width skips because the verified add layout has no
  parser-owned CResRef branch. Verified with focused `exact_placeable_`,
  `area_context_`, `placeable_context_`, `placeable_update`, `cargo check -q
  -p hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`; next
  local replay should check whether remaining module-custom appearance conflicts
  are add-only fixed-width rows, missing module TemplateResRef proof,
  identity/no-overlap, or another packet family.
- 2026-06-13 follow-up module-custom placeable skip classification: packet
  bytes are unchanged. Area/static placeable appearance conflicts now carry the
  module TemplateResRef proof into the semantic snapshot, unresolved conflict
  summaries split module-custom targets with/without TemplateResRef, and exact
  `A/09`/`U/09` reconciliation summaries split fixed-width add skips from missing
  TemplateResRef skips. This keeps the decompile-backed `A/09` fixed WORD layout
  blocked while making the next local replay distinguish protocol-shape limits
  from missing module context. Verified with focused `exact_placeable_`,
  `area_context_`, `placeable_update`, `cargo check -q -p hgbridge-proxy2`,
  `cargo fmt --all --check`, and `git diff --check`; next local replay should
  compare the new custom-with-resref/missing-resref counters against unresolved
  module-custom conflicts.
- 2026-06-13 follow-up exact `A/09` fixed-width custom-target update
  correlation: packet bytes are unchanged. Exact live-object placeable
  reconciliation now splits module-custom add skips with TemplateResRef into
  same-payload exact `U/09` appearance rows versus true add-only fixed-width
  rows, and threads those counters through the M-frame dispatch summary. This
  preserves the decompile-backed `A/09` WORD-only layout while showing whether
  EE's parser-owned `U/09 WORD + CResRef` branch is already available in the
  same payload. Verified with focused `exact_placeable_`, `area_context_`,
  `placeable_update`, `cargo check -q -p hgbridge-proxy2`,
  `cargo fmt --all --check`, and `git diff --check`; next local replay should
  classify remaining module-custom add skips as `fixed_width_with_update` or
  `fixed_width_add_only` before considering any synthesized follow-up update.
- 2026-06-13 follow-up exact `U/09` custom-update branch classification:
  packet bytes are unchanged. Exact live-object placeable mentions now expose
  the parser-owned appearance WORD offset and optional CResRef offset, and the
  fixed-width `A/09` module-custom add-skip counters split same-payload `U/09`
  rows into normal-WORD branches that can be widened versus existing
  WORD+CResRef branches that can be overwritten. M-frame/server-dispatch
  summaries carry both branch counters beside the aggregate `with_update` and
  `add_only` counts. Verified with focused branch regressions plus
  `exact_placeable_`, `area_context_`, and `placeable_update`; next local
  replay should compare `with_normal_update`, `with_custom_update`, and
  `add_only` before adding any synthesized follow-up `U/09`.
- 2026-06-13 follow-up ordered `A/09` custom-target carrier classification:
  packet bytes are unchanged. The fixed-width module-custom add-skip
  classifier now treats `with_update`/`with_normal_update`/`with_custom_update`
  as same-object `U/09` appearance carriers that follow the `A/09` in payload
  order, and splits prior-only same-object updates into
  `pre_add_update_only` counters. A pre-add `U/09` may still rewrite its own
  parser-owned WORD+CResRef branch, but it is not evidence that the following
  fixed-width add has a usable custom CResRef carrier. Verified with focused
  ordered carrier regressions; next local replay should compare following
  `with_normal_update`/`with_custom_update`, `pre_add_update_only`, and
  `add_only` before adding any synthesized follow-up `U/09`.
- 2026-06-13 follow-up fixed-width `A/09` custom-carrier detail trace: packet
  bytes are unchanged. The exact placeable carrier helper now retains the
  nearest same-object `U/09` appearance record details on each side of a
  module-custom fixed-width add skip: record offsets, parser-owned appearance
  WORD/CResRef offsets, source appearance/resref, and fragment bit span. When
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM` is enabled, the skip branch emits one
  candidate trace beside the aggregate counters so the next local replay can
  distinguish add-only synthesis candidates from normal/custom following
  carriers without guessing. Verified with focused carrier-detail regression,
  `exact_placeable_`, `area_context_`, `placeable_update`, and `cargo check`;
  next local replay should inspect the new candidate trace before deciding
  whether to synthesize a following `U/09` writer.
- 2026-06-13 follow-up fixed-width `A/09` add-only custom synthesis: exact
  placeable reconciliation now emits a minimal same-object following
  `U/09 mask=0x20` when an exact fixed-width add targets a unique module-backed
  custom static row with proven `TemplateResRef` and no following parser-owned
  carrier. The original `A/09` stays fixed-width and fragment bits are not
  moved; the synthesized update uses the decompile-backed generic
  door/placeable update shape (`header -> appearance WORD -> CResRef`) and is
  immediately exact-validated at the add's next bit cursor. M-frame/server
  dispatch summaries now expose the synthesized-update count beside the
  add-only bucket. Verified with focused
  `exact_placeable_add_module_custom_add_only_synthesizes_following_update`,
  `exact_placeable_`, `area_context_`, `placeable_update`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`; next
  local replay should compare remaining custom skips, especially pre-add-only
  and lifecycle/identity-blocked rows.
- 2026-06-13 follow-up fixed-width `A/09` pre-add-only custom synthesis:
  exact placeable reconciliation now also synthesizes the same minimal
  following `U/09 mask=0x20` when the only same-object appearance update in
  the payload is before the fixed-width add. The pre-add update may still
  rewrite its own parser-owned custom branch, but it is not treated as a carrier
  for the later add. The add remains fixed-width, the synthesized update
  exact-validates from the add-owned next bit cursor, and no fragment bits move.
  Verified with focused
  `exact_placeable_add_module_custom_pre_add_only_synthesizes_following_update`
  and `exact_placeable_`; next local replay should compare remaining custom
  skips against lifecycle/identity-blocked rows and missing `TemplateResRef`
  context.
- 2026-06-13 follow-up identity-blocked module-custom row diagnostics: packet
  bytes and reconciliation policy are unchanged. Exact placeable reconciliation
  now counts module-backed custom static rows hidden behind non-unique area
  identity separately for `A/09` and appearance-owning `U/09` records, and
  splits those counts by rows with versus without `TemplateResRef` proof. The
  counters flow through the exact live-object summary and server-dispatch log so
  the next local replay can distinguish custom-carrier gaps from missing row
  identity before adding another writer rule. Verified with incremental
  disabled using focused `exact_placeable_summary_counts_identity_blocked_module_custom_rows`,
  `exact_placeable_`, `area_context_`, `placeable_update`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`.
- 2026-06-13 follow-up identity-blocked exact `U/09` position selection: exact
  placeable reconciliation now promotes an otherwise identity-blocked update to
  the module-backed static row only when the same verified `U/09` record owns
  the decompile-ordered position branch and exactly one overlapping static row
  has identical packed X/Y/Z position words/bits. Ambiguous overlaps remain
  diagnostic-only. The selected row is then shared by the position, orientation,
  appearance, and lock-state helpers without moving bits; the reader proof
  remains `header -> position words/z fragment bits -> orientation ->
  appearance -> scale/state -> state BOOLs`. Summaries now expose
  `exact_placeable_update_identity_resolved_by_position` so local replay can
  separate position-resolved custom statics from rows that still need a stronger
  identity source. Verified with incremental disabled using focused
  `exact_placeable_update_position*`, `exact_placeable_`, `area_context_`,
  `placeable_update`, `cargo check -q -p hgbridge-proxy2`, `cargo fmt --all
  --check`, and `git diff --check`.
- 2026-06-13 follow-up identity-blocked exact `A/09` fixed-field diagnostics:
  packet bytes and reconciliation policy are unchanged. Exact placeable add
  reconciliation now counts identity-blocked rows where the verified fixed-width
  add fields (`appearance` WORD plus add-state BOOLs) match exactly one
  module-backed static row, split by module-custom target and missing
  `TemplateResRef`. This deliberately does not promote a writer target because
  the decompiled `A/09` add reader owns no position or CResRef branch. Verified
  with incremental disabled using focused
  `exact_placeable_add_identity_blocked_fixed_fields_count_unique_static_match`,
  `exact_placeable_`, `area_context_`, `placeable_update`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`; next
  local replay should compare fixed-field matches against remaining
  identity-blocked custom skips before considering a stronger add identity
  source or another synthetic carrier rule.
- 2026-06-13 follow-up exact `A/09` fixed-field identity selection: exact
  placeable add reconciliation now promotes an otherwise identity-blocked add to
  a module-backed static row only when the parser-owned fixed-width add fields
  (`appearance` WORD plus add-state BOOLs) match exactly one overlapping static
  row. The add itself remains fixed-width; this selection only lets the
  decompile-backed synthetic following `U/09 mask=0x20` carrier emit a custom
  `TemplateResRef` when the selected row is module-custom. Ambiguous matches
  stay diagnostic-only. Verified with focused
  `exact_placeable_add_identity_blocked_fixed_fields_select_unique_static_match`;
  next local replay should compare remaining identity-blocked custom rows
  against fixed-field-resolved and ambiguous add counters.
- 2026-06-13 follow-up exact `A/09` fixed-field ambiguity counters: packet bytes
  and reconciliation policy are unchanged. Identity-blocked fixed-width add
  diagnostics now split ambiguous fixed-field candidate sets from no-evidence
  identity blocks, including total candidate rows, module-custom rows, and
  missing-`TemplateResRef` rows. This keeps ambiguous add rows diagnostic-only
  while making the next replay able to distinguish missing identity proof from
  genuinely non-matching static context. Verified with focused
  `exact_placeable_add_identity_blocked_fixed_fields_select_unique_static_match`;
  next local replay should compare the ambiguous counters against remaining
  identity-blocked custom rows before adding any stronger add selection rule.
- 2026-06-13 follow-up exact `A/09` fixed-field output-equivalence selection:
  ambiguous fixed-width add candidates now become writable only when every
  fixed-field-matching module-backed static row would emit the same custom
  appearance carrier (`0xFFFE+` WORD plus identical module `TemplateResRef`).
  The `A/09` row remains fixed-width and packet-authored; the bridge inserts
  the same decompile-backed following `U/09 mask=0x20` carrier at the add-owned
  next bit cursor and still counts the candidate set as ambiguous for replay.
  Divergent carrier bytes and missing `TemplateResRef` rows remain
  diagnostic-only. Verified with focused
  `exact_placeable_add_fixed_field_equivalent_custom_rows_synthesize_update`;
  next local replay should separate equivalent-resolved ambiguous rows from
  remaining divergent/missing-resref rows before widening identity selection.
- 2026-06-13 follow-up exact `A/09` add-output equivalence selection:
  identity-blocked exact placeable adds now promote to a module-backed static
  row when every concrete static candidate has unique object-id confidence and
  would emit identical add-visible output: the fixed appearance WORD, the
  add-state BOOL block, and, for custom appearances, the identical synthetic
  `U/09 mask=0x20` carrier `TemplateResRef`. The selected target is now passed
  through both add appearance and add state reconciliation; rows with light
  overlap, duplicate/alias confidence, unproven module state, divergent output,
  or missing custom `TemplateResRef` remain diagnostic-only. Verified with
  focused
  `exact_placeable_add_output_equivalent_duplicate_rows_rewrite_visible_fields`
  plus `exact_placeable_`, `area_context_`, and `placeable_update`; next local
  replay should compare the new output-equivalence counter against remaining
  identity-blocked add appearance/state conflicts before adding any position-
  or resource-based add identity rule.
- 2026-06-13 follow-up exact `A/09` following-position identity selection:
  identity-blocked exact placeable adds can now promote to a module-backed
  static row when a later same-object verified `U/09` before any same-object
  add/delete owns a position branch whose raw packed X/Y/Z match exactly one
  overlapping module-backed static row. The add still rewrites only its own
  fixed appearance WORD and parser-owned add-state BOOLs, and the following
  update keeps its own position proof/counters. Verified with focused
  `exact_placeable_add_identity_resolves_by_following_position_update`,
  `exact_placeable_`, `area_context_`, `placeable_update`, and `cargo check`;
  next local replay should compare
  `exact_placeable_add_identity_resolved_by_following_position` against
  remaining identity-blocked add conflicts and same-object lifecycle breaks.
- 2026-06-13 follow-up following-position blocker diagnostics: packet bytes are
  unchanged. Identity-blocked exact `A/09` adds that remain unresolved now split
  the unavailable following-position proof into no later position, same-object
  add/delete lifecycle fence, no matching module-backed static position, and
  ambiguous static-position matches, with the counters carried through
  live-object, M-frame, and server-dispatch summaries. The scanner still stops
  before any same-object `A`/`D`; a later `U/09` keeps its own parser-owned
  position/state rewrite proof but cannot identify the earlier add across that
  lifecycle boundary. Verified with focused
  `exact_placeable_add_following_position_stops_at_lifecycle_delete`,
  `exact_placeable_`, `area_context_`, `placeable_update`, and `cargo check`;
  next local replay should compare lifecycle-blocked versus missing/ambiguous
  following-position counters before adding any broader add identity rule.
- 2026-06-13 follow-up following-position output-equivalence selection:
  identity-blocked exact `A/09` adds now resolve ambiguous later same-object
  `U/09` position matches when every same-position module-backed static row
  would emit identical add-visible output: fixed appearance WORD, add-state
  BOOL block, and optional synthetic custom `TemplateResRef` carrier. The later
  `U/09` still owns its parser-position proof; divergent same-position outputs
  remain diagnostic-only. Summaries expose
  `exact_placeable_add_identity_resolved_by_following_position_equivalence`
  beside the existing aggregate following-position resolution counter. Verified
  with focused
  `exact_placeable_add_identity_resolves_by_following_position_output_equivalence`,
  `exact_placeable_`, `area_context_`, `placeable_update`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`; next
  local replay should compare this counter against remaining
  `following_position_ambiguous` rows and divergent/missing-resref custom
  candidates.
- 2026-06-13 follow-up following-position ambiguity-output diagnostics: packet
  bytes are unchanged. Unresolved identity-blocked exact `A/09` adds with an
  ambiguous later same-object `U/09` position proof now split the ambiguous
  static-row set by module-custom rows, missing module `TemplateResRef` rows,
  add-output-unavailable rows, and add-output-divergent matches. These counters
  are carried through live-object, M-frame, and server-dispatch summaries so the
  next replay can distinguish missing module resource proof from genuinely
  divergent add-visible output before any broader identity selection rule.
  Verified with focused
  `exact_placeable_add_following_position_ambiguous_counts_missing_output`;
  next local replay should compare the new ambiguous-output counters against
  remaining `identity_blocked` exact add rows.
- 2026-06-13 follow-up preceding-position exact `A/09` identity selection:
  identity-blocked exact placeable adds now also resolve from a prior
  same-object exact `U/09` position branch when no same-object `A`/`D`
  lifecycle row lies between the update and add, and the parser-owned raw
  X/Y/Z position identifies exactly one module-backed static row or an
  output-equivalent same-position row set. The add still rewrites only its
  fixed appearance WORD and add-state BOOLs; the preceding update keeps its
  own parser-owned position proof. New live-object, M-frame, and
  server-dispatch counters split preceding-position resolutions from existing
  following-position resolutions. Verified with focused
  `exact_placeable_add_identity_resolves_by_preceding_position_update`,
  `exact_placeable_`, `area_context_`, and `placeable_update`; next local
  replay should compare preceding-position resolutions against remaining
  following-position missing/lifecycle/ambiguous blockers.
- 2026-06-13 follow-up preceding-position blocker diagnostics: packet bytes and
  reconciliation policy are unchanged. Identity-blocked exact `A/09` adds that
  still cannot use a prior same-object `U/09` position proof now split that
  backward scan into missing prior position, same-object `A`/`D` lifecycle
  fence, no static-position match, and ambiguous static-position candidates,
  with the same module-custom/missing-`TemplateResRef`/output-unavailable/
  output-divergent detail already used by the following-position scan. Verified
  with focused `exact_placeable_add_preceding_position_stops_at_lifecycle_delete`,
  `exact_placeable_`, `area_context_`, `placeable_update`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`; next
  local replay should compare preceding and following blocker counters before
  adding any broader add identity rule.
- 2026-06-14 follow-up surrounding-position exact `A/09` identity selection:
  identity-blocked fixed-width placeable adds now treat bracketing same-object
  exact `U/09` position evidence as a shared proof source. When both the nearest
  preceding and following position mentions resolve, the add is rewritten only
  if they select the same module-backed row or rows with byte/bit-identical
  add-visible output; divergent bracketing positions stay packet-authored and
  increment `exact_placeable_add_identity_surrounding_position_conflicts`
  instead of silently trusting the later mention. New replay counters expose
  surrounding-position resolutions, output-equivalent resolutions, and
  conflicts. Verified with focused `surrounding_position`, `exact_placeable_`,
  `area_context_`, and `placeable_update`; next local replay should compare the
  new conflict counter against remaining preceding/following blockers before
  broadening position-derived add identity again.
- 2026-06-14 follow-up surrounding-position conflict classification: packet
  bytes and identity policy are unchanged. Bracketing exact `A/09` position
  conflicts now split into missing add-output proof versus concrete divergent
  add output via
  `exact_placeable_add_identity_surrounding_position_conflict_output_unavailable`
  and
  `exact_placeable_add_identity_surrounding_position_conflict_output_divergent`.
  The split is threaded through direct live-object, M-frame, and
  server-dispatch traces. Next local replay should compare unavailable-output
  conflicts against missing `TemplateResRef`/module context before treating
  them as true position disagreements.
- 2026-06-14 follow-up surrounding-position missing-output diagnostics: packet
  bytes and identity policy are unchanged. The unavailable-output conflict
  bucket now also counts selected bracketing rows whose custom appearance lacks
  module `TemplateResRef` proof via
  `exact_placeable_add_identity_surrounding_position_conflict_output_missing_template_resref_rows`.
  Next replay should compare this row count against the broad unavailable
  conflict count; any remainder needs separate module-context proof before a
  broader surrounding-position rule is safe.
- 2026-06-14 follow-up surrounding-position fixed-output equivalence: exact
  `A/09` placeable adds can now reconcile parser-owned add-state BOOLs when
  preceding and following same-object position mentions select different custom
  static rows whose A/09-visible fixed output (`appearance` WORD plus module
  state bits) is identical, while their full custom carrier output is still
  missing or divergent. The bridge records
  `exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_equivalence`,
  keeps the full-output conflict counters, and deliberately suppresses
  synthetic `U/09 mask=0x20` carrier emission for this path. Verified with
  focused `fixed_output_equivalence`, `surrounding_position`, `exact_placeable_`,
  `area_context_`, `placeable_update`, `cargo check`, formatter, and
  diff-check. Next local replay should compare fixed-output equivalence counts
  against remaining custom carrier conflicts before widening any carrier rule.
- 2026-06-14 follow-up one-sided position fixed-output equivalence: exact
  identity-blocked `A/09` adds now also use a single preceding or following
  same-object `U/09` position mention when its ambiguous same-position static
  row set has identical fixed `A/09` appearance/state output but missing or
  divergent full custom-carrier output. The add may reconcile state BOOLs, but
  custom appearance carriers remain suppressed unless full `TemplateResRef`
  output is proven. New direct/M-frame/server-dispatch counters split
  `exact_placeable_add_identity_resolved_by_following_position_fixed_output_equivalence`
  and
  `exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_equivalence`.
  Next local replay should compare these one-sided counters with remaining
  ambiguous output-unavailable rows and synthetic-carrier skips.
- 2026-06-14 follow-up one-sided fixed-output carrier blockers: packet bytes are
  unchanged. One-sided following/preceding fixed-output equivalence now carries
  the unproven custom-carrier reason through direct, M-frame, and
  server-dispatch summaries: missing `TemplateResRef` row counts versus
  concrete divergent carrier-output matches. Focused regressions cover missing
  resref and divergent-carrier variants. Next local replay should compare
  these counters against remaining fixed-output-only custom skips before adding
  any broader synthetic carrier rule.
- 2026-06-14 follow-up duplicate static-object-id module authority: duplicate
  object ids in the `P/04/01` static-placeable list still leave object-id-only
  reconciliation identity-blocked, but no longer erase one-to-one module row
  state/`TemplateResRef` proof. Area-object aliases continue to clear module
  authority because legacy/external-id normalization can alias unrelated live
  ids. Verified with
  `duplicate_static_ids_keep_module_authority_for_later_position_proof`,
  `area_context_`, `exact_placeable_`, and `placeable_update`; next replay
  should compare duplicate-object-id module-backed rows against remaining
  fixed-output-only carrier skips before widening custom-carrier synthesis.
- 2026-06-14 follow-up surrounding fixed-output carrier detail: packet bytes and
  reconciliation policy are unchanged. Exact `A/09` surrounding-position
  fixed-output equivalence now exposes its suppressed custom-carrier blockers
  separately from broad full-output conflict counters:
  `exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_missing_template_resref_rows`
  and
  `exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_divergent`.
  The fields flow through direct live-object summaries, M-frame aggregation, and
  server-dispatch trace events. Next replay should compare these against
  one-sided fixed-output blockers and remaining fixed-width custom skips before
  adding any broader carrier synthesis rule.
- 2026-06-14 follow-up exact `U/09` duplicate-row output equivalence: exact
  placeable updates that own a parser position branch can now resolve duplicate
  same-position module-backed static rows when every update-owned non-position
  output field (`orientation`, `appearance`, or lock bits) is byte/bit-identical
  across the candidate rows. Position-only `U/09` rows remain diagnostic proof
  for add-side reconciliation instead of becoming update targets. Verified with
  focused `exact_placeable_update_position` and full `exact_placeable_`
  diagnostics using isolated non-incremental target dirs after a rustc
  incremental-cache panic in the default target.
- 2026-06-14 follow-up exact `U/09` duplicate-output replay split: packet bytes
  and reconciliation policy are unchanged. Direct live-object, M-frame, and
  server-dispatch summaries now split duplicate same-position
  output-equivalence update resolutions from ordinary unique-position matches via
  `exact_placeable_update_identity_resolved_by_position_output_equivalence`.
  Next replay should compare this counter against remaining fixed-output custom
  carrier skips before widening synthetic carrier emission.
- 2026-06-14 follow-up fixed-width custom carrier duplicate-output join: packet
  bytes and carrier policy are unchanged. Fixed-width custom `A/09` add skips
  now count whether their following or pre-add appearance-owning `U/09` carrier
  also resolved duplicate same-position static rows by update-owned output
  equivalence, via
  `exact_placeable_add_module_custom_template_resref_fixed_width_with_update_position_output_equivalence`
  and
  `exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only_position_output_equivalence`.
  The fields flow through direct live-object, M-frame, and server-dispatch
  summaries. Verified with focused carrier-output, `exact_placeable_`,
  `placeable_update`, `cargo check`, formatter, and diff-check. Next replay can
  compare these joined counters directly against remaining fixed-output custom
  carrier skips before widening synthetic carrier emission.
- 2026-06-14 follow-up fixed-output custom-carrier suppression diagnostics:
  packet bytes and carrier policy are unchanged. Fixed-width module-custom
  `A/09` adds selected only by fixed-output position proof now expose direct,
  M-frame, and server-dispatch counters for suppressed custom carriers, split by
  following, preceding, and surrounding position proof plus missing
  `TemplateResRef` rows and divergent carrier output. The selected-row
  missing-resref case now also increments the existing add-side missing
  `TemplateResRef` diagnostic instead of disappearing behind the broad
  fixed-output suppression branch. Verified with
  `cargo test -q -p hgbridge-proxy2 fixed_output_equivalence -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 exact_placeable_ -- --test-threads=1`,
  `cargo test -q -p hgbridge-proxy2 placeable_update -- --nocapture`,
  `cargo check -q -p hgbridge-proxy2`, `cargo fmt --all --check`, and
  `git diff --check`. A full serial proxy2 run still fails in pre-existing
  fixture/resource-dependent paths starting with
  `local_to_heir_compact_area_uses_module_resource_dimensions`; the focused
  `A/09`/`U/09` fixed-output carrier path passes. Next replay should inspect
  the `server exact placeable surrounding fixed-output carrier blockers` event
  and compare unproven-carrier missing-resref versus divergent counts before
  any broader synthetic carrier rule.
- 2026-06-14 follow-up fixed-output unproven-carrier location split: packet
  bytes and carrier policy are unchanged. Fixed-output suppressed custom
  carriers now report whether an exact parser-owned `U/09` appearance carrier
  already exists after the add, exists only before the add, or is truly add-only,
  split into normal/custom update branches through direct live-object, M-frame,
  and server-dispatch summaries. Verified with focused fixed-output,
  exact-placeable, placeable-update, `cargo check`, formatter, and diff-check.
  Next replay should compare `with_custom_update` versus `add_only` before
  deciding whether missing `TemplateResRef` proof or synthesis policy is the
  remaining blocker.
- 2026-06-14 follow-up exact following custom-carrier proof: fixed-output
  duplicate-position `A/09` adds now treat a following same-object `U/09` that
  owns both exact position and custom appearance `CResRef` as row identity proof
  when the carrier matches a module static row's `TemplateResRef`. Mismatched
  carrier resrefs, pre-add-only carriers, split position/appearance carriers,
  and add-only fixed-output cases remain suppressed and should be compared in
  the next local replay before any broader synthetic `U/09` rule.
- 2026-06-14 follow-up exact pre-add custom-carrier proof: fixed-output
  duplicate-position `A/09` adds now also treat a preceding same-object `U/09`
  as row identity proof when there is no following post-add position/appearance
  carrier, that one pre-add row owns both exact position and custom appearance
  `CResRef`, and the CResRef matches a module static row's `TemplateResRef`.
  The add still emits a bounded following synthetic `U/09 mask=0x20` carrier
  after its fixed-width add body; position-only pre-add rows, split
  position/appearance carriers, and add-only fixed-output cases remain
  suppressed pending replay counters.
- 2026-06-14 follow-up split custom-carrier proof: fixed-output
  duplicate-position `A/09` adds now join exact same-lifecycle `U/09` position
  rows with separate exact same-object custom-appearance `U/09` rows when the
  combined raw position plus `CResRef` matches a module static row's
  `TemplateResRef`. Following split carriers suppress synthetic insertion
  because the post-add custom branch already exists; pre-add split carriers
  still synthesize the bounded following `U/09 mask=0x20` so EE state order is
  restored after the add. Position-only pre-add rows and fixed-output-only
  add-only ambiguity remain for replay comparison.
- 2026-06-15 follow-up add-only fixed-output identity selection: exact
  duplicate-object `A/09` adds that have no parser-owned position or custom
  `CResRef` carrier can now select a module static row when every fixed-field
  matching static row would emit the same add-owned fixed output (`appearance`
  WORD plus module state bits). This resolves the ambiguous fixed fields but
  still treats the custom `TemplateResRef` carrier as unproven, splitting
  missing-resref and divergent-output blockers through direct, M-frame, and
  server-dispatch summaries. Next local replay should compare these add-only
  fixed-output counters against position-only pre-add rows before adding any
  broader synthetic custom-carrier rule.
- 2026-06-15 follow-up unproven-carrier pre-add position split: packet bytes
  are unchanged. Add-only unproven custom-carrier summaries now also count the
  subset selected by a preceding same-object position-only `U/09` fixed-output
  proof, propagated through direct live-object, M-frame aggregation, and
  server-dispatch logs. This makes the next local replay able to compare true
  add-only fixed-output rows against pre-add position-only rows without treating
  either as a proven `TemplateResRef` carrier.
- 2026-06-15 follow-up symmetric position-only fixed-output carrier split:
  packet bytes are unchanged. Fixed-output unproven custom-carrier summaries now
  separately count following-position-only and bracketing surrounding-position-only
  `U/09` proof when no parser-owned appearance update exists, alongside the
  existing pre-add position-only bucket. Direct live-object, M-frame, and
  server-dispatch logs now distinguish true add-only fixed-output rows from all
  position-only fixed-output proofs before any broader custom-carrier synthesis
  rule is considered.
- 2026-06-15 follow-up fixed-output carrier blocker source split: packet bytes
  are unchanged. Unproven fixed-width custom `A/09` carrier diagnostics now keep
  missing `TemplateResRef` row counts and divergent carrier-output counts per
  proof source: fixed-field fixed output, following position fixed output,
  preceding position fixed output, and surrounding position fixed output. The
  fields flow through direct live-object summaries, M-frame aggregation, and
  the server-dispatch fixed-output blocker trace; direct summaries emit the new
  detail as a separate trace event to avoid the existing large summary macro
  limit. Verified with `fixed_output_equivalence`, `exact_placeable_`,
  `placeable_update`, `cargo check`, formatter, and diff-check. Next local
  replay should compare per-source missing-resref versus divergent counts
  against the position-only/add-only buckets before adding any synthetic
  custom-carrier rule.
- 2026-06-15 follow-up normal update carrier writability split: packet bytes
  are unchanged. Fixed-width custom `A/09` carrier diagnostics now classify
  normal WORD-only `U/09` appearance carriers as custom-rewrite-ready only when
  the same unique/static or duplicate-position output-equivalence rules used by
  the update rewriter can emit the EE `TemplateResRef` branch; identity-blocked
  normal carriers get a separate blocked count. Direct live-object summaries,
  direct debug traces, and M-frame aggregation carry the split; the oversized
  server-dispatch summary macro was left unchanged. Verified with the fixed-width
  skip and module-custom carrier tests. Next local replay should compare ready
  versus blocked normal carriers before changing synthetic insertion order.
- 2026-06-15 follow-up fixed-width custom carrier synthesis policy: packet
  bytes are unchanged. The add-side custom carrier path now uses an explicit
  `ExactPlaceableCustomCarrierSynthesisPolicy` instead of the broad
  `!has_following` gate, splitting following custom carriers, following normal
  carriers the update rewriter can widen, following normal carriers that remain
  deferred until post-carrier insertion-order proof exists, and the current
  add/pre-add synthesis-at-add path. The debug carrier trace emits the selected
  policy, and focused tests pin both rewrite-ready and blocked following-normal
  rows. Next replay should compare the emitted policy names; if blocked
  following-normal rows remain, prove from capture/decompile whether a synthetic
  `U/09 mask=0x20` must be inserted after that following row rather than at the
  add boundary.
- 2026-06-15 follow-up blocked following-normal custom carrier synthesis:
  fixed-width custom `A/09` adds now synthesize the minimal same-object
  `U/09 mask=0x20` after a verified following normal WORD-only appearance
  carrier when that carrier cannot be widened by the update rewriter. The
  deferred insertion validates against the following carrier's parser-owned
  `fragment_bit_end`, consumes no fragment bits, and is adjusted around earlier
  in-place appearance rewrites before emit. Verified with focused fixed-width,
  exact-placeable, placeable-update, formatter, and cargo-check runs. Next
  replay should compare remaining fixed-width custom skips; following normal
  blocked rows should now emit `synthesizes-after-following-normal-rewrite-blocked`.
- 2026-06-15 follow-up custom carrier synthesis origin telemetry: packet bytes
  and synthesis policy are unchanged. Pending fixed-width custom `A/09`
  synthetic carriers now carry a typed insertion origin and summary counters
  split emitted `U/09 mask=0x20` rows into `after-add` versus
  `after-following-normal`. Direct, M-frame, and server-dispatch traces expose
  the origin split plus normal-carrier rewrite-ready/blocked counts without
  expanding the oversized server-dispatch exact-summary event. Verified with
  focused exact-placeable/add and placeable-update tests plus formatter,
  cargo-check, and diff-check. Next local replay should compare
  `synthesized_update_after_following_normal` against any remaining blocked
  normal carrier skips before broadening synthesis rules.
- 2026-06-15 follow-up after-add custom carrier reason split: packet bytes are
  unchanged. The typed synthesis policy now splits post-add synthetic
  `U/09 mask=0x20` carriers into no-carrier, pre-add normal rewrite-ready,
  pre-add normal rewrite-blocked, and pre-add custom reasons while preserving
  the physical after-add insertion point. Direct, M-frame, and server-dispatch
  traces expose the reason buckets through a focused event to avoid the
  oversized tracing macro recursion limit. Verified with focused
  module-custom, fixed-width, exact-placeable, placeable-update, formatter,
  cargo-check, and diff-check runs. Next local replay should compare
  `synthesized_update_after_add_*` reason buckets against
  `synthesized_update_after_following_normal` before broadening carrier rules.
- 2026-06-15 follow-up exact `U/09` cursor/writer guard: tightened the EE
  door/placeable update exact parser so even zero-fragment-bit rows reject a
  start cursor beyond the valid CNW fragment stream, and made synthetic
  fixed-width custom `U/09 mask=0x20` carrier emission validate on a candidate
  live-byte buffer before mutating the planned payload. This changes only
  invalid shifted-cursor paths; valid no-bit custom appearance carriers still
  accept at `cursor == fragment_bits.len()`. Verified with focused cursor and
  transactional writer regressions plus exact-placeable, placeable-update,
  fixed-width, formatter, cargo-check, and diff-check runs. Next local replay
  should still compare `synthesized_update_after_add_*` buckets against
  `synthesized_update_after_following_normal` before broadening carrier rules.
- 2026-06-15 follow-up synthetic custom carrier planning diagnostics: packet
  bytes are unchanged on valid paths. Pending fixed-width custom `A/09`
  synthetic `U/09 mask=0x20` carriers now plan through a typed insertion record
  before emission, count planned carriers separately from emitted carriers, and
  report offset-adjustment versus exact-validation rejects through direct,
  M-frame, and server-dispatch carrier-policy traces. The synthetic writer still
  validates against a candidate live-byte buffer before mutating the payload.
  Verified with focused cursor/planning, module-custom, fixed-width,
  placeable-update, formatter, cargo-check, and diff-check runs. Next local
  replay should compare `synthesized_update_planned`,
  `synthesized_update_emit_rejected`, and `synthesized_update_after_*` buckets;
  any nonzero planned-minus-emitted count needs offset/cursor proof before a
  broader carrier synthesis rule.
- 2026-06-15 follow-up synthetic custom carrier anchor validation: valid packet
  bytes are unchanged. Pending fixed-width custom `A/09` synthetic
  `U/09 mask=0x20` carriers now retain the exact parser-owned anchor row
  (`A/09` add or blocked following-normal `U/09`) and reject planning if the
  adjusted insertion point no longer lands at that row's verified byte end and
  fragment cursor. Direct and M-frame summaries distinguish offset-adjustment,
  anchor-validation, and emit-validation rejects. Next replay should treat any
  anchor reject as stale row-boundary evidence before broadening synthesis.
- 2026-06-15 follow-up lifecycle-bounded custom carrier selection: fixed-width
  custom `A/09` carrier lookup now takes the add's parser-owned byte span and
  ignores same-object `U/09` appearance rows across intervening `A`/`D`
  lifecycle fences. A pre-add custom row before a delete/add fence and a
  following custom row after a delete/add fence no longer suppress or relocate
  the add's bounded synthetic `U/09 mask=0x20`; those rows remain separate
  parser-owned update rows for their own lifecycle. Verified with focused
  lifecycle carrier regressions plus `exact_placeable_`, `fixed_width`,
  `placeable_update`, formatter, cargo-check, and diff-check. Next local replay
  should compare remaining add-only synthesis against true same-lifecycle
  carrier buckets rather than post-lifecycle rows.
- 2026-06-15 follow-up synthetic custom carrier anchor branch validation: valid
  packet bytes are unchanged. Pending fixed-width custom `A/09` synthetic
  carriers now record the expected anchor branch (`A/09` add versus normal
  WORD-only `U/09` appearance update) and reject a post-normal insertion anchor
  if the adjusted exact row is already the custom `WORD + CResRef` branch. This
  keeps deferred `U/09 mask=0x20` insertion tied to the specific parser-owned
  normal carrier it is meant to follow. Verified with focused planning,
  exact-placeable, placeable-update, fixed-width, formatter, cargo-check, and
  diff-check runs. Next local replay should still compare plan-anchor rejects
  and emitted origin buckets before broadening carrier synthesis.
- 2026-06-15 follow-up synthetic custom carrier batch transactionality: valid
  packet bytes are unchanged. Pending fixed-width custom `A/09` synthetic
  carriers now emit into a staged live-byte buffer and commit only after every
  planned `U/09 mask=0x20` row validates; a later emit reject cannot leave an
  earlier staged carrier in the helper buffer or count/log it as emitted. The
  post-commit trace reports final offsets after lower-offset synthetic inserts.
  Verified with focused rollback coverage plus `exact_placeable_`,
  `placeable_update`, cargo-check, formatter, and diff-check. Next local replay
  should compare planned/emitted/rejected buckets before widening synthesis.
- 2026-06-15 follow-up synthetic custom carrier duplicate-anchor guard: valid
  packet bytes are unchanged. Pending fixed-width custom `A/09` synthetic
  carriers now reject duplicate plans for the same adjusted insert boundary,
  object id, fragment cursor, and exact anchor before staging any bytes. This
  keeps replay anomalies from emitting two identical `U/09 mask=0x20` carrier
  rows at one parser-owned boundary; duplicate rejections increment the anchor
  reject bucket. Verified with focused pending-carrier, exact-placeable,
  placeable-update, fixed-width, cargo-check, formatter, and diff-check runs.
  Next replay should treat nonzero plan-anchor rejects as stale or duplicated
  anchor evidence before broadening synthesis.
- 2026-06-15 follow-up synthetic custom carrier source-anchor guard: valid
  packet bytes are unchanged. Pending fixed-width custom `A/09` synthetic
  carrier anchors now retain the parser-owned source appearance branch and
  reject if the adjusted `A/09` fixed WORD or normal `U/09` appearance WORD no
  longer matches before staging bytes. The same anchor-reject bucket now
  distinguishes stale source-row evidence from emit failures. Verified with
  pending-carrier, exact-placeable, placeable-update, fixed-width, cargo-check,
  formatter, and diff-check runs. Next replay should treat source-anchor
  rejects as stale row or earlier-rewrite evidence before widening synthesis.
- 2026-06-15 follow-up synthetic custom carrier anchor-reject taxonomy: packet
  bytes are unchanged. The fixed-width custom `A/09` synthetic carrier planner
  now splits aggregate plan-anchor rejects into boundary, source-anchor, and
  duplicate-anchor buckets, and carries those counters through direct
  live-object, M-frame, and server-dispatch summaries. Focused coverage pins
  each bucket so the next replay can distinguish stale offset math, stale
  source row proof, and duplicate planning before widening carrier synthesis.
  Verified with pending-carrier, exact-placeable, placeable-update,
  fixed-width, formatter, cargo-check, and diff-check runs.
- 2026-06-15 follow-up synthetic custom carrier anchor branch/origin taxonomy:
  packet bytes are unchanged. The same plan-anchor reject path now also counts
  rejects by expected parser-owned anchor branch (`A/09` add versus normal
  WORD-only `U/09`) and insertion origin (after-add versus after-following
  normal), with the counters aggregated through M-frame/server-dispatch traces.
  Focused pending-carrier tests pin the branch/origin sums. Next replay should
  compare reason, branch, and origin buckets before widening synthesis.
- 2026-06-15 follow-up synthetic custom carrier batch exact-claim guard: valid
  packet bytes are unchanged. Staged fixed-width custom `A/09` synthetic
  `U/09 mask=0x20` batches now build the full candidate live-object payload and
  require the exact EE live-object claim before committing staged bytes or
  emitted counters. Batch claim rejects are reported separately from per-row
  emit rejects through direct, M-frame, and server-dispatch summaries. Focused
  pending-carrier coverage pins a locally valid synthetic `U/09` row followed
  by unclaimed live bytes as transactional rejection. Next replay should compare
  batch-claim rejects against per-row emit rejects and anchor rejects before
  broadening carrier synthesis.
- 2026-06-15 follow-up synthetic custom carrier batch reject taxonomy: valid
  packet bytes are unchanged. The exact live-object claim validator now exposes
  the first reject stage to the synthetic carrier batch guard, and staged custom
  `U/09` batches split batch rejects into payload-build, header/declared,
  fragment, boundary, record-validator, and cursor buckets through direct,
  M-frame, and server-dispatch summaries. Focused pending-carrier coverage pins
  a malformed full-payload batch as a record-validator reject. Next replay
  should compare the new batch reason buckets against per-row emit and anchor
  rejects before widening custom-carrier synthesis.
- 2026-06-15 follow-up synthetic custom carrier batch footprint taxonomy: valid
  packet bytes are unchanged. Full-batch exact-claim rejects now also report
  whether the rejected staged batch contained parser-owned `A/09` add anchors,
  normal WORD-only `U/09` anchors, after-add insertions, or
  after-following-normal insertions, aggregated through direct live-object,
  M-frame, and server-dispatch summaries. Focused pending-carrier coverage pins
  both malformed add-anchor and malformed normal-update-anchor batches. Next
  replay should compare batch reason buckets with these branch/origin footprints
  before widening custom-carrier synthesis.
- 2026-06-15 follow-up synthetic custom carrier batch reject focus: valid
  packet bytes are unchanged. Full-batch exact-claim rejects now localize the
  validator failure against staged synthetic `U/09 mask=0x20` rows using final
  post-insertion offsets, split into before/inside/after-synthetic-row counters
  and focused warning fields carrying the row object, anchor branch, and
  insertion origin. Existing malformed add-anchor and normal-update-anchor
  regressions reject inside the staged row. Next replay should compare the new
  focus buckets with claim reason and branch/origin footprint counters before
  changing synthesis scope.
- 2026-06-15 follow-up synthetic custom carrier offset-reject taxonomy: valid
  packet bytes are unchanged. Pending fixed-width custom `A/09` synthetic
  carriers now split stale offset-adjustment failures into insert-boundary,
  anchor-start, and anchor-end buckets, plus expected anchor branch and
  insertion-origin counters through direct live-object, M-frame, and
  server-dispatch summaries. Focused coverage pins add-anchor/after-add and
  normal-update/after-following-normal offset rejects before byte staging. Next
  replay should compare offset, anchor, emit, and batch-claim rejects before
  broadening custom-carrier synthesis.
- 2026-06-16 follow-up synthetic custom carrier emit isolation: fixed-width
  custom `A/09` synthetic carrier batches now skip and count a locally invalid
  planned `U/09 mask=0x20` row instead of aborting every other locally valid
  sibling in the same live-object payload. The remaining staged carriers still
  commit only after the existing full `P/05/01` exact-claim validator accepts
  the rebuilt payload, so full-payload rejects remain transactional. Verified
  with pending-carrier, exact-placeable, placeable-update, fixed-width,
  formatter, diff-check, and cargo-check runs. Next local replay should compare
  emit rejects against emitted carrier origin buckets; a nonzero emit reject
  no longer proves all same-payload carrier opportunities were lost.
- 2026-06-16 follow-up synthetic custom carrier batch reject isolation: staged
  fixed-width custom `A/09` carrier batches now use the exact-claim focus row
  to drop only an inside-failing synthetic `U/09 mask=0x20` row, then rebuild
  and re-run the full `P/05/01` exact claim before committing any remaining
  carriers. Before/after-row batch rejects and payload-build rejects still abort
  the whole staged batch. Next replay should compare inside-focused batch
  rejects against final emitted origin buckets to see whether mixed batches now
  preserve independent valid carrier opportunities.
- 2026-06-16 follow-up synthetic custom carrier plan reject isolation:
  fixed-width custom `A/09` carrier planning now skips a stale offset, stale
  source/boundary anchor, or duplicate anchor row without discarding other
  parser-verified carrier plans in the same live-object payload. A plan batch
  still returns no change if every candidate rejects before byte staging.
  Focused regressions pin duplicate-anchor and stale-source mixed batches; next
  replay should compare plan-offset/anchor rejects against final emitted origin
  buckets before widening carrier synthesis.
- 2026-06-16 follow-up fixed-width custom carrier rewrite diagnostics: valid
  packet bytes are unchanged. Following and pre-add custom `U/09` carriers now
  split into custom-rewrite-ready versus custom-rewrite-blocked counters in the
  direct live-object summary, M-frame aggregation, and focused server-dispatch
  traces. Matching custom carriers can still be identity evidence without being
  safe rewrite targets; next replay should compare these custom rewrite buckets
  with fixed-width skips and synthetic emitted origins before suppressing or
  broadening following-custom carrier handling.
- 2026-06-16 follow-up exact `U/09` appearance row guard: normal/custom
  placeable update appearance rewrites now stage the edited row in a candidate
  live-object buffer, re-run the parser-owned exact `U/09` claim at the original
  fragment cursor, and commit only if the rewritten appearance WORD/CResRef
  branch still exact-claims. Row-local rejects increment
  `exact_placeable_update_appearance_exact_rejected` without mutating the source
  row. Verified with exact-placeable, placeable-update, fixed-width,
  pending-carrier, formatter, and cargo-check runs; next replay should watch
  this counter beside custom-rewrite buckets.
- 2026-06-16 follow-up exact module-custom `U/09` rewrite counter: packet bytes
  are unchanged. Successful exact placeable update appearance rewrites now split
  module-backed custom static targets from source-custom rows through the
  live-object, M-frame, and server-dispatch summaries. Verified with the focused
  module-custom exact update regressions plus cargo-check, formatter, and
  diff-check; next replay should compare rewritten module-custom targets against
  fixed-width skips and exact-row rejects.
- 2026-06-16 follow-up synthetic custom `U/09` payload guard: valid packet bytes
  are unchanged. Synthetic fixed-width custom appearance carriers now exact-claim
  the staged `U/09 mask=0x20` row by object id, mask, appearance offset,
  unchanged fragment cursor, and the inserted WORD+CResRef payload before
  committing. Parser-valid rows with stale or mismatched CResRefs now reject as
  exact-claim mismatches. Verified with the focused synthetic pending-carrier
  regressions; next replay should compare these rejects with fixed-width
  module-custom skip and emitted-origin counters.
- 2026-06-16 follow-up synthetic custom carrier batch row-drop counters: packet
  bytes are unchanged. Inside-focused full-payload batch rejects now count the
  specific synthetic `U/09 mask=0x20` row dropped by anchor branch and insertion
  origin through direct live-object, M-frame, and server-dispatch summaries. This
  separates row-isolated batch recovery from whole-batch aborts during the next
  local replay. Verified with focused pending-carrier, exact-placeable,
  placeable-update, formatter, diff-check, and cargo-check runs.
- 2026-06-16 follow-up synthetic custom carrier after-row batch isolation:
  staged fixed-width custom `U/09 mask=0x20` batches now retry after dropping a
  focused staged carrier when the full exact-claim failure lands inside or
  after that row; failures before the first staged row still abort the batch.
  The full `P/05/01` exact claim remains the commit gate, and an unclaimable
  original suffix still prevents output mutation. Verified with focused
  pending-carrier, exact-placeable, placeable-update, formatter, diff-check,
  and cargo-check runs.
- 2026-06-16 follow-up synthetic custom carrier rewrite-count commit gate:
  valid packet bytes are unchanged. Synthetic fixed-width custom `U/09
  mask=0x20` carriers now increment `add_records_rewritten` only after the
  staged carrier survives the full exact-claim commit gate; add rows already
  rewritten in place carry that count into the pending carrier so committed
  synthesis does not double count. Verified with focused pending-carrier tests
  using a non-incremental cargo target after the default incremental cache hit a
  rustc ICE.
- 2026-06-16 follow-up blocked following-custom carrier synthesis: exact
  fixed-width custom `A/09` adds now suppress synthetic `U/09 mask=0x20`
  insertion only when an existing following custom carrier is rewrite-ready or
  already matches the selected module row's appearance and `TemplateResRef`.
  A stale/parser-owned following custom row that cannot be rewritten now stays
  unchanged and is followed by an exact-claimed replacement carrier at that
  row's fragment cursor. The after-following-custom origin is counted through
  direct live-object, M-frame, and server-dispatch summaries. Verified with
  non-incremental `pending_synthesized_custom_placeable_update`,
  `exact_placeable_`, `placeable_update`, cargo-check, formatter, and
  diff-check runs.
- 2026-06-16 follow-up custom-origin carrier reject counters: valid packet
  bytes are unchanged. Synthetic fixed-width custom `U/09 mask=0x20` plan
  offset rejects, anchor rejects, full-batch rejects, and focused row drops now
  split `after-following-custom` from `after-following-normal` origins through
  direct live-object summaries, M-frame aggregation, and server-dispatch traces.
  A fixture-free blocked-custom-carrier batch regression pins the transactional
  reject/drop path. Next replay should compare custom-origin rejects and drops
  against emitted after-following-custom carriers before broadening synthesis.
- 2026-06-16 follow-up synthetic custom carrier final-row proof: valid packet
  bytes are unchanged. After a staged fixed-width custom `U/09 mask=0x20` batch
  passes the full `P/05/01` exact claim, each emitted carrier must still
  exact-claim at its final post-insertion byte offset with the staged object id,
  appearance, `TemplateResRef`, and unchanged fragment cursor. A parser-valid
  but stale final row is now dropped and the batch rebuilt before commit.
  Verified with focused pending-carrier/final-row, exact-placeable,
  placeable-update, formatter, diff-check, and non-incremental cargo-check
  runs. Next replay should treat any final-row drop as stale staged-output or
  offset-rebuild evidence before widening custom-carrier synthesis.
- 2026-06-16 follow-up divergent duplicate synthetic carrier guard: ambiguous
  fixed-width custom `A/09` synthetic carrier plans that share the same
  parser-owned insert boundary/object/cursor but want different `WORD+CResRef`
  output are now treated as non-emitting duplicate-anchor conflicts. Identical
  duplicates still collapse to one planned row; divergent duplicates reject both
  candidate outputs so pending-row order cannot choose a module-custom carrier
  without unique output proof. Verified with non-incremental
  `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, `cargo check -q -p hgbridge-proxy2`, and
  `cargo fmt --all --check`.
- 2026-06-16 follow-up synthetic custom carrier custom-anchor counters: valid
  packet bytes are unchanged. Synthetic fixed-width custom `U/09 mask=0x20`
  plan offset rejects, anchor rejects, full-batch rejects, and focused row drops
  now distinguish normal `U/09` WORD anchors from custom `U/09` WORD+CResRef
  anchors through direct live-object summaries, M-frame aggregation, and
  server-dispatch traces. Fixture-free custom-anchor regressions cover offset,
  source-anchor, and transactional batch reject paths. Next replay should
  compare custom-anchor rejects with after-following-custom origin counters
  before broadening custom-carrier synthesis. Verified with non-incremental
  `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, `cargo check -q -p hgbridge-proxy2`, formatter, and
  diff-check runs.
- 2026-06-21 follow-up fixed-width custom carrier row-order selection: exact
  fixed-width custom `A/09` carrier lookup now selects the latest same-lifecycle
  following or pre-add `U/09` appearance row, not a custom row merely because
  one exists. A matching custom `WORD+CResRef` carrier followed by a later normal
  WORD-only appearance row now synthesizes after the later normal row when that
  row cannot be widened, preserving sequential live-object state; the pre-add
  side likewise classifies and widens the nearest prior appearance row. Verified
  with focused latest-following/latest-pre-add regressions plus non-incremental
  `exact_placeable_`, `placeable_update`,
  `pending_synthesized_custom_placeable_update`, and
  `cargo check -q -p hgbridge-proxy2`; next replay should compare remaining
  custom-anchor rejects against emitted normal/custom following-origin counters.
- 2026-06-21 follow-up fixed-width custom carrier replay telemetry: packet bytes
  are unchanged. The exact fixed-width `A/09` carrier candidate trace now emits
  selected following/pre-add carrier kind and selected rewrite-ready/blocked
  booleans, and the aggregate synthesis-policy trace exposes the normal
  following/pre-add buckets alongside custom buckets and emitted origins. Next
  replay should compare remaining custom-anchor rejects against the selected
  normal/custom buckets before widening any carrier rule.
- 2026-06-22 follow-up fixed-width custom carrier rewrite-target guard: selected
  same-lifecycle `U/09` appearance carriers now retain the exact module-custom
  output the translator can emit. A following normal or custom row suppresses
  the add-side synthetic `U/09 mask=0x20` carrier only when that rewrite output
  equals the selected fixed-width `A/09` module row; if the following row
  rewrites to a different custom output, the add gets its synthetic carrier at
  the add boundary so the later update remains the final state. Direct,
  M-frame, and server-dispatch traces now expose the following-normal/custom
  rewrite-target-mismatch buckets. Verified with focused carrier-policy tests,
  formatter, diff-check, and `cargo check -q -p hgbridge-proxy2`; next replay
  should inspect those mismatch buckets before widening any custom-carrier
  suppression rule.
- 2026-06-22 follow-up fixed-width custom carrier target-state telemetry: no
  packet bytes changed. The selected carrier decision now classifies a custom
  rewrite target as `matches-module-row`, `target-mismatch`, or
  `target-unavailable`; the old rewrite-blocked counters remain aggregate
  buckets while direct, M-frame, and server-dispatch summaries split following
  normal/custom target-mismatch from unavailable writers. The per-candidate
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM` trace also prints the selected
  following/pre-add target state. Verified with focused carrier-policy,
  exact-placeable, pending-carrier, placeable-update, formatter, diff-check,
  and `cargo check -q -p hgbridge-proxy2`; next replay should compare
  target-mismatch against unavailable before broadening suppression or
  after-following synthesis.
- 2026-06-22 follow-up pre-add carrier target-state counters: packet bytes are
  unchanged. Pre-add normal/custom `U/09` carriers now use the same
  `matches-module-row` / `target-mismatch` / `target-unavailable` split as
  following carriers while keeping the prior ready/blocked aggregate buckets.
  Direct live-object, M-frame, and server-dispatch focused traces preserve the
  split so the next `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM` replay can distinguish
  pre-add mismatch from unavailable before changing any synthesis placement
  rule. Verified with focused carrier tests, formatter, diff-check, and
  `cargo check -q -p hgbridge-proxy2`.
- 2026-06-22 follow-up pre-add committed-origin split: packet bytes are
  unchanged. Synthetic fixed-width custom `U/09 mask=0x20` carriers that are
  emitted after an `A/09` because of a pre-add normal/custom carrier now split
  their committed origin by rewrite target state while preserving the old
  aggregate ready/blocked/custom counters. This lets replay compare candidates
  against carriers that survive planning and exact-claim validation. Verified
  with focused committed-origin, `exact_placeable_`,
  `pending_synthesized_custom_placeable_update`, `placeable_update`, formatter,
  diff-check, and `cargo check -q -p hgbridge-proxy2`; next local replay should
  compare committed pre-add target-mismatch/unavailable origins against the
  candidate buckets before any placement rule change.
- 2026-06-22 follow-up carrier counter helper: packet bytes are unchanged. The
  fixed-width custom `A/09` add path now routes selected following and pre-add
  `U/09` carrier target-state accounting through one shared production helper,
  so normal/custom and match/mismatch/unavailable buckets cannot drift before a
  synthesis-placement rule change. Verified with focused `carrier` tests,
  formatter, diff-check, and `cargo check -q -p hgbridge-proxy2`; next replay
  should still compare candidate and committed mismatch/unavailable buckets
  before changing where synthetic carriers are anchored.
- 2026-06-22 follow-up following custom target-unavailable placement:
  fixed-width custom `A/09` adds now treat a later explicit custom `U/09`
  carrier with unavailable rewrite target as later packet-authored state unless
  its `WORD+CResRef` already matches the selected module row. The add target is
  synthesized at the add boundary, not after the later custom row, and direct,
  M-frame, and server-dispatch summaries expose the committed
  following-custom target-unavailable after-add bucket. Verified with focused
  `carrier`, `exact_placeable_`, `pending_synthesized_custom_placeable_update`,
  `placeable_update`, formatter, diff-check, and
  `cargo check -q -p hgbridge-proxy2`; next replay should compare remaining
  following-normal target-unavailable after-following synthesis against
  committed add-boundary following-custom target-unavailable carriers.
- 2026-06-22 follow-up following normal target-unavailable placement:
  fixed-width custom `A/09` adds now treat a later normal `U/09` carrier with
  unavailable custom rewrite target as later packet-authored state too. The
  add's module-custom carrier is synthesized at the add boundary, preserving the
  later normal row as final same-lifecycle appearance state, and direct,
  M-frame, and server-dispatch summaries expose a committed
  following-normal target-unavailable after-add bucket. Verified with focused
  `carrier`, `exact_placeable_`, `pending_synthesized_custom_placeable_update`,
  and `placeable_update` regressions; next replay should compare remaining
  target-unavailable buckets against visual/static drift before changing any
  broader custom-carrier suppression rule.
- 2026-06-22 follow-up selected carrier target-unavailable reason trace: packet
  bytes are unchanged. Selected normal/custom `U/09` carrier records now retain
  the reason their custom rewrite target is unavailable (`missing-position`,
  `position-output-unavailable`, or unique-target output unavailable), and the
  focused `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM` candidate trace prints that reason
  beside the existing selected target-state. Verified with focused
  fixed-width/carrier, exact-placeable, pending-carrier, placeable-update,
  formatter, diff-check, and `cargo check -q -p hgbridge-proxy2`; next replay
  should split target-unavailable visual/static drift by missing position versus
  failed position-output proof before changing carrier suppression.
- 2026-06-22 follow-up selected carrier target-unavailable reason counters:
  packet bytes are unchanged. The fixed-width custom `A/09` selected-carrier
  counters now split target-unavailable reasons by selected following/pre-add
  normal/custom scope, and direct live-object, M-frame, and server-dispatch
  summaries expose `missing-position`, `position-output-unavailable`, and
  unique-module-target-unavailable buckets beside the existing aggregate
  unavailable counts. Verified with focused `carrier`, `exact_placeable_`,
  `pending_synthesized_custom_placeable_update`, `placeable_update`, formatter,
  diff-check, and `cargo check -q -p hgbridge-proxy2`; next replay should compare
  reason buckets against remaining visual/static drift before changing carrier
  suppression or placement.
- 2026-06-22 follow-up stale after-following carrier origin cleanup: packet
  bytes are unchanged. The fixed-width custom `A/09` synthetic carrier planner
  no longer exposes the obsolete post-following normal/custom insertion origins
  or their direct/M-frame/server-dispatch counters; after the target-mismatch
  and target-unavailable ordering audits, every emitted module-custom carrier is
  add-boundary anchored unless a following carrier is proven to suppress it.
  Pending-carrier rejection tests now keep normal/custom anchor classes while
  counting only add-boundary origin classes. Verified with focused `carrier`,
  `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, formatter, diff-check, and `cargo check -q -p
  hgbridge-proxy2`; next replay should compare selected target-unavailable
  reason buckets against visual/static drift, not the removed after-following
  origin counters.
- 2026-06-22 follow-up committed target-unavailable reason counters: packet
  bytes are unchanged. Synthetic fixed-width custom `U/09 mask=0x20` carriers
  now carry the selected carrier target-unavailable reason through planning to
  the pending add-boundary carrier and count it only after the staged carrier
  survives full exact-claim commit, split by pre-add/following normal/custom
  after-add origin in direct live-object, M-frame, and server-dispatch
  summaries. Focused committed-origin coverage pins the committed
  `position-output-unavailable` reason buckets for pre-add normal/custom
  carriers. Verified with focused `carrier`,
  `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, formatter, diff-check, and `cargo check -q -p
  hgbridge-proxy2`; next replay should compare committed reason buckets against
  visual/static drift before changing carrier suppression or placement.
- 2026-06-22 follow-up target-unavailable uncommitted delta diagnostics:
  packet bytes are unchanged. Fixed-width custom `A/09` carrier summaries now
  derive selected, committed, and selected-minus-committed target-unavailable
  reason buckets, with saturating per-reason subtraction so overcommitted scoped
  buckets cannot hide remaining missing-position or position-output gaps.
  Direct live-object and server-dispatch traces expose
  `target_unavailable_uncommitted_*` fields for the next replay. Verified with
  focused bucket-delta and `carrier` tests plus `cargo check -q -p
  hgbridge-proxy2`; next replay should inspect uncommitted reason deltas before
  changing carrier suppression or placement.
- 2026-06-22 follow-up target-unavailable satisfied-carrier split: packet bytes
  are unchanged. Fixed-width custom `A/09` carrier summaries now split a
  selected following custom `U/09` whose rewrite target is unavailable but whose
  source `WORD+CResRef` already matches the module row into
  `target_unavailable_satisfied_by_matching_following_custom_*`, and derive
  `target_unavailable_unresolved_*` after subtracting both committed synthetic
  carriers and this packet-authored satisfied carrier bucket. Direct
  live-object and server-dispatch traces use a separate target-unavailable
  resolution event to avoid growing the already-large synthesis-policy event.
  Verified with focused following-carrier and bucket-delta tests, `carrier`,
  `exact_placeable_`, `pending_synthesized_custom_placeable_update`,
  `placeable_update`, formatter, and `cargo check -q -p hgbridge-proxy2`. Next
  local replay should compare unresolved missing-position versus
  position-output-unavailable buckets against visual/static drift before
  changing carrier suppression or placement.
- 2026-06-22 follow-up scoped target-unavailable resolution diagnostics: packet
  bytes are unchanged. The fixed-width custom `A/09` carrier summary now derives
  selected-minus-committed-minus-satisfied target-unavailable reasons per
  following-normal, following-custom, pre-add-normal, and pre-add-custom scope
  before aggregating, so a matching following-custom carrier cannot erase
  unresolved missing-position or position-output evidence from a different
  scope. Direct live-object and server-dispatch resolution traces expose the
  scoped unresolved buckets. Verified with focused `carrier`, `exact_placeable_`,
  `pending_synthesized_custom_placeable_update`, `placeable_update`, formatter,
  diff-check, and isolated-target `cargo check -q -p hgbridge-proxy2` after the
  default target hit a stale Windows PDB. Next replay should compare scoped
  unresolved buckets against visual/static drift before changing carrier
  suppression or placement.
- 2026-06-22 follow-up synthetic carrier reject trace context: packet bytes are
  unchanged. Pending synthetic `U/09` custom-carrier planning, emit, batch-drop,
  and commit traces now include the add anchor offsets/source appearance plus
  the selected carrier target-unavailable reason, so a debug replay can connect
  scoped unresolved counters to concrete `A/09` rows and rejected staged
  carriers. Verified with focused `carrier`, `exact_placeable_`,
  `pending_synthesized_custom_placeable_update`, `placeable_update`, formatter,
  diff-check, and isolated-target `cargo check -q -p hgbridge-proxy2`. Next
  replay should compare row-level `insertion_target_unavailable_reason` and
  anchor offsets against the scoped unresolved buckets before changing carrier
  suppression or placement.
- 2026-06-22 follow-up target-unavailable satisfied predicate: packet bytes are
  unchanged. Following-custom target-unavailable satisfaction now lives behind a
  policy predicate that requires the selected custom source to match the module
  row, so a stale explicit custom carrier cannot clear scoped unresolved
  evidence merely because its rewrite target was unavailable. Focused coverage
  pins both the stale-custom rejection and the matching-custom satisfaction
  path. A local `To Heir is Human` bridge attempt at
  `C:\nwnbridge\local-diamond-bridge-20260622-150423` reached only BN
  enumeration/crypto, produced no live-object/carrier trace, and created no
  quarantine; the next replay still needs a gameplay connection before
  comparing row-level target-unavailable reasons against scoped unresolved
  buckets.
- 2026-06-22 follow-up synthetic carrier emitted-row state: packet bytes are
  unchanged. Staged fixed-width custom `U/09 mask=0x20` carriers now use a
  named emitted-row record that owns final insert-offset and record-end
  calculation after lower-offset inserts or row drops. Batch focus, final-row
  exact validation, candidate rebuild, commit traces, and byte accounting now
  share that production offset rule instead of destructuring raw tuple fields.
  Verified with focused synthetic-carrier, pending-carrier, carrier, and
  exact-placeable regressions plus `cargo check`, formatter, and diff-check.
  Next replay still needs a gameplay live-object path to compare scoped
  target-unavailable reason buckets against visual/static drift.
- 2026-06-22 follow-up custom carrier target-decision state: packet bytes are
  unchanged. Selected fixed-width custom `A/09` carrier target state now flows
  through a typed decision that carries the unavailable reason into synthesis
  policy, pending insertion, counters, and debug trace. This prevents
  `target-unavailable` reason evidence from drifting between the separate
  policy/counter paths before the next gameplay replay. Verified with focused
  target-decision, `carrier`, `pending_synthesized_custom_placeable_update`,
  `exact_placeable_`, formatter, diff-check, and isolated-target `cargo check
  -q -p hgbridge-proxy2`. Next replay still needs a gameplay live-object path
  to compare scoped unresolved target-unavailable buckets against visual/static
  drift.
- 2026-06-22 follow-up custom carrier synthesis decision owner: packet bytes
  are unchanged. Fixed-width custom `A/09` carrier planning now materializes one
  synthesis decision that owns the selected following/pre-add scope, selected
  `U/09` carrier, rewrite target decision, and source-match predicate before
  policy, counters, and pending insertion consume it. The debug candidate trace
  now prints the selected synthesis scope, and focused coverage pins that the
  pending writer and counters share the same target-unavailable reason. Verified
  with focused target-decision, `carrier`,
  `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, formatter, diff-check, and isolated-target `cargo check
  -q -p hgbridge-proxy2`. Next replay still needs a gameplay live-object path
  to compare scoped unresolved target-unavailable buckets against visual/static
  drift before changing carrier suppression or placement.
- 2026-06-22 follow-up pending custom carrier selected-row evidence: packet
  bytes are unchanged. Pending synthetic fixed-width custom `U/09 mask=0x20`
  carriers now retain the selected following/pre-add carrier row scope, byte
  offsets, appearance/resref offsets, fragment cursor span, source appearance,
  and rewrite-target decision through plan, emit, batch-reject/drop, and commit
  traces. Focused coverage pins that pending insertion and counters share the
  same selected row evidence. Next replay still needs a gameplay live-object
  path to tie row-level carrier evidence to scoped unresolved
  target-unavailable buckets and visual/static drift.
- 2026-06-22 follow-up custom carrier policy derivation: packet bytes are
  unchanged. Fixed-width custom `A/09` synthesis policy is now derived only from
  the selected following/pre-add `U/09` carrier scope and target decision, with a
  scope/custom-shape debug invariant, so counters, insertion, and traces cannot
  pair a selected carrier with a separately supplied policy. Verified with
  focused policy, carrier, pending-carrier, exact-placeable, placeable-update,
  formatter, diff-check, and isolated-target `cargo check -q -p
  hgbridge-proxy2`. Next replay still needs a gameplay live-object path to
  compare scoped unresolved target-unavailable buckets against visual/static
  drift.
- 2026-06-22 follow-up custom carrier decision ownership: packet bytes are
  unchanged. Fixed-width custom `A/09` synthesis decisions are now an enum over
  no-carrier versus one selected `U/09` carrier, deriving policy and pending
  insertion origin from that state instead of storing a separately cached policy.
  Target-unavailable reasons are carried only through policies that actually
  synthesize from unavailable targets, so the pending writer cannot pair a stale
  policy with a different selected carrier. Verified with focused decision tests,
  `carrier`, `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, formatter, diff-check, and isolated-target `cargo check -q
  -p hgbridge-proxy2`. Next replay still needs a gameplay live-object path to
  compare scoped unresolved target-unavailable buckets against visual/static
  drift.
- 2026-06-22 follow-up custom carrier rewrite-target resolution: packet bytes
  are unchanged. Selected `U/09` carrier rows now store rewrite-target evidence
  as one `Available(output)` or `Unavailable(reason)` value instead of separate
  optional output/reason fields, and `TargetUnavailable` decisions require a
  concrete reason before policy, pending insertion, counters, or traces consume
  them. This removes another way for selected-carrier state to drift while the
  gameplay replay is still blocked. Verified with focused custom-carrier,
  `carrier`, `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, formatter, and isolated-target `cargo check -q -p
  hgbridge-proxy2`. Next replay still needs a gameplay live-object path to
  compare scoped unresolved target-unavailable buckets against visual/static
  drift.
- 2026-06-22 follow-up target-unavailable resolution owner: packet bytes are
  unchanged. Direct live-object and M-frame/server-dispatch diagnostics now
  consume one `ExactPlaceableCustomCarrierTargetUnavailableResolution` value
  derived from selected, committed, and satisfied scoped reason buckets, and the
  older separate uncommitted/unresolved helper paths were removed. Verified with
  focused bucket-delta, `carrier`,
  `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, formatter, diff-check, and isolated-target `cargo check
  -q -p hgbridge-proxy2`. Next replay still needs a gameplay live-object path
  to compare the owned scoped resolution buckets against visual/static drift.
- 2026-06-23 follow-up scoped target-unavailable resolution model: packet bytes
  are unchanged. The fixed-width custom carrier target-unavailable resolver now
  materializes per-scope selected/committed/satisfied/uncommitted/unresolved
  snapshots and derives aggregate totals from that scoped model; direct
  live-object trace consumption no longer flattens raw unresolved buckets
  separately from the production resolver. Verified with focused bucket-delta,
  `carrier`, `pending_synthesized_custom_placeable_update`, `exact_placeable_`,
  `placeable_update`, formatter, diff-check, and isolated-target `cargo check
  -q -p hgbridge-proxy2`. Next replay still needs a gameplay live-object path to
  compare the scoped snapshots against visual/static drift before changing
  custom-carrier suppression or placement.
- 2026-06-23 follow-up target-unavailable resolution state owner: packet bytes
  are unchanged. The fixed-width custom carrier target-unavailable resolver now
  stores one scoped resolution model and derives selected/committed/satisfied,
  uncommitted, unresolved, aggregate, and by-scope views through accessors; the
  dispatcher trace can no longer peek at duplicated raw unresolved fields. Verified
  with focused bucket-delta, `carrier`, `pending_synthesized_custom_placeable_update`,
  `exact_placeable_`, `placeable_update`, formatter, and isolated-target
  `cargo check -q -p hgbridge-proxy2`. Next replay still needs a gameplay
  live-object path to compare scoped unresolved buckets against visual/static
  drift before changing custom-carrier suppression or placement.
- 2026-05-25 `P/04/01` zero-count static-tail ownership audit: hardened the
  static direction normalizer and module-resource static-row repair helpers so
  row-shaped bytes after a zero static-placeable count remain unclaimed until
  the dedicated drop path trims them. This follows the Diamond/EE decompile
  rule that the preceding WORD count owns the loop. Added public fixture-free
  coverage proving normalization and GIT repair do not promote the tail, and
  that the drop path preserves the exact source cursor proof.
- 2026-05-25 `P/04/01` post-static zero-WORD verifier audit: tightened the
  exact EE `Area_ClientArea` reader proof so both proxy-owned post-static tail
  counts must be zero. The bridge dialect inserts these two WORDs for legacy
  packets, and no non-empty first-list row shape has decompile proof yet; the
  old proof could skip arbitrary WORDs after static-placeable rows and still
  claim an exact EE cursor. Added fixture-free public coverage proving the
  zero/zero tail is accepted and nonzero first or second post-static counts are
  rejected.
- 2026-05-25 `P/04/01` transition-label cursor audit: shared the
  decompile-backed transition-row fragment-bit walk across the exact legacy/EE
  tail proofs, zero-sound repair, and placeable-context collector. Transition
  labels consume a visibility BOOL, the CExoLocString TLK/direct selector, and
  one extra BOOL before the DWORD TLK branch; the context collector previously
  skipped every transition as an inline CExoString and could silently lose later
  static-placeable state when a valid TLK branch preceded it. Added
  fixture-free public coverage proving a TLK transition preserves the exact
  source cursor and still exposes following static-placeable context.
- 2026-05-25 `P/04/01` map-pin cursor audit: shared the decompile-backed
  byte-only map-pin walker with the placeable-context collector. Map-pin rows
  are `DWORD id + CExoString label + three FLOAT coordinates` and do not consume
  CNW fragment bits; the old context-only walker used a narrower label bound
  than the exact Area_ClientArea proof and could drop later static-placeable
  context after a valid long label. Added fixture-free coverage proving a long
  map-pin label preserves the exact source cursor and still exposes following
  static-placeable rows.
- 2026-05-25 `P/04/01` sound-row cursor audit: shared the decompile-backed
  sound-list walker across the legacy exact proof, EE exact proof, and
  placeable-context collector. Sound rows are a `WORD` count, a fixed 54-byte
  sound-object byte body with a nested `WORD` CResRef count plus 16-byte resrefs,
  and six CNW fragment BOOLs per row; later light/static placeable rows are now
  exposed as context only when that byte and bit cursor movement is proven.
  Added fixture-free coverage for a sound row followed by a static placeable,
  and for rejecting the same byte shape when the six sound bits are missing.
- 2026-05-26 `P/04/01` zero sound-count repair audit: generalized the compact
  single-CResRef sound repair so it no longer depends on the originally observed
  no-map-pin area shape. The repair now reaches sound rows only after the shared
  transition walker and byte-only map-pin walker prove the decompiled cursor;
  the row-local rule remains the same (`WORD` count zero plus one plausible
  fixed CResRef after the 54-byte sound body becomes count one), and the final
  legacy/EE exact proofs still own the six sound BOOLs. Public fixture-free
  coverage now proves a map-pin row before a compact sound row repairs to an
  exact source cursor and still exposes the following static-placeable context.
  Verified with `cargo test -q -p hgbridge-proxy2
  zero_sound_count_repair_uses_shared_map_pin_cursor -- --nocapture` and
  `cargo test -q -p hgbridge-proxy2 public_static_direction_tests --
  --nocapture`.
- 2026-05-28 `P/04/01` zero sound-count staged-repair audit: hardened the same
  compact sound helper so row-local `0 -> 1` CResRef count fixes are staged
  first, then accepted only when the full post-tile source proof consumes the
  exact six sound BOOLs and following lists. A malformed compact row with an
  extra sound fragment bit is now left untouched instead of being partially
  mutated before final rejection. Verified with `cargo test -q -p
  hgbridge-proxy2 zero_sound_count_repair -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 sound -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  exact_ee_area_proof_rejects -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 public_static_direction_tests -- --nocapture`.
- 2026-05-25 `P/04/01` light-placeable cursor audit: shared the decompile-backed
  light-row walker across the legacy exact proof, EE exact proof, and
  placeable-context collector. Light rows are byte-only after their `WORD`
  count: `OBJECTID`, `WORD appearance`, and one position triplet, with no CNW
  fragment BOOLs before the following static-placeable count. Context collection
  now rejects unproven light rows instead of exposing later static rows from a
  shifted cursor. Added fixture-free coverage for a valid light row before a
  static row and for rejecting a light row outside the legacy object-id
  namespace.
- 2026-05-25 `P/04/01` static context tail-proof audit: tightened
  `AreaPlaceableContext` extraction so static/live-overlap state is exported
  only after the full legacy post-tile tail proof succeeds. The collector now
  checks the shared exact proof's static count cursor before exposing rows,
  follows the decompiled static `WORD` count when zero-count row-shaped bytes
  are present, and rejects trailing bytes after claimed static rows instead of
  letting a partial prefix seed later live-object diagnostics. Added
  fixture-free coverage for both zero-count tail ownership and non-exact
  post-static trailing bytes.
- 2026-05-27 `P/04/01` module-backed static-row match audit: no packet behavior
  changed, but the generalized GIT repair rule now has public fixture-free
  coverage. Static rows have no tag/resref, so module-backed appearance/
  position/bearing repair may only run after the decompiled static row cursor is
  proven and a unique module row matches by appearance plus at least two
  placement coordinates. The regression proves appearance plus one coordinate
  leaves bytes untouched, while exactly two matching coordinates can repair the
  drifted third coordinate and bearing without moving the source cursor.
  Verified with `cargo test -q -p hgbridge-proxy2
  module_static_row_repair_requires_appearance_plus_two_coordinates --
  --nocapture` and `cargo test -q -p hgbridge-proxy2
  public_static_direction_tests -- --nocapture`.
- ~~2026-06-03 `P/04/01` malformed module static-geometry proof audit:
  tightened module-backed static-placeable repair/context so a local GIT row can
  only serve as packet proof when its replacement coordinates and bearing stay
  inside the same decompiled static-row value domain accepted by the source and
  EE row validators. Appearance plus two coordinates still proves the intended
  generalized repair, but malformed finite resource geometry no longer poisons
  the packet cursor proof or seeds later trap/use/lock state context. Verified
  with `cargo test -q -p hgbridge-proxy2
  malformed_module_static_geometry_is_not_resource_proof -- --nocapture`.~~
- ~~2026-06-03 `P/04/01` fragmented no-name area resource ambiguity audit: no
  packet behavior changed, but public fixture-free coverage now pins the
  resource selector used by zero-dimension no-name area packets whose static
  CResRef bytes are split into multiple ASCII fragments. The packet-local
  tileset and fragmented resref may identify one module ARE, but duplicate
  matching ARE rows or a tileset mismatch stay unowned so module-backed tile and
  static-placeable repair cannot become a wildcard resource guess. Verified
  with `cargo test -q -p hgbridge-proxy2
  fragmented_no_name_area_resource_requires_unique_resref_tileset_match --
  --nocapture`.~~
- ~~2026-06-03 `P/04/01` named static module-resource candidate audit: tightened
  the named-area static-placeable resource selector so area resref, tileset,
  tile grid, and static count are not enough to authorize a GIT-backed
  candidate. The selector now reuses the same decompile-owned static-row cursor
  and unique appearance/coordinate row-identity proof as the staged repair
  writer before any module state can seed static-row repair/context. Verified
  with `cargo test -q -p hgbridge-proxy2
  named_static_resource_candidate_requires_unique_row_identity_not_count --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 module_static_row_repair --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 area::tests:: --
  --test-threads=1`, `cargo fmt --all --check`, `cargo check -q -p
  hgbridge-proxy2`, and `git diff --check`.~~
- ~~2026-05-27 `P/04/01` module-backed zero-appearance static-row audit:
  extended the same GIT repair rule for legacy rows whose appearance WORD is
  zero while the decompiled static cursor and local module resource prove the
  row. Nonzero appearances must still match the GIT appearance with at least
  two coordinates, but zero appearances are treated as missing only when all
  three placement coordinates match one remaining static GIT placeable. This
  repairs appearance and bearing from module state without accepting a shifted
  static-row cursor. Public fixture-free coverage now proves the accepted
  zero-appearance/full-coordinate case, the rejected
  zero-appearance/two-coordinate case, and the rejected nonzero-appearance
  mismatch case; the local Contest item-area fixture still rewrites to exact
  EE proof under that generalized rule. Verified with `cargo test -q -p hgbridge-proxy2
  module_static_row_repair_allows_zero_appearance_only_with_all_coordinates --
  --nocapture` and `cargo test -q -p hgbridge-proxy2 area::tests:: -- --nocapture`.
  2026-06-03 re-audit added context-boundary coverage with
  `module_context_state_allows_zero_appearance_only_with_full_row_identity`.~~
- ~~2026-05-27 `P/04/01` static-placeable context proof audit: corrected the HG
  Docks zero-sound-count fixture expectation so absent local module proof stays
  absent. The test now supplies an explicit empty module context and proves that
  static-row context does not invent GIT trap/use/lock state when no module ARE
  resource is resolved; module-backed state remains reserved for rows uniquely
  matched to a proven local resource. Verified with `cargo test -q -p
  hgbridge-proxy2 docksofascension_rewrite_repairs_legacy_zero_sound_counts --
  --nocapture` and `cargo test -q -p hgbridge-proxy2`. 2026-06-03 re-audit
  added `module_context_state_allows_zero_appearance_only_with_full_row_identity`
  to prove the same strict row identity rule at the module-state handoff.~~
- 2026-05-27 `P/04/01` transition direct-label cursor audit: no packet behavior
  changed, but public fixture-free coverage now proves the decompiled
  transition-row CExoString branch owns exactly the visibility BOOL and
  TLK/direct selector before the direct string bytes. The TLK-only guard BOOL
  is consumed only on the TLK DWORD branch; an extra bit after a direct label
  keeps the post-tile tail unclaimed and prevents static-placeable context from
  being exposed from a shifted cursor. Verified with `cargo test -q -p
  hgbridge-proxy2 direct_transition -- --nocapture`.
- 2026-05-26 `P/05/01` creature full-appearance locstring-name audit: no packet
  behavior changed, but public fixture-free coverage now proves the full
  `P/5` appearance name branch where the outer selector enters the locstring
  pair, the first component takes the TLK/custom-token branch, and the second
  component takes the inline CExoString branch. The tests prove Diamond source
  cursor ownership of the outer bit, TLK inner bit, client-TLK/language bit,
  and second inline bit, reject a missing component bit or direct-string
  reinterpretation of token bytes, then verify that the same four-bit cursor
  survives the legacy-to-EE build-0x23 body widening. Verified with `cargo test
  -q -p hgbridge-proxy2 full_appearance_locstring_name -- --nocapture`.
- 2026-05-26 `P/05/01` creature full-appearance direct-name/zero-body audit:
  no packet behavior changed, but public fixture-free coverage now proves the
  direct `CExoString` name branch consumes exactly one source BOOL and rejects
  locstring reinterpretation of the same read-buffer bytes. The same pass
  covers the decompiled body selector `0` branch: it consumes only the selector
  byte, does not synthesize a nineteen-part body table, inserts only the EE
  build-0x23 scalar high byte plus the feature-0x0E tail byte, and preserves the
  one-bit cursor through exact EE validation. Verified with `cargo test -q -p
  hgbridge-proxy2 full_appearance_ -- --nocapture`.
- 2026-05-26 `P/05/01` creature full-appearance visible-equipment audit: no
  packet behavior changed, but public fixture-free coverage now proves the
  decompiled handoff from full-body fields to visible-equipment count and a
  nested `A` item row. A direct creature-name source consumes one BOOL, the
  visible item no-name active-property tail consumes the next four Diamond BOOLs
  after the model-type-2 item body, a missing active-property BOOL rejects the
  record, and the EE-only active-property BOOL is inserted immediately after the
  shared pre-DWORD BOOL without moving the item boundary. Verified with
  `cargo test -q -p hgbridge-proxy2 full_appearance_visible_equipment -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2 full_appearance_ -- --nocapture`.
- 2026-05-26 `P/05/01` creature full-appearance visible-equipment token-name
  repair: fixed a generalized bit-cursor bug in the already-EE-shaped
  appearance name repair path. When a nested visible-equipment item is
  byte-proven as the active-property locstring-token branch but stale source
  bits still select direct or locstring-inline name mode, the repair must
  materialize both the token inner selector and the token language bit before
  active-property BOOLs. Reusing the next semantic bit as the language selector
  could leave the exact EE validator green while shifting active-property state.
  Added fixture-free coverage for direct -> token, locstring-inline -> token,
  and full `P/5` EE repair through exact validation. Verified with `cargo test
  -q -p hgbridge-proxy2 visible_equipment_item_token_rewrite -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2
  full_appearance_visible_equipment_locstring_token_repair -- --nocapture`.
- 2026-05-26 `P/05/01` creature full-appearance visible-equipment inline-name
  repair: fixed the reciprocal stale-selector case for nested item names. The
  decompiled item-name fragment widths are direct `CExoString` = one BOOL,
  locstring-inline = outer + inner BOOLs, and locstring-token = outer + inner +
  language BOOLs. The repair path now resizes that selector prefix before
  active-property BOOLs, so token -> locstring-inline drops the stale language
  bit and token -> direct drops both locstring helper bits instead of leaving an
  exact-validator-green shifted active-property state. Added fixture-free
  coverage for direct/locstring-inline branch selection, EE widening, helper
  insertion/removal, and full already-EE-shaped `P/5` repair. Verified with
  `cargo test -q -p hgbridge-proxy2 visible_equipment_item_ -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2 full_appearance_visible_equipment --
  --nocapture`.
- 2026-05-26 `P/05/01` creature full-appearance visible-equipment
  active-property tail audit: no packet behavior changed, but public
  fixture-free coverage now proves nonzero active-property rows and the
  two-BYTE mask trailer inside a nested item row. The second mask owns exactly
  one following BYTE for each set bit; missing or trailing value bytes reject
  the tail. The full appearance rewrite still consumes only the direct creature
  name bit plus Diamond's four active-property BOOLs, then inserts EE's single
  `CanUseItem` BOOL after the shared pre-DWORD BOOL without shifting the byte
  tail. Verified with `cargo test -q -p hgbridge-proxy2 value_mask --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  full_appearance_visible_equipment -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 full_appearance_ -- --nocapture`.
- 2026-05-26 `P/1E/01` quickbar model-type-3 appearance audit: fixed the
  quickbar EE writer to mirror the live-object item appearance rule for
  `sub_14079FAC0`. Diamond supplies `DWORD base item + 19 BYTE model parts + 6
  palette bytes`; EE build-0x23 reads the same model parts as 19 WORDs, keeps
  the six palette bytes, then reads the EE-only 19x6 armor/accessory color
  table before the visual-transform map and active-property reader. The
  quickbar writer now repeats Diamond's six palette bytes for each EE table row
  instead of zero-filling a cursor-valid but visually wrong table. Verified
  with `cargo test -q -p hgbridge-proxy2 model_type_3_quickbar --
  --nocapture`.
- 2026-05-26 `P/1E/01` quickbar active-property cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the nested
  item active-property bit order after quickbar item appearance widening.
  Direct `CExoString` names consume one BOOL, custom-token locstring names
  consume outer + inner BOOLs before language/string-ref bytes, and EE's
  `sub_14076BD30` `CanUseItem` BOOL is inserted after the shared pre-DWORD
  active-property BOOL rather than stealing a post-DWORD state bit. The tests
  also cover nonzero active-property rows, value-mask bytes, secondary-item
  guard placement, and validator rejection when the EE-only bit is flipped
  true. Verified with `cargo test -q -p hgbridge-proxy2 active_property_ --
  --nocapture` and `cargo test -q -p hgbridge-proxy2 quickbar -- --nocapture`.
- 2026-05-26 `P/1E/01` quickbar item-appearance validator audit: tightened the
  exact EE SetAllButtons validator so model-type 0/1/2/3 fields widened from
  Diamond BYTEs must remain zero-extended WORDs. This mirrors the live-object
  item proof for EE build-0x23 and prevents a semantically unsupported nonzero
  high byte from passing the byte-shape validator. Public fixture-free coverage
  now proves model-type 0 shield, type 1 cloak, type 2 weapon, type 3 armor,
  and locstring-inline active-property name cursor order. Verified with
  `cargo test -q -p hgbridge-proxy2 model_type_ -- --nocapture`, `cargo test
  -q -p hgbridge-proxy2 active_property_locstring_inline -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 quickbar -- --nocapture`, and `cargo check
  -q -p hgbridge-proxy2`.
- 2026-05-29 `P/1E/01` quickbar command-tail cursor audit: hardened the compact
  item recovery boundary so the trailing command-line compatibility tail accepts
  only the decompiled type-18 two-CExoString command shape with no suffix or a
  single zero DWORD empty-string-length artifact. It no longer discards an
  arbitrary four-byte read-buffer suffix after the two strings. Verified with
  `cargo test -q -p hgbridge-proxy2 quickbar_command_tail -- --nocapture`.
- 2026-05-30 `P/1E/01` final-slot command-tail re-audit: applied the same
  zero-DWORD suffix rule to the normal 36th-slot type-18 reader path. Diamond
  `sub_469FD0` and EE `sub_14079DB00` own only the two CExoString command
  fields; a four-byte suffix can prove a legacy empty-string artifact only when
  it is zero, not arbitrary read-buffer data. Public fixture-free coverage now
  accepts the zero suffix and rejects a nonzero suffix while the full quickbar
  suite still passes.
- 2026-05-30 `P/1E/01` quickbar arbitrary-resync audit: removed the generic
  non-item slot resync scorer from the source reader. Diamond `sub_469FD0` and
  EE `sub_14079DB00` iterate exactly 36 slot records; they do not skip an
  unowned byte before a later plausible spell/general slot. Compact item
  recovery remains available only through the typed item-body parser plus the
  36-slot boundary scorer. Public fixture-free coverage now proves a shifted
  byte before slot 0 cannot be discarded to claim a later spell slot. Verified
  with `cargo test -q -p hgbridge-proxy2
  split_rejects_resynced_leading_byte_before_spell_slot -- --nocapture` and
  `cargo test -q -p hgbridge-proxy2 quickbar -- --nocapture`.
- 2026-05-29 `P/1E/01` quickbar exact-validator padding audit: hardened the
  EE `SetAllButtons` shape validator so, after the 36-slot read-buffer cursor
  and final CNW fragment bit cursor are exact, every unused low bit in the
  final fragment byte must be zero. This is intentionally limited to the
  bridge-emitted EE validator: legacy source captures can carry nonzero
  fragment storage bytes, but those bytes are not proof for accepted EE output.
  Verified with `cargo test -q -p hgbridge-proxy2
  exact_quickbar_validator_rejects_nonzero_fragment_padding_bits --
  --nocapture` and `cargo test -q -p hgbridge-proxy2 quickbar -- --nocapture`
  using `CARGO_TARGET_DIR=C:\nwnbridge\codex-target\nwn-ee-bridge` to avoid
  Google Drive target-cache stalls.
- 2026-05-26 `P/05/01` live-object item appearance audit: tightened the shared
  top-level item-add / GUI item-create EE model-type-3 validator so the
  19x6 armor/accessory color table must repeat the six Diamond palette bytes
  immediately before it. A zero-filled table is byte-cursor-valid but loses
  nonzero palette semantics, so it is no longer accepted as a bridge-owned
  exact shape. Public fixture-free coverage now proves model-type 0/1/2/3
  widened high-byte rejection for item-add and item-create records, and
  model-type-3 zero-table rejection with exact active-property bit order.
  Verified with `cargo test -q -p hgbridge-proxy2
  item_add_exact_validator_rejects_nonzero_widened_appearance_high_bytes --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  item_create_exact_validator_rejects_nonzero_widened_appearance_high_bytes --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  model_type_3_item_exact_validator_requires_palette_seeded_ee_table --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 visible_equipment --
  --nocapture`.
- 2026-05-26 `P/05/01` creature identity/class-row rules-table audit: fixed
  the `U/5` mask `0x1000` identity branch so a loaded `classes.2da` owns the
  row optional-byte policy before the stock fallback. EE `sub_140781E80`
  (`loc_140785330`) and Diamond `sub_44ADD0` both read the fixed
  class-id/class-level bytes, then conditionally read one spell-option byte
  from the rules `+0x4F8` flag and two domain bytes from `+0x4F4`; a
  server-specific merged table can therefore change the cursor width for a
  stock row id. Public fixture-free tests now prove 2DA parsing for fixed,
  cleric/domain, wizard/spell-option, and custom spell-option rows; loaded
  table precedence over stock Wizard fallback; and rejection of absent loaded
  rows instead of silently falling back. Verified with `cargo test -q -p
  hgbridge-proxy2 class_row -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 identity -- --nocapture`, `cargo fmt --all --check`, and
  `cargo check -q -p hgbridge-proxy2`.
- 2026-05-26 `P/05/01` inventory `0x4000` state-stream cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the
  decompile-backed row bit ownership for Diamond `sub_455940` and EE
  `sub_1407B4F70`. The branch reads a WORD row count; `S` rows consume only two
  read-buffer bytes, while `U` rows consume WORD, one BOOL, one BYTE, then a
  final WORD. The exact inventory validator must therefore advance one fragment
  bit per `U` row, none for `S`, reject missing row bytes, and reject
  byte-complete records when the owned `U` BOOLs are absent. Verified with
  `cargo test -q -p hgbridge-proxy2 inventory_4000_state_stream -- --nocapture`.
- 2026-05-26 `P/05/01` generic inventory `0x0200` branch-candidate audit:
  fixed exact validation so ambiguous generic mask candidates are selected by
  the owned fragment BOOL requirements instead of the first byte-valid cursor.
  Diamond `sub_455940` and EE `sub_1407B4F70` read two BOOLs before the branch;
  the second BOOL selects the DWORD-count path when false, while the first BOOL
  controls whether counted cells own per-cell BOOLs and can vary on a zero
  count without moving the read-buffer cursor. Public fixture-free coverage now
  proves zero-count first BOOL false/true, counted-cell first BOOL true with no
  cell BOOLs, counted-cell first BOOL false with per-cell BOOLs, and rejection
  when the second BOOL selects the byte-mask branch. Verified with `cargo test
  -q -p hgbridge-proxy2 inventory_0200_ -- --nocapture` and `cargo test -q -p
  hgbridge-proxy2 inventory -- --nocapture`.
- 2026-05-26 `P/05/01` inventory `0x0400` equipment-delta cursor audit: no
  packet behavior changed, but public fixture-free coverage now proves the
  Diamond `sub_455940` (`00457182..004572D2`) and EE `sub_1407B4F70`
  (`1407B6D51..1407B6FA9`) legacy-build order. Clear slots consume only
  read-buffer bytes; set slots consume one CNW BOOL each; and those set-slot
  BOOLs are owned before a following `0x0200` branch reads its two BOOLs. The
  tests also reject truncated clear/set byte lists and byte-complete records
  with missing set-slot BOOLs. Verified with `cargo test -q -p hgbridge-proxy2
  inventory_0400_ -- --nocapture`.
- 2026-05-26 `P/05/01` inventory `0x0800`/`0x1000` cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the adjacent
  Diamond `sub_455940` and EE `sub_1407B4F70` branches. `0x0800` reads one CNW
  BOOL; false consumes no read-buffer bytes, true consumes exactly twelve
  BYTEs before the later `0x4000` state stream. `0x1000` is local UI-state work
  and consumes no bytes or BOOLs (`004559D0..00455AAD` / `1407B50D7..1407B51ED`).
  The tests prove the selector-only false branch, exact twelve-byte true branch,
  `0x0800`-before-`0x4000` bit order, missing `0x4000` `U` row BOOL rejection,
  and no phantom `0x1000` cursor. Verified with `cargo test -q -p
  hgbridge-proxy2 inventory_0800_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 inventory_1000_ -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 inventory -- --nocapture`.
- 2026-05-26 `P/05/01` inventory `0x0100` opcode-stream cursor audit: no
  packet behavior changed, but public fixture-free coverage now proves this
  branch is byte-only. Diamond `sub_455940` (`00457939..00457EB3`) and EE
  `sub_1407B4F70` (`1407B7686..1407B79BA`) read a BYTE row count and one CHAR
  opcode per row. `D` rows read two WORDs; `S`/`U` rows read two WORDs plus an
  OBJECTID; `A` rows read two WORDs, optional OBJECTID, optional three FLOATs
  for item types `0`/`2`/`4`/`12`/`19`, and a trailing DWORD for types `4`/`19`.
  No `0x0100` row calls `ReadBOOL`; following mask branches own the next CNW
  fragment bit. Tests now prove no `0x0100` fragment-bit ownership, exact `A`
  row widths, and aligned handoff to following `0x2000` Feature-25 BOOLs.
  Verified with `cargo test -q -p hgbridge-proxy2 inventory_0100_ --
  --nocapture` and `cargo test -q -p hgbridge-proxy2 inventory --
  --nocapture`.
- 2026-05-26 `P/05/01` inventory `0x2000` Feature-25 cursor audit: fixed a
  generalized generic-mask handoff bug. Diamond `sub_455940` and EE
  `sub_1407B4F70` (`loc_1407B79E6`) read `DWORD first_count`, first-list
  OBJECTIDs, `DWORD second_count`, second-list OBJECTIDs, then three CNW BOOLs
  per second-list object; the branch is read before `0x0800` and `0x4000`.
  Generic combined masks must therefore prefix-claim the `0x2000` object lists
  and leave following bytes/bits to later decompiled branches instead of
  requiring `0x2000` to end the record. Public fixture-free tests now prove
  standalone second-list BOOL ownership, `0x2000 -> 0x0800` selector handoff,
  and `0x2000 -> 0x4000` update-BOOL handoff. Verified with `cargo test -q -p
  hgbridge-proxy2 inventory_2000_ -- --nocapture`.
- 2026-05-26 `P/05/01` inventory `0x0001` compact/extended cursor audit:
  fixed the stale `0x0401` mask-specific bypass that accepted a true
  `0x0001` BOOL as if it could hand off directly to `0x0400`, then modeled the
  legacy true-branch tail. Diamond `sub_455940` (`00455AAD..00455D80`) and EE
  `sub_1407B4F70` (`1407B51ED..1407B559F`) both read `SHORT, DWORD, INT,
  BOOL`; false is the compact handoff, while true reads `WORD`, one BYTE per
  standard legacy Skills.2DA row, `INT+CExoString`, `INT+CExoString`, the
  legacy-build BYTE, and the final BYTE before later mask branches. Public
  fixture-free tests now prove compact false acceptance, true-without-tail
  rejection, full legacy extended-tail ownership, exact handoff to following
  `0x0400`, and truncated CExoString rejection. Verified with `cargo test -q
  -p hgbridge-proxy2 inventory_0001_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 inventory_0401_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 inventory -- --nocapture`, `cargo fmt --all --check`, `git
  diff --check`, and `cargo check -q -p hgbridge-proxy2`. Remaining caveat:
  EE's newer build-gated first tail scalar is DWORD-sized; proxy2 still claims
  only the Diamond/HG legacy BYTE path until an EE-to-EE or target-server
  reference needs that variant.
- 2026-05-31 `P/05/01` inventory `0x0001` EE extended-tail width audit:
  closed the earlier caveat by making the generic inventory cursor model accept
  both decompile-backed true-branch tail widths. Diamond/HG legacy streams keep
  the BYTE scalar plus final BYTE after the skill/string tail, while newer EE
  streams own a DWORD scalar plus final BYTE at the same branch. Both variants
  consume the same single `0x0001` selector BOOL and no additional fragment
  bits before later mask branches such as `0x0400`. Verified with
  `cargo test -q -p hgbridge-proxy2 inventory_0001 -- --nocapture`, `cargo
  test -q -p hgbridge-proxy2 inventory -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`.
- 2026-05-26 `P/05/01` inventory fragment-neutral branch audit: no packet
  behavior changed, but public fixture-free coverage now proves the
  decompile-backed cursor rules for adjacent generic mask branches that can
  otherwise hide shifted fragment BOOLs. Diamond `sub_455940` and EE
  `sub_1407B4F70` read `0x0002`/`0x0008` as DWORDs, `0x8000` as three INTs,
  `0x0080`/`0x0040` as grouped ten-bit byte values, and `0x0004` as two
  counted three-byte icon/list tuple streams, all without `ReadBOOL`.
  Legacy-build `0x0010` reads three simple category pairs without BOOLs, while
  `0x0020` reads three rich categories where only each second-list row owns two
  CNW BOOLs after its seven read-buffer bytes. Tests now prove fixed-scalar
  handoff to `0x0200`, ten-bit handoff to `0x4000`, `0x0024`
  rich-category -> icon-list handoff, icon-list -> `0x0200` handoff, and
  rejection of truncated ten-bit/rich/icon tuple bodies. Verified with
  `cargo test -q -p hgbridge-proxy2 inventory_00 -- --nocapture`,
  focused `inventory_0040_`, `inventory_0080_`, `inventory_0010_`,
  `inventory_002`, `inventory_0004_`, `inventory_0024_`, and
  `inventory_ten_bit_group` filters, full `cargo test -q -p hgbridge-proxy2
  inventory -- --nocapture`, `cargo fmt --all --check`, `git diff --check`,
  and `cargo check -q -p hgbridge-proxy2`.
- ~~2026-05-28 `P/05/01` inventory `0xD5FF` terminal fragment-cursor audit:
  removed the terminal compatibility drain. Diamond `sub_455940` and EE
  `sub_1407B4F70` return to the caller after the enabled inventory mask
  branches; no decompiled post-`0x4000` terminal storage byte owner exists.
  D5FF rows now consume only candidate bits proven by typed branch counts,
  whether terminal or midstream; residual terminal bits are not treated as
  inventory-owned and exact claim rejects them. For terminal legacy captures,
  the rewrite pass trims the transport fragment storage only after the live
  records advance with a reliable decompile-backed cursor and the final
  rewritten payload exact-claims. Public coverage rejects one-bit and full-byte
  D5FF reader residue, and fixture coverage now proves both Starcore5 and
  private XP2 terminal D5FF storage rewrite to exact EE-shaped payloads instead
  of being drained by the inventory validator. Verified with
  `RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q -p
  hgbridge-proxy2 local_xp2_seq26_current_player_d5ff_inventory_terminal_tail_rewrites_to_exact_claim
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 d5ff -- --nocapture`,
  and `cargo test -q -p hgbridge-proxy2 live_object_update -- --nocapture`.~~
- ~~2026-05-29 `P/05/01` inventory `0xD500` missing-low D5FF mask repair
  audit: resolved 2026-05-29. The `0xD500 -> 0xD5FF` repair now requires the
  same typed inventory cursor advance used by the live-object rewrite loop
  before mutating the mask byte: Diamond `sub_455940` and EE `sub_1407B4F70`
  both read the compact `0x0001` branch BOOL first, and this repaired shape
  owns that BOOL as false. Terminal residual fragment storage is still trimmed
  only after that reliable typed cursor proof. A byte-complete D500 body with a
  true compact-branch bit now rejects and leaves the legacy mask untouched
  instead of turning a shifted cursor into an apparently valid D5FF row.
  Verified with `cargo test -q -p hgbridge-proxy2 d5ff -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2
  local_winds_eremor_seq22_placeable_stream_rewrites_to_exact_shape --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 live_object_update --
  --nocapture`.~~
- ~~2026-05-29 `P/05/01` inventory `0x2000` Feature-25 terminal-tail audit:
  resolved 2026-05-29. The generic inventory mask walker no longer lets the
  standalone legacy zero-first/sentinel-tail compatibility shape claim bytes
  when later mask branches still exist. Diamond `sub_455940` and EE
  `sub_1407B4F70` both process `0x2000` before `0x0800`/`0x4000`; therefore
  combined masks must parse Feature-25 as a prefix and hand off to the later
  branch BOOL/read-buffer contract. Public fixture-free coverage proves a
  sentinel-looking tail cannot be swallowed as `0x2000` before an `0x0800`
  branch. Verified with `cargo test -q -p hgbridge-proxy2 inventory_2000 -- --
  nocapture`, `cargo test -q -p hgbridge-proxy2 inventory -- --nocapture`, and
  `cargo test -q -p hgbridge-proxy2 live_object_update -- --nocapture`.~~
- ~~2026-05-26 `P/05/01` looping visual-effect update mixed-map audit: fixed
  exact ownership so a `U/* 0x00000008` row list may be all Diamond/HG short
  rows or all EE build-0x23 rows with `ObjectVisualTransformData`, but not a
  mixed partial rewrite. EE `sub_1407B1F00` reads the compact count/opcode/WORD
  rows and, on the modern visual-transform branch, owns one transform map after
  every row; a per-row mixture leaves the next boundary ambiguous and must stay
  quarantined instead of receiving duplicate maps. Public fixture-free coverage
  now proves mixed rows are neither rewritten nor exact-claimed. Verified with
  `cargo test -q -p hgbridge-proxy2
  mixed_looping_effect_transform_rows_remain_unclaimed -- --nocapture`.~~
- ~~2026-05-26 `P/05/01` creature status-effect compact-target audit: tightened
  the combined `U/5` status-effect cursor so EE-shaped rows with the optional
  compact target payload are exact-owned only for the single-entry shape proven
  without `visualeffects.2da` row-type state. EE `sub_1407B1F00` reads any
  row-type target payload before `ObjectVisualTransformData`; mixed target /
  no-target rows or multi-row target payload lists make the row/map boundary a
  guess even when the byte cursor can land exactly. Fixture-free public-repo
  coverage now proves single compact-target acceptance and multi/mixed target rejection.
  Verified with `cargo test -q -p hgbridge-proxy2 creature_status_effect_ --
  --nocapture`.~~
- ~~2026-05-27 `P/05/01` visual-effect target-payload width audit: corrected the
  prior status-effect target proof from a three-byte compact span to the actual
  decompiled `DWORD object id + BYTE` branch. Diamond `sub_44ED20` and EE
  `sub_1407B1F00` both read `BYTE opcode`, `WORD visualeffects.2da row`, then
  for `Type_FD` `P`/`B` rows read the five-byte target payload before EE's
  `ObjectVisualTransformData` map. Public fixture-free coverage now accepts the
  five-byte single-target shape and rejects the stale three-byte shape.
  Verified with `cargo fmt --all --check`, `git diff --check`,
  `cargo test -q -p hgbridge-proxy2 creature_status_effect_ -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2
  looping_effect_target_payload_owns_dword_object_id_plus_byte -- --nocapture`,
  and `cargo check -q -p hgbridge-proxy2`.~~
- ~~2026-05-27 `P/05/01` visualeffects.2da row-type cursor audit: added a
  shared `visualeffects.2da` `Type_FD` row-policy helper for live-object
  visual-effect lists. When row state is loaded, `P`/`B` rows own exactly the
  decompiled five-byte target payload before `ObjectVisualTransformData`, other
  rows own none, and absent loaded rows reject instead of falling back to byte
  shape. The helper now follows an explicit `NWN_BRIDGE_VISUALEFFECTS_2DA`
  source or the observed `Module_Info` HAK order, invalidates cached table state
  when a new HAK stack is observed, and only treats a direct base-game table as
  authoritative after the server proves a zero-HAK module. Both looping-effect
  `U/* 0x00000008` records and creature status-effect helper cursors now use
  that row policy before the conservative no-table single-target fallback.
  Public fixture-free coverage proves mixed target / no-target row boundaries
  with loaded table state and rejection of missing target bytes / absent rows.
  Verified with `cargo test -q -p hgbridge-proxy2 loaded_visualeffects --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 creature_status_effect_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  looping_effect_target_payload_owns_dword_object_id_plus_byte -- --nocapture`,
  `cargo fmt --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`.~~
- ~~2026-06-03 `P/05/01` visualeffects.2da duplicate-row policy audit:
  hardened the loaded row-policy parser so duplicate numeric
  `visualeffects.2da` rows reject the table instead of letting the later row
  overwrite the earlier target/no-target proof. The row policy is used as
  packet-boundary evidence for Diamond `sub_44ED20` / EE `sub_1407B1F00`
  target-payload ownership, so ambiguous resource metadata cannot certify a
  shifted `ObjectVisualTransformData` cursor. Verified with `cargo test -q -p
  hgbridge-proxy2 duplicate_visualeffects_rows_are_not_boundary_proof --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 loaded_visualeffects --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 looping_effect --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2
  creature_status_effect -- --nocapture`.~~
- ~~2026-06-03 `P/05/01` no-table visual-effect target-payload fallback audit:
  removed the remaining single-row no-`visualeffects.2da` target-payload
  acceptance path from looping-effect exact validation, creature status-effect
  exact validation, and legacy status-effect identity-map insertion. Diamond
  `sub_44ED20` and EE `sub_1407B1F00` both branch on resolved
  `visualeffects.2da` `Type_FD`; without loaded row state, a five-byte
  target-width shape is only negative ambiguity evidence for stream-boundary
  scanning and cannot prove packet ownership. Loaded row policy still proves
  `P`/`B` target width and non-`P`/`B` no-target width. Verified with
  `cargo test -q -p hgbridge-proxy2 target_payload -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 creature_status_effect -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 looping_effect -- --nocapture`, and
  `cargo test -q -p hgbridge-proxy2 loaded_visualeffects -- --nocapture`.~~
- ~~2026-06-03 `P/05/01` compact placeable add/missing-opcode update proof:
  generalized the compact token-name `A/09` same-object update proof so a
  following legacy body that has lost its top-level `U` opcode is tested using
  the same missing-opcode door/placeable boundary repair as the normal update
  walker. The add rewrite must still exact-claim at the EE add cursor, and the
  following update must either exact-prove in the compact proof or be owned by
  the next record pass and final exact validator; typed add-marker rows no
  longer fall through to the legacy cursor fallback. Verified with the
  Chapter1 seq19 compact door/placeable stream plus focused
  `local_chapter1_seq19_placeable_door_stream_rewrites_to_exact_shape` and
  `live_object_update` tests.~~
- ~~2026-06-03 `P/05/01` terminal `W current total` storage audit: Diamond
  `sub_44F160` and EE `sub_1407B85A0` read exactly `W` plus current/total
  counters and no CNW BOOLs. A bounded non-empty storage suffix after terminal
  `W` may now be removed only as transport cleanup when prior rewrites have
  already proven a cursor at least as long as the decoded storage payload and
  the truncated `W`-legal stream exact-claims; this includes the exact-cursor
  case where only terminal storage bytes remain after the preceding source
  rewrite. A standalone or longer nonzero terminal storage suffix still
  rejects. Verified with `work_remaining_terminal_storage`,
  `work_remaining_terminal_storage_after_exact_cursor_update_rewrite_is_byte_tail`,
  `local_cepv22_seq11_zero_declared_stream`, and full `live_object_update`
  coverage.~~
- ~~2026-06-03 `P/05/01` post-`W` compact-pair source-bit fixture audit:
  tightened the promoted-storage regression seed so valid compact `A/09`
  pairs use Diamond `sub_44E4A0`'s four compact add BOOLs plus the following
  update's own scalar/state cursor bits. The previous six-zero "valid" seed
  left one terminal low-tail bit unowned; terminal `U/09 mask=0xF7` correctly
  rejects that extra bit. The shifted `1000_11_101101` handoff still rolls the
  whole promoted-storage candidate back unchanged. Verified with
  `work_remaining_storage_rolls_back_when_later_compact_pair_is_shifted`.~~
- ~~2026-05-31 `P/05/01` effect-only status row-policy audit: generalized the
  legacy `U/5 mask=0x00000008` feature-0x0E-false validator and stale
  zero-count repair so they use the shared `visualeffects.2da` `Type_FD` row
  policy when loaded. Diamond `sub_44ED20` and EE `sub_1407B1F00` both read
  compact opcode/WORD rows, with no target payload for non-`P`/`B` rows and a
  five-byte target payload for `P`/`B`; loaded row state now proves arbitrary
  no-target rows instead of a named LowLightVision-only count repair. No-table
  fallback remains limited to the previously proven feature-0x0E-false
  no-target row. Verified with `cargo test -q -p hgbridge-proxy2
  feature0e_false_effect_rows -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 effect_only_zero_count_repair -- --nocapture`, and
  `cargo test -q -p hgbridge-proxy2 creature_status_effect_ -- --nocapture`.~~
- ~~2026-05-27 `P/05/01` creature update interleaved-fragment cursor audit:
  removed the last adjacent `bit_cursor +/- 1` retry from the `U/5` compact
  fragment-span promoter. The EE/Diamond live-object dispatcher hands each
  record the exact CNW fragment cursor left by the previous reader; a shortened
  creature update may promote a bounded read-buffer suffix only after the
  decompile-owned creature reader validates from that exact cursor. Public
  fixture-free coverage now proves C40F/C44F/8047 spans that are byte-valid only
  from a neighboring cursor remain unpromoted. Verified with
  `cargo test -q -p hgbridge-proxy2
  creature_interleaved_fragment_span_requires_exact_bit_cursor -- --nocapture`,
  plus focused `3967`, `c40f`, and `c44f` capture-backed filters.~~
- ~~2026-05-27 `P/05/01` C40F/C44F/8047 interleaved-fragment fixture-free
  audit: fixed the previous cursor-regression seed so it exercises real
  promoter-owned `U/5 0xC40F`, `0xC44F`, and `0x8047` families instead of
  returning early on unsupported `0xC408`. The public test now proves both
  exact inherited-cursor promotion and neighboring shifted-cursor rejection,
  including the scalar-orientation split of one read-buffer BYTE plus four CNW
  fragment bits, the C44F/8047 low `0x0040` state BOOL, and the 8047 action
  state/follow-up count before the visibility BOOLs. Verified with
  `cargo test -q -p hgbridge-proxy2 creature_interleaved_fragment_span_
  -- --nocapture`.~~
- ~~2026-05-27 `P/05/01` live GUI `G/S` character-sheet build-mode cursor
  audit: fixed candidate selection so the exact validator no longer accepts the
  first byte-plausible legacy parse when the isolated record's owned fragment
  bits prove a newer EE shape. EE `CNWSMessage::WriteGameObjUpdate_CharacterSheet`
  (`0x1404E6880`) / client `sub_1407B2740` read `G S`, OBJECTID, DWORD mask,
  then mask branches including combat-info fragment fields and effect-icon
  lists; newer builds widen the second combat action field from four to five
  bits and effect-icon counts/ids from BYTEs to WORDs. The parser now collects
  all exact candidates, chooses only a following-boundary-proven record or the
  isolated candidate that consumes the full fragment cursor, and leaves
  same-byte-boundary/different-bit-width ambiguity unclaimed instead of
  guessing. Public fixture-free tests prove WORD effect-icon rows do not split
  on the legacy zero-byte prefix, changed effect-icon rows require their BOOL,
  and build-8193.35 five-bit combat actions own the extra bit. Verified with
  `cargo test -q -p hgbridge-proxy2 live_gui_character_sheet_ -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 live_gui -- --nocapture`, `cargo fmt
  --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`.~~
- ~~2026-05-31 `P/05/01` live GUI `G/S` character-sheet build-width handoff
  audit: no packet behavior changed, but fixture-free coverage now explicitly
  pins the same-byte-boundary ambiguity from the decompile-backed
  `sub_1407B2740` combat-info branches. An isolated `G S mask=0x40` row can
  choose the EE build-8193.35 five-bit second-list action branch when the final
  fragment cursor proves it, but the same bytes followed by a live-object `W`
  boundary remain unclaimed if the legacy four-bit branch lands on the same
  read cursor with a different bit cursor. Verified with `cargo test -q -p
  hgbridge-proxy2
  live_gui_character_sheet_build_mode_boundary_ambiguity_stays_unclaimed --
  --nocapture`.~~
- ~~2026-05-27 `P/05/01` live GUI inventory update row cursor audit: fixed the
  read-buffer-only `G I/i U` validator so it owns exactly ten bytes, not the
  wider repository update row shape. Diamond `sub_4589A0`
  (`00458BF1..00458C0B`) and EE `sub_1407B3F30`
  (`1407B4300..1407B432C`) read the inner `U`, `OBJECTID/INT32`, `SHORT`, and
  final `BYTE`, with no `ReadBOOL`; only repository `G R/r U` reads object id
  plus two DWORDs for the fifteen-byte row. Public fixture-free coverage now
  proves ten-byte inventory update ownership, handoff before a following `G Q`
  row, rejection of five stale repository-width tail bytes, and preservation of
  the fifteen-byte repository row. Verified with `cargo test -q -p
  hgbridge-proxy2 live_gui_inventory_update_ -- --nocapture` and `cargo test
  -q -p hgbridge-proxy2
  live_gui_repository_update_remains_fifteen_read_buffer_bytes --
  --nocapture`.~~
- ~~2026-05-27 `P/05/01` live GUI read-buffer row cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the adjacent
  `G I/i D` and `G R/r M` byte-only row shapes around the existing update-row
  proof. Diamond `sub_4589A0`, EE `sub_1407B3F30`, and repository helper
  `sub_1407B4620` consume inventory delete as `inner D + OBJECTID`, repository
  update as `inner U + OBJECTID + two DWORDs`, and repository move as
  `inner M + two BYTEs + OBJECTID`, with no CNW fragment BOOLs before following
  GUI rows. Tests now prove inventory delete handoff, repository move handoff,
  and rejection of a repository move whose OBJECTID is at the update/delete
  cursor. Verified with `cargo test -q -p hgbridge-proxy2 live_gui_ --
  --nocapture`.~~
- ~~2026-05-27 `P/05/01` live GUI `G/S` isolated-cursor audit: tightened the
  character-sheet candidate selector to match the earlier decompile proof from
  EE `CNWSMessage::WriteGameObjUpdate_CharacterSheet` (`0x1404E6880`) and
  client `sub_1407B2740`. A following-boundary candidate may claim a bounded
  byte record, but an isolated `G S` record must consume the exact final CNW
  fragment cursor; the old most-bits-consumed fallback could otherwise treat a
  byte-complete character sheet as exact while leaving unowned fragment bits.
  Public fixture-free coverage now proves that an isolated `G S` with a zero
  mask and one extra fragment bit stays unclaimed. Verified with `cargo test
  -q -p hgbridge-proxy2 live_gui_character_sheet_ -- --nocapture` and
  `cargo test -q -p hgbridge-proxy2 live_gui -- --nocapture`.~~
- 2026-05-27 `P/05/01` trigger update cursor audit: no packet shape changed,
  but public fixture-free coverage now proves the decompile-owned `U/7`
  position update contract. The accepted legacy all-bits trigger row
  (`0xFFFF_FFF3`) must carry exactly the bounded three-byte legacy trigger tail
  before it can collapse to EE mask `0x00000001`; the EE position-only row then
  owns three WORD read-buffer fields plus exactly two CNW fragment bits, and
  any extra or missing trigger fragment bit remains unclaimed. Removed stale
  unused `U/5 0x3967` neighboring-cursor helpers that no longer participate in
  strict validation. Verified with `cargo test -q -p hgbridge-proxy2
  trigger_update_ -- --nocapture` and `cargo test -q -p hgbridge-proxy2 3967
  -- --nocapture`.
- ~~2026-05-27 `P/05/01` trigger add geometry cursor audit: superseded by the
  2026-05-31 full-trigger-add cursor correction below. The geometry-only note
  was incomplete because it ignored the decompile-owned name/state BOOL span
  before the cursor/height/vertex fields.~~
- ~~2026-05-31 `P/05/01` trigger add name/state cursor correction: fixed the Rust
  proxy2 `A/7` model to match Diamond `sub_4552E0` and EE `sub_1407B1670`
  rather than only `AddTriggerGeometryToMessage`. Both readers consume the
  name selector first; the locstring/token branch owns the client-TLK selector
  bit plus a DWORD StrRef in the read buffer, while the direct branch owns one
  selector bit before `ReadCExoString(32)`. They then read two trigger state
  BOOLs, an optional third state BOOL when the first state BOOL is true, a
  cursor BYTE, height FLOAT, vertex-count BYTE, and complete XYZ FLOAT triples.
  The proxy now preserves the trigger-add bytes while advancing exactly that
  fragment span, supports dynamic geometry offsets for direct names, and keeps
  terminal residual bits unclaimed. Verified with `cargo test -q -p
  hgbridge-proxy2 trigger_add -- --nocapture`. 2026-06-03 re-audit tightened
  exact validation so the fragment name selector must match the byte cursor
  branch: locstring/token bits use the four-byte token cursor, while direct
  bits use the CExoString cursor. A byte-plausible direct-name geometry boundary
  can no longer be claimed with locstring selector bits. Verified with
  `cargo test -q -p hgbridge-proxy2
  trigger_add_name_bits_must_match_byte_cursor_branch -- --nocapture` and
  `cargo test -q -p hgbridge-proxy2 trigger_add -- --nocapture`.~~
- 2026-05-27 `P/05/01` door state update cursor audit: no packet behavior
  changed, but public fixture-free coverage now proves the decompile-backed
  `U/10` mask `0x10` state-BOOL handoff. Diamond client reader `sub_44E2C0`
  reads five door state BOOLs; EE `sub_140797780` owns those same five in order
  plus one neutral sixth BOOL. The bridge rewrite must insert only that false
  sixth bit,
  exact EE validation rejects a true sixth bit, and any extra fragment bit
  remains unclaimed. Verified with `cargo test -q -p hgbridge-proxy2
  door_state_update -- --nocapture`.
- 2026-05-27 `P/05/01` door/placeable low-tail cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the shared
  `U/9`/`U/10` low `0x40/0x80` rule. EE `sub_14079C050` plus
  `sub_140797780` own only the generic position/orientation/appearance/
  scale-state prefix and the object state BOOLs; those low mask bits are
  Diamond-only input unless a bounded 2/4/6-byte legacy control suffix is
  proven after the prefix. The rewrite removes only the bounded suffix, clears
  the low mask bits, appends the neutral EE state BOOL, and leaves malformed
  three-byte tails unclaimed. Verified with `cargo test -q -p hgbridge-proxy2
  low_tail -- --nocapture` and `cargo test -q -p hgbridge-proxy2
  door_placeable -- --nocapture`.
- ~~2026-06-04 `P/05/01` door/placeable missing-opcode low-tail boundary audit:
  tightened the missing-opcode and missing-type low-tail transport scanners to
  use the same unique 2/4/6-byte suffix-end rule as top-level `U/9`/`U/10`
  low-tail boundary detection. Diamond `sub_467AE0` plus
  `sub_44E2C0`/`sub_44EB40` own the generic update prefix and state bits, while
  EE `sub_14079C050` plus `sub_140797780` has no low `0x40/0x80` reader; a
  suffix can be dropped only when exactly one bounded suffix width lands on the
  next live-object boundary. Public coverage now proves an internal
  boundary-looking low-tail suffix stays unclaimed for both missing-opcode and
  missing-type handoffs. Verified with `cargo test -q -p hgbridge-proxy2
  missing_opcode_low_tail_record_end_must_be_unique -- --nocapture`.~~
- ~~2026-06-04 `P/05/01` door/placeable scalar/vector boundary fallback audit:
  tightened the top-level live-object boundary scanner so a focused `U/9`/`U/10`
  transport ambiguity is a hard stop instead of falling through to the generic
  opcode search. Diamond `sub_467AE0` and EE `sub_14079C050` both read the
  orientation selector BOOL before choosing one scalar byte or six vector bytes,
  so W-shaped bytes inside a vector body cannot prove a scalar boundary.
  Verified with `cargo test -q -p hgbridge-proxy2
  door_placeable_update_boundary_keeps_scalar_vector_ambiguity_unclaimed --
  --nocapture`.~~
- 2026-05-29 `P/05/01` terminal trigger/low-tail residual-bit audit: hardened
  the legacy `U/7 0xFFFF_FFF3` trigger repair and shared `U/9`/`U/10`
  no-name low-tail repairs so a terminal record may not use the outer
  live-object fragment trim to discard unowned residual source bits. Midstream
  records may still hand off later fragment bits to the following proven
  record, but a terminal trigger update must own exactly the two decompiled
  position BOOLs, and a terminal name-free door/placeable low-tail repair must
  end exactly after the typed position/orientation/state bits plus any proven
  Diamond-only low-tail BOOL removals. Public regressions now prove one extra
  terminal fragment bit rejects the repair and leaves the payload unchanged.
  Name-bearing low-tail updates remain covered by the separate legacy
  name/drop cursor path. Verified with
  `cargo test -q -p hgbridge-proxy2 terminal_extra_fragment_bit --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 trigger_update --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 low_tail --
  --nocapture`.
- 2026-05-29 `P/05/01` named door/placeable low-tail terminal-bit audit:
  extended the same terminal residual-bit guard to legacy `U/9`/`U/10`
  low-tail records that also carry Diamond's input-only name bit. The source
  reader owns the position/state BOOLs plus the low-tail/name BOOLs, then the
  EE writer clears the low-tail/name bits, drops the direct CExoString/name
  bytes, and appends the neutral door/placeable state BOOL; any additional
  terminal fragment bit is now rejected before the outer live-object pass can
  trim it as transport residue. Plain legacy name/drop updates without low-tail
  bits and older all-bits tail9 facing/scale/state records remain on their
  separate established cursor paths. Verified with `cargo test -q -p
  hgbridge-proxy2 legacy_named_low_tail -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 terminal_extra_fragment_bit --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 low_tail -- --nocapture`,
  and `cargo test -q -p hgbridge-proxy2 trigger_update -- --nocapture`.
- 2026-05-29 `P/05/01` all-bits door/placeable tail9 terminal-bit audit:
  extended the terminal residual-bit guard to the older `U/9`/`U/10`
  `0xFFFFFFF7` tail9 converter. The capture-backed compact-tail cursor is
  position residual bits, five state BOOLs, and the legacy name BOOL; the EE
  writer inserts scalar-orientation bits and one neutral door/placeable state
  BOOL, but any remaining terminal fragment bit is unowned until a decompile
  trace proves another reader. Public synthetic coverage now proves a terminal
  door/placeable tail9 update with one extra bit rejects without mutating the
  source. The live Ascension West mixed-burst fixture remains active evidence:
  after the compact `A/9` visual-map rewrite, its final `U/9 0xFFFFFFF7`
  tail9 row still has residual fragment bits and no following record to own
  them, so it is now kept unclaimed instead of relying on outer live-object
  trimming. Verify with `cargo test -q -p hgbridge-proxy2 legacy_all_bits --
  --nocapture` and `cargo test -q -p hgbridge-proxy2 door_placeable --
  --nocapture`.
- 2026-05-29 `P/05/01` inline-name door/placeable terminal-bit audit:
  tightened the plain legacy `U/9`/`U/10` name-drop path adjacent to the
  low-tail/tail9 guards. Terminal inline-name repairs may now proceed only
  through the compact reader/capture-backed fragment cursor: position BOOLs,
  five state BOOLs, and Diamond's input-only name BOOL before the direct
  CExoString bytes are dropped. If the record would instead need the
  read-body interleaved-fragment fallback, it must have a following live-object
  boundary to prove alignment; an isolated terminal record with one extra
  fragment bit is left untouched and unclaimed rather than letting the extra
  bit alter how many bits are borrowed from the name bytes. Verified with
  `cargo test -q -p hgbridge-proxy2 inline_name -- --nocapture`. The XP2 seq19
  post-door compact placeable/door GUI stream is not the terminal inline-name
  case after the later terminal-trim audit: its final residue is owned by a
  terminal GUI item-create record and exact-rewrites through that GUI-specific
  proof. The Chapter1 seq19 door/placeable stream is likewise not the same
  case: its final residue is owned by a terminal `W` fragment storage span and
  still exact-rewrites through that W-specific proof.
- 2026-05-29 `P/05/01` live-object terminal trim ownership gate: narrowed the
  outer update pass so terminal fragment bits are trimmed only when the final
  cursor is tied to the family that owns the terminal storage: typed
  inventory/D5FF storage, terminal GUI item-create storage proven by exact
  item-create validation, promoted fragment-storage spans, exact creature
  update tails, terminal `W` storage, or a door/placeable update record whose
  typed bit path proves a non-state terminal cursor. A prior rewrite earlier
  in the same live-object stream no longer justifies trimming after later
  fragment-neutral GUI/delete records. Public coverage now proves a trigger
  rewrite followed by a read-buffer-only GUI row rejects a final unowned bit,
  and state-only `U/9`/`U/10` door/placeable updates reject a seventh terminal
  bit after the five Diamond state BOOLs plus EE's neutral sixth BOOL.
  Captures demoted to active evidence until a decompile-backed terminal owner
  or record-boundary handoff is found: To Heir `U/5 0x4408 + I/0x2A00 +
  GUI/delete`, XP1 single-WORD `I/0x2A00 + GQ`, XP2 Chapter2 seq16, CEP v2.2
  builder seq16 rebuilt pending stream (post-rewrite `U/0x55` boundary), and
  CEP v2.3 starter seq17 Lance/Lute/Patron live-object stream (`U/6` handoff
  plus terminal tail).
  Verified with
  `cargo test -q -p hgbridge-proxy2 live_object_update -- --nocapture`.
- 2026-05-29 `P/05/01` delete-record fragment-cursor audit: no packet behavior
  changed, but public fixture-free coverage now pins the delete bit table that
  terminal-tail decisions rely on. Diamond `sub_455720` and EE `sub_1407B35B0`
  both route `D/5`, `D/6`, and `D/9` through helpers that read OBJECTID plus
  one CNW BOOL, while `D/7` and `D/10` read only the OBJECTID. The exact
  validator now has a regression proving the one-BOOL deletes reject missing or
  extra terminal bits and the read-buffer-only deletes reject any fragment
  residue. This does not resolve the active To Heir/XP1/XP2/CEP terminal-tail
  captures above; it prevents their trailing delete rows from becoming an
  accidental generic trim owner. Verified with `cargo test -q -p
  hgbridge-proxy2 live_object_delete_records_own_exact_fragment_bits --
  --nocapture`.
- 2026-05-29 `P/05/01` GUI/delete terminal handoff audit: no packet behavior
  changed, but public fixture-free coverage now pins the multi-record form of
  the To Heir terminal-tail evidence. A prior typed rewrite may prove an earlier
  cursor, but a following read-buffer-only `GQ` row plus `D/5` delete rows own
  only their decompiled delete BOOLs; they do not inherit a previous terminal
  trim owner or turn final storage-looking bits into family-owned residue.
  Verified with `cargo test -q -p hgbridge-proxy2
  terminal_delete_rows_do_not_inherit_prior_trim_owner -- --nocapture`.
- 2026-05-29 `P/05/01` inventory/GQ terminal-bit audit: no packet behavior
  changed, but public fixture-free coverage now proves the generalized
  `I/0x2A00` word-list branch before `GQ` cursor rule behind the XP1/To Heir
  evidence. Diamond `sub_455940` and EE `sub_1407B4F70` own exactly the two
  `0x0200` BOOLs, three `0x2000` Feature-25 BOOLs per second-list object, and
  the single `0x0800` selector; `GQ` then owns read-buffer bytes only. An
  extra terminal fragment bit after that exact `I/0x2A00 + GQ` stream remains
  unclaimed and the update pass leaves the payload untouched. This keeps the
  active captures quarantined until a real terminal owner or continuation
  handoff is proven. Verified with `cargo test -q -p hgbridge-proxy2
  inventory_2a00_word_list_before_gq_rejects_terminal_extra_fragment_bit --
  --nocapture`.
- 2026-06-03 `P/05/01` `U/5 0x4408 + I/0x2A00 + GQ` terminal-tail rollback
  audit: no packet behavior changed. Re-ran the active To Heir and XP1 local
  capture regressions with live-claim tracing. The To Heir stream validated the
  repaired creature update, `I/0x2A00`, `GQ`, and delete rows, then stopped at
  `bit_cursor=22` of `fragment_bits=124`; the XP1 stream stopped at
  `bit_cursor=16` of `fragment_bits=17`. In both cases the terminal trim gate
  found no family-owned terminal cursor. Added fixture-free coverage proving a
  prior compact Diamond `U/5 0x4408` status-effect repair exact-claims before
  exact `I/0x2A00 + GQ`, but an extra terminal bit after `GQ` rolls back the
  earlier repair and leaves the source payload visible for quarantine. This
  keeps the active captures pending a decompile-backed stream-boundary or
  continuation owner. Verified with `cargo test -q -p hgbridge-proxy2
  creature_4408_inventory_2a00_gq_terminal_bit_rolls_back_prior_rewrite
  -- --nocapture`.
- 2026-05-29 `P/05/01` item `U/6` 0x40 transactional cursor audit: hardened
  the item update rewrite so legacy mask/tail edits are staged and committed
  only after the exact EE item validator owns the read cursor and fragment
  cursor. Follow-up re-audit corrected the decompile pointer: Diamond
  `sub_459700` reads the live-object opcode/object-id/mask envelope and
  dispatches object type `0x06` to `sub_451AF0`, but `sub_451AF0` proves the
  item-name `0x80000` branch, not the `0x40` tail; its post-name
  `sub_4FBB40` call is an overflow check. Follow-up 2026-05-29 re-audited
  Diamond `sub_459700 -> sub_467AE0 -> sub_451AF0` and EE item state handling:
  Diamond's shared generic reader owns only low `0x1/0x2/0x4/0x8/0x20`, while
  `sub_451AF0` owns only the name selector/data. The prior six-byte plus
  optional-OBJECTID low-`0x40` read-tail claim had no Diamond client owner, so
  proxy2 now rejects it instead of collapsing it into EE's one hidden-state
  BOOL. Exact EE-shaped item hidden-state updates still own one BOOL and no
  read tail, and raw low `0x80` is still dropped only as a mask-only translation
  when no extra bytes are attributed to it. This does not resolve the active CEP
  v2.3 `U/6` handoff/terminal-tail capture; any low-`0x40` tail bytes in that
  evidence are now unclaimed unless a separate server/client handoff owner is
  proven.
  Verified with `cargo test -q -p hgbridge-proxy2 item_update_40 -- --nocapture`.
- 2026-05-29 `P/05/01` item `U/6` low-`0x80` read-tail audit: tightened the
  same item-update path so raw mask `0x80` is not allowed to extend the
  guarded legacy `0x40` read-buffer tail with padding-like zero bytes.
  Re-auditing Diamond `sub_459700` -> item helper `sub_451AF0` showed no
  separate item `0x80` read-buffer owner; `0x80` may still be dropped from the
  emitted mask when the record otherwise lands exactly, but any extra bytes
  remain unclaimed and leave the source payload untouched. Public coverage now
  proves the `0x40` optional-object-id tail, exact `0x40|0x80` mask
  translation with no extra bytes, and rejection/rollback when `0x80` is used
  to hide three zero bytes. The CEP v2.3 starter `U/6` handoff/terminal-tail
  capture remains active evidence rather than exact-claimable. Verified with
  `cargo test -q -p hgbridge-proxy2 item_update_40 -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 live_object_update -- --nocapture`,
  private `dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit`,
  and private `local_xp2_chapter2_inventory_live_objects_rewrite_to_exact_shape`.
- 2026-05-29 `P/05/01` item `U/6` name-selector cursor audit: no packet
  behavior changed, but public fixture-free coverage now pins the decompiled
  name branch bit order. Diamond `sub_451AF0` tests mask `0x80000`, reads one
  selector BOOL, then either `sub_53E700` locstring data or
  `ReadCExoString(32)`; EE item update helper `sub_1407A08F0` uses the same
  selector before the next item-state BOOL. Tests now prove name-only updates
  own only the selector bits, combined name+hidden updates consume the hidden
  BOOL after the name branch, and terminal extra bits reject instead of being
  mistaken for the Diamond overflow check. The CEP v2.3 `U/6`
  handoff/terminal-tail capture remains active pending the final cross-record
  handoff or `U/9`/`W` terminal-tail proof; the low-`0x40` item tail is now
  recorded as unowned by the Diamond client reader.
  Verified with `cargo test -q -p hgbridge-proxy2 item_update_name -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2 live_object_update -- --nocapture`.
- 2026-05-31 `P/05/01` item `U/6` read-body boundary audit: fixed the shared
  live-object transport scanner so item update read-buffer fields are walked
  before generic opcode scans. Diamond `sub_459700 -> sub_467AE0 -> sub_451AF0`
  owns the generic item update order: position bytes, scalar/vector orientation
  bytes, appearance/resref, scale/state bytes, then item name bits/string; EE's
  item validator uses the same byte order before the hidden-state BOOL. A
  position body whose first three bytes spell `W current total` is therefore
  still a `U/6` body, not a top-level `W` row, and the following live-object row
  is considered only after all six position bytes and the two position fragment
  bits are owned. This does not resolve the active CEP v2.3 terminal-tail
  evidence; low-`0x40` item read tails and `U/9`/`W` shortages remain
  quarantined unless a separate owner is proven. Verified with `cargo test -q
  -p hgbridge-proxy2 item_update_position -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 item_update -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`.
- ~~2026-06-04 `P/05/01` item `U/6` scalar/vector transport-boundary ambiguity
  audit: tightened the byte-only item update scanner so distinct scalar and
  vector orientation endpoints that both land on plausible live-object
  boundaries keep the whole scan window ambiguous instead of falling back to an
  internal opcode split. Diamond `sub_467AE0` and EE `sub_14079C050` choose
  scalar vs vector from the orientation BOOL before reading orientation bytes,
  so a scalar cursor that lands on `W current total` inside vector bytes is not
  boundary proof. Verified with `cargo test -q -p hgbridge-proxy2
  scalar_vector_boundary_ambiguity -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 item_update -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 live_object_update::boundary::tests:: -- --nocapture`.~~
- ~~2026-06-04 `P/05/01` item `U/6` name-branch transport-boundary ambiguity
  audit: no packet behavior changed, but public boundary coverage now pins the
  same bit-cursor rule for item names. Diamond `sub_451AF0` and EE
  `sub_1407A08F0` read the item-name selector BOOL before choosing direct
  `CExoString` or locstring-token bytes; without that selector, a direct
  empty-name endpoint that exposes a `W` row and a locstring-token endpoint that
  exposes a `D/6` row are byte-shape ambiguity, not boundary proof. Verified
  with `cargo test -q -p hgbridge-proxy2 item_update_boundary_keeps_name_branch_ambiguity_unclaimed -- --nocapture`.~~
- 2026-05-31 `P/05/01` item `U/6` locstring-token name audit: no packet
  behavior changed, but public fixture-free coverage now pins the token branch
  of the same decompile-backed item-name bit order. Diamond `sub_451AF0` and EE
  `sub_1407A08F0` read the outer item-name selector, then the locstring
  token/client-TLK selector bit and selector BYTE + DWORD token payload, before
  any following item hidden-state BOOL or next live-object record. Tests now
  prove token-name + hidden owns those bits in order, rejects a missing or extra
  terminal hidden-state bit, and hands off to a following `D/6` delete only
  after the token payload. Verified with `cargo test -q -p hgbridge-proxy2
  item_update_locstring_token_name -- --nocapture` and `cargo test -q -p
  hgbridge-proxy2 item_update_name -- --nocapture`.
- 2026-05-31 `P/05/01` item `U/6` hidden-state before `W` audit: no packet
  behavior changed, but public fixture-free coverage now pins the terminal-tail
  negative proof for the item sibling of the CEP v2.3 evidence. Diamond
  `sub_451AF0` has no low-`0x40` item read-buffer tail, EE `sub_1407A08F0`
  owns exactly one hidden-state BOOL for mask `0x40`, and `W current total`
  (`sub_44F160` / `sub_1407B85A0`) owns only its three read-buffer bytes and
  zero CNW BOOLs. An item hidden update before `W` therefore exact-claims with
  one item BOOL, rejects a missing BOOL, and rejects an extra terminal BOOL
  rather than borrowing or trimming through `W`. Verified with `cargo test -q
  -p hgbridge-proxy2 work_remaining_does_not_supply_missing_item_hidden_bit
  -- --nocapture`.
- 2026-06-01 `P/05/01` typed item-create handoff audit: fixed the `A/6`
  boundary classifier so a live-object add row whose byte after `A` is a typed
  object marker stays on the typed item-create path (`A`, type `0x06`, OBJECTID,
  shared item body) instead of trying top-level visible-equipment item-add
  name/appearance repair or fallback cursor advancement. The generalized public
  regression uses a stock model-type-2 item body shaped like the CEP v2.3
  Lance evidence: EE appearance/visual-map bytes are present, but EE's
  active-property BOOL is still missing from the fragment stream. The rewrite
  inserts exactly that BOOL and preserves the following `U/6` position bits.
  Private CEP v2.3 debug after this change still quarantines at the following
  `U/6 mask=0xFFFF_FFF3` boundary (`offset=104`, `bit_cursor=28`), so the next
  active work remains proving or rejecting that update mask/body owner rather
  than the preceding `A/6` item-create row. Verified with `cargo test -q -p
  hgbridge-proxy2 typed_item_create_rewrite_keeps_following_bits_aligned --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  update_rewrite_typed_item_create_preserves_following_update_bits --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  live_object_update::fixture_free_tests -- --test-threads=1`, and private
  `dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit`.
- 2026-06-01 `P/05/01` typed item-create property-tail boundary audit: fixed
  the live-object rewrite/claim loops to prefer the fragment-proven shared item
  body endpoint for typed `A/6` rows before trusting the generic byte-boundary
  scanner. Diamond `sub_451020` and EE `sub_14076BD30` both consume the counted
  active-property tail inside the item body, and those value bytes can legally
  resemble a top-level `U/6` boundary. The public regression now injects a
  `U/6`-looking sequence into valid active-property value bytes, proves the
  byte-only scanner would split early, and verifies the decompile-backed item
  parser plus fragment proof keeps the following full `U/6` cursor exact. The
  private CEP v2.3 fixture still quarantines at the following shifted `U/6`, so
  this closes another `A/6` boundary false-split risk without changing the
  remaining source-cursor hypothesis. Verified with `cargo test -q -p
  hgbridge-proxy2 typed_item_create -- --nocapture`, `cargo check -q -p
  hgbridge-proxy2`, and private `RUSTFLAGS='--cfg hgbridge_private_fixtures'
  cargo test -q -p hgbridge-proxy2
  dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit
  -- --nocapture`.
- 2026-06-01 `P/05/01` item full-mask `U/6` scalar/vector audit: no packet
  behavior changed, but public fixture-free coverage now pins the exact
  all-bits item-update rule behind the remaining CEP v2.3 `U/6` boundary.
  Diamond `sub_459700 -> sub_467AE0 -> sub_451AF0` and EE
  `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0` both read the generic
  update prefix as position bytes plus two bits, one orientation selector BOOL,
  either one scalar byte plus four bits or six vector bytes, appearance/resref,
  five state bits, and item name selector/data. A 2026-06-07 decompile
  correction superseded the earlier hidden-state assumption and the stale
  server-writer attribution: Diamond item type `0x06` full updates are still
  modeled as ending after the name branch, while EE hidden-state BOOLs belong
  only to explicit EE-shaped mask `0x40` records. Tests now prove a raw Diamond
  `0xFFFF_FFF3` item row with decompile-correct scalar bits and direct
  `CExoString` name translates to EE mask `0x00080033` without moving bytes or
  fragment bits, and that the same scalar-looking read-buffer bytes stay
  unclaimed/unchanged when the orientation BOOL is true and therefore selects
  the vector branch. The typed `A/6` repair also exact-claims when followed by a
  full `U/6` whose source bits are correct through orientation, state, and name.
  Current CEP v2.3 debug still reaches the following
  `U/6 mask=0xFFFF_FFF3` at `offset=104`, `bit_cursor=28` with the next bits
  selecting vector while the bytes look scalar/direct-name, so do not add a
  U/6 scalar-byte rescue. Next step: trace the preceding source fragment cursor
  and original writer/handoff that produced those bits (typed `A/6` active-item
  body, earlier rewrite cursor, or server-side fragment storage) before changing
  translation behavior. Verified with `cargo test -q -p hgbridge-proxy2
  item_full_update -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  update_rewrite_typed_item_create_preserves_following_full_item_update_bits
  -- --nocapture`, and private
  `dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit`.
- ~~2026-06-06 `P/05/01` creature-add to full item `U/6` fragment-prefix
  handoff audit: fixed a generalized transport split where a verified EE-shaped
  `A/5` creature add could be followed by the first byte of the same CNW MSB
  fragment stream before the next real top-level `U/6` boundary, while the
  remaining fragment bytes were already in the packet tail. The promoter now
  moves only the bounded prefix before a following live-object boundary, then
  the item update must exact-prove its own decompile-backed cursor before the
  Diamond `0xFFFF_FFF3` mask can translate to EE `0x00080033`; no neighboring
  item cursor search is allowed. The private CEP v2.3 fixture still quarantines
  at the unresolved `offset=104`, `bit_cursor=28` `U/6`, so the two-bit owner
  question remains active. Verified with `cargo test -q -p hgbridge-proxy2
  creature_add_fragment_prefix_before_item_update_feeds_exact_u6_cursor --
  --nocapture`, the neighboring-cursor rejection regressions, and private
  `dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit`.~~
- 2026-06-01 `P/05/01` full item `U/6` locstring-inline audit: no packet
  behavior changed, but public fixture-free coverage now pins the locstring
  inline sibling of the same all-bits item-update rule. Diamond `sub_451AF0`
  and EE `sub_1407A08F0` read the item-name outer selector, then the
  locstring component selector before the inline `CExoString`. The 2026-06-07
  decompile correction later superseded the stale server-writer attribution, but
  kept the same client-reader rule: the Diamond full item row ends after that
  name payload; EE hidden-state remains a separate explicit `0x40` update. The
  typed `A/6` handoff coverage now proves the
  active-property insertion preserves those following U/6 locstring-inline bits
  just as it does direct-name bits, so the remaining CEP v2.3 boundary should
  stay focused on the actual source cursor/handoff bits rather than adding a
  special U/6 name-branch rescue. Verified with `cargo test -q -p
  hgbridge-proxy2 locstring_inline -- --nocapture`.
- 2026-06-04 `P/05/01` full item `U/6` locstring-token audit: no packet
  behavior changed, but public fixture-free coverage now pins the token sibling
  of the same all-bits item-update rule. Diamond `sub_451AF0` and EE
  `sub_1407A08F0` read the outer item-name selector, the token/client-TLK
  selector bit, and the read-buffer selector BYTE plus DWORD token. The
  2026-06-07 decompile correction later superseded the stale server-writer
  attribution, but kept the same client-reader rule: the Diamond full item row
  ends there; EE hidden-state remains a separate explicit `0x40` update. Typed
  `A/6`
  active-property insertion also preserves the following full `U/6` token-name
  cursor exactly, so the remaining CEP v2.3
  two-bit handoff evidence is not a missing token-name branch. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-item-token cargo test -q -p hgbridge-proxy2 item_full_update -- --nocapture`
  and `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-item-token cargo test -q -p hgbridge-proxy2 typed_item_create -- --nocapture`.
- 2026-06-01 `P/05/01` typed item-create / full item-update handoff negative
  proof: no packet behavior changed, but public fixture-free coverage now
  captures the remaining CEP v2.3 source-bit shape directly. A preceding typed
  `A/6` row may insert EE's active-property BOOL only transactionally; if the
  following `U/6 0xFFFF_FFF3` bits select the vector-orientation branch while
  the read-buffer bytes look like the scalar/direct-name item shape, the update
  pass must roll back and leave bytes/bits untouched. Diamond `sub_467AE0` and
  EE `sub_14079C050` branch on the orientation BOOL before reading orientation
  bytes, so this is shifted-cursor evidence, not a safe scalar-byte rescue.
  The CEP v2.3 starter capture remains active pending a real owner for those
  source bits or the later `U/9`/`W` terminal tail. Verified with `cargo test
  -q -p hgbridge-proxy2
  typed_item_create_handoff_rejects_vector_selected_full_item_update --
  --nocapture`.
- 2026-06-06 `P/05/01` CEP v2.3 full item `U/6` neighboring-cursor ambiguity
  audit: no packet behavior changed. Re-ran the private Lance/Lute/Patron
  fixture with live-claim tracing after the normalized `A/10`, tail9 `U/10`,
  and no-map `A/6` repairs. The stream still reaches `U/6 mask=0xFFFF_FFF3`
  at `offset=104`, `record_end=148`, `bit_cursor=28`; the exact item reader
  rejects that true cursor because the bits select vector orientation while
  the bytes are scalar/direct-name shaped, but nearby cursors `-4`, `-3`,
  `-2`, `+2`, and `+4` can validate the translated EE item row. This is
  ambiguity, not ownership: Diamond `sub_467AE0` and EE `sub_14079C050`
  choose scalar/vector from the current BOOL before reading orientation bytes.
  Added public fixture-free coverage that reconstructs the post-rewrite
  CEP-style prefix cursor at bit 28, proves the neighboring fits, and still
  requires packet-level rollback until a prior decompiled reader owns the
  skipped bits. Verified with `CARGO_INCREMENTAL=0
  CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260606-neighbor cargo
  test -q -p hgbridge-proxy2
  cep_no_map_raw_u6_neighboring_cursor_fits_are_not_ownership_proof --
  --nocapture`. Next step remains tracing the preceding source writer,
  chunk-local fragment storage, continuation boundary, or later terminal-tail
  owner; do not add a scalar-byte rescue to `U/6`.
- 2026-06-06 `P/05/01` item full `U/6` record-local neighboring-cursor audit:
  no packet behavior changed. Added item-family unit coverage for the same
  generalized CEP-style ambiguity: the translated scalar/direct-name full item
  row rejects at the inherited cursor because Diamond `sub_467AE0` and EE
  `sub_14079C050` read the current orientation BOOL before orientation bytes,
  while `cursor + 2` can validate only if a separate prior owner consumes the
  residue. `rewrite_update_record_for_ee` now has direct public coverage
  proving it leaves the raw Diamond row and record end unchanged instead of
  trying neighboring cursors. Verified with `CARGO_INCREMENTAL=0
  CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260606-item-proof
  cargo test -q -p hgbridge-proxy2 item_update -- --nocapture`,
  `raw_neighbor_u6`, `typed_item_create`, serial `live_object_update`, and
  `cargo check -q -p hgbridge-proxy2`. The true two-bit owner remains
  unresolved; continue with source writer / chunk-local fragment storage /
  continuation-boundary tracing rather than scalar-byte rescue.
- 2026-06-06 `P/05/01` typed item-create repair transaction audit: packet
  shape behavior changed only for failed-repair rollback. The GUI/top-level
  item-create extra inserter now stages the active-item fragment BOOL inserts
  together with item appearance byte edits, and the shared byte insert helper
  commits `bytes`/`record_end` only after every insert proof succeeds. This
  prevents a failed `A/6` byte proof from stranding EE-only bits before the
  following `U/6` cursor; it is not evidence for the unresolved full-item
  cursor owner. Verified with `cargo test -q -p hgbridge-proxy2
  byte_insert_application_rolls_back_after_later_failed_insert -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 typed_item_create -- --nocapture`,
  private `dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit`
  with live-claim tracing still rejecting `offset=104`, `bit_cursor=28`, and
  `cargo check -q -p hgbridge-proxy2`.
- 2026-06-06 `P/05/01` full item `U/6` decompile-owner audit: no packet
  behavior changed. Re-ran the private CEP v2.3 starter live-claim trace and
  the stream still reaches `U/6 mask=0xFFFF_FFF3` at staged `offset=104`,
  `record_end=148`, `bit_cursor=28` after exact `A/10`, `U/10 tail9`, and
  transactional no-map `A/6` repairs. Diamond `sub_467AE0` and EE
  `sub_14079C050` both read the orientation selector BOOL at the inherited
  cursor before choosing scalar versus vector bytes; Diamond `sub_451AF0` and
  EE `sub_1407A08F0` both read only the item-name selector before locstring or
  direct-string bytes. Diamond `sub_451020` and EE `sub_14076BD30` also confirm
  the typed item-create active-property owner is only Diamond's four BOOLs plus
  EE's inserted post-DWORD BOOL. The raw private fixture still has the
  top-level `A/10`, `U/10`, `A/6`, `U/6` byte sequence before staged rewrites,
  so the two leading bits before the full item update remain unowned by the
  item-create/update readers. Next trace should move to the CNW fragment
  storage/continuation boundary or the original server-side writer/handoff that
  serialized the fragment tail before the `U/10`/`A/6`/`U/6` sequence; do not
  add scalar-byte rescue or neighboring-cursor retry behavior to `U/6`.
- 2026-06-06 `P/05/01` item-add extra transaction audit: packet shape behavior
  changed only for failed-repair transactionality in top-level item `A` extras.
  `insert_ee_item_add_extras_for_ee` now stages the EE-only active-property BOOL
  with item appearance byte inserts before committing, matching the GUI
  item-create transaction model and preventing partial fragment mutation if a
  later byte proof fails. Public unit coverage proves a legacy-width
  model-type-2 top-level item add inserts exactly the three EE item high bytes,
  the EE visual-transform map, and one active-property BOOL before exact EE item
  validation. The private CEP v2.3 trace is intentionally unchanged: after
  exact `A/10`, `U/10 tail9`, and typed `A/6`, the following full `U/6
  mask=0xFFFF_FFF3` still rejects at `offset=104`, `record_end=148`,
  `bit_cursor=28`, while neighboring cursors remain ambiguity only. Verified
  with `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260606-itemadd
  cargo test -q -p hgbridge-proxy2 item_add_extra_insert_stages_legacy_width_bits_and_bytes
  -- --nocapture`, `typed_item_create`, `raw_neighbor_u6`, `cargo check -q -p
  hgbridge-proxy2`, and private
  `RUSTFLAGS='--cfg hgbridge_private_fixtures'
  HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM=1 cargo test -q -p hgbridge-proxy2
  dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit
  -- --nocapture`. Next step remains the CNW fragment storage/continuation
  boundary or true source writer/handoff before the `U/6`; do not add
  scalar-byte rescue or neighboring-cursor retry behavior.
- 2026-06-06 `P/05/01` stale-declared item cursor capacity audit: packet
  behavior changed only for declared-window candidate rejection. The
  source-side declared-length preflight now counts typed `A/6` item-create
  Diamond-owned item-name/active-property BOOLs and item `U/6` update BOOLs
  before accepting a read-window split. This prevents stale-declared repair from
  treating item-family source bits as free CNW tail storage; top-level item-add
  rows were left on the existing byte-boundary parser because their compact
  source shape is not the CEP typed `A/6` risk. The private CEP v2.3 trace is
  intentionally unchanged: after exact `A/10`, `U/10 tail9`, and transactional
  no-map `A/6`, the following full `U/6 mask=0xFFFF_FFF3` still rejects at
  `offset=104`, `record_end=148`, `bit_cursor=28`; the trace showed no
  adjacent read-buffer fragment span after the repaired `A/6`. Next owner
  search remains the source writer or continuation boundary before the
  `U/10`/`A/6`/`U/6` sequence. Verified with `cargo test -q -p
  hgbridge-proxy2 declared_length_ -- --nocapture`, serial `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`, `cargo check -q -p
  hgbridge-proxy2`, and private
  `RUSTFLAGS='--cfg hgbridge_private_fixtures'
  HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM=1 cargo test -q -p hgbridge-proxy2
  dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit
  -- --nocapture`.
- 2026-06-06 `P/05/01` raw-prefixed continuation boundary audit: tightened the
  live-object zlib stream assembler so a raw-prefixed continuation no longer
  treats the first byte as CNW fragment storage when that byte already starts a
  decompile-recognized live-object submessage boundary. The older raw-prefixed
  path accepted any nonzero one-byte prefix after a pending stream, which could
  move an opcode such as `U/6` into the fragment tail and shift all following
  record cursors before exact validation. The observed Docks-style one-byte
  `0xA7` prefix remains accepted when it precedes read bytes; opcode-boundary
  starts are left unclaimed unless another decompile-backed owner proves a
  prefix. This does not claim the active CEP v2.3 item row: the private trace
  still quarantines after exact `A/10`, `U/10 tail9`, and no-map `A/6` at full
  `U/6 mask=0xFFFF_FFF3`, `offset=104`, `record_end=148`, `bit_cursor=28`.
  Verified with `cargo test -q -p hgbridge-proxy2 raw_prefixed_continuation --
  --nocapture` and the private CEP quarantine audit.
- 2026-06-06 `P/05/01` raw-prefixed read-buffer-boundary audit: extended the
  same continuation guard from object-bearing `A/D/U/P` starts to the shared
  live-object boundary predicate, covering read-buffer-only `W current total`,
  `GQ`, inventory, and other decompile-recognized submessage starts. A leading
  `W` or `G` byte in a continuation is no longer treated as a one-byte CNW
  fragment prefix unless another owner proves it, preventing a stream-layer
  cursor shift before the exact packet-family validator runs. Public tests now
  pin `W` and `GQ` starts while preserving the observed Docks one-byte
  non-boundary prefix. Verified with `cargo test -q -p hgbridge-proxy2
  raw_prefixed_continuation -- --nocapture`, serial `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`,
  `cargo fmt --all --check`, and private
  `RUSTFLAGS='--cfg hgbridge_private_fixtures'
  HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM=1 cargo test -q -p hgbridge-proxy2
  dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit
  -- --nocapture`. The private CEP fixture is intentionally unchanged and still
  needs the original source writer, continuation, or terminal-tail owner before
  the full item `U/6`.
- 2026-06-06 `P/05/01` raw-prefixed typed-boundary coverage audit: no packet
  behavior changed. Added public fixture-free coverage proving the stream-layer
  continuation guard also leaves typed `A/6 + OBJECTID` item-create starts and
  `P/5 + OBJECTID + mask` creature-appearance starts in the read buffer instead
  of treating their opcode byte as CNW fragment storage. This pins the same
  decompile-owned boundary predicate around the active CEP typed item-create /
  full item-update handoff, but it is not a two-bit owner for the remaining
  `U/6 mask=0xFFFF_FFF3` cursor at `offset=104`, `record_end=148`,
  `bit_cursor=28`; the private CEP audit still quarantines at that same cursor.
  Verified with `CARGO_INCREMENTAL=0
  CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260606-raw-cont cargo
  test -q -p hgbridge-proxy2 raw_prefixed_continuation -- --nocapture` and
  private `dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit`
  with live-claim tracing.
- 2026-06-07 `P/05/01` raw-prefixed short-boundary coverage audit: no packet
  behavior changed. Added public stream-layer regressions proving raw-prefixed
  continuations do not strip leading `I + OBJECTID + WORD mask` inventory rows
  or `D/type/OBJECTID` delete rows into CNW fragment storage. Diamond
  `sub_455940` / EE `sub_1407B4F70` own the inventory read-buffer prefix before
  any mask BOOLs, and Diamond `sub_455720` / EE `sub_1407B35B0` own the delete
  read-buffer row before optional delete BOOLs. This closes another false
  prefix class around the active CEP item handoff without claiming the
  unresolved two-bit `U/6` cursor. Verified with `CARGO_INCREMENTAL=0
  CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260607-raw-cont-short
  cargo test -q -p hgbridge-proxy2 raw_prefixed_continuation -- --nocapture`.
- 2026-06-07 `P/05/01` CNW fragment-header/finalization audit: no packet
  behavior changed. Diamond server `CNWMessage::CreateWriteMessage`
  (`nwserver` 0x507E30) and EE `CNWMessage::CreateWriteMessage`
  (`nwn` 0x1402D54A0) both start the write buffer at byte offset 7 and reserve
  exactly three MSB fragment-header bits before semantic BOOLs. Diamond
  `GetWriteMessage` (`nwserver` 0x507F30) and EE `GetWriteMessage`
  (`nwn` 0x1402D5880) overwrite only those top three bits with the final-byte
  valid-bit count, while Diamond `WriteBOOL` (`0x507FC0 -> 0x507340`) and EE
  `WriteBOOL` (`0x1402DA920 -> 0x1402DB990`) write later BOOLs with the same
  MSB-first cursor. Added public coverage proving repack/final-count handling
  preserves the next record's semantic bits at cursor 3. This rules out the CNW
  fragment header or finalization step as the two-bit owner for the active CEP
  v2.3 `U/6 mask=0xFFFF_FFF3` cursor; continue with the original server
  writer/handoff or pre-`U/10` continuation evidence.
- 2026-06-06 `P/05/01` full item `U/6` declared-window tail audit: no packet
  shape changed. Split the short update-tail ambiguity detector so item rows
  call the item-specific EE update verifier instead of the door/placeable-named
  wrapper, and added public declared-length coverage proving a scalar/direct-name
  full item `U/6 mask=0xFFFF_FFF3` can look like compact CNW fragment storage
  but must remain a read-buffer row until its own fragment bits prove the cursor.
  The private CEP v2.3 fixture still intentionally quarantines at `offset=104`,
  `record_end=148`, `bit_cursor=28` with neighboring item cursors
  `-4/-3/-2/+2/+4` as ambiguity only. Verified with
  `declared_length_window_rejects_full_item_update_as_fragment_tail`,
  `declared_length_`, `raw_neighbor_u6`, and the private CEP quarantine audit
  with live-claim tracing. Next owner search remains the source writer,
  CNW fragment storage/continuation boundary, or terminal-tail owner before the
  `U/10`/`A/6`/`U/6` sequence.
- 2026-06-06 `P/05/01` full item `U/6` later-row terminal-tail audit: no packet
  behavior changed. Re-ran the private CEP v2.3 live-claim trace and confirmed
  the stream still rejects the scalar-shaped full item `U/6` at `offset=104`,
  `record_end=148`, `bit_cursor=28` before later placeable rows are considered.
  Added fixture-free coverage proving a full item `U/6` followed by otherwise
  valid compact `A/9` plus exact `U/9` rows exact-claims when the item cursor is
  decompile-correct, but remains unclaimed and rolls back when the item row needs
  a two-bit neighboring-cursor skip. Later live-object rows therefore are not the
  missing owner for the CEP `U/6`; continue with the true source writer or
  CNW fragment storage/continuation handoff before `U/10`/`A/6`/`U/6`.
- 2026-06-07 `P/05/01` Diamond full item `U/6` server-writer recheck: no
  packet behavior changed. Direct PE disassembly of
  `C:\NWN\NWN Diamond\nwserver.exe` maps `0x445160` inside server `.text`, while
  the local `fullNwnDecompilePart*.txt` `0x445160`/`sub_444CC0` neighborhood is a
  separate client-reader decompile and must not be cited as server proof. The
  server call graph has exactly three direct `sub_445160` call sites
  (`0x43F7EC`, `0x444F23`, `0x4450AC`); the serializer writes `U`, object type,
  object id, and mask at `0x4451DC..0x44520D` (`0x508080`, `0x507FE0`,
  `0x508CB0`, `0x508450`). Server data bytes confirm object type 5 at
  `0x6338AC` and item type 6 at `0x6338AD`; the later `0x446247` branch compares
  against type 5, so item type 6 exits before the low-`0x40` branch. This agrees
  with the Diamond client-reader rule
  `sub_459700 -> sub_467AE0 -> sub_451AF0` and the EE reader rule
  `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0`: Diamond full item mask
  `0xFFFF_FFF3` translates to EE `0x0008_0033`, drops low `0x40`, and must not
  consume the next source fragment bit as hidden state. The recheck reconciles
  the earlier stale text-decompile warning, but still does not own the two bits
  before the CEP v2.3 `U/10`/`A/6`/`U/6` sequence; continue with local Diamond
  capture or higher-level write-message/list-handoff evidence before changing
  cursor ownership.
- 2026-06-07 `P/05/01` server call-site neighborhood audit: no packet behavior
  changed. Direct `nwserver.exe` disassembly of the three `0x445160` call
  neighborhoods keeps the serializer proof above but does not assign the CEP
  pre-`U/6` residue. The `0x43F7EC` path calls the U serializer after the
  `0x440130` add/snapshot setup and then enters `0x44AC70`/`0x44B520` snapshot
  and appearance helpers; the `0x444F23` and `0x4450AC` list walkers call the U
  serializer and then `0x444C70`, which copies selected snapshot fields. In the
  inspected neighborhoods these helpers do not write inter-record fragment
  BOOLs between U records, so the two active bits remain an upstream
  write-message/list-handoff or local Diamond capture target, not an item-local
  continuation or nearby shifted-cursor license.
- 2026-06-06 `P/05/01` typed item-create/update declared-capacity handoff
  audit: no packet behavior changed. Added public fixture-free coverage proving
  source-side declared-length capacity rejects an `A/6 -> U/6` read prefix when
  the typed item-create row is one Diamond source BOOL short, even if the
  following full item `U/6` has enough bits to look plausible. The positive
  sibling still accepts when `A/6` owns its five source bits and the following
  full item update owns its own decompile-correct scalar/direct-name bits. This
  rules out stale-declared capacity as the two-bit owner in the CEP v2.3 handoff;
  continue with true source writer / CNW fragment storage / continuation
  evidence before the `U/10`/`A/6`/`U/6` sequence. Verified with `cargo test -q
  -p hgbridge-proxy2
  declared_length_capacity_rejects_item_create_borrowing_following_update_bits
  -- --nocapture`.
- 2026-06-01 `P/05/01` full item `U/6` vector-orientation audit: no packet
  behavior changed, but public fixture-free coverage now pins the positive
  vector sibling of the all-bits item-update rule. Diamond `sub_467AE0` and EE
  `sub_14079C050` branch on the orientation BOOL before reading orientation
  bytes; when that BOOL is true, both readers consume six vector bytes and no
  scalar residual orientation bits before appearance/state/name. Tests now prove
  a correctly shaped vector `U/6 0xFFFF_FFF3` translates to EE mask
  `0x0008_0033` without moving the cursor, and that a preceding typed `A/6`
  active-property insert preserves those following vector bits exactly. This
  keeps the CEP v2.3 capture narrowed to shifted source bits rather than an
  unsupported vector-path gap. Verified with `cargo test -q -p
  hgbridge-proxy2 item_full_update -- --nocapture` and `cargo test -q -p
  hgbridge-proxy2 typed_item_create -- --nocapture`.
- 2026-06-01 `P/05/01` full item `U/6` before `W` audit: no packet behavior
  changed, but public fixture-free coverage now pins the item sibling of the
  terminal `W` rule. A decompile-correct scalar full item update can translate
  its Diamond `0xFFFF_FFF3` mask and exact-claim before `W current total`, but a
  vector-selected/scalar-shaped `U/6` row remains unclaimed and unchanged even
  when `W` follows. Diamond `sub_44F160` and EE `sub_1407B85A0` own only three
  read-buffer bytes and zero fragment BOOLs, so the remaining CEP v2.3 starter
  `U/6` failure is not a `W` suffix rescue. Verified with `cargo test -q -p
  hgbridge-proxy2 work_remaining_does_not_rescue_shifted_full_item_update_cursor
  -- --nocapture`. Next step remains finding the real owner for the shifted
  source bits or proving the stream-boundary artifact.
- 2026-06-01 `P/05/01` typed item-create legacy-width handoff audit: no packet
  behavior changed, but public fixture-free coverage now pins the Diamond-body
  sibling of the CEP v2.3 `A/6` handoff. When the EE object visual-map is
  already present but the model-type-2 appearance bytes are still Diamond-width
  BYTE fields and EE's active-property BOOL is absent, the bridge may widen only
  those three model bytes and insert only the EE active-property BOOL without
  moving the following decompile-correct scalar full `U/6` bits. The matching
  negative proof still rejects and rolls back a vector-selected/scalar-shaped
  following `U/6`, so the CEP v2.3 failure is not explained by typed `A/6`
  byte widening or active-property insertion. Continue tracing the source
  fragment cursor, original Diamond writer/handoff, or stream-boundary artifact.
  Verified with `cargo test -q -p hgbridge-proxy2 typed_item_create --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 item_full_update --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 work_remaining --
  --nocapture`.
- 2026-06-01 `P/05/01` tail9-before-item handoff audit: no packet behavior
  changed, but public fixture-free coverage now pins the upstream cursor that
  makes the current CEP v2.3 starter failure significant. The preceding
  `U/10 mask=0xFFFF_FFF7` tail9 door row owns eight legacy compact source bits
  (position residual, five state BOOLs, legacy name branch) and emits thirteen
  EE bits (inserted scalar-orientation fragment, the same five state BOOLs,
  EE-neutral state BOOL, with the legacy name branch removed). That net +5
  shift before `A/6`, plus the typed item-create active-property insertion,
  makes the following `U/6 mask=0xFFFF_FFF3` start at the observed source
  cursor where the orientation selector is vector-shaped while the bytes are
  scalar-shaped. The correct exact sibling still rewrites and claims, while the
  CEP-like shifted sibling rejects and rolls back, so do not add a U/6 scalar
  rescue here. Continue tracing the original Diamond writer/handoff or a real
  stream-boundary artifact before the `U/10`/`A/6`/`U/6` sequence. Verified
  with `cargo test -q -p hgbridge-proxy2 tail9_door_update_before_typed_item_create
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 typed_item_create --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 item_full_update --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 legacy_all_bits --
  --nocapture`, and `RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q
  -p hgbridge-proxy2
  dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit
  -- --nocapture`.
- 2026-06-01 `P/05/01` item cursor-neighbor audit: no packet behavior
  changed. Re-reading the item create decompiles confirmed the preceding typed
  `A/6` row is not the missing owner: Diamond `sub_451680 -> sub_451020`
  reads the item-name selector and four active-property BOOLs, while EE
  `sub_14079FE30 -> sub_14076BD30` reads the same source shape plus one
  inserted active-property/CanUseItem BOOL. Debug-only rejection tracing now
  reports nearby item-update cursors when `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM`
  is set; the current CEP v2.3 run still rejects at `offset=104`,
  `bit_cursor=28`, `raw_mask=0xFFFFFFF3`, corrected translated mask
  `0x00080033`, with neighboring cursors `-4`, `-3`, `-2`, `+2`, and `+4` also
  capable of
  validating if the parser were allowed to cheat. Public coverage now pins the
  negative rule: a full scalar `U/6` remains unclaimed/unchanged when two
  unowned pre-cursor fragment bits are present, even though the same item
  validator would succeed after an external owner consumed those bits. Next
  trace must prove the real fragment owner or stream-boundary handoff before
  the `U/6`; do not add cursor search/skip behavior. Verified with
  `cargo test -q -p hgbridge-proxy2
  full_item_update_does_not_skip_unowned_pre_cursor_residue -- --nocapture`
  and private `RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q -p
  hgbridge-proxy2
  dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit
  -- --nocapture`.
- 2026-06-01 `P/05/01` tail9/A6 two-bit residue audit: no packet behavior
  changed, but public fixture-free coverage now ties the cursor-neighbor rule
  back to the full CEP-like prefix. After the `U/10 0xFFFF_FFF7` tail9 rewrite
  and typed `A/6` item-create active-property insertion both succeed, two
  extra fragment bits before the following full `U/6` still remain unowned. A
  translated item reader would accept the `U/6` at `cursor + 2`, but neither
  preceding record may skip or consume those bits, so the combined rewrite must
  roll back unchanged. Next trace still needs the real source owner or a
  stream-boundary explanation before the `U/6`; do not add cursor search/skip
  behavior. Verified with `cargo test -q -p hgbridge-proxy2
  tail9_item_create_handoff_does_not_skip_two_unowned_bits_before_item_update
  -- --nocapture`.
- 2026-06-01 `P/05/01` short-strref door-add prefix audit: no packet behavior
  changed, but the CEP v2.3 prefix trace now also rules out the preceding
  raw `A/10` door add as the two-bit owner. Diamond short-strref door adds own
  five source BOOLs (name branch plus four post-name state bits), and the EE
  normalization emits the canonical six-bit empty-name/state shape by inserting
  one BOOL; it must not borrow the two following residue bits. Public coverage
  proves the exact `A/10` + `U/10` tail9 + typed `A/6` + full `U/6` sibling
  rewrites and claims, while the same prefix with two unowned pre-`U/6` bits
  rolls back unchanged. Next trace still needs a different fragment owner or a
  stream-boundary explanation before the `U/6`. Verified with `cargo test -q
  -p hgbridge-proxy2 short_strref_door_add_before_tail9_item_handoff --
  --nocapture`.
- 2026-06-01 `P/05/01` EE-shaped door-add prefix audit: no packet behavior
  changed, but public fixture-free coverage now pins the direct-empty generic
  `A/10` shape seen at the start of the current CEP v2.3 debug stream. The
  already-EE-shaped add owns the two door DWORDs, EE object visual-map, empty
  direct `CExoString`, state WORD, and exactly six fragment BOOLs; it is a
  valid boundary before `U/10` tail9 but cannot donate the two residue bits
  before the later full `U/6`. The positive sibling rewrites and exact-claims
  `A/10` + `U/10` tail9 + typed `A/6` + full `U/6`, while the same prefix with
  two unowned pre-`U/6` bits rolls back unchanged. Next trace still needs a
  different fragment owner or stream-boundary explanation before the `U/6`.
  Verified with `cargo test -q -p hgbridge-proxy2
  ee_shaped_door_add_before_tail9_item_handoff -- --nocapture`.
- 2026-06-01 `P/05/01` item-update stale downstream cursor audit: hardened the
  live-object update walker so a bounded item `U/6` rewrite failure marks the
  global fragment cursor unreliable instead of letting later rows rewrite from
  the stale pre-item cursor. Diamond `sub_467AE0` and EE `sub_14079C050` both
  branch on the item orientation BOOL before reading orientation bytes, so a
  vector-selected/scalar-shaped item update has no decompile-owned cursor to
  hand off. This does not claim the CEP v2.3 starter row; the private fixture
  still quarantines unchanged, and the next trace still needs the real owner or
  stream-boundary explanation before the `U/6`. Verified with `cargo test -p
  hgbridge-proxy2 failed_item_update_marks_following_fragment_cursor_unreliable
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  full_item_update_does_not_skip_unowned_pre_cursor_residue -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 ee_shaped_door_add_before_tail9_item_handoff
  -- --nocapture`, and private `RUSTFLAGS='--cfg hgbridge_private_fixtures'
  cargo test -q -p hgbridge-proxy2
  dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit
  -- --nocapture`.
- 2026-06-01 `P/05/01` item-update post-rewrite rollback audit: hardened the
  shared live-object rewriter so a bounded item `U/6` cursor failure after a
  reliable cursor had already staged earlier rewrites aborts final emit instead
  of committing those partial changes. This is the transactional half of the
  stale-downstream-cursor rule above; an item update with unowned or shifted
  orientation/name bits now leaves the whole source payload unchanged. Verified
  with `cargo test -q -p hgbridge-proxy2
  tail9_door_update_before_typed_item_create -- --nocapture`, which covers the
  exact-positive sibling and the shifted-following-`U/6` rollback.
- 2026-06-01 `P/05/01` CEP tail9 name-suffix cursor audit: no additional packet
  behavior changed, but public fixture-free coverage now pins the exact raw CEP
  v2.3 `U/10 0xFFFF_FFF7` tail9 source bits (`01100011`). The final legacy name
  branch is true and owns the four-byte suffix after the tail9 state WORD, but
  Diamond consumes only that one legacy name BOOL before returning to the next
  `A/6`; EE drops the branch and suffix while preserving the later `U/6` cursor.
  Positive/negative tests prove this tail9 variant exact-claims when the
  following `U/6` cursor is decompile-correct and rolls back when two unowned
  bits precede the `U/6`. The CEP fixture remains active; next trace still needs
  a different owner or stream-boundary explanation before the `U/6`. Verified
  with `cargo test -q -p hgbridge-proxy2 cep_tail9_name_suffix -- --nocapture`.
- 2026-06-01 `P/05/01` no-visual-map typed item-create handoff audit: no
  packet behavior changed, but public coverage now pins the generalized `A/6`
  sibling seen in the CEP v2.3 handoff. Diamond `sub_451020` owns the
  model-type-2 BYTE appearance body, item-name selector, and four active-item
  property BOOLs; EE `sub_14079FAC0` widens the model bytes, reads the object
  visual-transform map before active properties, and `sub_14076BD30` adds only
  the active-property/CanUseItem BOOL. The rewrite may insert those bytes/bits
  transactionally inside `A/6`, but it must preserve a following full `U/6`
  cursor and must roll back when two unowned bits precede that `U/6`. This
  rules out the missing EE item visual-map branch as the two-bit owner; the CEP
  fixture still needs a different fragment owner or stream-boundary
  explanation before offset 104. Verified with `cargo test -q -p
  hgbridge-proxy2 legacy_width_typed_item_create_without_visual_map --
  --nocapture` and `cargo test -q -p hgbridge-proxy2
  cep_tail9_name_suffix_before_legacy_width_item_create_without_visual_map --
  --nocapture`.
- 2026-06-02 `P/05/01` creature `U/5` zero-mask/visual-selector boundary
  audit: no packet behavior changed. The XP2 seq19 debug replay showed an early
  `U/5` row that byte-aligns like `OBJECTID + DWORD mask 0`, but the legacy
  creature visual-transform selector branch also starts `U/5 + OBJECTID + 00`
  and owns only that selector byte before interleaved CNW fragment storage.
  Public fixture-free coverage now pins both sides: a zero-looking selector row
  must wait for the following live-object boundary, while an isolated ten-byte
  zero-mask `U/5` exact-claims as a creature update. This rules out adding a raw
  ten-byte split before proving a real boundary at that cursor. The XP2 seq19
  terminal `G I 00` issue remains active; continue tracing later
  door/placeable/update cursor ownership rather than GUI search/skip behavior.
  Verified with `cargo test -q -p hgbridge-proxy2
  zero_mask_looking_creature_selector_storage_waits_for_following_boundary --
  --nocapture` and the filtered private XP2 debug replay.
- ~~2026-06-04 `P/05/01` creature `U/5 0x47` action-4 zero-followup cursor
  audit: no packet behavior changed, but public fixture-free coverage now pins
  the existing Diamond/EE movement-followup cursor rule used by local XP2 replay
  traces. Diamond/EE read position, scalar/vector orientation, optional target,
  action scalar/code, action-state byte, then the action follow-up count; action
  code 4 with zero follow-up count may carry one implicit 2D point only when the
  remaining read buffer still includes that point plus the full `0x0040` state
  tail, and the sibling no-point form remains exact when the state tail begins
  immediately. The exact validator rejects a truncated implicit-point/state-tail
  shape and restores the fragment cursor. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-creature-0047 cargo test -q -p hgbridge-proxy2 creature_update_0047_action4 -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 creature_update_mask_0047 -- --nocapture`,
  and `cargo test -q -p hgbridge-proxy2 live_object_update --
  --test-threads=1`.~~
- ~~2026-06-04 `P/05/01` creature `U/5 0x47` target/vector branch audit: no
  packet behavior changed, but public fixture-free coverage now pins two
  missing branch variants in the shared exact validator. Diamond can omit the
  orientation-target guard entirely before the action extra-float BOOL, and the
  same mask can carry vector orientation, an explicit target OBJECTID, the
  action-4 zero-count implicit point, and a mode-2 `0x0040` OBJECTID tail in one
  cursor. Removed the unused duplicate `0x47` parser so future bit-order work
  cannot diverge from the shared decompile-backed simulator. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-creature-0047-branches cargo test -q -p hgbridge-proxy2 creature_update_0047_action4 -- --nocapture`.~~
- ~~2026-06-04 `P/05/01` creature `U/5 0x4000` master-detail locstring audit:
  the exact cursor now accepts the optional dominated/master detail branch only
  in the decompiled order: five status BOOLs, guarded OBJECTID, two
  `WriteCExoLocStringServer` values, then the final two status BOOLs. The
  server locstring cursor follows the Diamond/EE bit-fronted shape proven in
  `sub_53E700` / `WriteCExoLocStringServer`: selector BOOL, optional
  language-selector BOOL plus DWORD strref, or inline length-prefixed
  CExoString. Fixture-free coverage proves no-master, direct/direct, TLK/direct,
  and shifted/missing-bit rejection. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-creature-4000-master cargo test -q -p hgbridge-proxy2 creature_update_4000_master_detail -- --nocapture`.~~
- 2026-06-01 `P/05/01` active-property tail and short-strref state audit:
  no packet behavior changed, but the generalized handoff proof now covers the
  exact leading `A/10` short-strref fragment values seen in the CEP v2.3
  fixture (`11010`: short-name branch plus post-name state bits `1010`).
  Diamond `sub_451020` and EE `sub_14076BD30` confirm the active-property
  body after an item name owns only the four Diamond BOOLs plus EE's single
  inserted post-DWORD BOOL; property rows, trailer masks, and value-mask bytes
  are read-buffer bytes, not fragment bits. Public coverage proves that the
  actual short-strref state variant plus CEP `U/10` suffix and no-visual-map
  `A/6` still preserves a following decompile-correct full `U/6`, while the
  shifted sibling rolls back unchanged. This rules out the first `A/10` state
  variation and the active-property value-mask tail as the two-bit owner; the
  CEP fixture still needs a different fragment owner or stream-boundary
  explanation before offset 104. Verified with `cargo test -q -p
  hgbridge-proxy2 cep_tail9_name_suffix_with_actual_short_strref_state --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 cep_tail9_name_suffix --
  --nocapture`, and the private CEP quarantine audit.
- 2026-06-01 `P/05/01` CEP raw handoff bit replay: no packet behavior changed,
  but public fixture-free coverage now replays the unresolved private-stream
  source bits without relying on semantic-looking helper defaults. The replay
  uses raw `A/10` bits `11010`, CEP `U/10` tail9 bits `01100011`, no-map
  `A/6` bits `00100`, and following `U/6` bits beginning
  `01110101100000`. After the bounded `A/10`, local/HG compact tail9 `U/10`,
  and decompile-owned `A/6` rewrites, the real `U/6` cursor still selects
  vector orientation while the bytes are scalar-shaped; the translated item
  reader accepts only at `cursor + 2`. Those two leading bits therefore remain
  unowned rather than a license for cursor search/skip behavior. Continue
  tracing a real fragment owner or stream-boundary artifact before offset 104.
  Verified with
  `cargo test -q -p hgbridge-proxy2
  cep_tail9_name_suffix_no_map_replays_raw_neighbor_u6_bits_without_repair --
  --nocapture`.
- 2026-06-06 `P/05/01` CEP raw handoff bit replay evidence correction: no
  packet behavior changed. Re-dumped the checked-in private CEP v2.3 starter
  fixture and confirmed the first post-header A/10 bits are `11010`
  (short-name branch plus state bits `1010`), superseding the stale `11011`
  label. Updated the raw no-map replay to use that actual A/10 state; the
  normalized EE-shaped A/10 sibling is unchanged. Both still reject unless a
  separate decompile-backed owner consumes the two bits before the following
  `U/6`, while the item reader still validates at cursor `+2`. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260606-auto-current cargo test -q -p hgbridge-proxy2 raw_neighbor_u6 -- --nocapture`.
- 2026-06-07 `P/05/01` CEP raw fragment-tail cursor audit: no packet behavior
  changed. Replayed the checked-in CEP v2.3 starter fragment tail at the CNW
  storage layer and pinned the first bytes/bits as `7A 63 23 AC...`: the first
  three MSB bits are only the CNW final-valid-bit header `011`; semantic bits
  begin at bit 3 with raw A/10 `11010`, U/10 tail9 `01100011`, no-map A/6
  `00100`, then the following full U/6 starts `01110101100000`. The two bits
  that make the item reader validate at cursor `+2` are therefore part of the
  raw U/6 source row, not header bits, continuation prefix storage,
  inventory/delete short-boundary storage, or terminal tail. Client decompile
  cross-check remains Diamond `sub_459700 -> sub_467AE0 -> sub_451AF0` and EE
  `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0`; server-side symbolic
  writer search was too noisy to identify the exact source writer. Next useful
  trace is still the Diamond server `WriteGameObjUpdate_UpdateObject` item
  mask `0xFFFF_FFF3` path or a local harness capture around the chunk boundary
  before the `U/10`/`A/6`/`U/6` sequence. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target-codex-verify/run-20260607-cep-proof cargo test -q -p hgbridge-proxy2 cep_ -- --nocapture`.
- 2026-06-07 `P/05/01` CEP declared-offset split audit: no packet behavior
  changed. Pinned the stream-layer rule that the CNW declared value in
  `P/05/01` is an absolute high-level payload offset, not a post-envelope
  length. The private CEP v2.3 starter fixture has `declared=393`, fragment
  tail length 18, and starts the tail at offset 393 with `7A 63 23 AC`; adding
  the seven-byte envelope width would skip into the middle of the real tail at
  `93 A9 C8 39`. This rules out a local declared-offset interpretation bug as
  the source of the two disputed pre-`U/6` bits. Next evidence target remains
  the Diamond server writer for item update mask `0xFFFF_FFF3` or a local
  Diamond harness capture around the chunk boundary before `U/10`/`A/6`/`U/6`.
  Verified with `cargo test -q -p hgbridge-proxy2 live_stream -- --nocapture`
  and private
  `RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q -p hgbridge-proxy2
  local_cepv23_starter_tail_starts_at_declared_offset -- --nocapture`.
- 2026-06-07 `P/05/01` CEP single-frame stream audit: no packet behavior
  changed. Rechecked local Diamond harness run
  `C:\nwnbridge\local-diamond-bridge-20260523-190505`; server seq17 logged as
  `inflated_length=411 expected_frames=1 packetized_sequence=1 zlib_stream=true`
  and duplicate replay `frames=1 ... compressed=210 replay="verified-packets"`.
  Added private stream-layer regression
  `local_cepv23_starter_single_frame_is_left_for_dispatcher` to prove that the
  complete high-level `P/05/01` fixture bypasses stream buffering unchanged and
  leaves no pending stream/proxy-owned zlib state. This rules out proxy
  chunk/continuation boundary ownership for the two disputed bits in this
  fixture; the next useful evidence target remains the Diamond server writer
  for item update mask `0xFFFF_FFF3` before `U/10`/`A/6`/`U/6`. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target-codex-verify/run-20260607-single-frame
  RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q -p hgbridge-proxy2
  local_cepv23_starter_single_frame_is_left_for_dispatcher -- --nocapture`.
- 2026-06-07 `P/05/01` item `U/6` helper cross-reference audit: no packet
  behavior changed. Corrected the active proof target for item update
  name/hidden bits from the shared EE item body reader to the EE `U/6` helper:
  Diamond `sub_459700 -> sub_467AE0 -> sub_451AF0` and EE
  `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0` read item name before
  EE's hidden-state BOOL. `sub_14076BD30` remains the EE item body/add path for
  active-property tails and is not the following full `U/6` reader. Added public
  fixture-free coverage proving a hidden-first bit order is rejected instead of
  being accepted as a swapped name/hidden cursor. This does not resolve the CEP
  v2.3 two-bit owner; the next useful evidence target remains the Diamond
  server writer/handoff for item update mask `0xFFFF_FFF3` before
  `U/10`/`A/6`/`U/6`.
- 2026-06-07 `P/05/01` Diamond server full item `U/6` writer audit
  (historical; the later server/client proof correction below supersedes the
  `sub_445160` server-writer attribution): no packet behavior changed. Do not
  use this earlier note as server-writer proof. Its stable conclusion is only
  the client-reader rule: full `U/6` item rows end after the generic prefix
  plus `sub_451AF0` item-name branch, and extra post-name bytes remain
  unowned until a separate server writer/handoff trace assigns them. The CEP
  v2.3 two-bit cursor remains unresolved; next work should capture the local
  Diamond server around the exact `U/10`/`A/6`/`U/6` handoff or continue
  tracing the upstream writer handoff before `U/6` to prove which earlier
  branch owns the disputed bits.
- 2026-06-07 `P/05/01` Diamond full item `U/6` call-site audit
  (historical; corrected below): no packet behavior changed. `sub_44AC70`
  still only populates the update snapshot from the selected mask and is not
  the fragment-bit writer, but the later local-decompile correction rejects the
  claim that `0x43F7EC`, `0x444F23`, `0x4450AC`, or `sub_445160` prove a
  Diamond server `U` serializer. The remaining useful evidence target is still
  a local Diamond harness capture or deeper server trace of the stream/chunk
  handoff before the top-level `U/10`/`A/6`/`U/6` sequence.
- 2026-06-07 `P/05/01` item `U/6` server/client proof correction: no packet
  behavior changed. The checked local decompile files are
  `C:\NWN\NWN Decompile\fullNwnDecompilePart1.txt` and `Part2.txt`; there is
  no separate checked `nwserver diamond decompile.txt` under that directory.
  The local `0x44515F`/`0x4451DE` neighborhood is labeled inside Diamond
  client read handler `sub_444CC0` and calls reader/state helpers such as
  `sub_4FB840`, `sub_4FB4C0`, `sub_4FBB40`, and `sub_407340`; it is not
  server-writer proof. Keep the client-side `U/6` bit-order proof as Diamond
  `sub_459700 -> sub_467AE0 -> sub_451AF0` versus EE
  `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0`, and keep extra full-item
  post-name bytes unowned. The active CEP v2.3 two-bit owner still requires a
  true server writer/handoff trace or local Diamond harness capture before the
  top-level `U/10`/`A/6`/`U/6` sequence.
- 2026-06-10 `P/05/01` item `U/6` rewrite-claim propagation: packet behavior
  remains exact-validator gated. The item update rewrite now carries the
  accepted immutable `ItemUpdateRewriteClaim` through the shared update-record
  rewrite, and the live-object rewrite ledger refuses to commit an item `U/6`
  row if the emitted mask, read end, or final bit cursor no longer match that
  accepted claim. This does not assign the CEP v2.3 two disputed bits and does
  not add cursor retry/skip behavior; it makes future diagnostics and ledger
  source-delta accounting consume the same typed `U/6` proof as the parser.
  Verified with `CARGO_INCREMENTAL=0
  CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260610-item-claim-commit
  cargo test -q -p hgbridge-proxy2
  successful_item_update_rewrite_reports_accepted_claim -- --nocapture`,
  `item_update_rewrite_claim`, `item_failure_source_window`, and
  `raw_neighbor_u6`.
- 2026-06-11 `P/05/01` item `U/6` failure evidence propagation: packet bytes
  and cursor ownership remain unchanged. A retained item-update cursor failure
  now carries a bounded structured snapshot of the contiguous rewrite ledger
  tail, including the preceding row families, source/emitted bit spans, and
  insertion/removal deltas. Strict dispatch logs that typed tail evidence with
  the existing focus failure and unowned-neighbor gap data, so a local Diamond
  capture or compact writer trace can compare the exact `A/10 -> U/10 -> A/6`
  handoff without depending on debug-only stderr text. Next proof target is
  still a source capture or source-side writer/list-handoff owner before the
  `U/10`/`A/6`/`U/6` sequence; do not add cursor skip/retry behavior.
- 2026-06-11 follow-up `P/05/01` item `U/6` bit-preview evidence: packet bytes
  and cursor ownership remain unchanged. Retained item-update cursor failures
  now include bounded source/emitted bit previews for the unowned neighboring
  cursor gap, and each retained contiguous-tail ledger row carries source bits
  from the immutable source snapshot plus emitted bits from the rewritten
  fragment stream. Strict dispatch logs those structured previews, so the next
  compact source capture can compare the exact `U/10 -> A/6 -> U/6` handoff
  without scraping lower-level debug-only source-window text. The two active
  pre-`U/6` bits remain unowned pending compact source capture or a
  source-side writer/list-handoff proof.
- 2026-06-11 follow-up `P/05/01` item `U/6` source-window evidence: packet
  bytes and cursor ownership remain unchanged. Retained item-update cursor
  failures now carry the bounded source-window row replay as structured bridge
  evidence: row offsets, opcodes, object ids, masks, bit starts/ends, and parser
  claim families around the failed full item `U/6`. The raw CEP-style
  `A/10 -> U/10 -> A/6 -> U/6` handoff now proves in the failure object that the
  preceding item add/create claim ends exactly at the failed cursor while the
  following `U/6 mask=0xFFFF_FFF3` remains unclaimed unless a separate source
  owner consumes its first two position bits. Next proof target remains compact
  source capture or a source-side writer/list-handoff owner before this row.
- 2026-06-11 follow-up `P/05/01` item `U/6` source-window bit previews:
  packet bytes and cursor ownership remain unchanged. Retained source-window
  evidence now includes bounded source-coordinate bit previews per row, so the
  compact `U/10 -> A/6 -> U/6` handoff can compare the exact two unowned lead
  bits and following item position bits from the normal failure object rather
  than from debug stderr. The two active pre-`U/6` bits still require compact
  source capture or source-side writer/list-handoff proof before assignment.
- 2026-06-11 follow-up `P/05/01` item `U/6` handoff evidence model: packet
  bytes and cursor ownership remain unchanged. Retained item cursor failures now
  carry a first-class handoff summary tying the previous ledger owner, failed
  focus `U/6` row, source/emitted two-bit gap, and first validating neighbor
  into one structured object, and the quarantine evidence report emits stable
  `item_handoff_*` lines. The next proof target remains a local compact source
  capture or source-side writer/list-handoff owner before assigning those bits.
- 2026-06-11 follow-up `P/05/01` item `U/6` neighbor-origin classifier:
  packet bytes and cursor ownership remain unchanged. The item handoff
  diagnostic now classifies a validating neighboring cursor by the parsed
  scalar/vector orientation branch before deciding whether the gap falls in
  orientation, state, or name bits. This follows the Diamond
  `sub_459700 -> sub_467AE0 -> sub_451AF0` and EE
  `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0` branch order and keeps
  source-capture comparisons from labeling post-orientation state/name gaps as
  generic inside-row evidence. The active two pre-`U/6` bits remain unassigned
  pending compact source capture or source-side writer/list-handoff proof.
- 2026-06-11 follow-up `P/05/01` item `U/6` focus-failure windows: packet
  bytes and cursor ownership remain unchanged. Retained item cursor failures now
  carry the translated focus failure mask plus bounded read-buffer and fragment
  bit windows at the exact typed parser stop point. Failure artifacts emit
  stable `focus_failure_mask`, `focus_failure_read_window`, and
  `focus_failure_bit_window` lines so a compact Diamond/source capture can
  compare the parser stop with the existing source-window/handoff rows without
  relying on debug stderr. The two active pre-`U/6` bits remain unassigned
  pending compact source capture or source-side writer/list-handoff proof.
- 2026-06-11 follow-up `P/05/01` item `U/6` handoff-verdict classifier:
  packet bytes and cursor ownership remain unchanged. The rewrite bit ledger
  now produces a typed handoff verdict with `source_owner`,
  `claimable_handoff`, and `handoff_blocker`, and retained failure/source-window
  evidence emits that verdict for the focus cursor and nearby validating
  neighbors. Inside-row cursors are classified before contiguous-tail gaps, so a
  shifted `U/6` neighbor cannot become claimable through saturated gap math. The
  active two pre-`U/6` bits remain unassigned pending compact source capture or
  source-side writer/list-handoff proof.
- 2026-06-23 follow-up `P/05/01` item `U/6` compact handoff sequence evidence:
  packet bytes and cursor ownership remain unchanged. Retained item handoff
  evidence now derives a typed sequence kind from bounded source-window rows,
  distinguishing direct `A/6 -> U/6` from the compact
  `U/10 -> A/6 -> U/6` handoff after the door update mask has been translated
  to EE `0x00000017`. The CEP-style no-map regression now proves that sequence
  is recognized while still blocked by the unowned emitted/source two-bit gap;
  the next production path remains compact source capture or source-side
  writer/list-handoff ownership before assigning those bits.
- 2026-06-07 `P/05/01` Diamond `CreateWriteMessage` fragment-header audit: no
  packet behavior changed. Diamond `nwserver` `0x507E30` initializes the CNW
  write cursor at byte offset 7, clears the bit cursor, and immediately calls
  the shared MSB bit writer `0x507340` for three zero bits; that same bit writer
  uses bit position `7 - cursor` when setting bits. Packetized fragment storage
  later overwrites those three MSB bits with the final-byte valid-bit count.
  Added public fixture-free coverage proving `pack_msb_valid_bits` and
  `decode_msb_valid_bits` reserve exactly those first three bits before
  semantic payload bits. This confirms the checked-in CEP raw fragment-tail
  audit's starting point but does not resolve the active two-bit owner: the
  disputed bits after the `A/10`/`U/10`/`A/6` prefix are still live-object
  payload bits, not CNW framing bits. Next useful target remains a local
  Diamond harness capture or deeper writer trace before the top-level
  `U/10`/`A/6`/`U/6` sequence.
- 2026-06-07 `P/05/01` full item `U/6` stale server-writer proof scrub: no
  packet behavior changed. Rechecked the local decompile set and found only
  `C:\NWN\NWN Decompile\fullNwnDecompilePart1.txt` plus `Part2.txt`; the
  separate `nwserver diamond decompile.txt` cited by older notes is absent.
  `0x445160` is inside Diamond client `sub_444CC0` and calls reader helpers such
  as `sub_4FB840`, `sub_4FB4C0`, `sub_4FBB40`, and `sub_407340`, so it remains
  invalid as source-writer proof. Cleaned the active issue text and item
  translator comment to cite only the retained client-reader/EE-reader rule for
  dropping low `0x40`. The CEP v2.3 two-bit owner remains active; next evidence
  still needs a true source writer/handoff trace or local Diamond capture before
  the `U/10`/`A/6`/`U/6` sequence.
- 2026-06-07 follow-up stale-attribution scrub: no packet behavior changed.
  Removed remaining code/test comments that still described direct Diamond
  server-writer proof for full item `U/6` low `0x40`. The retained proof is only
  the Diamond client-reader / EE reader bit order above; extra full-item tails
  and the two CEP v2.3 pre-`U/6` bits remain unowned until a true source-writer
  trace or local Diamond capture assigns them.
- 2026-06-07 source-writer symbol search note: no packet behavior changed. A
  focused `rg` pass over `C:\NWN\NWN Decompile\fullNwnDecompilePart1.txt`,
  `Part2.txt`, and `C:\NWN\HGX Source\hgx.server decompile\hgx.server
  decompile.txt` found only the known Diamond client-reader anchors
  (`sub_459700`, `sub_467AE0`, `sub_451AF0`) and no direct
  `WriteGameObjUpdate`, `UpdateObject`, `0xFFFF_FFF3`, or `4294967283` server
  writer match in HGX by those strings/constants. The next useful trace likely
  needs binary/callgraph work or a local Diamond capture around the exact
  `U/10`/`A/6`/`U/6` handoff rather than repeating those literal searches.
- 2026-06-07 `P/05/01` direct `nwserver.exe` item `U/6` writer audit: no
  packet behavior changed. Capstone/PE inspection of
  `C:\NWN\NWN Diamond\nwserver.exe` separates the absent text-decompile proof
  above from direct server-binary evidence: the server U serializer at
  `0x445160` writes `U`, object type, object id, and mask at
  `0x4451DC..0x44520D`; its three observed update-list call sites are
  `0x43F7EC`, `0x444F23`, and `0x4450AC`. In that same server function, the
  item/name branch writes the five state BOOLs and optional `0x80000` name path,
  then reaches `0x446247`, where only object type `5` continues into the later
  low-`0x40` branch at `0x4463B0`; object type `6` exits. This restores
  server-writer evidence for the retained rule that Diamond full item mask
  `0xFFFF_FFF3` drops EE's explicit hidden-state bit instead of consuming a
  following source BOOL. It does not assign the two active pre-`U/6` bits in the
  CEP v2.3 handoff; the next useful trace remains the upstream writer/handoff or
  local Diamond capture before the top-level `U/10`/`A/6`/`U/6` sequence.
- 2026-06-07 `P/05/01` server orchestrator/list-handoff audit: no packet
  behavior changed. Direct `nwserver.exe` disassembly of the outer live-object
  writer at `0x43FD30` shows `CreateWriteMessage` at `0x43FDB3`, then
  record-family helpers/list walkers separated by `0x508B70` write-length
  checks. The update-list handoff at `0x43FF38..0x43FF5A` only chooses
  `0x444E60` or `0x445010`; there is no direct `WriteBOOL` call in that
  handoff. The inspected `0x445010` global walker path calls the mask builder
  `0x4447D0`, the `U` serializer at `0x4450AC`, then snapshot copier
  `0x444C70`; no inter-record BOOL writer was found outside typed serializers.
  This rules out an outer `P/05/01` orchestration/list-handoff BOOL as the two
  active bits before the CEP v2.3 full item `U/6`. Continue with exact
  preceding-record serializer proof or a fresh local Diamond capture around the
  `U/10`/`A/6`/`U/6` boundary.
- 2026-06-07 `P/05/01` compact tail9 source-writer audit: no packet behavior
  changed. Direct `nwserver.exe` disassembly of the normal server `U`
  serializer at `0x445160` shows mask `0x0002` writes an orientation BOOL at
  `0x4452EF` or `0x445311` before scalar/vector orientation payload, mask
  `0x0010` writes the five door/placeable state BOOLs at
  `0x446034..0x44605C`, and mask `0x0008_0000` writes only the legacy name
  branch BOOL before optional name bytes (`0x4460BE`/`0x4460E7` and sibling
  type branches). Therefore the existing compact `0xFFFF_FFF7` tail9 fixture
  remains valid local/HG legacy evidence, but it is not proven to be the normal
  Diamond server writer path and must not be used to assign extra pre-`U/6`
  cursor ownership. The active CEP v2.3 item handoff still needs the compact
  writer/source capture or another decompile-backed owner before the
  `U/10`/`A/6`/`U/6` sequence.
- 2026-06-07 `P/05/01` compact tail9 proof-boundary scrub: no packet behavior
  changed. Rechecked the five `0xFFFFFFF7` literal neighborhoods in
  `C:\NWN\HGX Source\hgx.server decompile\hgx.server decompile.txt`; the
  inspected hits are mask/string helper cleanup paths rather than identifiable
  `P/05/01` live-object packet writers. Updated public fixture comments so the
  CEP `U/10` tail9 bit span is described as local/HG compact evidence, not as
  normal Diamond `0x445160` writer or decompile-owned width proof. The two
  pre-`U/6` bits remain unowned; next useful work is still a compact
  writer/source capture or another server-binary/decompile-backed owner before
  the `U/10`/`A/6`/`U/6` boundary.
- 2026-06-07 `P/05/01` server `A/6` no-map writer proof: no packet behavior
  changed. Generated a temporary Capstone PE listing for
  `C:\NWN\NWN Diamond\nwserver.exe` under `target-codex-verify/` and traced the
  server add/snapshot writer. `0x4401F0` selects item type 6 with table byte
  `0x6338AD`, writes `A` at `0x4403E3`, object type at `0x4403F0`, object id at
  `0x4403FA`, then enters the item branch at `0x4404F5`. That branch calls
  `0x436E80` for byte-only model/appearance data and `0x436C60` for item body
  fragment bits. In `0x436C60`, the no-name path writes exactly one name BOOL
  at `0x436D1B` and then the four Diamond active-property/status BOOLs at
  `0x436D52`, `0x436D8F`, `0x436D9D`, and `0x436DAB`; `0x436E80` uses only
  byte/word writers in the checked model-type paths. This server-side proof
  matches the public CEP no-map `A/6` source bits `00100` and rules out the
  typed item-create row as owner for the two following pre-`U/6` bits. The
  unresolved owner remains before or inside the compact `U/10` source/capture
  boundary, not in `A/6`; next useful work is a compact writer/source capture or
  deeper proof for the `U/10` tail9 source family before the `U/6` handoff.
- 2026-06-07 `P/05/01` CEP seq17 raw-M provenance audit: no packet behavior
  changed. Rechecked the local Diamond packet dump
  `C:\nwnbridge\local-diamond-bridge-20260523-190505\diamond-packets`; the
  first raw server seq17 datagram is
  `000042_sendto_socket740_len226.bin` (SHA-256
  `AE5FECB58CED090785FE1162F4DE34DBD2F25F36FA9DF284142AA4CF8E659CFB`), with
  `flags=0x0F`, `packetized_sequence=1`, payload length 214, inflated length
  411, and 210 compressed bytes. The proxy log shows the checked-in inflated
  CEP fixture comes from the proxy's server inflater immediately after seq16 was
  semantically rewritten with `used_server_stream=true proxy_owned_stream=true`;
  that fixture remains valid for live-object/dispatcher regression tests, but
  the raw M datagram is transport provenance only, not a compact `U/10`
  source-writer proof. The two pre-`U/6` bits therefore remain unowned; continue
  with a true compact writer/source capture or decompile-backed owner before
  the `U/10`/`A/6`/`U/6` handoff.
- 2026-06-07 `P/05/01` compact door/placeable reader-vs-writer address audit:
  no packet behavior changed. Direct `nwserver.exe` disassembly around
  `0x44E2C0`/`0x44E4A0` shows those VAs are not direct-call server writer
  entries in the server binary (no direct `E8` call targets were found; the
  range lies inside a different server writer body). Treat `sub_44E2C0` and
  `sub_44E4A0` only as Diamond client-reader anchors from the local client
  decompile. They remain useful for reader bit order, but they do not prove the
  compact `U/10 tail9` source writer or assign the two active pre-`U/6` bits.
  Next work still needs a compact writer/source capture or another
  server-binary-backed owner before the `U/10`/`A/6`/`U/6` handoff.
- 2026-06-08 `P/05/01` stock server `U` writer census: no packet behavior
  changed. A direct `nwserver.exe` Capstone scan over `CNWMessage` char-writer
  calls with pushed opcode `0x55` found candidate server call sites at
  `0x43F246`, `0x43F2AA`, `0x4417C9`, `0x443EA8`, `0x4445F1`, and `0x4451E2`.
  Only the `0x4451E2` path inside serializer `0x445160` writes the typed
  live-object update header in the required order: `U`, object type, object id,
  then mask. The other candidates emit different subprotocol rows such as
  id/mask-only updates or `G/i/U` and `G/M/U` GUI/state rows, so they are not
  alternate stock writers for the compact `U/10 mask=0xFFFF_FFF7` tail9 record.
  This strengthens the boundary that the compact tail9 row remains local/HG
  capture evidence, not a proven normal Diamond writer shape; it still does not
  assign the two active pre-`U/6` bits.
- 2026-06-08 `P/05/01` stock-vs-compact `U/10` cursor guard: no packet behavior
  changed. Added public fixture-free coverage that pairs the normal stock
  `U/10` position/orientation/scale-state/state read-body layout with both the
  plain and CEP-name-suffix compact tail9 source-bit sequences. Both variants
  must reject exact claim/rewrite and leave the payload unchanged. This pins the
  `nwserver.exe` `0x445160` proof that the mask-`0x0002` orientation BOOL is
  written before mask-`0x0010` state BOOLs, so compact tail9 capture bits cannot
  be reused as a normal stock writer cursor. The two active pre-`U/6` bits
  remain unowned; next useful work is still a compact-source capture or another
  source-writer/handoff proof before the `U/10`/`A/6`/`U/6` boundary.
- 2026-06-09 `P/05/01` inverse stock-vs-compact `U/10` cursor guard: no packet
  behavior changed. Added public fixture-free coverage that pairs compact
  `U/10 mask=0xFFFF_FFF7` tail9 bytes with the normal stock scalar-orientation
  `U/10` source cursor. The rewrite must reject and roll back unchanged instead
  of treating stock orientation selector/residual bits as compact tail9
  state/name bits. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-20260609-tail9-audit cargo test -q -p hgbridge-proxy2 compact_tail9_bytes_do_not_match_stock_u10_scalar_source_bits -- --nocapture`.
  This only strengthens the bit-order boundary; it still does not assign the
  two active pre-`U/6` bits or prove a stock compact-tail source writer.
- 2026-06-09 `P/05/01` item `U/6` neighboring-cursor diagnostic audit: no
  packet behavior changed. The full item-update reader remains exact at the
  inherited CNW cursor, but packet bytes alone can be ambiguous when two
  disputed pre-`U/6` bits are treated as position residuals and later rows
  consume the real item tail. Added debug-only accepted-cursor tracing under
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM` so harness/live replays report item
  updates whose current cursor validates while nearby cursors would also
  validate. This is evidence collection only: do not add a heuristic rejection
  or cursor search from that ambiguity. The active owner question remains the
  compact source writer/handoff before the `U/10`/`A/6`/`U/6` boundary.
- 2026-06-09 private compact handoff trace rerun: no packet behavior changed.
  Re-ran
  `dispatcher_quarantines_local_cepv23_starter_lance_lute_patron_live_object_after_boundary_audit`
  with `RUSTFLAGS=--cfg hgbridge_private_fixtures`,
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM=1`, and isolated non-incremental cargo
  target `C:\nwnbridge\codex-target-ee-bridge-20260609-u6-debug`. The trace
  still normalizes the leading A/10, rewrites compact `U/10 mask=0xFFFF_FFF7`,
  reaches the no-map A/6 row, and rejects the following full item
  `U/6 mask=0xFFFF_FFF3` at `offset=104`, `record_end=148`, `bit_cursor=28`.
  The rejected item row reports neighboring fits `-4`, `-3`, `-2`, `+2`, and
  `+4`, matching the existing public ambiguity fixture. No
  `item update accepted with neighboring cursor(s)` diagnostic appeared before
  the rejection, so the accepted-cursor tracer found no earlier same-family
  owner in this compact path. Continue with compact-source capture or a
  decompile/server-binary proof for the source writer/handoff before the
  `U/10`/`A/6`/`U/6` boundary; do not add item cursor search, scalar-byte
  rescue, or accepted-neighbor rejection heuristics.
- 2026-06-09 `P/05/01` item `U/6` source-window diagnostic: no packet behavior
  changed. Added production debug tracing for the exact failure point where a
  reliable cursor becomes unreliable on an item `U/6` rewrite. Under
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM`, the rewriter now emits a bounded window
  of nearby live-object row starts, object ids, update masks, read-byte
  previews, and before/after fragment bits around the inherited cursor. This is
  evidence collection only and does not try neighboring cursors or trim bits.
  Next verification should rerun the private compact handoff fixture with
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM=1` and
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM_OWNER_OFFSET=104,148` to inspect whether
  the transformed row window exposes a source-side boundary or confirms that
  the owner remains before the compact `U/10` record.
- 2026-06-09 private compact source-window ledger rerun: no packet rewrite
  behavior changed. Re-ran the private compact handoff fixture with the source
  window enabled and extended the production debug path to report cumulative row
  bit claims. The real stream now shows the current proof boundary exactly:
  EE-shaped `A/10` owns bits `3..9`, compact rewritten `U/10` owns `9..22`,
  no-map typed `A/6` owns `22..28`, and the following full item
  `U/6 mask=0xFFFF_FFF3` remains unclaimed at bit `28`; the nearby scalar fits
  remain cursor-neighbor ambiguity only. Next source-side target is still the
  compact `U/10` source writer/capture before assigning two extra bits; do not
  add a `U/6` cursor skip, scalar-byte rescue, or generic inter-row trim.
- 2026-06-09 `P/05/01` item `U/6` neighbor-owner diagnostic: no packet rewrite
  behavior changed. Extended the source-window debug path so rejected item
  update neighbors report whether a validating nearby cursor is already owned
  by a prior row, overlaps a prior claim, or starts inside the failed focus
  row. Public fixture-free coverage pins the active `+2` shape as
  `inside-focus-row`: the preceding claims end at the inherited cursor, so a
  scalar-shaped fit two bits later is not ownership proof. Continue with compact
  source writer/capture evidence before assigning those bits.
- 2026-06-09 `P/05/01` item `U/6` cursor-stage diagnostic: no packet rewrite
  behavior changed. Refactored the production item-update cursor validator to
  return a typed EE claim/failure result with stage, read cursor, bit cursor,
  translated mask, and orientation branch metadata, then wired that into the
  live-object source-window debug output. Focused coverage proves a
  vector-selected cursor over scalar-shaped item update bytes reports the
  `orientation-vector-read-bytes` failure stage, while the existing full-mask
  `U/6` shifted-cursor regression still refuses to search neighboring cursors.
  The active two pre-`U/6` bits remain unowned; next useful production path is
  still compact source writer/capture evidence before the `U/10`/`A/6`/`U/6`
  handoff.
- 2026-06-09 `P/05/01` item `U/6` neighbor-claim diagnostic: no packet rewrite
  behavior changed. Extended the typed item-update claim result so accepted
  candidates carry read end, translated mask, and scalar/vector orientation
  branch metadata, and emitted those fields from the source-window neighboring
  cursor trace. Public coverage pins the active `+2` neighbor as a translated
  full-item scalar claim that starts inside the failed `U/6` row and consumes
  the bounded item bytes; this remains ambiguity only, not owner proof. Next
  useful evidence is still compact source writer/capture proof before the
  `U/10`/`A/6`/`U/6` handoff.
- 2026-06-09 `P/05/01` compact tail9 typed-claim refactor: packet behavior
  remains gated by the same exact validators, but the production
  door/placeable `U` rewriter now parses the compact nine-byte tail into a
  `CompactDoorPlaceableTail9UpdateClaim` before mutating bytes or bits. The
  claim owns the tail offset, translated mask with EE appearance/name cleared,
  source mask with stock orientation removed, and the inserted scalar
  orientation value. Focused coverage proves the claim accepts the bounded
  `0xFFFF_FFF7` tail9 shape and rejects invalid name-only suffix bytes without
  an orientation/scale tail owner. This makes the compact `U/10` boundary a
  reusable production parser state; it still does not assign the two active
  pre-`U/6` bits.
- 2026-06-09 `P/05/01` item `U/6` rewrite-claim refactor: packet behavior
  unchanged. The item update rewriter now validates an immutable
  `ItemUpdateRewriteClaim` carrying raw mask, translated mask, read end,
  next bit cursor, and scalar/vector orientation metadata before mutating the
  row. The live-object source-window ledger uses the same claim instead of an
  ad hoc translated-mask candidate, so neighboring-cursor diagnostics now match
  the parser contract the rewriter can actually commit. This still does not
  assign the two active pre-`U/6` bits; next proof remains compact source
  writer/capture evidence before `U/10`/`A/6`/`U/6`.
- 2026-06-09 `P/05/01` live-object rewrite bit-ledger diagnostic: no packet
  rewrite behavior changed. The update pass now records a bounded debug ledger
  for committed add/update rows with source bit cursor, emitted EE bit cursor,
  and inserted/removed bit deltas. When a reliable cursor fails at item `U/6`,
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM` source-window output includes that ledger
  before the row-window reconstruction, so the compact `U/10 tail9` and typed
  `A/6` source-vs-emitted cursor gap can be checked directly on the private
  compact handoff fixture. This still does not assign the two active
  pre-`U/6` bits; next verification is another compact-source capture/debug run
  before the `U/10`/`A/6`/`U/6` boundary, not a cursor skip or scalar rescue.
- 2026-06-09 `P/05/01` live-object rewrite ledger cursor-gap diagnostic: no
  packet rewrite behavior changed. The committed-row ledger now classifies a
  focus or neighboring cursor against the previous emitted EE row and reports
  `after-previous-emitted-end`, `inside-previous-emitted-row`, or
  `unowned-emitted-gap` plus the source/emitted delta. A private CEP v2.3
  handoff rerun with `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM=1` and
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM_OWNER_OFFSET=104,148` shows the real
  `U/6` focus cursor at bit `28` is exactly after the committed no-map `A/6`
  emitted range `22..28`, while the scalar-looking `+2` item candidate starts
  at bit `30` with `ledger_relation=unowned-emitted-gap` and
  `ledger_emitted_gap_bits=2`. This confirms the neighboring scalar fit still
  requires two unowned emitted bits after committed rows; continue with compact
  source writer/capture proof before assigning those bits.
- 2026-06-09 `P/05/01` live-object rewrite ledger state refactor: packet
  rewrite behavior remains unchanged. The add/update rewrite pass now commits
  source/emitted row spans through a `LiveObjectRewriteBitLedger` state object
  instead of a loose `Vec` plus external source cursor, so source consumption,
  EE-emitted cursor spans, inserted/removed bit deltas, and gap classification
  share one production contract. The private CEP v2.3 boundary rerun still
  quarantines the same full item `U/6`: focus bit `28` is exactly after the
  no-map `A/6` emitted row, while the scalar-looking `+2` fit remains an
  unowned emitted gap. Next production path remains compact source
  writer/capture proof or another source-side owner before `U/10`/`A/6`/`U/6`;
  do not add U/6 cursor skip, scalar-byte rescue, or generic inter-row trim.
- 2026-06-09 `P/05/01` live-object rewrite ledger source-gap diagnostic: no
  packet rewrite behavior changed. The committed-row ledger now maps an
  emitted cursor back through the previous row's cumulative inserted/removed
  bit delta and reports `source_relation`, `source_gap_bits`, and
  `implied_source_cursor` beside the existing emitted-gap relation in the
  gated item `U/6` source-window trace. Focused coverage proves that a cursor
  exactly after an EE-inserting row maps back to `after-previous-source-end`,
  while the scalar-looking `+2` item candidate still leaves a two-bit
  `unowned-source-gap` after subtracting the committed EE insertion delta. The
  next owner proof remains compact source writer/capture evidence or another
  source-side handoff before `U/10`/`A/6`/`U/6`.
- 2026-06-09 `P/05/01` live-object rewrite ledger delete-row audit: no packet
  rewrite behavior changed. Exact `D` rows now commit to the same
  `LiveObjectRewriteBitLedger` after their decompile-owned delete fragment bits
  are proven, so a later item `U/6` source-window trace attributes cursor gaps
  to the immediately preceding delete row instead of an older add/update row.
  Focused coverage pins exact and shifted cursors after a `delete-exact` span.
  This does not assign the two active pre-`U/6` bits; continue with compact
  source writer/capture or another source-side owner before `U/10`/`A/6`/`U/6`.
- 2026-06-09 `P/05/01` live-object rewrite ledger commit invariant: production
  state machinery now commits add/update/delete ownership through a typed
  ledger span with checked emitted/source bit deltas. Impossible spans, such as
  a backwards emitted cursor or an inserted-bit count larger than the emitted
  row delta, mark the cursor unreliable and stop row-local trim/promotion work
  instead of silently omitting the diagnostic entry. This does not assign the two
  active pre-`U/6` bits; it makes the next compact source-writer/capture proof
  consume the same checked source/emitted ownership contract.
- 2026-06-09 `P/05/01` live-object add-row ledger audit: production state
  machinery now exposes exact inserted/removed fragment-bit counts from the
  door/placeable add rewriter and commits early add repair, guard-repair, and
  cursor-only verified add rows through the same `LiveObjectRewriteBitLedger`
  contract as update/delete rows. This closes a diagnostic blind spot before
  compact-source handoff proof: later item `U/6` cursor-gap traces can now
  attribute gaps to an immediately preceding compact add repair instead of
  falling back to an older row. This does not assign the two active pre-`U/6`
  bits; continue with compact source writer/capture or another source-side
  owner before `U/10`/`A/6`/`U/6`.
- 2026-06-10 `P/05/01` creature-update ledger audit: production state machinery
  now commits verified creature `U/5` rows into `LiveObjectRewriteBitLedger`
  after their exact cursor is proven, including zero-fragment visual-transform
  rows and decompile-owned promoted-span creature updates. Promoted storage bits
  are treated as source bits, not EE-generated insertions, so later item/creature
  cursor-gap diagnostics no longer skip over a preceding `U/5` row or attribute
  its zero-bit handoff to an older add/update/delete entry. Packet rewrite
  behavior is unchanged; the compact `U/10`/`A/6`/`U/6` two-bit owner still
  needs compact source-writer/capture or another source-side proof.
- 2026-06-10 `P/05/01` compact tail9 bit-claim ledger audit: production
  rewrite state now carries the typed compact door/placeable tail9 parser's
  source-bit claim through `RecordRewrite`, and the live-object ledger refuses
  to commit the row if final inserted/removed bit counts no longer match that
  immutable claim. Private CEP boundary tracing now labels the compact
  `U/10 mask=0xFFFF_FFF7` row as `update-compact-tail9-rewrite` with
  source bits `9..17` and emitted bits `9..22` before the no-map `A/6`
  owns source bits `17..22` and emitted bits `22..28`; the following full
  item `U/6 mask=0xFFFF_FFF3` still rejects at bit `28`, while the +2 scalar
  fit remains an unowned source/emitted gap. This tightens the compact-tail
  source contract but still does not assign the two active pre-`U/6` bits;
  continue with compact source-writer/capture or another source-side handoff
  proof before changing cursor ownership.
- 2026-06-10 `P/05/01` exact GUI/inventory/W ledger audit: production rewrite
  state now commits verified `G` GUI rows, `I` inventory rows, and zero-bit
  `W current total` work-remaining rows into `LiveObjectRewriteBitLedger` after
  their exact validators prove the cursor. Interleaved GUI/inventory fragment
  promotions remain source-bit movement, while GUI rewrite bit deltas are
  recorded as emitted EE insertions/removals. The source-window diagnostic now
  names the same exact row families so a later compact `U/10 -> A/6 -> U/6`
  cursor-gap trace can attribute ownership to immediate `G`/`I`/`W` neighbors
  instead of an older row. This does not assign the active +2 pre-`U/6` bits;
  continue with compact source-writer/capture or another decompile-backed
  source owner before changing cursor ownership.
- 2026-06-10 follow-up `P/05/01` typed item-create ledger audit: production
  rewrite state now carries the decompile-selected `A/6` item-create source
  and emitted fragment-bit counts out of the shared item-create translator, and
  the live-object ledger verifies those counts before committing the row as
  `item-create-rewrite`. This pins the no-map `A/6` handoff as a five-source/
  six-emitted-bit owner after EE's active-property BOOL insertion, so a later
  `U/6` neighbor fit at cursor +2 remains an unowned source/emitted gap rather
  than bits donated by the item-create row. Packet bytes are unchanged; the
  remaining owner still needs compact source-writer/capture proof or another
  source-side handoff before changing `U/6` cursor ownership.
- 2026-06-10 follow-up `P/05/01` item source-gap range diagnostic: production
  source-window tracing now prints the exact emitted/source gap ranges and bit
  values for ledger focus, expected, and neighboring `U/6` cursors. This keeps
  the compact `U/10 -> A/6 -> U/6` replay focused on the concrete two active
  pre-`U/6` bits instead of only reporting aggregate gap counts. Packet bytes
  are unchanged; continue with compact source-writer/capture proof before
  assigning those bits.
- 2026-06-10 follow-up `P/05/01` contiguous handoff ledger diagnostic:
  production source-window tracing now reports the contiguous committed
  source/emitted row chain behind each focus, expected, and neighboring `U/6`
  cursor. The ledger stops that chain at real emitted/source gaps, so the
  compact `U/10 -> A/6 -> U/6` replay can distinguish "unowned two-bit gap
  after the no-map item-create handoff" from "same contiguous writer chain
  still owns this cursor". Packet bytes are unchanged; next proof remains a
  compact source capture or decompile-backed source owner before changing
  `U/6` cursor ownership.
- 2026-06-10 follow-up `P/05/01` row-level handoff ledger diagnostic:
  production source-window tracing now includes the contiguous tail's exact
  row spans in the single-line focus/neighbor summary:
  `offset..record_end:family[src=start..end,emit=start..end,+inserted,-removed]`.
  This makes compact `U/10 tail9` and no-map `A/6` provenance visible on the
  same line as the remaining `U/6` cursor gap. Packet bytes are unchanged; the
  two pre-`U/6` bits still need compact source-writer/capture proof or another
  source-side handoff before any cursor ownership change.
- 2026-06-10 follow-up `P/05/01` item cursor-failure reason audit:
  production rewrite state now stores the first fatal item `U/6` cursor failure
  with the exact focus row and a ledger-derived reason such as
  `item-update-cursor-failed-after-unowned-ledger-gap`. The final rejection and
  source-window trace now use that structured reason instead of a generic
  after-rewrite marker. Packet bytes and cursor ownership are unchanged; the
  compact `U/10 -> A/6 -> U/6` path still requires compact source capture or
  decompile-backed source ownership before any `U/6` cursor change.
- 2026-06-10 follow-up `P/05/01` item valid-neighbor gap classifier:
  production failure state now records the nearest forward item `U/6` cursor
  that validates only after an unowned ledger gap, and upgrades the fatal reason
  to `item-update-cursor-failed-before-valid-neighbor-unowned-gap` only when the
  inherited cursor itself is an exact ledger handoff. The private CEP handoff
  fixture now reports focus bit `28`, valid neighbor bit `30`, emitted gap
  `28..30`, source gap `22..24`, and previous owner
  `51..104:item-create-rewrite`. Packet bytes and cursor ownership are still
  unchanged; the next useful proof remains compact source capture or a
  source-side writer/handoff owner before changing the `U/6` cursor.
- 2026-06-11 follow-up `P/05/01` ledger source-bit snapshot diagnostic:
  production rewrite state now snapshots the live-object source fragment bits
  when `LiveObjectRewriteBitLedger` starts, after any pre-ledger fragment-span
  promotion. Source-window gap diagnostics print source gap values from that
  immutable source coordinate space while keeping emitted gap values tied to the
  current EE-facing bitstream, so prior EE insertions no longer make a
  post-rewrite vector slice look like source proof. Packet bytes and cursor
  ownership are unchanged; continue with compact source capture or a
  source-side writer/handoff owner before assigning the two pre-`U/6` bits.
- 2026-06-11 follow-up `P/05/01` ledger source-bound/anchor audit: production
  rewrite state now rejects ledger commits whose source span would run past the
  captured source bitstream, and item `U/6` source-window diagnostics replay
  mid-stream row claims from the first ledgered row cursor instead of the failed
  focus cursor. Packet bytes and cursor ownership are unchanged; this keeps the
  compact `U/10 -> A/6 -> U/6` handoff evidence tied to exact source/emitted
  ownership while the active proof still needs compact source capture or a
  source-side writer/handoff owner. Verified with focused ledger/source-window
  tests and `cargo check -q -p hgbridge-proxy2`.
- 2026-06-11 follow-up `P/05/01` item-neighbor failure-state audit:
  production item `U/6` cursor diagnostics now preserve the inherited-cursor
  parser failure stage/read cursor/bit cursor/orientation branch when a later
  neighboring cursor validates only after an unowned ledger gap. The item parser
  also carries the orientation branch through final record-end mismatches, so the
  compact no-map `A/6 -> U/6` trace can report both facts on one failure: the
  real cursor selected the wrong branch, while the +2 scalar fit remains after a
  two-bit unowned source/emitted gap. Packet bytes and cursor ownership are
  unchanged; continue with compact source capture or source-side writer/handoff
  proof before assigning those two bits.
- 2026-06-11 follow-up `P/05/01` item cursor quarantine classification:
  production live-object update rewrites now expose a bounded attempt result
  carrying the fatal item `U/6` ledger cursor reason, and the strict M-frame
  live-object dispatcher preserves that reason as the quarantine classification
  when no later translator proves exact ownership. The compact no-map
  `U/10 -> A/6 -> U/6` fixture-free shape now asserts
  `item-update-cursor-failed-before-valid-neighbor-unowned-gap` while still
  rolling back packet bytes and fragment bits unchanged. This is classification
  only, not cursor ownership; the two active pre-`U/6` bits still need compact
  source capture or source-side writer/handoff proof before translation changes.
  Verified with focused `cep_tail9_name_suffix_no_map_replays_raw_neighbor_u6_bits_without_repair`,
  `item_update_cursor_failure_reason_uses_ledger_gap_shape`, `cargo fmt
  --all --check`, `cargo check -q -p hgbridge-proxy2`, and `git diff --check`.
- 2026-06-11 follow-up `P/05/01` ledger row bit-value diagnostic: production
  source-window tracing now prints bounded source and emitted bit previews for
  each recent committed ledger row. Source previews come from the immutable
  source fragment snapshot while emitted previews come from the current
  EE-facing bitstream, so the next compact `U/10 -> A/6 -> U/6` rerun can see
  exactly which original bits each prior row consumed before the remaining
  two-bit gap. Packet bytes and cursor ownership are unchanged; next proof
  remains compact source capture or source-side writer/handoff evidence.
- 2026-06-11 follow-up `P/05/01` item-neighbor gap-origin diagnostic:
  production item `U/6` cursor failure state now classifies where a validating
  neighboring cursor begins inside the failed focus row. The compact no-map
  `A/6 -> U/6` shape reports `gap_origin=focus-position-bits`, meaning the
  +2 scalar-looking neighbor starts only after the inherited cursor's two
  decompile-owned item position bits. Packet bytes and cursor ownership remain
  unchanged; this tightens the next compact source-writer/capture proof target.
- 2026-06-11 follow-up `P/05/01` item cursor typed-failure audit: production
  rewrite attempts now carry a `LiveObjectUpdateRewriteFailureKind` plus typed
  item-neighbor gap origin while preserving the existing quarantine reason text.
  The compact no-map `A/6 -> U/6` shape is now structured as
  `ItemUpdateCursorBeforeValidNeighborUnownedGap` with `FocusPositionBits`, so
  future source-owner policy can branch on the decompile-owned item position-bit
  proof instead of parsing diagnostic strings. Packet bytes and cursor ownership
  remain unchanged; next proof still needs compact source capture or a
  source-side writer/handoff owner before any `U/6` cursor change.
- 2026-06-11 follow-up `P/05/01` item cursor evidence propagation: production
  rewrite failures now carry structured parser/gap evidence for the failing
  item `U/6` row: focus parser stage/read cursor/bit cursor/orientation branch,
  nearest validating unowned neighbor, emitted/source gap ranges, previous
  ledger owner, and typed gap origin. The M-frame server dispatcher preserves
  that same live-object failure object alongside the existing quarantine reason,
  so harness/strict diagnostics can consume exact state instead of scraping
  debug text. Packet bytes and cursor ownership remain unchanged; next proof
  still needs compact source capture or a source-side writer/handoff owner
  before changing `U/6` cursor handling.
- 2026-06-11 follow-up `P/05/01` source-window neighbor evidence: production
  failure state now retains bounded nearby item `U/6` cursor fits inside
  `LiveObjectUpdateSourceWindowEvidence`, including focus-row relation, typed
  gap origin, ledger source/emitted gap ranges, gap bit previews, and previous
  ledger owner. Strict dispatch logs the retained neighbor count and first fit.
  Packet bytes and cursor ownership are unchanged; next proof still needs
  compact source capture or a source-side writer/handoff owner before assigning
  the two pre-`U/6` bits. Verified with focused source-window/item-cursor
  regressions, the CEP-like no-map handoff regression, `cargo check`, and
  `git diff --check`.
- 2026-06-11 follow-up `P/05/01` item failure artifact capture: production
  strict dispatch now writes paired `.bin`/`.txt` diagnostics for retained item
  `U/6` rewrite failures when the quarantine diagnostics directory is enabled.
  The binary keeps the exact rejected live-object candidate; the text report
  includes the structured failure kind, focus parser stage, unowned neighbor,
  contiguous rewrite tail, source-window rows, and nearby cursor fits. Packet
  bytes and cursor ownership are unchanged; this makes the next compact source
  capture/local harness run compare normal artifacts rather than reconstructing
  the handoff from logs. The two pre-`U/6` bits still need source-writer or
  capture proof before assignment.
- 2026-06-11 follow-up `P/05/01` item failure artifact row report:
  production failure-artifact text now emits stable, one-line contiguous-tail,
  source-window row, and nearby-neighbor records with source/emitted bit
  previews and bounded row bytes. This replaces relying on Rust debug dumps for
  the compact `U/10 -> A/6 -> U/6` handoff comparison; packet bytes and cursor
  ownership remain unchanged. Verified focused report/source-window/raw-neighbor
  tests, `cargo check`, `cargo fmt --all --check`, and `git diff --check`.
  A broader `live_object_update` filter still has existing rewrite-test
  failures unrelated to this formatter-only path and should be handled as a
  separate packet-behavior task.
- 2026-06-11 follow-up `P/05/01` item `U/6` source-owner verdict:
  production failure evidence now carries a typed
  `LiveObjectUpdateItemCursorSourceOwner` for unowned neighbors, handoff
  summaries, and source-window neighboring cursor fits. Bridge traces and
  failure artifacts emit the verdict as `source_owner`, distinguishing a real
  `contiguous-tail` handoff from `unowned-emitted-source-gap`. Packet bytes and
  cursor ownership remain unchanged; the active raw no-map `U/6` regression
  pins the `+2` scalar-shaped fit as unowned in both emitted and source
  coordinates. Next production path remains compact Diamond/source capture or
  source-side writer/list-handoff proof before any item cursor ownership change
  can key off a contiguous-tail verdict.
- 2026-06-11 follow-up `P/05/01` item `U/6` focus-cursor ledger evidence:
  production failure evidence now records the inherited focus cursor's ledger
  verdict separately from any validating shifted neighbor. Failure artifacts
  emit `focus_cursor_ledger=...` with source owner, source/emitted gap ranges,
  implied source cursor, cumulative source/emitted delta, and previous ledger
  row. Packet bytes and cursor ownership remain unchanged; this makes the
  active compact `U/10 -> A/6 -> U/6` proof compare the source-owned true
  cursor (`contiguous-tail`) against the `+2` scalar-looking unowned neighbor
  without parsing Rust debug dumps. Next proof remains compact source capture or
  source-side writer/list-handoff evidence before assigning the two pre-`U/6`
  bits. Verified with focused failure-report, contiguous-tail, unowned-neighbor,
  and source-window tests plus `cargo fmt --all --check`, `git diff --check`,
  and `cargo check -q -p hgbridge-proxy2`.
- 2026-06-23 follow-up `P/05/01` item cursor source-owner policy: packet bytes
  and cursor ownership are unchanged. Item `U/6` failure evidence now stores the
  source-owner verdict as the single production state, and derives
  `claimable_handoff` plus `handoff_blocker` from that verdict when traces or
  artifacts are formatted. This prevents future source-owner policy from
  branching on duplicated booleans/strings while the active compact
  `U/10 -> A/6 -> U/6` proof still needs compact source capture or a
  source-side writer/list-handoff owner. Verified with focused cursor-failure
  and failure-report regressions, `cargo check -q -p hgbridge-proxy2`, and the
  full `live_object_update` filter.
- 2026-06-23 follow-up `P/05/01` item handoff source-decision owner: packet
  bytes and cursor ownership are unchanged. Item `U/6` handoff evidence now
  stores a typed source decision derived from the bounded source-window sequence
  plus source-owner verdict, so the compact `U/10 -> A/6 -> U/6` case records
  `blocked-unowned-emitted-source-gap` through one policy path rather than
  recomputing sequence/source checks in the formatter. Next implementation path
  remains proving or implementing a compact source-writer/list-handoff owner
  before assigning the two pre-`U/6` bits. Verified with focused decision,
  report, raw-neighbor handoff tests and the full `live_object_update` filter.
- 2026-06-23 follow-up `P/05/01` item handoff sequence-context evidence:
  packet bytes and cursor ownership are unchanged. Item `U/6` failure evidence
  now carries typed bounded-row context for the compact handoff sequence:
  optional carrier `U/10`, previous typed `A/6`, and failed focus `U/6`,
  including masks, object ids, bit ranges, claim family, and source-bit
  previews. Failure artifacts can now compare the source-capture/decompile
  writer target without reconstructing those rows from generic source-window
  text. The active compact no-map case still records
  `blocked-unowned-emitted-source-gap`; the two pre-`U/6` bits remain unowned
  until compact source-writer/list-handoff proof assigns them. Verified with
  isolated-target focused no-map, `item_handoff`, `cargo check -q -p
  hgbridge-proxy2`, formatter/diff checks, and serial `live_object_update`
  (`580 passed`).
- 2026-06-23 follow-up `P/05/01` compact item handoff source-contract gate:
  packet bytes and cursor ownership are unchanged. Item `U/6` failure evidence
  now derives source decisions from a typed handoff source contract, and the
  compact `U/10 tail9 -> A/6 -> U/6` contract is recognized only when the
  carrier row has the exact expected widths: eight source bits and thirteen
  EE-facing bits. A broader bounded door update before `A/6` is now
  `unclassified-source-contract` instead of a claimable compact handoff. The
  active compact no-map case still records
  `blocked-unowned-emitted-source-gap`; the two pre-`U/6` bits remain unowned
  pending compact source capture or source-writer/list-handoff proof. Verified
  with isolated-target focused source-contract, compact no-map, `item_handoff`,
  `cargo check -q -p hgbridge-proxy2`, formatter/diff checks, and serial
  `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` compact item handoff ledger-backed contract:
  packet bytes and cursor ownership are unchanged. Source-window and item
  handoff row evidence now carries the production rewrite ledger family,
  emitted-bit preview, and insert/remove counts beside the source-bit preview.
  The compact `U/10 tail9 -> A/6 -> U/6` source contract now requires the
  carrier to be backed by the actual `update-compact-tail9-rewrite` ledger
  claim, not only a byte-plausible door update with eight source bits and
  thirteen EE bits. The active no-map case still reports
  `blocked-unowned-emitted-source-gap`; the two pre-`U/6` bits remain unowned
  pending compact source capture or source-writer/list-handoff proof. Verified
  with isolated-target focused `item_handoff`, compact no-map, `cargo check -q
  -p hgbridge-proxy2`, formatter/diff checks, and serial
  `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` compact item handoff residue evidence:
  packet bytes and cursor ownership are unchanged. Item handoff failure
  evidence now carries a typed sequence-residue summary for the bounded
  `U/10 tail9 -> A/6 -> U/6` prefix: compact tail9 plus no-map `A/6` owns
  thirteen source bits, emits nineteen EE-facing bits, and fully explains the
  +6 prefix delta before the failed `U/6`. The only validating shifted cursor
  is now classified as a two-bit `focus-row-prefix` gap inside the `U/6`, not
  an inter-row donation from the compact prefix. The active no-map case remains
  `blocked-unowned-emitted-source-gap`; next proof still needs compact
  source-capture or source-writer/list-handoff ownership for those first two
  item-update bits. Verified with isolated-target focused no-map, report, and
  source-contract tests, `cargo check -q -p hgbridge-proxy2`, formatter/diff
  checks, and serial `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff rewrite-backed `A/6` contract:
  packet bytes and cursor ownership are unchanged. The direct `A/6 -> U/6`
  source contract and the compact `U/10 tail9 -> A/6 -> U/6` source contract
  now both require the preceding item add/create row to have the production
  `item-create-rewrite` or `item-add-rewrite` ledger claim with matching
  emitted/source bit widths; a byte-shaped `A/6` row is still useful sequence
  context but is no longer a bounded source contract by itself. The active
  no-map case still reports `blocked-unowned-emitted-source-gap`; the two
  pre-`U/6` bits remain unowned pending compact source capture or
  source-writer/list-handoff proof. Verified with focused source-contract,
  compact no-map, `item_handoff`, `cargo check -q -p hgbridge-proxy2`,
  formatter/diff checks, and serial `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff row-role contract:
  packet bytes and cursor ownership are unchanged. Handoff sequence rows now
  expose a typed role derived from opcode/marker, exact bit widths, source
  preview width, and rewrite-ledger provenance. The source-contract gate now
  consumes those roles so a broad door predecessor remains sequence context,
  while only compact tail9 and rewrite-backed `A/6` rows become compact/direct
  source-contract proof. Failure reports include the role beside each retained
  sequence row. The active no-map case still reports
  `blocked-unowned-emitted-source-gap`; the two pre-`U/6` bits still require
  compact source capture or source-writer/list-handoff proof. Verified with
  isolated-target focused source-contract, `item_handoff`,
  `cargo check -q -p hgbridge-proxy2`, formatter/diff checks, and serial
  `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff focus-cursor contract:
  packet bytes and cursor ownership are unchanged. Item handoff evidence now
  carries the inherited focus `U/6` cursor ledger verdict inside the typed
  handoff object and emits it as `item_handoff_focus_cursor`, so artifacts show
  the real focus cursor as `contiguous-tail` separately from the validating
  `+2` neighbor blocked by `unowned-emitted-source-gap`. The compact no-map
  regression asserts the focus source cursor lands before the two disputed
  item-update lead bits. Next proof still needs compact source capture or
  source-writer/list-handoff ownership before assigning those bits. Verified
  with isolated-target focused report/no-map/`item_handoff`, `cargo check -q
  -p hgbridge-proxy2`, formatter/diff checks, and serial `live_object_update`
  (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff focus-prefix replay: packet bytes
  and cursor ownership are unchanged. Item handoff evidence now retains a typed
  replay of the decompile-owned `U/6` field prefix skipped by a validating
  shifted neighbor. The compact no-map `+2` case records the disputed bits as
  `position-residuals` at the focus cursor before the scalar-shaped neighbor,
  so the next compact source capture can compare source bits to field ownership
  without parsing formatter text. The two pre-`U/6` bits remain unowned pending
  compact source capture or source-writer/list-handoff proof. Verified with
  isolated-target focused report, neighbor, `item_handoff`, `cargo check -q -p
  hgbridge-proxy2`, formatter/diff checks, and serial `live_object_update`
  (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff focus-prefix blocker: packet
  bytes and cursor ownership are unchanged. The typed source decision now uses
  retained sequence-residue evidence, so a bounded compact/direct handoff whose
  only validating neighbor skips decompile-owned focus-row prefix bits reports
  `blocked-focus-row-prefix` instead of the generic unowned emitted/source gap.
  The raw source-owner verdict is still emitted separately, and the compact
  no-map `U/10 tail9 -> A/6 -> U/6` case remains unclaimable until compact
  source capture or source-writer/list-handoff proof assigns those first item
  update bits. Verified with isolated-target focused `item_handoff`, focused
  compact no-map, `cargo check -q -p hgbridge-proxy2`, formatter/diff checks,
  and serial `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff focus-prefix ownership proof:
  packet bytes and cursor ownership are unchanged. Sequence-residue evidence
  now carries the typed focus gap origin plus source-owner verdict, and
  `blocked-focus-row-prefix` is emitted only when the skipped prefix is one of
  the decompile-owned `U/6` focus fields proven by the Diamond/EE item update
  readers. Failure artifacts print the residue `gap_origin` and `source_owner`
  beside the pre-focus source/emitted bit totals, so the next compact source
  capture can compare the disputed first item-update bits without treating an
  unclassified inside-row skip as proof. The compact no-map handoff remains
  unclaimable pending source capture or source-writer/list-handoff ownership.
  Verified with focused source-contract, compact no-map, `item_handoff`,
  `cargo fmt --all --check`, `git diff --check`, `cargo check -q -p
  hgbridge-proxy2`, and serial `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff failure-kind narrowing: packet
  bytes and cursor ownership are unchanged. Runtime item `U/6` cursor failures
  now classify the compact `U/10 tail9 -> A/6 -> U/6` shifted-neighbor case as
  `item-update-cursor-failed-before-valid-neighbor-focus-row-prefix` when the
  shared typed handoff decision is `blocked-focus-row-prefix`; generic
  unowned-neighbor failures remain separate. Debug live-claim output now prints
  the typed handoff source decision beside the raw source-owner verdict. This
  keeps the active no-map handoff unclaimable while making the blocker visible
  to dispatch/quarantine handling and the next compact source capture. Verified
  with focused compact no-map and dispatcher tests, `cargo fmt --all --check`,
  `git diff --check`, `cargo check -q -p hgbridge-proxy2`, and serial
  `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff prefix-source replay evidence:
  packet bytes and cursor ownership are unchanged. Item handoff failure evidence
  now carries a typed `prefix_source_replay` verdict that flattens the retained
  decompile-owned focus-prefix stages in reader order and compares them with the
  source-coordinate gap bits. The active compact no-map
  `U/10 tail9 -> A/6 -> U/6` case records
  `source-gap-matches-focus-prefix` for the two skipped position residual bits,
  but remains unclaimable as `blocked-focus-row-prefix` until compact source
  capture or source-writer/list-handoff proof assigns those bits. Verified with
  focused compact no-map, report, `item_handoff`, `cargo fmt --all --check`,
  `git diff --check`, `cargo check -q -p hgbridge-proxy2`, and serial
  `live_object_update` (`581 passed`).
- 2026-06-23 follow-up `P/05/01` item handoff source-span contract: packet
  bytes and cursor ownership are unchanged. Source-window and handoff sequence
  rows now carry exact `source_bit_start/source_bit_end/source_bit_delta`
  fields separately from retained source-bit previews, and the compact tail9 /
  rewrite-backed `A/6` source contract consumes those exact spans instead of
  overloading preview length as ownership proof. Failure reports print
  `source_span` and `source_delta`, so the next compact source capture can
  compare the `U/10 -> A/6 -> U/6` source handoff without reconstructing spans
  from truncated previews. The active no-map handoff remains
  `blocked-focus-row-prefix` pending compact source capture or
  source-writer/list-handoff proof. Verified with local-target focused
  compact no-map and `item_handoff` tests, `cargo check -q -p
  hgbridge-proxy2`, formatter, and diff-check.
- 2026-06-23 follow-up `P/05/01` item handoff structured source capture:
  packet bytes and cursor ownership are unchanged. Rewrite-failure diagnostics
  now emit a deterministic `.handoff.tsv` artifact beside the existing payload
  and text probe dumps when item `U/6` handoff evidence is present. The artifact
  serializes the typed handoff decision, source contract, source-window rows,
  exact source/emitted spans, contiguous-tail entries, decompile-owned focus
  prefix stages, and nearby validating cursor evidence without parsing freeform
  report text. This is implementation-enabling capture plumbing only; the
  compact no-map `U/10 tail9 -> A/6 -> U/6` handoff remains
  `blocked-focus-row-prefix` until compact source capture or
  source-writer/list-handoff proof assigns the two pre-`U/6` bits. Verified
  with focused report, `item_handoff`, compact no-map tests,
  `cargo fmt --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`.
- 2026-06-23 follow-up `P/05/01` item handoff prefix-stage replay:
  packet bytes and cursor ownership are unchanged. Item handoff diagnostics now
  derive per-stage source replay rows from the existing decompile-owned focus
  prefix evidence, so the `.handoff.tsv` and text report say that the active
  compact no-map two-bit gap replays the `U/6` position-residual prefix, not
  just an untyped bit string. The handoff still remains
  `blocked-focus-row-prefix`; next production proof remains compact source
  capture or source-writer/list-handoff ownership before assigning those bits.
  Verified with focused report/capture and compact no-map tests, `cargo check
  -q -p hgbridge-proxy2`, formatter, and diff-check.
- 2026-06-23 follow-up `P/05/01` item handoff boundary audit:
  packet bytes and cursor ownership are unchanged. Item handoff failure
  evidence now derives a typed source/emitted boundary audit from the retained
  sequence residue, and both text reports and `.handoff.tsv` captures expose
  whether the validating neighbor is at a true row boundary, inside the
  previous row, between rows, or inside the decompile-owned focus-row prefix.
  The active compact no-map `U/10 tail9 -> A/6 -> U/6` case reports
  `focus-row-prefix` with source and emitted relations
  `inside-focus-row-prefix`, so the next compact source capture can compare
  the boundary mechanically instead of reconstructing it from freeform text.
  The two pre-`U/6` bits remain unowned pending compact source capture or
  source-writer/list-handoff proof. Verified with focused report/capture,
  compact no-map, and `item_handoff` tests plus `cargo check -q -p
  hgbridge-proxy2`.
- 2026-06-24 follow-up `P/05/01` compact item handoff carrier contiguity:
  packet bytes and cursor ownership are unchanged. The typed compact
  `U/10 tail9 -> A/6 -> U/6` source contract now requires the carrier row to
  end exactly where the item add begins in both emitted and source bit spans;
  an otherwise exact compact carrier with an unowned emitted or source gap
  before `A/6` is now `unclassified-source-contract`. This keeps future compact
  source captures from treating a gapped carrier as proof for the two pre-`U/6`
  bits. Verified with focused `item_handoff` source-contract regressions,
  formatter, diff-check, and `cargo check -q -p hgbridge-proxy2`.
- 2026-06-24 follow-up `P/05/01` item handoff source-prefix audit: packet
  bytes and cursor ownership are unchanged. Item handoff evidence now derives a
  typed source-prefix audit from the retained focus row source span and
  decompile-owned focus-prefix stages. Text reports and `.handoff.tsv` captures
  now state whether the source-coordinate gap exactly matches, sits inside, or
  crosses the focus `U/6` prefix source span; the active compact no-map two-bit
  shape should report `matches-focus-prefix-span`, which is still blocker
  evidence rather than source-writer ownership proof. Next proof remains compact
  source capture or Diamond/HG/EE source-writer/list-handoff evidence before
  assigning the two pre-`U/6` bits.
- 2026-06-09 `P/05/01` stock snapshot mask-owner proof: no packet behavior
  changed. Re-ran a direct PE scan of `NWN Diamond/nwserver.exe` to keep the
  compact-tail source-writer boundary reproducible without trusting the text
  decompile alone. Little-endian `F7 FF FF FF` appears only at executable VA
  `0x44036E` (the immediate in `mov dword ptr [eax], 0xFFFFFFF7` at
  `0x44036C`) and `.rdata` VA `0x633594`. The `0x4401F0` add/snapshot path
  passes that side mask to `0x44AC70`, whose checked range has no direct calls
  to the CNW byte/word/dword/bool/string/float writers, then returns to
  `0x4401F0` to emit `A`, object type, and object id at `0x4403E3`,
  `0x4403F0`, and `0x4403FA`. The typed `U` serializer remains `0x445160`,
  reached only at direct call sites `0x43F7EC`, `0x444F23`, and `0x4450AC` in
  the checked stock binary, and it writes `U/type/id/mask` at
  `0x4451DC..0x44520D`. This further rules out the stock `0x4401F0`
  `0xFFFFFFF7` mask seed and `0x44AC70` snapshot copier as owners for the two
  active pre-`U/6` bits. The compact `U/10 tail9` source family is still
  local/HG capture evidence; continue with compact-source capture or another
  decompile/server-binary writer/handoff proof before assigning those bits.
- 2026-06-09 `P/05/01` HGX `0xFFFFFFF7` literal audit: no packet behavior
  changed. Rechecked all five literal hits in
  `C:\NWN\HGX Source\hgx.server decompile\hgx.server decompile.txt`:
  `0x1001B119`, `0x1001B541`, `0x1001C216`, `0x100325B7`, and
  `0x10040627`. They are mask normalization or string cleanup/destructor
  neighborhoods, with no nearby `CNWMessage` byte/word/dword/bool/string/float
  writer calls and no `U/type/id/mask` emission. This rules out the HGX
  `0xFFFFFFF7` text-decompile hits as compact `U/10 tail9` source-writer proof.
  The two active pre-`U/6` bits remain unowned; next useful evidence is still a
  compact source capture or a different decompile/server-binary handoff proof
  before the `U/10`/`A/6`/`U/6` boundary.
- 2026-06-09 `P/05/01` Diamond client row-dispatch audit: no packet behavior
  changed. Rechecked the client decompile around `sub_44EF00 -> sub_455720`.
  `sub_455720` reads the live-object row opcode with `sub_4FB4D0(8)`, whose
  byte path advances only the read-buffer cursor by one byte for width `>= 8`;
  the following `sub_4FBB40` call is a cursor/overflow status check, not a
  fragment BOOL reader. The `D` branch then calls `sub_44AC70` directly, while
  sibling branches read their object-id/list payloads inside the row-specific
  handler. This rules out a Diamond client-side generic inter-row BOOL between
  `A`/`U`/`D` live-object rows as an explanation for the two active pre-`U/6`
  bits. It is only reader-side boundary evidence; the unresolved CEP v2.3 item
  handoff still needs compact source capture or server writer/handoff proof
  before assigning the bits before `U/10`/`A/6`/`U/6`.
- 2026-06-09 `P/05/01` alternate server update-list walker audit: no packet
  behavior changed. Direct `nwserver.exe` disassembly of the handoff selected
  at `0x43FF38..0x43FF5A` shows both walker branches are typed-row plumbing,
  not fragment-bit owners. The `0x444E60` path and the already-inspected
  `0x445010` path both call the mask builder `0x4447D0`, conditionally call the
  typed `U` serializer `0x445160` (`0x444F23` or `0x4450AC`) when the computed
  mask is nonzero, then call `0x444C70` to copy the selected snapshot fields.
  No direct CNW byte/word/dword/bool/string/float writer calls were found in
  the walker handoff or snapshot-copy path; fragment BOOL ownership remains
  inside the typed serializer calls. This rules out the alternate server
  update-list walker as a generic owner for the two active pre-`U/6` bits. The
  unresolved CEP v2.3 handoff still needs compact source capture or a different
  decompile/server-binary writer proof before changing cursor ownership.
- 2026-06-09 `P/05/01` server writer finalization/handoff audit: no packet
  behavior changed. Direct `nwserver.exe` disassembly of the same outer writer
  keeps the row-family cursor continuous: `0x43FD30` calls
  `CNWMessage::CreateWriteMessage` once at `0x43FDB3`, writes the live-object
  helper rows and update-list rows, then calls the finalizer/packet handoff
  `0x508B80` once at `0x4400B9` after the last row helper. The repeated
  `0x508B70` calls between row-family helpers are not fragment finalization or
  cursor reset points; `0x508B70` is a pure length expression
  (`[message+0x18] + [message+0x0C] + 1`) and returns immediately. This rules
  out a stock Diamond server row-family boundary reset/handoff as the two active
  pre-`U/6` bits in the compact `U/10 tail9` -> no-map `A/6` -> full `U/6`
  path. The remaining proof target is still the compact source writer/capture or
  another source-side owner before the `U/10` record; do not add a cursor skip,
  scalar-byte rescue, or generic inter-row padding trim.
- 2026-06-08 `P/05/01` `0xFFFFFFF7` binary-hit audit: no packet behavior
  changed. A direct byte/Capstone scan of
  `C:\NWN\NWN Diamond\nwserver.exe` found exactly one executable little-endian
  `F7 FF FF FF` hit plus the `.rdata` mask table entry: `.text` VA
  `0x44036C` and `.rdata` VA `0x633594`. The `.text` hit is inside server
  add/snapshot writer `0x4401F0`: type table byte `0x6338B1 = 0x0A` selects the
  `0xFFFFFFF7` side mask, then the function passes that mask to `0x44AC70` and
  writes an add row (`A` at `0x4403E3`, type at `0x4403F0`, object id at
  `0x4403FA`). The typed update row remains the `0x445160` path, which writes
  `U/type/id/mask` at `0x4451DC..0x44520D` and uses the stock orientation BOOL
  cursor for mask `0x0002`. Therefore the executable `0xFFFFFFF7` hit is not a
  compact `U/10` source-writer proof and still cannot assign the two active
  pre-`U/6` bits.
- 2026-06-07 `P/05/01` CEP raw zlib-stream replay audit: no packet behavior
  changed. Replayed the archived raw Diamond server send stream from
  `C:\nwnbridge\local-diamond-bridge-20260523-190505\diamond-packets` with the
  proxy2 M-frame rules: combine packetized seq1 frames
  `000022..000027`, feed the coalesced deflated seq7 span in
  `000030_sendto_socket740_len408.bin` (inflated 539, SHA-256
  `DB940505C5600FA7A852B6D892F7F08BC0B6DBA4F9DBA3BBF5CEF3834E638B67`), then
  continue the persistent raw server-deflate stream through seq10/12/16. The
  first seq17 raw server datagram
  `000042_sendto_socket740_len226.bin` inflates to the checked-in CEP fixture
  exactly: len 411, declared 393, SHA-256
  `5B8475DA3E00E0C653F0D60BDBF0FB10A6CD13DFFD5CF907413DD61FFB0632CF`, prefix
  `50 05 01 89 01 00 00 41 0A 04 00 00 80 ...`. This corrects the narrower
  seq17-only provenance note above: the raw M datagram alone is not
  independently inflatable, but the full raw Diamond server stream proves the
  active CEP bytes are original source output rather than a proxy chunk,
  declared-offset, or post-transform artifact. It still does not identify the
  compact `U/10 tail9` writer or assign the two active pre-`U/6` bits; continue
  with server-binary/decompile proof for that source family before changing the
  cursor owner.
- 2026-06-01 `P/05/01` private exact-adapter fixture reclassification: no
  packet behavior changed, but the two stale positive private expectations
  from the live-object sweep now stay unclaimed under the bit-order standard.
  The Chapter1 seq20 stream reaches a compact `A/09` whose byte shape is
  plausible but whose decompile-owned add cursor has no valid source bits at
  the current fragment position. The pre-clean-fragment rebuilt XP2 seq19
  fixture rewrites many door/placeable rows but ultimately reaches terminal
  `GI` live-GUI rows that the focused GUI reader still cannot prove. Both
  fixtures now assert that the exact adapter rolls back without emitting
  partial rewrites. Public
  fixture-free coverage now pins the live-GUI side of the rule: a missing
  inner item-create opcode row may expose plausible no-name or token-name byte
  endpoints, but the nested item body still owns at least four source BOOLs
  (seven for the token-name branch) before EE's inserted active-property BOOL.
  If the inherited cursor cannot prove those bits, the bridge must not promote
  nearby bytes or choose a neighboring cursor. Keep these as generalized
  compact-add and live-GUI cursor-handoff regressions; the fresh 2026-06-03 XP2
  replay after clean-fragment assembly no longer reproduces the old terminal
  `GI` owner search. Verified with focused
  `cargo test -q -p hgbridge-proxy2
  local_chapter1_seq20_transition_placeable_stream_stays_unclaimed_after_add_cursor_audit
  -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2
  local_xp2_seq19_door_placeable_gui_stream_stays_unclaimed_after_gui_cursor_audit
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 live_gui_ --
  --nocapture`, and the serial `live_object_update` suite.
- 2026-06-01 `P/05/01` compact `A/09` zero-source fallback audit: tightened the
  update-family compact placeable-add fallback so it cannot expand an isolated
  short-name/token `A/09` when the current CNW cursor has zero source bits.
  Diamond `sub_44E4A0` owns four compact tail BOOLs for that shape; a byte-only
  compact add with an empty bitstream is therefore shifted-cursor evidence
  unless an earlier update rewrite in the same pass proves those source bits
  were already consumed before the add row. Public coverage now keeps the
  isolated zero-source compact add unclaimed/unchanged, while the Chapter1 seq20
  private reclassification still rolls back without partial rewrites.
  Verified with `cargo test -q -p hgbridge-proxy2 compact_placeable_token --
  --nocapture`,
  `RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q -p
  hgbridge-proxy2
  local_chapter1_seq20_transition_placeable_stream_stays_unclaimed_after_add_cursor_audit
  -- --nocapture`, and the XP2 terminal-GI private check.
- 2026-06-01 `P/05/01` live-GUI terminal item fragment storage audit:
  tightened `G I/i A` / `G R/r A` promotion so already EE-shaped GUI
  item-create rows can recover stranded CNW item-name/active-property bits
  only by first finding the latest exact EE item endpoint before the storage
  span. This prevents a shorter legacy byte endpoint from swallowing EE item
  body bytes as promoted fragment storage while preserving the older
  Diamond-to-EE item-extra rewrite path when no exact EE endpoint exists.
  Public coverage now proves a terminal, already EE-shaped GUI inventory item
  promotes only the two storage bytes / six owned bits and exact-claims after
  rewrite. The old rebuilt XP2 seq19 fixture still rolls back unclaimed under
  the stricter path, but the fresh 2026-06-03 XP2 replay after clean-fragment
  assembly no longer emits the terminal `GI` owner search. Verified
  with `cargo test -q -p hgbridge-proxy2 live_gui_ -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 compact_placeable_token -- --nocapture`,
  `RUSTFLAGS='--cfg hgbridge_private_fixtures' cargo test -q -p
  hgbridge-proxy2
  local_xp2_seq19_door_placeable_gui_stream_stays_unclaimed_after_gui_cursor_audit
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 live_object_update --
  --nocapture`, and `cargo check -q -p hgbridge-proxy2`.
- 2026-06-01 `P/05/01` live-GUI missing-add-opcode proof audit: no packet
  behavior changed. The public fixture-free suite now covers the positive side
  of the Diamond capture quirk documented for `G I/i 00`: the zero inner opcode
  may be rewritten to `A` only when the inherited item-create cursor proves the
  shared item body and active-property bits at that exact row. The filtered XP2
  seq19 debug replay reached the terminal `G I 00` row with `bit_cursor=932`
  and no remaining source bits; its apparent suffix would decode as 85 promoted
  bits, so the row remains negative regression evidence. The fresh 2026-06-03
  XP2 replay after clean-fragment assembly no longer reproduces that terminal
  owner search, so do not add a GUI cursor search/skip for this case. Verified
  `CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge cargo test -p
  hgbridge-proxy2
  live_gui_missing_inventory_add_opcode_rewrites_only_with_item_bit_proof --
  --nocapture` and the filtered private XP2 debug replay.
- 2026-06-01 `P/05/01` live-GUI missing-add-opcode fragment-span guard:
  tightened the GUI item fragment-span promoter so `G I/i 00` and `G R/r 00`
  rows cannot use following item-body bytes as invented fragment storage. Missing
  inner add opcodes are still repairable only when the inherited item-create
  cursor proves the shared item body and active-property bits at that exact row.
  The low-level update rewriter now also aborts the whole staged rewrite when
  any later row makes the bit cursor unreliable, preventing a valid earlier
  typed update insertion from leaking into a stream whose terminal GUI item bits
  are unproven. Public fixture-free coverage pins the XP2-style sequence of a
  valid door-state rewrite, fragment-neutral `W current total`, and terminal
  `G I 00` with no item bits as rollback-only. The older local Diamond
  auto-inventory `U/5 0x4408` + GUI rows fixture has been reclassified from a
  positive rewrite to active shifted-cursor evidence: the first `G I 00` row has
  inherited proof, but later missing-inner-opcode rows still need a real fragment
  owner. Verified with `cargo test -q -p hgbridge-proxy2
  exact_adapter_rolls_back_prior_update_before_terminal_gui_missing_item_bits --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  local_diamond_auto_inventory_u5_4408_gui_rows_stream_stays_unclaimed_after_gui_cursor_audit
  -- --nocapture`, and `cargo test -q -p hgbridge-proxy2 live_object_update --
  --test-threads=1`.
- ~~2026-06-04 `P/05/01` live-GUI missing-add-opcode byte-boundary audit:
  tightened the GUI boundary classifier so `G I/i 00` remains visible to the
  focused missing-inner-opcode rewrite path but no longer counts as a byte-only
  live-object submessage boundary or item-create read-end fallback. Diamond
  `sub_4589A0` and EE `sub_1407B3F30` dispatch explicit `A`/`D`/`U` inner rows;
  the captured zero inner opcode is therefore repairable only when the shared
  item-create parser proves the row's name/active-property fragment bits at the
  inherited cursor. Public coverage now proves explicit `G I A` rows still
  byte-claim, `G I 00` rows stay rewrite-only before proof, and the positive
  missing-opcode repair still succeeds with item-bit evidence. Verified with
  `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge-gui-boundary cargo test -q -p hgbridge-proxy2 live_gui_missing_inventory_add_opcode -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 live_gui_ -- --nocapture`, `cargo test -q
  -p hgbridge-proxy2 live_object_update -- --test-threads=1`, `cargo fmt
  --all --check`, `cargo check -q -p hgbridge-proxy2`, and `git diff
  --check`.~~
- ~~2026-06-04 `P/05/01` live-GUI `GQ` quickbar-link row-offset audit: no
  packet behavior changed, but public fixture-free coverage now pins the
  byte-exact read-buffer-only row shape. Diamond `sub_4589A0` and EE
  `sub_1407B3F30` read `G Q`, a count byte, then nine-byte rows with the
  OBJECTID at row offset `+2`; a row whose only plausible object id starts one
  byte earlier remains shifted-cursor evidence and is not accepted as a
  quickbar-link boundary. Verified with `cargo test -q -p hgbridge-proxy2
  live_gui_quickbar_link_row_requires_object_id_at_row_offset_two --
  --nocapture`.~~
- ~~2026-06-04 `P/05/01` live-GUI `GQ` row-field follow-up: closed the
  formerly opaque row-field proof without changing packet behavior. Diamond
  `sub_458850` reads count, then per row `ReadBYTE`, `ReadBYTE`, raw DWORD
  object id via `sub_53E690`, `ReadBYTE`, and `ReadWORD`; EE `sub_1407B4390`
  reads the same count and row order via `ReadBYTE`, `ReadBYTE`,
  `sub_1409737C0`, `ReadBYTE`, and `ReadWORD`. Both clients discard the first
  two row bytes before object lookup; the post-object byte/word are passed as
  the quickbar button/use-count fields, with no extra byte-range rejection
  before the object/button lookup path. The Rust scanner now documents and
  length-proofs that cursor shape, and public coverage proves extreme
  auxiliary bytes do not gate the boundary or move the object-id offset.
  Verified with `cargo test -q -p hgbridge-proxy2
  live_gui_quickbar_link_ -- --nocapture`.~~
- 2026-05-29 `P/05/01` U/9-W handoff audit: no packet behavior changed, but
  public fixture-free coverage now pins the negative `W` proof behind the
  remaining CEP v2.3 starter evidence. Diamond `sub_44F160` and EE
  `sub_1407B85A0` both read only `W current total` and consume no CNW fragment
  BOOLs, so a following `W` row cannot donate missing position/orientation/state
  bits to a preceding door/placeable `U/9`/`U/10` update. The exact live-object
  adapter is also covered for rollback: earlier staged typed rewrites must be
  discarded when the final U/9-W cursor owner is still unproven. The CEP v2.3
  starter capture remains active pending a real owner for the final placeable
  update bits or a separate stream-boundary explanation.
- 2026-05-29 `P/05/01` door/placeable `0x37` appearance/scale-state cursor
  audit: hardened the exact EE validator against a same-length swapped row.
  Diamond `sub_467AE0` reads mask `0x20` appearance at loc_467C29 before mask
  `0x4` scale/state at loc_467C6B; EE `sub_14079C050` preserves that order at
  loc_14079C690 before loc_14079CB44. Public fixture-free coverage now proves
  an EE-ordered row exact-claims and a scale/state-before-appearance row stays
  unclaimed/unchanged even though it has the same byte length. This narrows the
  CEP v2.3 terminal `U/9` evidence: the observed final row still has only two
  fragment bits before `W` and its byte tail resembles the legacy scale-first
  order, so it remains active until a real owner or stream-boundary explanation
  is proven.
- 2026-05-29 private fixture reclassification: the older local Diamond seq12
  "rewritten/claimed" door-placeable streams and the XP2 seq19 mixed
  door/placeable + GUI stream now stay unclaimed under the same `0x37` audit.
  They contain or depend on the scale-before-appearance shape and are retained
  as generalized shifted-cursor evidence, not positive EE fixtures. The
  Chapter1 transition-pending claimed stream was also reclassified, but as the
  sibling mask-`0x17` stale absent-appearance gap before scale, not as a `0x37`
  row. The older M-frame local Prelude/Contest/Winds pending positives now
  stay quarantined for the related terminal `U/9 mask=0x37` fragment shortage:
  after the mask-`0x17` stale gap repairs, the final row still lacks the
  decompile-required scalar/state fragment bits before the following `W`/stream
  boundary. Next step: trace the original Diamond writer/server boundary for
  those rows, then either prove a bounded pre-EE bridge artifact repair or keep
  them quarantined until a real following-record owner is found.
- 2026-05-29 server-dispatch accepted-dump reclassification: dispatcher tests
  now allow a mismatch only when the typed rewrite still exact-claims and the
  old harness-dumped comparison payload is a known stale full-appearance or
  stale scale-before-appearance `U/9`/`U/10 mask=0x37` dump. This keeps
  dispatcher ownership proven without letting old semantically shifted EE-byte
  dumps become positive fixtures. Verified with `cargo test -q -p
  hgbridge-proxy2 server_dispatch -- --nocapture`.
- 2026-05-29 `P/05/01` door/placeable mask-`0x17` stale absent-appearance
  terminal-bit audit: hardened the byte-gap repair so removing the two stale
  bytes at the absent mask-`0x20` appearance cursor no longer grants terminal
  fragment-trim ownership. Diamond `sub_467AE0` and EE `sub_14079C050` still
  require scale/state immediately after the scalar orientation byte when
  mask `0x20` is absent; the repair may exact-claim only when the source owns
  the position, scalar-orientation, five state, and EE-neutral sixth state bits.
  An extra terminal fragment bit now rejects and leaves the evidence payload
  unchanged. This closes the stale-gap-specific trim leak but does not resolve
  the remaining terminal `U/9 mask=0x37` fragment shortage before `W`/stream
  boundary. Verified with `cargo test -q -p hgbridge-proxy2
  stale_absent_appearance_gap_repair_rejects_terminal_extra_fragment_bit --
  --nocapture`.
- 2026-05-29 `P/05/01` `U/9`/`U/10` mask-`0x37` before `W` audit: no packet
  behavior changed, but public fixture-free coverage now pins the suffix rule.
  `W current total` remains a fragment-neutral suffix only; it cannot rescue a
  same-length scale/state-before-appearance row, and it cannot supply missing
  orientation/state BOOLs for an otherwise EE-ordered row. The final CEP v2.3
  starter-style `U/9` shortage before `W` therefore stays quarantined until a
  real update-family owner or stream-boundary artifact is proven.
- 2026-05-30 `P/05/01` trigger `U/7` transport-boundary audit: wired the
  decompile-backed trigger update cursor into the shared live-object transport
  scanner. Diamond/HG `U/7 0xFFFF_FFF3` owns the generic position read fields,
  exactly two position fragment bits, and a bounded three-byte trigger tail that
  is dropped before EE emission; exact EE `U/7 0x00000001` owns the same
  position bytes/bits without that legacy tail. A stale declared read window may
  no longer split before trigger tail bytes even when those bytes decode as
  compact CNW fragment storage. Verified with `cargo test -q -p
  hgbridge-proxy2 trigger_update_tail_bytes_stay_inside_transport_record --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 trigger_update --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 declared_length --
  --nocapture`.
- 2026-05-30 `P/05/01` `U/9` before fragment-neutral `W` re-audit: split the
  no-fragment low-tail rule from the valid scalar/state `0x37` cursor. Public
  fixture-free coverage now proves a `U/9` low-tail row followed only by `W
  current total` remains unclaimed when the Diamond source has no fragment bits:
  `W` consumes zero BOOLs in Diamond `sub_44F160` and EE `sub_1407B85A0` and
  cannot donate missing position/orientation/state bits. The Winds seq16
  pending fixture is reclassified as positive evidence because its final `U/9`
  rows do have the 12 Diamond source bits; the typed update rewrite inserts
  only EE's neutral sixth placeable-state BOOL and leaves `W` as a
  fragment-neutral identity row. The Contest/Prelude siblings still remain
  active negative evidence. Verified with `cargo test -q -p hgbridge-proxy2
  no_fragment_low_bits_placeable_update -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 after_37_fragment_audit -- --nocapture`, and `cargo test -q
  -p hgbridge-proxy2 -- --test-threads=1` (797 passed).
- 2026-05-31 `P/05/01` vector door/placeable update before `W` audit: no
  packet behavior changed, but public fixture-free coverage now pins the
  vector-orientation sibling of the `U/9`/`U/10 mask=0x37` handoff rule.
  Diamond `sub_467AE0` and EE `sub_14079C050` both read the generic
  orientation selector before choosing one scalar byte or six vector bytes; a
  following `W current total` still owns only its three read-buffer bytes and
  zero CNW BOOLs. Exact vector updates followed by `W` claim only when the
  update owns its position selector bits, vector selector, five state bits, and
  EE-neutral sixth state bit; bit-short vector rows stay unclaimed and
  unchanged for quarantine. Verified with `cargo test -q -p hgbridge-proxy2
  work_remaining_ -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  door_placeable_update -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1` (334 passed).
- 2026-05-31 `P/05/01` terminal `U/9`/`U/10` before `W` trim audit: fixed
  Rust proxy2 so `W current total` remains fragment-neutral even when it is the
  only row after a rewritten door/placeable update. Diamond `sub_44F160` and
  EE `sub_1407B85A0` consume exactly the three read-buffer counter bytes and
  zero CNW BOOLs; terminal family trim may cross a final `W` only when the
  preceding typed rewrite has already removed or changed decompile-owned source
  bits and the trimmed candidate exact-claims. Pure insertion-only `U/9`/`U/10`
  rows before `W` now reject and preserve an unowned extra fragment bit instead
  of letting `W` donate ownership. The separate post-`W` storage path is still
  limited to the proven bounded CNW storage-byte promotion case. Verified with
  `cargo test -q -p hgbridge-proxy2
  work_remaining_suffix_does_not_let_low_tail_update_trim_extra_fragment_bit
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 work_remaining --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 low_tail -- --nocapture`,
  and `cargo test -q -p hgbridge-proxy2 live_object_update -- --test-threads=1`
  (341 passed).
- 2026-05-30 `P/05/01` declared-length `W` tail boundary audit: hardened
  stale-declared split classification so a candidate whose CNW tail starts with
  aligned `W current total` is treated as an ambiguous live-object read boundary,
  even though the three bytes can masquerade as compact CNW fragment storage.
  Diamond `sub_44F160` and EE `sub_1407B85A0` consume `W` as exactly three
  read-buffer bytes with no fragment bits. `W` remains a suffix only after a
  family-owned read window; it is not a fragment-tail start and still cannot
  repair the CEP `U/9 mask=0x37` shortage before `W`. Verified with
  `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_w_current_total_as_fragment_tail --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 work_remaining --
  --nocapture`.
- 2026-05-30 `P/05/01` `W current total` counter-shape audit: removed the
  observed `total == 0x0E` boundary assumption from Rust proxy2's live-object
  transport scanners and fragment-span promoters. The decompiled reader/writer
  contract is exactly opcode `W` plus two read-buffer BYTE counters; neither
  counter byte is a fragment guard. The stale-declared capacity walk now also
  accepts `W` only as an exact three-byte record, so trailing bytes after `W`
  still require the dedicated bounded fragment-storage proof. Verified with the
  public fixture-free `work_remaining_record_accepts_general_counter_bytes`,
  `work_remaining_boundary_uses_three_byte_counter_shape`, and updated
  declared-length `W` tail regressions, `cargo test -q -p hgbridge-proxy2
  work_remaining -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  declared_length -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  live_object_update -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  live_object -- --nocapture`, `cargo fmt --all --check`, `git diff --check`,
  and `cargo check -q -p hgbridge-proxy2`.
- ~~2026-05-31 `P/05/01` `W current total` fragment-span top-level-boundary
  audit: hardened the pre-loop post-`W` fragment-storage promoter so it may use
  only a `W current total` reached by a top-level live-object boundary walk.
  Diamond `sub_44F160` and EE `sub_1407B85A0` still read `W` as exactly three
  read-buffer bytes with no CNW BOOLs, while Diamond `sub_4589A0` / EE
  `sub_1407B3F30` read `G I U` as a ten-byte GUI row whose OBJECTID bytes can
  legally spell `W current total`. The promoter now refuses such nested
  W-shaped bytes instead of truncating them as fragment storage. Verified with
  `cargo test -q -p hgbridge-proxy2 work_remaining_ -- --nocapture`, `cargo
  test -q -p hgbridge-proxy2 live_gui_inventory_update -- --nocapture`, `cargo
  test -q -p hgbridge-proxy2 live_object_update -- --test-threads=1`, and
  `cargo check -q -p hgbridge-proxy2`.~~
- 2026-05-30 `P/05/01` short GUI/delete declared-tail audit: generalized the
  aligned short-read-boundary guard that was added for `W current total`.
  Zero-row `GQ` and six-byte delete rows can also be shorter than the broad
  ambiguous-tail scanner while their bytes still decode as compact CNW fragment
  storage. Stale-declared repair now treats aligned short `W`, GUI read-buffer,
  and delete rows as live-object read boundaries, and the read-prefix walker can
  advance over short `GQ` rows instead of requiring an object-id-bearing GUI
  shape. Verified with `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`.
- 2026-05-30 `P/05/01` character-sheet declared-tail audit: extended the same
  stale-declared tail guard to short `G S` character-sheet rows that own CNW
  BOOLs. EE `sub_1407B2740` reads mask `0x20` as one read-buffer BYTE followed
  by one fragment BOOL; that 11-byte read row can start with bytes that decode
  as compact fragment storage, so a proposed CNW tail beginning at aligned
  `G S` must stay a live GUI read-boundary ambiguity until the normal exact
  GUI reader proves the record. Verified with `cargo test -q -p
  hgbridge-proxy2 character_sheet -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 declared_length_ -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --nocapture`.
- 2026-05-30 `P/05/01` character-sheet effect-icon declared-tail audit:
  extended the stale-declared `G S` tail guard beyond the short-row floor.
  EE `sub_1407B2740` mask `0x0100` effect-icon rows can sit exactly on the
  generic 16-byte ambiguous-tail scanner floor while still owning one changed-row
  CNW BOOL after the read-buffer body. A proposed CNW tail beginning at such an
  aligned `G S` effect-icon row is now kept as live GUI read-boundary ambiguity
  instead of fragment storage. Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_character_sheet_effect_icon_row_as_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 character_sheet --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`.
- 2026-05-30 `P/05/01` large character-sheet combat declared-tail audit:
  hardened the same proofless `G S` ambiguity guard so combat-info rows with
  many list entries cannot be missed just because they need more than 256
  placeholder fragment bits. EE `sub_1407B2740` mask `0x40` owns interleaved
  bit fields before and inside the combat lists; when such a row starts at a
  stale-declared split, the bytes remain live GUI read-boundary ambiguity until
  the exact character-sheet reader owns the real fragment cursor. Verified with
  `CARGO_TARGET_DIR=C:\nwnbridge\codex-target\nwn-ee-bridge cargo test -q -p
  hgbridge-proxy2
  declared_length_window_rejects_large_character_sheet_combat_row_as_fragment_tail
  -- --nocapture`.
- 2026-05-30 `P/05/01` combined character-sheet declared-tail audit: replaced
  the fixed proofless `G S` placeholder bit cap with a computed bound from the
  modeled EE `sub_1407B2740` branch widths: mask `0x20` BOOL, max build-8193.35
  combat false-optional lists, max changed effect-icon BOOLs, and max changed
  feat BOOLs. A stale-declared split beginning at an aligned combat+feat
  character-sheet row can require more than 8192 minimum fragment bits while
  still looking like compact CNW storage, so it now remains live GUI
  read-boundary ambiguity. Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_combined_character_sheet_combat_feat_row_as_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 character_sheet --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 live_object_update --
  --nocapture`, `cargo fmt --all --check`, `git diff --check`, `cargo check
  -q -p hgbridge-proxy2`, and the full serial `cargo test -q -p
  hgbridge-proxy2 -- --test-threads=1` suite.
- 2026-05-30 `P/05/01` short update declared-tail audit: generalized the
  stale-declared tail guard for aligned sub-16-byte `U` update rows whose
  decompiled readers own a short read-buffer header and then fragment BOOLs or
  no body. `U/6` item hidden-state, `U/9`/`U/10` door/placeable state-only, and
  `U/5` zero-mask update bytes can all decode as compact CNW fragment storage;
  the declared-length classifier now treats them as live-object read-boundary
  ambiguity until the focused update validator owns the real fragment cursor.
  Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 declared_length_ -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --nocapture`.
- 2026-05-30 `P/05/01` zero-mask appearance declared-tail audit: extended the
  same short read-boundary guard to eight-byte `P/5` creature appearance
  no-op rows. Diamond `sub_448E30` and EE `sub_14077FE10` both read `P/5`,
  OBJECTID, and WORD appearance mask before mask-gated branches; a zero mask
  consumes no CNW BOOLs, but its bytes can still decode as compact fragment
  storage. Stale-declared transport repair now leaves those aligned rows for
  the focused appearance/live-object validator instead of stealing them as a
  CNW tail. Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_creature_zero_appearance_as_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, `cargo check -q -p hgbridge-proxy2`, `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`, `cargo fmt --all
  --check`, `git diff --check`, and full serial `cargo test -q -p
  hgbridge-proxy2 -- --test-threads=1`.
- 2026-05-30 `P/05/01` short inventory/add declared-tail audit: extended the
  same stale-declared tail guard to aligned sub-16-byte live-object read
  records outside `U`. A short `I/0x0002` inventory scalar row and the 15-byte
  compact Diamond `A/09` placeable add (`OBJECTID + legacy name/token DWORD +
  BYTE/WORD/WORD tail`) can both decode as compact CNW fragment storage. They
  now remain live-object read-boundary ambiguity until the focused inventory or
  add translator proves the real cursor, rather than letting declared-length
  repair steal those bytes as a fragment tail. Verified with `cargo test -q -p
  hgbridge-proxy2 declared_length_window_rejects_ -- --nocapture`.
- 2026-05-30 `P/05/01` top-level item-add declared-tail audit: shared the
  parsed visible-equipment item-add boundary with the declared-length transport
  scanner. Top-level item adds are `A + OBJECTID + slot DWORD + item body`
  rather than `A/<object-type>`, and the item body can contain opcode-looking
  bytes; stale-declared repair must therefore use the item parser's
  decompile-owned body cursor before treating later bytes as fragment storage.
  Fixture-free coverage now proves a token-name model-type-2 item add is kept
  as a read-buffer row and that prefix walking advances over the whole item
  body before a following `W current total` record. Verified with `cargo test
  -q -p hgbridge-proxy2 top_level_item_add -- --nocapture`.
- 2026-05-30 `P/05/01` compact placeable-add token-tail audit: narrowed the
  update pass bridge for compact Diamond `A/09` token-name adds to the case
  where prior update repairs have consumed every source BOOL. Diamond
  `CNWSMessage::AddPlaceableAppearanceToMessage` can carry a four-byte
  legacy name/token slot before the compact type/appearance/static tail, but
  raw compact rows are not exact EE until the focused add writer materializes
  the empty CExoString selector, EE visual-transform map, and neutral guard
  bits. Fixture-free coverage now proves legacy cursor advancement consumes
  only the four compact source BOOLs, exact EE validation rejects the raw
  compact row, bounded empty-add residue is drained before neutral guard
  insertion, and zero-source compact token-name adds expand exactly in the
  update pass. Verified with `cargo test -q -p hgbridge-proxy2
  live_object_update -- --nocapture`.
- 2026-05-30 `P/05/01` live-object add/update cursor-starvation audit:
  kept raw compact `A/09` placeable adds out of the exact EE validator and
  instead added bounded update-pass trials that must prove both the rewritten
  add row and the immediate same-object `U/09` row exact-claim from the
  resulting cursor. Already EE-shaped empty `A/09` rows use the same
  same-object proof before draining four Diamond compact source BOOLs into
  EE's eleven neutral add guard/state BOOLs, preventing the add from borrowing
  the following update's bits. Door `A/0A` visual-map insertion is similarly
  gated by an immediate same-object `U/0A` mask-`0x17` stale
  absent-appearance update proof; shifted or bit-short mask-`0x37` evidence
  remains quarantined because that branch still carries the appearance
  cursor-order risk until the exact update reader owns it. Verified with `cargo
  test -q -p hgbridge-proxy2 live_object_update -- --nocapture`.
- 2026-05-30 `P/05/01` creature effect-only update boundary audit:
  promoted already-EE-shaped creature `U/05 mask=0x0008` rows to an explicit
  transport boundary shape before the live-object scanner looks for embedded
  `A` bytes. EE `sub_1407A4E50` reads `WORD count` plus bounded short
  status-effect rows for this mask; those row bytes must not split into
  top-level add records merely because a later effect id resembles an opcode.
  The transport helper also refuses the longer target-payload interpretation
  when a shorter legacy effect row lands exactly on the following live-object
  boundary, so a one-row `U/05` cannot swallow a following door/placeable add.
  Fixture-free coverage proves both sides, and the HG seq31 mixed
  creature/trigger/door burst plus local Chapter1 seq19 stream exact-claim
  after this boundary proof.
- 2026-05-30 `P/05/01` empty placeable-add direct-name bit audit: widened the
  already-EE-shaped `A/09` guard repair so empty direct `CExoString` names use
  the same decompile-owned direct branch repair as non-empty inline names when
  stale source bits still say `outer=true, inner=true`. EE
  `sub_1407A7800` would route that bit pair into the TLK/object-table helper,
  not the inline string bytes; the bridge now forces `outer=false`, reuses the
  former inner bit as the first post-name state BOOL, and then proves the
  optional-object guard plus EE-only visual-transform guard at their final
  cursors. Verified by fixture-free empty-name coverage and the Winds seq16
  pending stream exact-claim.
- 2026-05-30 `P/05/01` door-add/`0x37` same-object cursor audit: generalized
  the door `A/0A` visual-map insertion gate from the mask-`0x17` stale-gap
  sibling to any immediate same-object door update that exact-claims at the
  post-add fragment cursor. The following `U/0A mask=0x37` may now prove the
  add rewrite only when the decompiled `sub_467AE0` / `sub_14079C050` order is
  appearance before scale/state and all position/orientation/state BOOLs are
  present. Same-length scale-first rows and bit-short `0x37` rows still reject
  and stay visible as active shifted-cursor evidence. The XP2 seq19
  door/placeable + GUI private fixture now exact-claims under this generalized
  rule while still asserting no stale scale-first `0x37` rows survive. Verified
  with `cargo test -q -p hgbridge-proxy2 door_add_visual_map_repair --
  --nocapture`.
- 2026-05-30 `P/05/01` compact placeable-add/`0x37` same-object audit: no
  packet behavior changed, but public fixture-free coverage now proves the
  analogous compact `A/09` token-name path. The add expansion may use an
  immediate same-object `U/09 mask=0x37` only when the update exact-claims at
  the post-add cursor with the decompiled appearance-before-scale/state order
  and all position/orientation/state BOOLs present. Scale-first same-length
  rows and bit-short `0x37` rows still reject and leave the source payload
  untouched so shifted-cursor evidence remains quarantinable. Verified with
  `cargo test -q -p hgbridge-proxy2 compact_placeable_token_add --
  --nocapture`.
- 2026-06-02 `P/05/01` low-tail/compact placeable cursor audit: promoted the
  top-level `U/09`/`U/0A` low-tail transport boundary to the same bounded
  `0x40`/`0x80` control-suffix proof used by the typed row rewriter, so a
  full update no longer swallows a following compact add. The compact `A/09`
  token-name bridge may now drop two extra source-only selector bits after the
  four compact tail BOOLs only when the following same-object low-tail update
  exact-validates at the resulting cursor. Same-object proof accepts compact
  and external live-object id aliases (`0x0000NNNN`/`0x8000NNNN`) before later
  EE canonicalization. The XP2 seq19 private fixture now advances past the
  earlier offset-131 and offset-566 add/update handoffs but remains active at
  offset 953 on an all-zero compact-add source run before compact-id
  `U/09 mask=0xF7`; no GUI cursor search/skip behavior is proven. Verified
  with focused low-tail, compact-placeable-token, object-id, and private XP2
  replay tests.
- 2026-06-02 `P/05/01` compact `A/09` five-bit low-tail handoff audit: no
  packet behavior changed, but public coverage now pins the generalized XP2
  seq19 terminal shape. A compact token-name `A/09` with only five all-zero
  source bits before a same-object `U/09 mask=0xF7` remains unclaimed and
  unchanged: Diamond `sub_44E4A0` owns four compact add tail BOOLs, while the
  single remaining bit cannot prove the following update's
  position/orientation/state cursor or EE's inserted add guard run. The private
  replay still rolls back at the same upstream bit-owner problem; continue
  tracing which earlier row consumed or stranded the bits before offset 953,
  not GUI search/skip behavior.
- 2026-06-02 `P/05/01` compact `A/09` shifted low-tail bit audit: no packet
  behavior changed, but public fixture-free coverage now proves the two
  source-only compact selector bits after the four Diamond compact add tail
  BOOLs are an exact count, not a resync window. One extra unowned bit before a
  following same-object low-tail `U/09 mask=0xF7` with otherwise plausible
  update bits must roll back and leave both bytes and bits untouched. Verified
  with `cargo test -q -p hgbridge-proxy2
  compact_placeable_token_add_rejects_unowned_bit_before_low_tail_update_bits
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 compact_placeable_token
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 low_tail --
  --nocapture`, `cargo fmt --all --check`, and `cargo check -q -p
  hgbridge-proxy2`.
- 2026-06-02 `P/05/01` low-tail/add transaction rollback audit: no packet
  behavior changed, but public fixture-free coverage now pins the XP2 seq19
  rollback shape with a valid prior low-tail `U/09`, then an external-id compact
  token-name `A/09` followed by a compact-id alias `U/09 mask=0xF7` and only
  five all-zero source bits. The prior low-tail row is independently
  rewriteable, but Diamond `sub_44E4A0` still owns four compact add BOOLs and
  the following update must prove its own cursor; the bridge must roll back the
  whole candidate instead of committing the earlier repair. Verified with
  `cargo test -q -p hgbridge-proxy2
  prior_low_tail_rewrite_rolls_back_when_compact_alias_add_has_only_five_bits
  -- --nocapture`. The upstream bit-owner search before the terminal compact
  handoff remains active.
- 2026-06-02 `P/05/01` compact `A/09` missing-opcode alias-boundary audit:
  fixed the shared door/placeable missing-opcode update-body and low-tail span
  boundary guards, plus the older visual/add-map update-like splitter, so they
  use the same compact/external legacy OBJECTID equivalence
  (`0x0000NNNN`/`0x8000NNNN`) as the exact add/update verifier. Diamond and EE
  still read the object id field as a DWORD; this is only the proxy scanner's
  same-object guard before typed update parsing proves the rest of the body and
  cursor. Public regressions cover an external-id compact `A/09` followed by a
  compact-id missing-opcode `U/09` body and the same alias in the low-tail span
  scanner. Verified with focused alias tests plus `cargo test -q -p
  hgbridge-proxy2 live_object_update::boundary::tests:: -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 compact_placeable_token -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 low_tail -- --nocapture`, `cargo test -q
  -p hgbridge-proxy2 object_id -- --nocapture`, `cargo fmt --all --check`,
  `cargo check -q -p hgbridge-proxy2`, and `git diff --check`.
- 2026-06-02 `P/05/01` post-`W` interleaved storage audit: fixed the
  midstream top-level cleanup so a bounded CNW storage span after `W current
  total` is promoted into the live-object fragment cursor when the next
  top-level row is a bit-owning live-object family, instead of being discarded
  as `W` trailing bytes. Diamond `sub_44F160` and EE `sub_1407B85A0` still read
  only `W current total`; the promoted span is not `W` payload, it is
  interleaved CNW fragment storage for the following `A/U/G/I/P` row and must
  pass the exact downstream row claim. Public coverage pins a `W + storage +
  compact A/09 + same-object low-tail U/09` stream. Verified with
  `cargo test -q -p hgbridge-proxy2
  work_remaining_midstream_storage_promotes_bits_before_compact_add_update --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 work_remaining --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 compact_placeable_token --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 low_tail --
  --nocapture`, plus `cargo fmt --all --check`, `cargo check -q -p
  hgbridge-proxy2`, and `git diff --check`. The private XP2 seq19 replay
  advances from the prior offset 953 rollback to offset 1145
  (`add-record-cursor-advance-failed`).
- 2026-06-23 promoted live-object source-bit ledger audit: production rewrite
  state now inserts read-buffer fragment bits promoted by midstream `W`,
  trailing creature-add prefixes, creature-update spans, GUI item spans, and
  inventory spans into `LiveObjectRewriteBitLedger` before the following row
  commits. This preserves the Diamond/EE bit order as source-owned MSB bits
  rather than treating the promoted bytes as synthetic EE output. Verified with
  focused `work_remaining_midstream_storage_promotes_bits_before_compact_add_update`,
  creature-add prefix, GUI item, inventory interleaved-tail, and Dark Ranger
  creature-update/inventory regressions plus `work_remaining_`.
- ~~2026-06-23 translated creature appearance ledger follow-up: fixed the
  remaining CEP zero-declared `P/5 -> U/5 0x3967` source-delta failure by
  committing verified creature appearance rows to `LiveObjectRewriteBitLedger`
  and inserting appearance-adjacent promoted source bits before the following
  record commit. Verified with the private
  `local_cepv22_seq11_zero_declared_stream_rewrites_and_claims_exactly`
  regression, public ledger source-insert regression, and private
  `live_object_update` filter (`579 passed / 0 failed`).~~
- 2026-06-02 `P/05/01` compact `A/09` shifted low-tail replay audit: no packet
  behavior changed, but public coverage now pins the later raw XP2 seq19
  rollback shape. After a valid prior low-tail rewrite, the raw trace reaches
  compact `A/09` bytes `41 09 01 00 00 80 7B 74 01 00 05 00 00 00 00` at
  rewritten offset 1145, followed by same-object `U/09 mask=0xF7`. The next
  source bits are `1000_11_101101`: Diamond `sub_44E4A0` owns exactly four
  compact add BOOLs, and the known two source-only token selector bits are
  allowed only when the following update exact-proves its own cursor. Neither
  handoff proves the low-tail update's decompiled position/orientation/state
  cursor, so the whole candidate must roll back rather than committing the
  prior low-tail rewrite. Verified with `cargo test -q -p hgbridge-proxy2
  prior_low_tail_rewrite_rolls_back_when_compact_add_has_shifted_xp2_low_tail_bits
  -- --nocapture`. The separate current XP2 Chapter1 area-entry seq19 fixture
  exact-claims; keep it distinct from the older raw unclaimed door/placeable
  GUI stream. Next trace should find the real bit owner or stream-boundary
  artifact before offset 1145, not add a compact-add/low-tail cursor resync.
- 2026-06-02 `P/05/01` post-`W` promoted compact-pair rollback audit: no
  packet behavior changed, but public coverage now pins the generalized second
  XP2 seq19 storage-span hazard. A bounded post-`W` CNW storage span can feed
  earlier compact token `A/09` plus same-object low-tail `U/09` pairs and exact
  claim, but if a later pair exposes the shifted `1000_11_101101` handoff then
  the whole promoted-storage candidate must roll back unchanged. Verified with
  `cargo test -q -p hgbridge-proxy2
  work_remaining_storage_rolls_back_when_later_compact_pair_is_shifted --
  --nocapture`. This proves the storage span is not the rejected byte owner;
  the unresolved search stays with the per-record source-bit owner before the
  later compact add/update handoff.
- 2026-06-02 `P/05/01` mask-`0x17` stale-gap/compact-pair audit: no packet
  behavior changed. Public regression now proves a preceding valid `U/09
  mask=0x17` stale absent-appearance repair also cannot be the shifted
  compact-pair bit owner: it consumes only the decompiled position residuals,
  scalar-orientation selector/bits, and five Diamond state BOOLs, plus EE's
  inserted neutral state BOOL in the rewritten stream. If the following compact
  `A/09` and same-object low-tail `U/09 mask=0xF7` expose the XP2
  `1000_11_101101` bit run, the whole candidate must roll back unchanged.
  Verified with `cargo test -q -p hgbridge-proxy2
  prior_stale_gap_rewrite_rolls_back_when_compact_add_has_shifted_low_tail_bits
  -- --nocapture`. The unresolved owner search remains before the later
  compact add/update handoff, but not in the earlier stale-gap update row.
- 2026-06-02 `P/05/01` compact-add/stale-gap neighbor audit: no packet behavior
  changed. The private XP2 seq19 trace shows the immediate neighbor before the
  offset-1145 compact `A/09`/low-tail `U/09` rollback is itself a compact
  token-name `A/09` plus same-object `U/09 mask=0x17` stale-gap pair. Public
  regression now proves that complete pair can consume its own decompiled four
  add BOOLs plus the stale-gap update cursor exactly, but still must roll back
  unchanged when the following compact add exposes the shifted
  `1000_11_101101` low-tail handoff. Verified with `cargo test -q -p
  hgbridge-proxy2
  prior_compact_stale_gap_pair_rolls_back_before_shifted_compact_low_tail_bits
  -- --nocapture`. The remaining owner search moves before that preceding
  compact/stale-gap pair or to an upstream stream-boundary artifact; do not add
  compact-add/low-tail cursor resync.
- 2026-06-02 `P/05/01` repeated compact/stale-gap pair-run audit: no packet
  behavior changed. A debug replay of the raw XP2 seq19 stream with
  `HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM=1` shows the two rows before the immediate
  neighbor are the same generalized shape: compact `A/09` at rewritten offset
  1049 followed by same-object stale-gap `U/09 mask=0x17` at 1072, then compact
  `A/09` at 1097 followed by stale-gap `U/09 mask=0x17` at 1120, before the
  shifted offset-1145 compact `A/09`/low-tail `U/09 mask=0xF7` handoff. Public
  coverage now proves a run of exact compact/stale-gap pairs can claim its own
  decompiled four add BOOLs plus stale-gap update cursor, but must still roll
  back unchanged when the following compact low-tail handoff exposes
  `1000_11_101101`. Verified with `cargo test -q -p hgbridge-proxy2
  prior_compact_stale_gap_pair_run_rolls_back_before_shifted_compact_low_tail_bits
  -- --nocapture`. The remaining owner search moves before the repeated
  compact/stale-gap run or to an upstream stream-boundary artifact; no
  compact-add/low-tail resync is justified.
- 2026-06-02 `P/05/01` post-`W` storage plus stale-gap run audit: no packet
  behavior changed, but public coverage now pins the upstream storage sibling
  from the same XP2 seq19 trace. A bounded CNW storage span after
  `W current total` can feed a run of compact token-name `A/09` plus
  same-object `U/09 mask=0x17` stale-gap pairs, and those pairs exact-claim
  when their four Diamond compact add BOOLs and stale-gap update cursors are
  present. If a following compact `A/09` plus same-object low-tail
  `U/09 mask=0xF7` exposes the shifted `1000_11_101101` bit run, the
  promoted-storage candidate still rolls back unchanged. Verified with
  `cargo test -q -p hgbridge-proxy2
  work_remaining_storage_rolls_back_after_stale_gap_pair_run_before_shifted_low_tail
  -- --nocapture`. The remaining owner search moves before the post-`W`
  storage/stale-gap run or to a still-unmodeled stream-boundary artifact; no
  compact-add/low-tail cursor resync is justified.
- 2026-06-02 `P/05/01` long post-`W` storage-span audit: no packet behavior
  changed. Public fixture-free coverage now pins the span-size side of the XP2
  seq19 evidence: a 31-byte CNW storage span after `W current total` can commit
  only when repeated compact token-name `A/09` plus same-object stale-gap
  `U/09 mask=0x17` rows consume every promoted bit from their decompiled
  cursors. The sibling regression adds one unowned bit to the same long span and
  proves the whole candidate rolls back unchanged. Verified with `cargo test -q
  -p hgbridge-proxy2 long_storage_span -- --nocapture`. The unresolved search
  remains upstream of the long storage/stale-gap run or in an unmodeled
  stream-boundary artifact.
- 2026-06-02 `P/05/01` compact-add source-bit pattern audit: no packet behavior
  changed. Private XP2 seq19 replay after the 31-byte post-`W` storage
  promotion shows the exact compact `A/09` plus `U/09 mask=0x17` rows before
  offset 1145 carry mixed four-bit compact add prefixes including `1101`,
  `0001`, `0010`, and `1110`, then exact-claim their following stale-gap update
  cursors. Diamond `sub_44E4A0` still owns exactly four compact tail BOOLs for
  compact token-name/no-optional-object `A/09` rows; the bridge must drain those
  source-only bits and materialize neutral EE guard/state BOOLs instead of
  interpreting their values. Public writer coverage now pins the observed
  mixed-prefix variants. The final shifted low-tail handoff at rewritten offset
  1145 remains unresolved, so the search stays upstream or at an unmodeled
  stream-boundary artifact; do not add compact-add/low-tail resync.
- 2026-06-02 `P/05/01` long post-`W` mixed-prefix storage audit: no packet
  behavior changed. Public fixture-free coverage now ties the private XP2
  mixed compact-add prefixes (`1101`, `0001`, `0010`, `1110`) to the same
  31-byte post-`W` promoted storage span as the repeated-prefix sibling.
  Diamond `sub_44E4A0` still owns only the four source compact-add BOOLs per
  `A/09`; the following same-object `U/09 mask=0x17` stale-gap row must then
  consume its own decompiled update cursor exactly. Verified with
  `cargo test -q -p hgbridge-proxy2
  work_remaining_long_storage_span_accepts_mixed_compact_add_prefix_bits --
  --nocapture` using `CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge`.
  The later shifted low-tail handoff remains active; this only closes the
  repeated-prefix assumption in the long-span proof.
- 2026-06-02 `P/05/01` long post-`W` storage plus shifted low-tail audit: no
  packet behavior changed. Public fixture-free coverage now combines the
  XP2-sized 31-byte post-`W` mixed-prefix storage span, the exact compact
  token-name `A/09` plus same-object stale-gap `U/09 mask=0x17` run, and the
  later shifted compact `A/09` plus same-object low-tail `U/09 mask=0xF7`
  handoff. The bridge must roll back the whole promoted-storage candidate
  rather than committing the exact upstream rows when the final
  `1000_11_101101` low-tail cursor is still unowned. Verified with `cargo test
  -q -p hgbridge-proxy2
  work_remaining_long_storage_span_rolls_back_before_shifted_low_tail_handoff
  -- --nocapture` plus the focused work-remaining/long-storage/compact-token/
  low-tail suites. The unresolved search remains upstream of the long
  storage/stale-gap run or in an unmodeled stream-boundary artifact.
- 2026-06-02 `P/05/01` shifted low-tail following-boundary audit: no packet
  behavior changed. The private XP2 seq19 replay has a plausible top-level
  compact row after the shifted compact `A/09` plus same-object
  `U/09 mask=0xF7` handoff. Public fixture-free coverage now proves that a
  valid following compact token-name `A/09` plus same-object stale-gap
  `U/09 mask=0x17` pair still cannot act as a stream resync point: the
  long promoted-storage transaction must roll back unchanged before the
  shifted low-tail handoff, and the following pair is only accepted when tested
  by itself with its own decompiled add/update bits. Verified with `cargo test
  -q -p hgbridge-proxy2
  work_remaining_long_storage_span_rolls_back_before_shifted_low_tail_with_following_boundary
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 work_remaining_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 long_storage_span --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 compact_placeable_token --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 low_tail --
  --nocapture`. The unresolved owner search stays upstream of the long
  storage/stale-gap run or in a still-unmodeled boundary artifact; it is not
  justified by the row after the shifted low-tail update.
- 2026-06-02 `P/05/01` pre-`W` compact/stale-gap boundary audit: no packet
  behavior changed. Public fixture-free coverage now moves one boundary
  upstream of the long post-`W` storage span: a valid compact token-name `A/09`
  plus same-object stale-gap `U/09 mask=0x17` immediately before `W current
  total` exact-claims by itself and can coexist with the 31-byte promoted
  storage/stale-gap run, but still cannot donate, skip, or resync bits for the
  later shifted compact `A/09` plus low-tail `U/09 mask=0xF7` handoff. Verified
  with `cargo test -q -p hgbridge-proxy2
  work_remaining_long_storage_span_rolls_back_after_preceding_pair_before_shifted_low_tail
  -- --nocapture` using `CARGO_TARGET_DIR=C:\nwnbridge\codex-target-ee-bridge`.
  The unresolved owner search remains before that pre-`W` pair or in a
  still-unmodeled stream-boundary artifact.
- 2026-06-02 `P/05/01` pre-`W` full-update run boundary audit: no packet
  behavior changed. A filtered private XP2 seq19 debug replay shows a long run
  of compact token-name `A/09` plus full `U/09 mask=0x37` placeable updates
  before the first `W current total`; public fixture-free coverage now proves
  that run can exact-claim with its own four compact-add BOOLs and
  decompile-backed full-update cursors, and it can coexist with the later
  31-byte post-`W` storage/stale-gap run. The same proven run still must roll
  back unchanged when the later compact `A/09` plus same-object low-tail
  `U/09 mask=0xF7` exposes the shifted `1000_11_101101` source bits. Verified
  with `cargo test -q -p hgbridge-proxy2
  pre_w_full_update_run_does_not_resync_shifted_low_tail -- --nocapture`.
  The unresolved bit-owner search moves upstream of the pre-`W` full-update run
  or to a stream-boundary artifact; no compact-add / low-tail resync is
  justified.
- 2026-06-02 `P/05/01` leading creature/door pre-run boundary audit: no packet
  behavior changed. The same private XP2 seq19 replay starts with two
  EE-shaped `U/05` creature visual-transform rows, then a door `A/0A` plus full
  `U/0A mask=0x37`, before the compact placeable full-update run. Public
  fixture-free coverage now proves those leading rows exact-claim at their own
  cursors, can coexist with the later pre-`W` full-update run and 31-byte
  post-`W` storage/stale-gap run, and still must roll back unchanged when the
  later compact `A/09` plus low-tail `U/09 mask=0xF7` exposes the shifted
  `1000_11_101101` source bits. Verified with `cargo test -q -p
  hgbridge-proxy2 leading_creature_and_door_run_does_not_resync_shifted_low_tail
  -- --nocapture`, plus the focused pre-`W`, work-remaining, compact-token, and
  low-tail suites. The unresolved bit-owner search moves to the stream start or
  a still-unmodeled stream-boundary / packet-framing artifact; no compact-add /
  low-tail resync is justified.
- ~~2026-06-03 `P/05/01` stream-start terminal-trim audit: fixed a generalized
  shifted-cursor hazard in the update rewrite terminal trim gate. A compact
  token-name `A/09` followed by a byte-exact full `U/09 mask=0x37` legitimately
  inserts EE-only add/update bits, but that insertion-only final update does not
  prove any source bits may be discarded. The old gate could trim leftover
  terminal bits after the final update and thereby hide an extra bit inserted
  before the first compact add cursor. Terminal trim now requires the same final
  record to remove or change source bits before truncating the fragment tail.
  Public fixture-free coverage proves the unshifted compact add/full-update pair
  still rewrites and exact-claims, while the same bytes with one stream-start
  extra bit stay unclaimed and unchanged. Verified with `cargo test -q -p
  hgbridge-proxy2
  compact_placeable_token_add_rejects_stream_start_bit_shift_before_exact_37_update
  -- --nocapture`, focused compact-token, low-tail, work-remaining, leading
  creature/door, and pre-`W` full-update suites. Re-run the private/live XP2
  seq19 stream next to determine whether the remaining shifted handoff is
  resolved by this generalized trim fix or still needs a separate
  stream-boundary owner.~~
- ~~2026-06-03 `P/05/01` clean-fragment stream assembly audit: fixed the pending
  legacy high-level fragment append path so it uses the same normalized
  read-buffer / CNW tail split that the clean-fragment detector already proves.
  The XP2 seq19 `pending-live-object-unclaimed-seq19-chunks1..6` diagnostics show
  repeated cumulative windows ending with a `W current total` read-buffer row
  plus trailing CNW storage bytes before the next window starts at a fresh
  `A/09` boundary. Appending raw bytes stranded each per-chunk tail in the
  pending live-object read stream, which can shift later add/update cursors even
  though the final rebuilt fixture looks byte-plausible. Added fixture-free
  coverage for a normalized clean fragment ending in `W` plus one tail byte.
  Private replay of the existing rebuilt XP2 seq19 fixture still stays unclaimed
  and unchanged, as expected for a fixture assembled before this correction; run
  a fresh local XP2 replay next to confirm whether the stream-boundary owner is
  now resolved. 2026-06-03
  follow-up: added public fixture-free chunk-level coverage for two
  zero-declared clean fragments, proving each chunk is normalized before append
  and that per-chunk CNW tails (`0xA0`, `0x80`) stay out of rebuilt read bytes.
  Verified with `cargo test -q -p hgbridge-proxy2
  zero_declared_clean_fragment_chunks_keep_each_tail_out_of_read_bytes --
  --nocapture`. Fresh local XP2 Chapter 2 replay
  `C:\nwnbridge\local-diamond-bridge-20260603-110950` confirmed the former
  seq19 stream-boundary owner search no longer reproduces: `P/05/01` sequence
  19 exact-claims through `GameObjUpdate_LiveObjectCombinedRecords`
  (`old_payload_length=112`, `new_payload_length=120`), no
  `pending-live-object-*` dumps are emitted, and the run has no quarantine
  files.~~
- ~~2026-06-03 client `P/1E/02 GuiQuickbar_SetButton` CNW-wrapper audit: fixed
  the client-originated quickbar SetButton verifier so it reads the declared CNW
  window before the slot/type bytes, validates the single no-BOOL fragment
  cursor byte, and keeps type-specific bodies bounded to the declared read
  window. Fresh XP2 Chapter 2 inventory-open replay previously quarantined two
  valid client frames (`slot=5,type=0` and `slot=5,type=43`) because the old
  parser treated the declared length byte as the slot. After the fix the same
  run claims both as `ClientQuickbar` with `body_kind=NoParam` and
  `body_kind=IntParam`; verified with `cargo test -q -p hgbridge-proxy2
  client_quickbar -- --nocapture` and local replay
  `C:\nwnbridge\local-diamond-bridge-20260603-110950`.~~
- ~~2026-06-03 `P/05/01` terminal `W current total` storage-bit ownership
  audit: tightened the terminal post-`W` cleanup path so a `W current total`
  row can remove only empty/all-zero CNW storage when no following family
  consumes it. Diamond `sub_44F160` and EE `sub_1407B85A0` read no CNW BOOLs;
  nonzero promoted storage bits now require a later decompile-owned family
  reader to advance the cursor or the update rewrite rolls back unchanged.
  Verified with `cargo test -q -p hgbridge-proxy2 work_remaining_terminal --
  --nocapture` and `cargo test -q -p hgbridge-proxy2 work_remaining_ --
  --nocapture`.~~
- ~~2026-06-02 `P/05/01` `W current total` trailing-span/compact-boundary
  audit: fixed over-promotion in the terminal `W` fragment-span path. Diamond
  `sub_44F160` and EE `sub_1407B85A0` read exactly `W current total` and no CNW
  BOOLs; Diamond `sub_44E4A0` still owns the four compact `A/09` tail BOOLs as
  a separate top-level add row. The trailing-span promoter now refuses a
  proposed post-`W` CNW storage tail when a bounded CNW prefix is followed by a
  valid top-level live-object boundary walk, so it cannot manufacture compact
  add guard bits or a following low-tail update cursor. Midstream post-`W`
  storage removal also requires exact ownership of the prefix through `W` at
  the current fragment cursor. Verified with `cargo test -q -p hgbridge-proxy2
  work_remaining -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  compact_placeable_token -- --nocapture`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, `git diff --check`, and the
  private XP2 seq19 replay. The replay still remains active at offset 953, so
  the unresolved upstream bit-owner search stays with the preceding low-tail
  handoff note.~~
- 2026-05-27 `P/04/01` static-placeable fragment-cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the Diamond
  and EE static-placeable row contract around the post-tile lists. The static
  list owns only the WORD count plus `OBJECTID + WORD + six FLOAT` read-buffer
  rows; it consumes no CNW fragment BOOLs before the EE bridge-owned two zero
  post-static WORDs. A byte-exact static row with one extra fragment bit is now
  explicitly unclaimed on both the legacy source proof and the exact EE
  `LoadArea` proof, so context collection cannot expose static rows from a
  shifted bit cursor. Verified with `cargo test -q -p hgbridge-proxy2
  static_placeable -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  public_static_direction_tests -- --nocapture`, `cargo fmt --all --check`,
  `git diff --check`, and `cargo check -q -p hgbridge-proxy2`.
- 2026-05-28 `P/04/01` light-placeable fragment-cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the adjacent
  light-placeable list uses the same read-buffer-only cursor discipline before
  static rows. Diamond and EE own only the WORD count plus
  `OBJECTID + WORD + three FLOAT` rows for light placeables; an extra CNW
  fragment bit before the static-list count now blocks both legacy source
  context collection and the exact EE `LoadArea` proof. Verified with
  `cargo test -q -p hgbridge-proxy2 light_placeable -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 public_static_direction_tests --
  --nocapture`, `cargo fmt --all --check`, `git diff --check`, and
  `cargo check -q -p hgbridge-proxy2`.
- 2026-05-28 `P/04/01` area tile-loop fragment-cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the
  dimension-driven Diamond/EE tile loop owns only tile read-buffer records
  before the post-tile list cursor. A tile-complete area with one extra CNW
  fragment bit is rejected by both the legacy source-tail proof and the exact
  EE `LoadArea` proof, so shifted tile-loop fragment state cannot be mistaken
  for transition/map/sound/light/static list state. Verified with `cargo test
  -q -p hgbridge-proxy2
  tile_rows_do_not_consume_fragment_bits_before_post_tile_lists --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  exact_ee_area_proof_rejects_tile_loop_fragment_tail -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 public_static_direction_tests --
  --nocapture`, `cargo fmt --all --check`, `git diff --check`, and
  `cargo check -q -p hgbridge-proxy2`.
- 2026-05-28 `P/04/01` map-pin fragment-cursor audit: no packet behavior
  changed, but public fixture-free coverage now proves the decompile-owned
  post-tile map-pin rows are read-buffer only before sound/light/static lists.
  Diamond and EE own the DWORD pin id, bounded CExoString label, and XYZ FLOAT
  triplet without consuming CNW fragment BOOLs; one extra fragment bit now
  rejects both the legacy source-tail proof and exact EE `LoadArea` proof, so
  shifted map-pin cursor state cannot expose later placeable context.
- 2026-05-28 `P/04/01` sound-object fragment-cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the post-tile
  sound list owns exactly the decompiled six CNW BOOLs per row after the fixed
  byte body and CResRef list. A sound row with a seventh fragment bit now
  rejects both legacy source-tail/context proof and exact EE `LoadArea` proof,
  so shifted sound-list state cannot expose later light/static placeable rows.
  Verified with `cargo test -q -p hgbridge-proxy2 sound -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 light_placeable -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 map_pin -- --nocapture`, and
  `cargo test -q -p hgbridge-proxy2 public_static_direction_tests --
  --nocapture`, plus `cargo fmt --all --check`, `git diff --check`, and
  `cargo check -q -p hgbridge-proxy2`.
- 2026-05-28 `P/04/01` transition-list fragment-cursor audit: no packet
  behavior changed, but public fixture-free coverage now proves the post-tile
  transition rows reject leftover fragment bits on both legacy source-tail/
  context proof and exact EE `LoadArea` proof. TLK transition labels own
  exactly visibility, TLK/direct selector, and TLK guard BOOL bits before the
  DWORD string ref; an extra fragment bit after that label is unclaimed and
  cannot expose later map/sound/light/static rows. Verified with `cargo test
  -q -p hgbridge-proxy2 transition -- --nocapture`, plus the final formatting
  and check suite for this run.
- 2026-05-28 `P/04/01` EE pre-tile inserted-bit audit: no packet behavior
  changed, but public fixture-free coverage now proves the exact EE
  `LoadArea` proof rejects drift in the bridge-owned build-36.3/build-36.5
  pre-tile fields. Legacy rewrites must keep the inserted tileset-options BOOL
  false, the inserted tileset-options count zero, and the inserted tile-loop
  BOOL false before tile rows; any true/nonzero branch is unclaimed until a
  row shape is proven from Diamond/EE decompiles. The same pass extended the
  exact transition fragment-tail proof to the direct CExoString branch, proving
  direct labels own only visibility plus selector bits before later lists.
  Verified with `cargo test -q -p hgbridge-proxy2
  exact_ee_area_proof_rejects -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 transition -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 public_static_direction_tests -- --nocapture`, `cargo fmt
  --all --check`, `git diff --check`, and `cargo check -q -p
  hgbridge-proxy2`.
- 2026-05-28 `P/04/01` missing-dimension repair cursor audit: hardened the
  legacy missing width/height repairs so inferred tile dimensions are staged
  first, then copied back only when the repaired dimension-driven tile loop
  lands on the exact decompile-owned post-tile byte and fragment cursor. A
  missing-height or missing-width area with one extra unowned post-tile
  fragment bit now leaves the dimension DWORDs untouched instead of accepting a
  semantically plausible tile grid. Verified with `cargo test -q -p
  hgbridge-proxy2 missing_height_repair_requires_exact_post_tile_fragment_cursor
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  missing_width_repair_requires_exact_post_tile_fragment_cursor --
  --nocapture`, plus the existing Docks/Voyage positive repair filters.
- 2026-05-28 `P/04/01` square-dimension repair cursor audit: hardened the
  legacy fixed-name square-dimension repair to stage trial width/height DWORDs
  and commit them only after the repaired scan proves the same layout, tile
  count, tile-end cursor, and exact post-tile legacy source proof. A BW167 demo
  square-area seed with one extra post-tile fragment bit now leaves both
  dimension DWORDs and the fragment stream untouched. Verified with
  `cargo test -q -p hgbridge-proxy2 square_dimension -- --nocapture`, the
  existing BW167 positive rewrite filter, `cargo fmt --all --check`,
  `git diff --check`, and `cargo check -q -p hgbridge-proxy2`.
- 2026-05-28 `P/04/01` compact post-tile repair cursor audit: tightened the
  module-backed compact transition/map-note plus sound-tail repair so a staged
  candidate must preserve the original area layout, dimensions, tile count, and
  tile-end cursor before the exact legacy post-tile source proof can commit it.
  Public synthetic coverage now proves a compact direct-label transition row
  plus sound row owns exactly the two transition label bits and six sound BOOLs;
  one extra fragment bit rejects the repair and leaves the source payload
  untouched. Verified with `cargo test -q -p hgbridge-proxy2
  compact_post_tile_tail_repair_requires_exact_fragment_cursor --
  --nocapture`.
- ~~2026-06-03 client `P/06/03 Input_ChangeDoorState` transition-close cleanup:
  no emitted packet behavior changed, but the semantic-state registry no longer
  carries a nearby-placeable display-name classifier for transition anchors.
  That classifier included module/place-name-like substrings while the actual
  rewrite already emitted the same decompile-owned `Input_WalkToWaypoint`
  shape from only the recent same-door open, verified current area OBJECTID,
  verified door object type, and verified door position. Public tests now prove
  a nearby transition-sounding placeable is ignored and the rewritten walk uses
  the door coordinates and exact two-BOOL fragment cursor. Verified with
  `cargo test -q -p hgbridge-proxy2 transition_door_close_ -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2 semantic::state::tests --
  --nocapture`.~~
- ~~2026-05-28 `P/04/01` static-row staged-repair audit: hardened the remaining
  static-placeable row mutators so direction normalization, module GIT-backed
  appearance/position/bearing repair, and zero-count tail dropping commit only
  after a candidate buffer preserves the exact decompile-owned post-tile source
  cursor. Public fixture-free regressions now prove that an unrepairable later
  zero direction vector or non-finite module bearing rejects the whole repair
  without partially rewriting earlier static rows. Verified with
  `cargo test -q -p hgbridge-proxy2
  static_direction_normalization_rejects_later_zero_vector_without_partial_write
  -- --nocapture` and `cargo test -q -p hgbridge-proxy2
  module_static_row_repair_rejects_nonfinite_bearing_without_partial_write --
  --nocapture`. 2026-06-03 re-audit added an original tile-layout identity
  guard before zero-count tail-drop commit and public coverage proving a bad
  later row-shaped tail rejects without shortening the packet:
  `cargo test -q -p hgbridge-proxy2
  zero_count_static_tail_drop_rejects_later_bad_row_without_partial_shorten --
  --nocapture`.~~
- ~~2026-05-27 `P/05/01` work-remaining `W` cursor audit follow-up~~:
  resolved 2026-05-27. Diamond `sub_44F160` and EE `sub_1407B85A0` both read
  exactly `W current total` and consume no CNW fragment BOOLs, so the identity
  helper is exact-only again. Captured post-`W` fragment-storage cleanup moved
  to the top-level live-object boundary path: midstream cleanup requires a
  verified three-byte `W`, a bounded CNW-shaped span, and an explicit following
  live-object boundary; terminal cleanup additionally requires the final exact
  EE payload validator to accept the truncated stream. Verified with
  `cargo test -q -p hgbridge-proxy2 work_remaining -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 appearance_update_slot0_visible_equipment_claims_exactly -- --nocapture`,
  and `cargo test -q -p hgbridge-proxy2 live_object_update -- --nocapture`.
- 2026-05-27 `P/11/02` and `P/11/04` CharList fragment-tail cursor audit:
  tightened the existing decompile-backed character-list proof without changing
  valid packet semantics. `CharList_ListResponse` now rejects nonzero padding
  bits after the exact `ReadCExoLocStringClient` cursor, and
  `CharList_UpdateCharResponse` must carry the byte-only BIC body followed by a
  single empty CNW fragment cursor byte. The 2026-06-30 fresh HG capture proved
  the update-response empty cursor by high bits (`0b011xxxxx`) rather than exact
  byte `0x60`; arbitrary post-BIC fragment storage is still not owned. Public
  fixture-free tests now cover the list response server-locstring bit cursor,
  padding-bit rejection, and update response empty-fragment handoff. Keep the
  broader player-model issue focused on live-object current-player appearance
  unless future evidence shows the BIC/CharList source fields are already wrong
  before the first `P/05/01`.
- 2026-05-28 `P/05/01` full creature appearance cross-record fence audit:
  tightened the byte-only `P/5` parser so it no longer assumes a three-bit
  packetized fragment fence before a following `U/5` record when no fragment
  proof is available. Cross-record fence bits are now accounted only when the
  focused following creature-update reader validates at the exact post-fence
  cursor; otherwise the caller must prove the explicit appearance byte
  boundary. Public fixture-free coverage proves a full appearance followed by a
  zero-mask `U/5` remains valid at the explicit boundary while the old no-proof
  cross-record parse stays rejected. Verified with `cargo test -q -p
  hgbridge-proxy2 full_appearance_no_proof_requires_explicit_boundary_before_following_u5
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 full_appearance --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 live_object_update --
  -- --nocapture`.
- ~~2026-05-28 `P/05/01` pending seq31 live-object exact-claim regression:
  resolved 2026-05-28. The first unclaimed rows were top-level `A` item adds
  whose read-buffer body matched the visible-equipment item body and was already
  EE-shaped (widened model parts plus EE visual transform), and whose item name
  bytes proved the locstring-token branch, but whose CNW fragment stream still
  held the shorter direct-name selector before the active-property BOOLs. Reused
  the decompile-backed item-name selector rewrite for top-level item adds, and
  made the update pass skip already exact add rows before legacy item expansion
  while refusing later cursor mutations after an unproven add cursor. Verified with
  `cargo test -q -p hgbridge-proxy2
  top_level_item_add_token_name_repair_rewrites_selector_prefix_only --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  update_rewrite_does_not_repeat_repair_exact_top_level_item_add --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  update_rewrite_can_repair_top_level_item_add_name_bits_midstream --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2
  pending_seq31_stream_rewrites_to_exact_live_object_claim --
  --nocapture`.~~
- ~~2026-06-08 broad `live_object` filter seq31 pre-pass failure: resolved
  2026-06-08. The root rule was generalized beyond the fixture: an inline-name
  `A/09` placeable add with Diamond BYTE/WORD/WORD tail and legacy scalar
  visual-transform identity owns its ten decompile-backed source BOOLs before a
  following same-object update. The update pre-pass may now normalize that add
  through the same transactional proof used for compact short-name rows:
  legacy cursor advance, add rewrite, exact EE add cursor, same-object following
  update boundary, and final exact validation. Verified with fixture-free
  `inline_placeable_scalar_add_rewrites_before_following_same_object_update`,
  private `pending_seq31_stream_rewrites_to_exact_live_object_claim`,
  `compact_placeable`, and `live_object_update`.~~
- ~~2026-06-08 broad `live_object` filter mixed door/placeable terminal-six:
  resolved 2026-06-08. The unowned tail was not final `W` storage; it was caused
  earlier by an already EE-byte-shaped inline/direct `A/09` placeable add whose
  fragment source still had Diamond-width direct-name bits immediately before a
  same-object `U/09`. Diamond `sub_44E4A0` owns the direct CExoString name BOOL,
  one post-name state BOOL, optional-object BOOL, and seven trailing state BOOLs;
  EE `sub_1407A7800` owns the same optional branch plus one extra neutral BOOL
  before `ObjectVisualTransformData`. The bridge now widens only that bounded
  shape, requires the optional-object bit to match the byte branch, proves the raw
  add does not exact-claim, then transactionally proves the widened add and
  following same-object update cursor before applying the repair. The prior final
  `W current total` remains fragment-neutral in Diamond `sub_44F160` and EE
  `sub_1407B85A0`; no generic terminal padding trim was added. Verified with
  fixture-free
  `ee_inline_placeable_add_legacy_width_bits_insert_guard_before_following_update`,
  add-guard helper tests, and private
  `hg_door_mixed_add_update_fixture_rewrites_to_exact_ee_claim`, plus
  `live_object_update`, `cargo fmt --all --check`, `git diff --check`, and
  `cargo check -q -p hgbridge-proxy2`.~~
- 2026-05-28 `P/05/01` door-add visual-map cursor audit: fixed two stale
  door-add name call sites that advanced past an EE object visual-transform
  identity as if it were the legacy 40-byte scalar identity. EE
  `AddDoorAppearanceToMessage` owns only the two-DWORD `ObjectVisualTransformData`
  empty map before the name/state tail; the add-name bit repair and verified
  record diagnostics now use the same 8-byte cursor as the boundary walker,
  add validator, and fragment cursor proof. Public fixture-free tests prove the
  exact direct-name six-bit branch reports the inline name and that a stale
  locstring-helper bit is collapsed to the direct-name selector after the
  eight-byte map. Verified with `cargo test -q -p hgbridge-proxy2
  door_add_name -- --nocapture`, `cargo test -q -p hgbridge-proxy2 door_add --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 add_guard --
  --nocapture`, plus `cargo test -q -p hgbridge-proxy2 live_object_update --
  --nocapture`, `cargo fmt --all --check`, `git diff --check`, and
  `cargo check -q -p hgbridge-proxy2`.
- 2026-05-28 `P/05/01` door/placeable add legacy-scalar visual-map audit:
  no packet shape changed, but public fixture-free coverage now proves the
  adjacent legacy 40-byte `CAurObjectVisualTransformData` identity replacement
  path. Raw door/placeable `A` rows carrying that scalar at the decompiled
  visual-transform cursor do not exact-claim as EE; the bridge rewrites exactly
  those 40 bytes to EE's two-DWORD `ObjectVisualTransformData` empty map, leaves
  the name/tail cursor immediately after the eight-byte map, and then exact EE
  add-fragment validation owns the final bit cursor. The same pass removed
  dormant live-object add/update suppression bookkeeping so missing door models
  or static-overlap diagnostics cannot revive an object-deletion workaround.
  Verified with `cargo test -q -p hgbridge-proxy2
  legacy_scalar_visual_transform -- --nocapture`.
- 2026-05-28 `P/05/01` looping visual-effect stream-boundary audit:
  tightened only the no-`visualeffects.2da` stream-boundary probe. When a
  single `U/* 0x00000008` row can be split as either no target plus EE identity
  map or five-byte target payload plus EE identity map, the scanner now refuses
  to choose a record end without row-type proof. Exact record validation still
  accepts either decompile-owned shape after a caller has already proven the
  record boundary. Verified with the focused
  `looping_effect_stream_boundary_rejects_ambiguous_target_fallback` and
  `loaded_visualeffects` cargo test filters.
- 2026-05-28 `P/05/01` legacy visual-effect target-payload boundary audit:
  extended the same no-row-policy ambiguity rule to pre-rewrite Diamond/HG
  effect rows. A five-byte target payload can itself begin with bytes that look
  like a zero-row live GUI `G/Q` record; without loaded `visualeffects.2da`
  `Type_FD` proof, the transport scanner now refuses to split at that shorter
  no-target cursor when the target-width cursor also lands on a real boundary.
  Verified with `cargo test -q -p hgbridge-proxy2 effect_target_payload --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 looping_effect --
  --nocapture`, and the full `cargo test -q -p hgbridge-proxy2` suite.
- 2026-05-28 `P/05/01` creature status-effect target-payload cursor audit:
  carried the same decompile-backed `visualeffects.2da` target-payload rule
  into creature status-effect boundary scans, exact C408 validation, and legacy
  identity-map insertion. Boundary helpers now account for `DWORD object id +
  BYTE` before the EE transform map, and without row-type proof reject a
  same-row no-target/target ambiguity instead of splitting on the shorter
  zero-looking map cursor. Verified with `cargo test -q -p hgbridge-proxy2
  creature_status_effect -- --nocapture` and `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --nocapture`, plus the serial full
  `cargo test -q -p hgbridge-proxy2 -- --test-threads=1` suite.
- 2026-05-28 `P/05/01` zero-count creature status-effect repair audit:
  generalized the malformed `U/5 0x4408`/`0xC408` count repair from captured
  row literals to compact no-target `A`/`D` row triplets before the fixed
  four-WORD scalar suffix. The production rewrite is now transactional: stage
  the count, insert EE ObjectVisualTransformData maps at compact-row cursors
  even when the following zero scalar suffix resembles an identity map, then
  commit only after the exact creature-update reader owns the final read and
  fragment-bit cursor. A missing final `0x4000` status BOOL now leaves the
  payload untouched. Verified with `cargo test -q -p hgbridge-proxy2
  creature_4408_zero_count -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 creature_status_effect -- --nocapture`, and `cargo test -q
  -p hgbridge-proxy2 live_object_update -- --nocapture`.
- 2026-05-30 `P/05/01` short live GUI declared-tail audit: no packet shape
  changed, but the stale-declared tail guard now names the decompile-backed
  short live GUI read-buffer boundary explicitly. Diamond `sub_4589A0` and EE
  `sub_1407B3F30` keep `G/Q`, `G/I D|U`, and `G/R D|U|M` rows in the read
  buffer with no CNW BOOLs, so a proposed CNW tail beginning at one of those
  aligned rows is read-boundary ambiguity, not fragment storage. Fixture-free
  coverage now proves short `G/I U` and `G/R M` rows can decode as compact CNW
  bits but are still rejected by declared-length transport plausibility.
  Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_gui -- --nocapture`, `cargo test -q
  -p hgbridge-proxy2 declared_length_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --nocapture`, `cargo fmt --all
  --check`, `git diff --check`, `cargo check -q -p hgbridge-proxy2`, and the
  serial full `cargo test -q -p hgbridge-proxy2 -- --test-threads=1` suite.
- 2026-05-30 `P/05/01` interior short-read declared-tail audit: tightened the
  stale-declared tail ambiguity guard so a proposed CNW fragment tail is scanned
  for decompile-owned short read-buffer rows after any leading fragment-looking
  byte, not only at the first tail byte. Diamond/EE `W current total`, zero-row
  `G/Q`, and six-byte delete rows remain live-object read boundaries wherever
  they are left inside the proposed tail; a compact CNW bit decode is not enough
  to hide them as fragment storage. Verified with `cargo test -q -p
  hgbridge-proxy2
  declared_length_window_rejects_interior_short_read_boundary_in_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 live_object_update --
  --nocapture`.
- 2026-05-30 `P/05/01` compact door-add declared-tail audit: extended the same
  stale-declared short-add guard to 12-byte `A/0A` door adds whose Diamond
  source owns `OBJECTID + nonzero door DWORD + WORD state tail` before the
  focused add translator inserts EE's object visual-transform map and empty
  direct `CExoString`. Those bytes can decode as compact CNW fragment storage,
  but they are still a live-object read-buffer row and the transport prefix
  walker must split before a following `W`/`U/0A` row. Verified with `cargo
  test -q -p hgbridge-proxy2 compact_door -- --nocapture`.
- 2026-05-30 `P/05/01` delete declared-tail bit-count audit: no packet shape
  changed, but public fixture-free coverage now pins delete rows under the
  stale-declared tail guard. Diamond and EE both read six delete read-buffer
  bytes; `D/5`, `D/6`, and `D/9` then own exactly one CNW BOOL, while `D/7`
  and `D/10` own none. A proposed CNW tail beginning at any aligned delete row
  therefore remains live-object read-boundary ambiguity, and prefix capacity
  must not borrow the following `W` row as delete bit storage. Verified with
  `cargo test -q -p hgbridge-proxy2 declared_length_ -- --nocapture`.
- 2026-05-30 `P/05/01` short creature body-delta appearance declared-tail
  audit: stale-declared repair now treats unmodeled short `P/5` mask `0x0100`
  body-part delta rows as live-object read-boundary ambiguity instead of CNW
  fragment storage. Diamond `sub_448E30` and EE `sub_14077FE10` both read the
  `P/5 + OBJECTID + WORD mask` header, then for body-part delta selector zero
  own only the selector byte and for selectors `1..=9` own selector plus
  index/value byte pairs. The semantic appearance translator still leaves this
  partial body-delta family unclaimed until a full typed model exists; the
  transport guard only prevents shifted declared-length repair from hiding it.
  Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_creature_body_delta_appearance_as_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 live_object_update --
  --test-threads=1`, `cargo fmt --all --check`, `cargo check -q -p
  hgbridge-proxy2`, and full serial `cargo test -q -p hgbridge-proxy2 --
  --test-threads=1`.
- 2026-05-31 `P/05/01` short creature equipment-delta appearance declared-tail
  audit: stale-declared repair now treats short zero-count `P/5` mask `0x0200`
  equipment-delta rows as live-object read-boundary ambiguity instead of CNW
  fragment storage, including scalar/`0x2000` prefixes that still stay below
  the short-row floor. Diamond `sub_448E30` and EE `sub_14077FE10` both read
  the `P/5 + OBJECTID + WORD mask` header, then after the scalar fields mask
  bit `0x0200` owns a BYTE count; count zero consumes no entry bytes and no CNW
  BOOLs, while nonzero entries own `CHAR opcode + OBJECTID + DWORD slot/field`
  before any opcode-specific item body. The semantic appearance translator
  still leaves partial equipment deltas unclaimed until the typed model/writer
  is implemented; the transport guard only prevents shifted declared-length
  repair from stealing the aligned short zero-count form. Verified with `cargo
  test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_creature_zero_equipment_delta_appearance_as_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_creature_body_delta_appearance_as_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 live_object_update --
  --test-threads=1`, `cargo fmt --all --check`, `git diff --check`, `cargo
  check -q -p hgbridge-proxy2`, and full serial `cargo test -q -p
  hgbridge-proxy2 -- --test-threads=1`.
- 2026-05-31 `P/05/01` combined short creature body/equipment appearance
  declared-tail audit: extended the partial body-delta transport guard beyond
  exact mask `0x0100`. Diamond `sub_448E30` and EE `sub_14077FE10` both read
  scalar appearance fields before the `0x0100` body selector, then read the
  post-body `0x2000` WORD+DWORD, skip the `0x4000` feature byte on the legacy
  build path, and finally read the `0x0200` equipment count. Short combined
  masks such as `0x0101`, `0x2100`, `0x0300`, and `0x4300` can still sit below
  the broad boundary scanner floor while decoding as plausible CNW fragment
  storage, so declared-length repair now treats them as unmodeled read-buffer
  rows instead of fragment tails. Zero-count equipment also accepts the
  legacy-skipped `0x4000` mask bit (`0x4200`). Verified with
  `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_creature_body_delta_appearance_as_fragment_tail
  -- --nocapture` and `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_creature_zero_equipment_delta_appearance_as_fragment_tail
  -- --nocapture`, plus `cargo fmt --all --check`, `git diff --check`, `cargo
  check -q -p hgbridge-proxy2`, `cargo test -q -p hgbridge-proxy2
  declared_length_ -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  live_object_update -- --test-threads=1`, and full serial `cargo test -q -p
  hgbridge-proxy2 -- --test-threads=1`.
- 2026-05-31 `P/05/01` short partial creature appearance prefix-capacity
  audit: wired the same decompile-backed short `P/5` body-delta and zero-count
  equipment-delta row cursors into the live-object transport boundary walker,
  not just the stale-tail ambiguity guard. Diamond `sub_448E30` and EE
  `sub_14077FE10` own selector-zero body rows and zero-count equipment rows as
  sub-10-byte read-buffer records with no CNW BOOLs; a following `D/5` delete
  still owns its own one BOOL. The prefix capacity proof now splits at those
  short `P/5` ends before counting following record bits, so an empty fragment
  tail cannot hide a delete BOOL shortage behind an unmodeled partial
  appearance row. Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_capacity_splits_short_partial_creature_appearance_before_delete_bits
  -- --nocapture`.
- 2026-05-31 `P/05/01` zero-mask creature appearance capacity-floor audit:
  fixed the transport prefix-capacity minimum for `P/5` appearance rows. Diamond
  `sub_448E30` and EE `sub_14077FE10` both read the no-op appearance row as
  exactly `P/5 + OBJECTID + WORD mask`, so a zero mask owns eight read-buffer
  bytes and no CNW BOOLs. The capacity preflight now uses that eight-byte floor
  instead of the ten-byte `U` update header floor, preventing valid standalone
  zero-mask `P/5` rows from being rejected before typed validation. Verified
  with `cargo test -q -p hgbridge-proxy2
  declared_length_capacity_accepts_zero_mask_creature_appearance_without_bool_bits
  -- --nocapture`.
- 2026-05-31 `P/05/01` name-only creature appearance capacity audit: tightened
  stale-declared prefix capacity for non-full `P/5` masks carrying `0x0400`.
  Diamond `sub_448E30` and EE `sub_14077FE10` both read the appearance header,
  then mask bit `0x0400` owns the outer name-mode BOOL before either one direct
  `CExoString` or the two helper locstring/client-string branches. A direct
  empty name therefore still consumes exactly one CNW BOOL after the fragment
  header; capacity proof can no longer accept a read prefix with only the CNW
  header bits. Full `0xFFFF` appearances remain with the typed appearance
  rewrite/validator because visible-equipment item name/property bits can be
  inserted or removed before exact EE validation. Verified with `cargo test -q
  -p hgbridge-proxy2
  declared_length_capacity_counts_name_only_creature_appearance_bits --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  declared_length_repair_claims_cepv22_full_stream_without_stranding_live_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 live_object_update --
  --test-threads=1`, and `cargo check -q -p hgbridge-proxy2`.
- 2026-05-31 `P/05/01` name-only creature appearance locstring-token audit:
  extended the same `0x0400` proof to the locstring-pair branch. Diamond
  `sub_448E30` / EE `sub_14077FE10` read the outer name-mode BOOL, and
  Diamond `sub_53E700` plus the matching EE locstring helper then read each
  component's token/inline selector; token components additionally read the
  client-TLK/language selector before the DWORD token reference, while inline
  components read a `CExoString`. Transport boundary selection now prefers a
  `P/5` name branch that lands on the live-object boundary, and the narrow
  name-only fallback consumes the exact component selector bits instead of
  treating token DWORDs as generic empty strings. Verified with `cargo test -q
  -p hgbridge-proxy2
  name_only_creature_appearance_locstring_token_requires_component_bits --
  --nocapture`, `cargo test -q -p hgbridge-proxy2
  declared_length_capacity_counts_name_only_creature_appearance_locstring_token_bits
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, and `cargo test -q -p hgbridge-proxy2 live_object_update --
  --test-threads=1`.
- 2026-05-31 `P/05/01` named partial creature appearance capacity audit:
  extended the transport-only partial `P/5` guard to masks that combine the
  name bit `0x0400` with body/equipment deltas. Diamond `sub_448E30` and EE
  `sub_14077FE10` read the name branch before scalar fields, the `0x0100`
  body selector, post-body `0x2000`, skipped legacy `0x4000`, and zero-count
  `0x0200` equipment; Diamond `sub_53E700` proves locstring components consume
  selector plus language bits for token components. Declared-length capacity
  now counts direct-name and locstring-pair name BOOLs before treating the
  remaining partial body/zero-equipment bytes as read-buffer-only transport
  rows. Later typed-parser work on 2026-05-31 promoted the direct-name and
  fragment-proven locstring body-delta rows into the structured appearance
  model, and later the same day promoted the nonzero equipment `A/D/U`
  item-change list. Verified with
  `cargo test -q -p hgbridge-proxy2
  declared_length_window_rejects_short_creature_named_body_delta_appearance_as_fragment_tail
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  declared_length_capacity_counts_named_partial_creature_appearance_bits --
  --nocapture`, and the prior
  `declared_length_window_rejects_short_creature_body_delta_appearance_as_fragment_tail`
  regression.
- 2026-05-31 `P/05/01` partial creature body full-selector transport audit:
  extended the read-boundary walker for non-full `P/5` body/equipment
  appearance deltas beyond the short-row floor. Diamond `sub_448E30` and EE
  `sub_14077FE10` both compare the `0x0100` body selector against `0x0A`;
  selectors `1..=9` own compact index/value byte pairs, while selectors
  `>=0x0A` own the selector plus a fixed nineteen body bytes before returning
  to the live-object dispatcher. Those body bytes can begin with opcode-like
  pairs such as `D/5`, so the generic transport scanner now uses the
  decompile-backed partial appearance read end instead of splitting inside the
  fixed body table. Later typed-parser work on 2026-05-31 promoted this
  selector branch into the structured appearance model/writer; later equipment
  work promoted the counted `A/D/U` delta rows too.
  Verified with `cargo test -q -p hgbridge-proxy2
  declared_length_capacity_keeps_full_selector_partial_creature_appearance_together
  -- --nocapture`.
- 2026-05-31 `P/05/01` partial creature appearance typed-parser audit:
  promoted decompile-backed non-full `P/5` body deltas and zero-count
  equipment deltas from transport-only guards into the structured appearance
  parser/writer. Diamond `sub_448E30` and EE `sub_14077FE10` both read no
  creature-name BOOL unless mask `0x0400` is set; both then read scalar fields,
  the `0x0100` body selector (`0`, `1..=9`, or `>=0x0A` fixed nineteen-part
  table), optional `0x2000`, legacy-skipped/EE-read `0x4000`, and the `0x0200`
  equipment count. The writer now inserts EE build-0x23 high bytes for scalar
  `0x0080`, compact body-delta values, and fixed body tables, inserts the EE
  build-0x0E tail byte before a
  zero equipment count, and validates the exact EE cursor without inventing a
  name selector for no-name masks. Unresearched non-full mask bits still
  quarantine; a later 2026-05-31 audit proved mask `0x8000` is an ignored
  zero-payload bit. Verified with `cargo test -q -p
  hgbridge-proxy2 partial_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 appearance -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 declared_length_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`, `cargo check -q -p
  hgbridge-proxy2`, and full serial `cargo test -q -p hgbridge-proxy2 --
  --test-threads=1`.
- ~~2026-06-04 `P/05/01` partial creature full-body selector EE-width audit:
  no packet behavior changed, but public coverage now proves the exact EE
  validator rejects shifted fixed-table high bytes for non-full body-delta rows.
  Diamond `sub_448E30` reads selector `>= 0x0A` followed by nineteen body-part
  BYTEs; EE `sub_14077FE10` keeps the selector byte but reads those nineteen
  values as WORDs under the build-0x23 gate. The writer inserts zero high bytes
  after each value, and the exact verifier now rejects a nonzero high byte so a
  byte-plausible shifted body table cannot move following appearance/equipment
  fields. Verified with `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target-codex-verify/run-20260604-partial-body-highbyte cargo test -q -p hgbridge-proxy2 partial_body_delta_full_selector_survives_ee_widening_without_name_bits -- --nocapture`.~~
- ~~2026-05-31 `P/05/01` compact partial creature body-delta EE-width audit:
  fixed the structured non-full `P/5` body-delta reader/writer for selector
  counts `1..=9`. Diamond `sub_448E30` reads selector count, then BYTE
  body-part index + BYTE value pairs. EE `sub_14077FE10`, after the
  `ServerSatisfiesBuild(0x2001,0x23,0)` gate, keeps the same selector/index
  order but reads each value as a WORD. The proxy-owned EE dialect now inserts
  zero high bytes after each compact value and the exact verifier rejects
  nonzero high bytes, so later appearance/equipment fields cannot silently
  shift by one byte per pair. Verified with `cargo test -q -p hgbridge-proxy2
  partial_body_delta_compact_selector -- --nocapture`.~~
- ~~2026-05-31 `P/05/01` ignored high-mask creature appearance audit:
  corrected the non-full `P/5` mask contract for `0x8000`. Diamond
  `sub_448E30` and EE `sub_14077FE10` both read the appearance header and have
  no payload branch for the high bit: Diamond proceeds from the `0x2000`
  WORD+DWORD tail to equipment `0x0200`, while EE proceeds from the
  build-gated `0x4000` byte to equipment `0x0200`. The structured parser,
  EE writer, exact verifier, and live-object transport preflight now model
  `0x8000` as zero-byte / zero-BOOL owned state, including combinations with
  body and zero-count equipment deltas. Verified with `cargo test -q -p
  hgbridge-proxy2 partial_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 declared_length_ -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 appearance -- --nocapture`, and `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`.~~
- ~~2026-05-31 `P/05/01` non-full creature equipment-delta item-change audit:
  promoted nonzero `0x0200` equipment deltas from quarantine into the
  structured appearance parser/writer for the decompile-backed counted row
  list. Diamond `sub_448E30` and EE `sub_14077FE10` read the count byte and
  then `CHAR opcode + OBJECTIDServer + DWORD slot/field` per row; `A` shares
  the existing visible-equipment item body and active-property BOOL cursor,
  `D` is header-only and byte-exact across Diamond/EE, and `U` owns one status
  byte before EE's object visual-transform identity map. The live-object
  declared-length capacity preflight now spends nested non-full appearance
  fragment bits for all non-full masks, so a following `D/5` row cannot steal
  an equipment item's active-property BOOLs. Verified with `cargo test -q -p
  hgbridge-proxy2 partial_equipment_delta_nonzero -- --nocapture`, `cargo test
  -q -p hgbridge-proxy2
  declared_length_capacity_counts_nonzero_equipment_add_item_bits_before_delete_bits
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 partial_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 declared_length_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 appearance -- --nocapture`,
  `cargo test -q -p hgbridge-proxy2 live_object_update -- --test-threads=1`,
  `cargo check -q -p hgbridge-proxy2`, `cargo fmt --all --check`, and `git
  diff --check`.~~
- ~~2026-05-31 `P/05/01` non-full creature scalar-only appearance audit: no
  packet behavior changed, but public regression coverage now pins the
  decompile-backed scalar branch order for masks without body/equipment deltas.
  Diamond `sub_448E30` reads the name branch only when `0x0400` is set, then
  scalar mask bits in order `0x0001`, `0x0002`, `0x0004`, `0x0080`, `0x0800`,
  `0x1000`, `0x0008`, `0x0010`, `0x0020`, `0x0040`; EE `sub_14077FE10`
  preserves that order, with only `0x0080` widened from Diamond BYTE to EE
  build-`0x2001/0x23` WORD. Tests now prove scalar-only rows own no fragment
  BOOLs unless the name bit is present, that the name selector is consumed
  before scalar fields, that the widened high byte must be zero, and that
  stale-declared transport cannot steal a short scalar-only `P/5` row as CNW
  fragment storage. Verified with `cargo test -q -p hgbridge-proxy2
  partial_scalar_only -- --nocapture`, `cargo test -q -p hgbridge-proxy2
  scalar_only_creature_appearance -- --nocapture`, `cargo test -q -p
  hgbridge-proxy2 live_object_update -- --test-threads=1`, `cargo check -q -p
  hgbridge-proxy2`, `cargo fmt --all --check`, and `git diff --check`.~~
- ~~2026-05-31 `P/05/01` non-full creature tail-only appearance audit: no
  packet behavior changed, but public regression coverage now pins the
  decompile-backed order for a no-name/no-body/no-equipment partial mask with
  `0x2000`, `0x4000`, and ignored `0x8000`. Diamond `sub_448E30` owns only the
  `0x2000` WORD+DWORD read-buffer tail and no CNW BOOLs; EE
  `sub_14077FE10` then reads the build-gated `0x4000` byte before any
  equipment count, while `0x8000` remains zero-payload. Verified with
  `cargo test -q -p hgbridge-proxy2
  partial_tail_only_appearance_inserts_ee_feature_byte_without_fragment_bits
  -- --nocapture`, `cargo test -q -p hgbridge-proxy2 partial_ --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 appearance --
  --nocapture`, `cargo test -q -p hgbridge-proxy2 live_object_update --
  --test-threads=1`, `cargo check -q -p hgbridge-proxy2`, `cargo fmt
  --all --check`, and `git diff --check`.~~
- ~~2026-05-27 `P/11/03` client CharList RequestUpdateChar cursor audit:
  tightened the client-to-server character-list verifier so the byte-only
  `BYTE + CResRef(16)` body may have no tail or one `GetWriteMessage` empty
  cursor byte (`0b011xxxxx`) only. A tail that advertises fragment data bits,
  or a multi-byte tail, is no longer accepted as owned by this no-BOOL reader.
  Verified with `cargo test -q -p hgbridge-proxy2 client_char_list --
  --nocapture`.~~
- ~~2026-05-27 `P/11/03` strict ClientCharList validator reuse audit: fixed the
  stale strict-mode known-opcode helper that still accepted any single
  fragment byte after `RequestUpdateChar`. Both `VerifiedFamily::ClientCharList`
  and ordinary known-high strict validation now delegate to the focused
  client CharList parser, so only no tail or the empty `GetWriteMessage`
  cursor byte are owned. Verified with `cargo test -q -p hgbridge-proxy2
  strict_client_char_list_uses_focused_fragment_tail_owner -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2 client_char_list -- --nocapture`.~~
- ~~2026-05-27 `P/31/03` PlayModuleCharacterList response padding-bit cursor
  audit: tightened the response verifier so it owns only the decompiled result
  BOOL plus any success-branch locstring bits, then rejects nonzero unused
  fragment padding bits. EE `SendServerToPlayerPlayModuleCharacterListResponse`
  writes `CreateWriteMessage`, result `WriteBOOL`, OBJECTID `WriteDWORD`,
  two `WriteCExoLocStringServer` fields only on success, and then
  `GetWriteMessage`; `GetWriteMessage` stores only the high final-bit-count
  header. Public fixture-free tests now prove the failed-response one-BOOL
  shape and padding-bit rejection, while the private Starcore success fixture
  still claims exactly. Verified with `cargo test -p hgbridge-proxy2
  play_module_character_list -- --nocapture`, `cargo test -p hgbridge-proxy2
  char_list -- --nocapture`, `cargo fmt --all --check`, `git diff --check`,
  and `cargo check -q -p hgbridge-proxy2`.~~
- ~~2026-05-27 `P/31/03` PlayModuleCharacterList success-branch class-count
  audit: fixed the exact response validator to follow the EE client reader's
  `nNumClasses <= 8` ceiling instead of the ordinary three-class character
  limit. The same fixture-free coverage now proves the success branch owns the
  custom-portrait `CResRef(16)` only when the portrait WORD is `>= 0xFFFE` and
  rejects a missing branch payload. Verified with `cargo test -q -p
  hgbridge-proxy2 play_module_character_list -- --nocapture`.~~

Most likely packet families to audit:
- `P/04/01 Area_ClientArea`: static placeable rows and module-resource-backed
  repairs. Check appearance, static/trap flags, and orientation fields against
  the exact Diamond and EE area readers.
- `P/05/01 GameObjUpdate_LiveObject`: placeable add/update tails, object visual
  transform insertion, scalar orientation bits, and current-player creature
  appearance records.
- `P/11/04 CharList_UpdateCharResponse` and local BIC handling only if the
  player appearance is already wrong before the first live-object current-player
  update.

Proof required before fixing:
- Capture/compare local Diamond baseline, local EE bridge output, and ideally
  EE reader/writer decompile evidence for the exact field order.
- Add semantic assertions for at least one affected fixture: placeable
  appearance id, trap/static state, orientation/bearing, and player
  `Appearance_Type`/model must match the Diamond source intent after rewrite.
- Keep the fix bounded to the responsible family. Do not raw-passthrough,
  relax exact validation, or add broad appearance/trap heuristics.

# Active proxy2 issues

This is the working active-issues document for recurring proxy2 development.
Use it to leave concise notes on unresolved generalized protocol/state issues,
suspected packet families, evidence gathered, and next verification steps.
When an issue is confirmed fixed, mark it crossed off with the confirming
evidence and date or remove it from the active list. Specific modules, assets,
resrefs, captures, and chapters belong here only as evidence for a broader rule,
not as standalone workaround targets.

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

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
- 2026-05-27 `P/04/01` module-backed zero-appearance static-row audit:
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
- 2026-05-27 `P/04/01` static-placeable context proof audit: corrected the HG
  Docks zero-sound-count fixture expectation so absent local module proof stays
  absent. The test now supplies an explicit empty module context and proves that
  static-row context does not invent GIT trap/use/lock state when no module ARE
  resource is resolved; module-backed state remains reserved for rows uniquely
  matched to a proven local resource. Verified with `cargo test -q -p
  hgbridge-proxy2 docksofascension_rewrite_repairs_legacy_zero_sound_counts --
  --nocapture` and `cargo test -q -p hgbridge-proxy2`.
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
- 2026-05-27 `P/05/01` trigger add geometry cursor audit: no packet behavior
  changed, but public fixture-free coverage now proves the decompile-owned
  `A/7` geometry contract. Diamond `CNWSMessage::AddTriggerGeometryToMessage`
  and EE's matching trigger-add reader own the BYTE vertex count and complete
  XYZ FLOAT triples as read-buffer fields only; they consume no CNW fragment
  BOOLs, and a byte-complete add with any extra fragment bit remains
  unclaimed. Verified with `cargo test -q -p hgbridge-proxy2
  trigger_add_geometry -- --nocapture`.
- 2026-05-27 `P/05/01` door state update cursor audit: no packet behavior
  changed, but public fixture-free coverage now proves the decompile-backed
  `U/10` mask `0x10` state-BOOL handoff. Diamond `sub_44E2C0` owns five door
  state BOOLs; EE `sub_140797780` owns those same five in order plus one
  neutral sixth BOOL. The bridge rewrite must insert only that false sixth bit,
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
  `CharList_UpdateCharResponse` must carry the byte-only BIC body followed by
  the exact empty CNW fragment byte `0x60`; arbitrary post-BIC fragment storage
  is no longer treated as owned. Public fixture-free tests now cover the list
  response server-locstring bit cursor, padding-bit rejection, and update
  response empty-fragment handoff. Keep the broader player-model issue focused
  on live-object current-player appearance unless future evidence shows the
  BIC/CharList source fields are already wrong before the first `P/05/01`.
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

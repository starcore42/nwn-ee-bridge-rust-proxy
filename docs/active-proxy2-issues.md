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
- 2026-05-29 `P/1E/01` quickbar command-tail cursor audit: hardened the compact
  item recovery boundary so the trailing command-line compatibility tail accepts
  only the decompiled type-18 two-CExoString command shape with no suffix or a
  single zero DWORD empty-string-length artifact. It no longer discards an
  arbitrary four-byte read-buffer suffix after the two strings. Verified with
  `cargo test -q -p hgbridge-proxy2 quickbar_command_tail -- --nocapture`.
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
  `0xFFFFFFF7` tail9 converter. The proven Diamond-owned source cursor is
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
  through the compact decompile-owned source fragment cursor: position BOOLs,
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
  `ReadCExoString(32)`; EE item body reader `sub_14076BD30` uses the same
  selector before the next item-state BOOL. Tests now prove name-only updates
  own only the selector bits, combined name+hidden updates consume the hidden
  BOOL after the name branch, and terminal extra bits reject instead of being
  mistaken for the Diamond overflow check. The CEP v2.3 `U/6`
  handoff/terminal-tail capture remains active pending the final cross-record
  handoff or `U/9`/`W` terminal-tail proof; the low-`0x40` item tail is now
  recorded as unowned by the Diamond client reader.
  Verified with `cargo test -q -p hgbridge-proxy2 item_update_name -- --nocapture`
  and `cargo test -q -p hgbridge-proxy2 live_object_update -- --nocapture`.
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
- 2026-05-28 `P/04/01` static-row staged-repair audit: hardened the remaining
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
  --nocapture`.
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

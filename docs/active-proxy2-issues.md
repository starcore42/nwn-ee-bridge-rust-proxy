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
- 2026-06-07 `P/05/01` Diamond full item `U/6` mask audit
  (historical; later proof correction supersedes the `0x445160` server-writer
  attribution): fixed a generalized mask translation error, but do not cite the
  original `nwserver.exe` writer claim as evidence. The local checked decompile
  set is `C:\NWN\NWN Decompile\fullNwnDecompilePart1.txt` and `Part2.txt`; in
  those files the `0x445160`/`sub_444CC0` neighborhood is a Diamond client read
  handler, not a server writer. The retained rule is client-reader-backed:
  Diamond `sub_459700 -> sub_467AE0 -> sub_451AF0` reads the full item update
  through the name branch and has no source hidden-state BOOL for low `0x40`,
  while EE `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0` reads hidden state
  only for explicit EE-shaped mask `0x40` records. Therefore Diamond full item
  mask `0xFFFF_FFF3` still translates to EE `0x0008_0033`, drops low `0x40`,
  and must not consume the next source fragment bit as hidden state. This removes
  a post-name overconsume risk, but the two unowned bits before the CEP v2.3
  `U/10`/`A/6`/`U/6` sequence remain active; continue tracing the true source
  writer/handoff or local Diamond capture before changing cursor ownership.
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
  `U/10 mask=0xFFFF_FFF7` tail9 door row owns eight Diamond source bits
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
  `01110101100000`. After the decompile-owned `A/10`, `U/10`, and `A/6`
  rewrites, the real `U/6` cursor still selects vector orientation while the
  bytes are scalar-shaped; the translated item reader accepts only at
  `cursor + 2`. Those two leading bits therefore remain unowned rather than a
  license for cursor search/skip behavior. Continue tracing a real fragment
  owner or stream-boundary artifact before offset 104. Verified with
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

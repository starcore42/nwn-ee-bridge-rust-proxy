# Active proxy2 issues

This is the working active-issues document for recurring proxy2 development.
Use it to leave concise notes on unresolved generalized protocol/state issues,
suspected packet families, evidence gathered, and next verification steps.
When an issue is confirmed fixed, mark it crossed off with the confirming
evidence and date or remove it from the active list. Specific modules, assets,
resrefs, captures, and chapters belong here only as evidence for a broader rule,
not as standalone workaround targets.

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

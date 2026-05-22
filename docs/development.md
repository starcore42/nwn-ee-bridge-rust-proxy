# Development

## Build

Use the bundled Visual Studio Build Tools through the repository build wrapper:

```powershell
.\tools\build.ps1 -Configuration Release
```

Outputs are written to `build\Release\`:

- `hgbridge_launcher.exe`
- `nwncx_hg.dll`
- `hgbridge_proxy.exe`

## Standalone Proxy

The standalone bridge pivot has started in `src\proxy`. The current `hgbridge_proxy.exe` is a multi-client plain UDP middle process for the harnessed EE legacy path: it listens on a local EE-side endpoint, creates isolated per-client bridge state and HG-facing UDP sockets, expires idle sessions with `--session-timeout-ms`, relays to a selected HG 1.69 endpoint, logs BN/control/game packets, can rewrite client `BNVS` into Diamond's three-CD-key verifier layout, consumes EE-only client `Device_AdvertiseProperty` gameplay frames by forwarding an empty reliable payload with repaired packetized length/CRC, and preserves the proven startup `M...` replay behavior by queueing those packets until `BNVR A`. It now also recognizes EE `BNK0`-`BNK4` and the broader EE `BN*` control family from the local decompile, and can opt in to stock EE BNK/encrypted player packet handling with `--ee-crypto`. For packet alignment, it parses and verifies the decompile-backed `M...` reliable-window frame header, high-level `70 03/1` module-info, `70 30/1` quickbar, `70 36/1` device-property, and `70 05/1` live-object CNW envelopes, also accepting harness/direct `P` envelopes. Module-info strips Diamond's legacy hak/module block with `--rewrite-module-hak-list` and trims the Diamond-only post-area/name tail so EE's `CNWCModule::LoadModule` sees the area table and shared post-table fields it expects; NWSync advertisement is supplied through the byte-oriented `BNXR` control packet plus an EE-shaped `ServerStatus_ModuleRunning` module-resources rewrite in the decompiled `NWSync::Advertisement`, module name, description, hak-list order, and `--nwsync-root` serves a generated repository over local HTTP while logging the selected manifest metadata. Complete quickbar `SetAllButtons` payloads are inventoried at button level, stock EE `P 30/1` opcode streams are currently rewritten to 36 empty slots as an alignment-preserving fallback, complete or minimally fragmented hook-era non-item quickbars can be canonicalized with packetized-length and CRC repair using `--rewrite-quickbar-simple`, and complete or sequentially fragmented legacy item buttons can be translated to EE shape with `baseitems.2da` appearance sizing, legacy visual-transform padding for EE's ServerSatisfiesBuild-false LerpFloat path, active item property preservation, inline locstring-name flattening, active-property value-mask expansion, and repaired lengths/CRC. Live-object `5/1` packets are summarized by legacy submessage boundary/opcode/type/name/update-mask hints so the injected cursor repairs can be moved out deliberately. `--rewrite-live-object-visual-transform-masks` ports the update-mask side of the hook's empty visual-transform-map shim by clearing EE's `0x00100000` gated-read bit on complete or reassembled fragmented legacy `U` masks. `--rewrite-live-object-material-shader-params` ports the hook's no-consume empty material-param-count shim by clearing EE's `0x00200000` gated-read bit on those same records. `--rewrite-live-object-updates` ports the first update-record cursor repairs into packet space for complete and fragmented server-to-client live-object payloads: Diamond-style door/placeable/trigger update masks are translated, legacy scalar tails are stripped, door's EE-only sixth state bit is inserted, door/placeable absent-name-mode bits are inserted, and recognized short update names are flattened to empty EE strings. Creature associate update tails are diagnostic-only after the v0.5.237 decompile pass, because Diamond and EE both read the HG legacy-build `0x2000` branch as object id + WORD + two BOOLs. `--rewrite-live-object-item-appearances` loads `baseitems.2da` (`--baseitems` or `HG_BRIDGE_BASEITEMS_PATH`, with the staged Diamond 1.72 merged table auto-detected) and inserts the missing `0x72` zero-byte EE extended-armor/accessory table after complete or reassembled fragmented live-object armor appearances, then repairs declared length, packetized length, and CRC. `--rewrite-live-object-item-appearance-word-parts` expands legacy model-part bytes to EE WORD fields for the feature-0x23 item appearance path, and `--rewrite-live-object-item-appearance-visual-transforms` implies that WORD layout while appending the feature-0x23 empty visual-transform map (`DWORD 0`, `DWORD 0`) after live item appearances. `--rewrite-live-object-add-visual-transforms` inserts empty maps for legacy creature adds plus EE door/placeable add map reads, placing the door map before the name payload and the placeable map after the legacy appearance tail. `--rewrite-live-object-add-records` handles the first add-record bool/locstring packet transforms: door and placeable short locstring names are flattened to empty EE strings with the required fragment bits, the legacy placeable bit at EE's optional-target read site is consumed/dropped while EE receives a false optional-target BOOL, the final EE-only placeable light bit is inserted false, placeable direct-name contradictions are logged separately, and trigger add records are preserved after decompile-backed validation because Diamond and EE read the same trigger add shape.

Current trigger-add correction: Diamond `sub_4552E0` and EE `sub_1407B1670` read the same locstring/state/cursor/height/vertex shape, so the proxy must preserve trigger adds and only advance their verified fragment cursor. Earlier notes that say trigger adds are rewritten should be read as superseded by the packet-alignment reference.

Current placeable correction: v0.5.347 consumes/drops the legacy placeable add bit at EE's optional-target read site, inserts EE's optional-target BOOL as false, and shifts the seven following legacy state/action bits into EE's decompile-backed slots (`[+0x194]`, `SetUseable`, `trap_disarmable`, `lockable`, `locked`, unknown `[+0x1AC]`, and name-valid). Logs now distinguish this from the older absent-optional heuristic with `legacy_optional_gate_consumed=1` and still record per-placeable add/update `state_history` deltas. Keep using those labels to investigate remaining duplicate/red/bad-state placeables; do not suppress same-appearance static/live rows or force lock/trap/use bits without reader evidence.

Recent proxy work adds several stock-EE-facing shims that were still future work in the last review. The proxy now normalizes stock EE `BNCS` private-build/public-key shape toward Diamond 1.69 by default, quarantines 1.69 `BNDM`/`BNDP`/`BNDR`/`BNDS` packets that collide with EE direct-control handlers, can inflate/rewrite/re-deflate server gameplay frames marked with the reliable-window deflate flag, can insert EE player-list identity placeholders with `--rewrite-player-list`, and can patch `Area_ClientArea` plus synthetic LoadBar completion/optional `Area_AreaLoaded` messages with `--rewrite-area-client-area` and `--synthetic-area-loaded`.

It does not yet implement every EE-facing server behavior required by unmodified EE clients. BNK/encryption and NWSync advertisement/HTTP serving now have first-pass implementations, including the early `BNXR` NWSync advert expected by stock EE, but the generated NWSync repository must exist, packet alignment still needs broader coverage, and mixed reliable-window interleaving, remaining creature-tail/add shims, broader update locstring recovery, and broader CNW bit-cursor simulation still need packet coverage before it is stock-EE complete.

Run it with:

```powershell
.\build\Release\hgbridge_proxy.exe --listen 127.0.0.1:5121 --server 213 --packet-dump
```

See `docs\standalone-proxy.md` for the pivot map and `docs\client-hook-compatibility-inventory.md` for the injected-hook migration checklist.

See `docs\proxy-translation-design.md` for the translation rule: production proxy code should be keyed by decompiled reader shape, session state, or active resource tables, not by named fixture examples.

## Asset Preparation

The primary HG/1.69 asset staging path is now a real copied bundle inside the Steam EE install:

```powershell
.\tools\prepare-steam-asset-bundle.ps1 -Apply
```

Default bundle path:

`C:\Program Files (x86)\Steam\steamapps\common\Neverwinter Nights\hg-bridge-assets`

The bundle copies Diamond base `*.key`, `data`, `texturepacks`, `hak`, `tlk`, `override`, portraits, music/ambient, plus the HG GUI/standard, HG override/overlay, and CEP 2.3 setup packs. It intentionally does not copy `nwncdkey.ini` or account credentials. The bridge launcher passes this path as `HG_BRIDGE_ASSET_BUNDLE`, `HG_BRIDGE_DIAMOND_ROOT`, and `HG_BRIDGE_HG_ASSET_ROOT`; the bridge then mounts Diamond texturepacks and HAKs from runtime aliases rooted in this Steam-local bundle.

For stock EE proxy testing, turn the staged assets into an NWSync repository:

```powershell
.\tools\prepare-nwsync-repository.ps1 -Apply
```

That helper expects `nwsync_write.exe` from the NWSync/neverwinter.nim toolchain. It writes the repository under `C:\Program Files (x86)\Steam\steamapps\common\Neverwinter Nights\hg-bridge-assets\nwsync` by default and records the hash/URL values in `hg-bridge-nwsync.env`.

The test wrapper refreshes the Steam-local bundle by default:

```powershell
.\tools\test-hg-bridge.ps1 -Server 213
```

Use `-SkipAssets` after the bundle is already current. Add `-PrepareEeUserAssets` only if you deliberately want the older EE-user folder staging too; combine it with `-SkipAssetBundle` if you want only that older path.

EE can also look for local haks, tlks, and override files through the user directory aliases in:

`C:\Users\User\Documents\Neverwinter Nights\nwn.ini`

The older user-folder staging helper is still available:

```powershell
.\tools\prepare-ee-assets.ps1
.\tools\prepare-ee-assets.ps1 -Apply
```

That script defaults to directory junctions and is no longer the preferred path for live HG testing because the Google Drive workspace can be slow. Use the Steam-local bundle above for real tests.

On this machine, the current applied setup exposes:

- `69` `.hak` files
- `5` `.tlk` files
- `148` override files

## First-Stage Run

From the repository root:

```powershell
.\build\Release\hgbridge_launcher.exe --server 213
```

Or use the test wrapper:

```powershell
.\tools\test-hg-bridge.ps1 -Server 213
.\tools\test-hg-bridge.ps1 -Server 213 -Launch
```

Without `-Launch`, the wrapper builds, verifies asset staging, dry-runs the launch command, and runs the suspended injection test. With `-Launch`, it starts the real EE client and lets the bridge start the selected HG connection after EE reaches the main menu.

Use the launcher/test wrapper for live testing rather than clicking EE's Multiplayer button manually. The launcher stages the Diamond CD-key/player identity into the EE user directory before injection, disables EE's startup movies/intro splash, and the bridge seeds the EE client internals from that staged identity.

The test wrapper defaults to Diamond account `5`, sourced from `C:\NWN\Config\5.nwncdkey.ini` and `C:\NWN\Config\5.nwnplayer.ini` as used by `C:\NWN\launch5-druid.bat`. That profile is the Starcore5/no-spoken-character-password path, which keeps module/asset testing separate from HG's optional in-game password script. Override with `-DiamondAccount N` or launcher options `--diamond-account N --diamond-config-root PATH`.

For non-5121 HG servers, the launcher passes only the host to EE's `+connect` argument, never `HOST:PORT`. Passing `HOST:PORT` directly to `+connect` makes EE try to resolve the whole string as a hostname and can stop at `Could not translate address`. The launcher also passes the target through environment variables, writes the selected port to `C:\Users\User\Documents\Neverwinter Nights\settings.tml` under `[server.net] port`, and the bridge repairs EE's direct-connect/session peer port to the selected HG port.

If EE marks the server as passworded, the launcher passes `+password` and writes the Diamond profile password from `NWN Diamond\nwnplayer.ini` to both `server.login.player-password` and the legacy `nwn.ini` password fields. If that profile value is missing, it falls back to `a`. The bridge also seeds the direct-connect password field as a fallback. Override it with `--password VALUE`, leave movies alone with `--keep-movies`, or disable only the in-process password seed by setting `HG_BRIDGE_DISABLE_AUTO_PASSWORD=1` before launch.

HG account login is validated by player name and Diamond CD keys. The in-game "speak your password" prompt is a separate HG character-script feature, so bridge auto-speak is disabled by default. For a character that deliberately needs that spoken password, launch with `HG_BRIDGE_ENABLE_AUTO_SPEAK_PASSWORD=1` and provide the spoken value with `--password VALUE` or `.\tools\test-hg-bridge.ps1 -Password VALUE -AutoSpeakPassword`. CD-key-derived spoken-password candidates are disabled unless explicitly testing legacy behavior with `HG_BRIDGE_ENABLE_CDKEY_PASSWORD_CANDIDATES=1`; raw CD-key candidates require `HG_BRIDGE_ENABLE_RAW_CDKEY_PASSWORD_CANDIDATES=1`.

EE still defaults the direct session peer to `5121` on this path, so the bridge hooks `CNetLayer::StartEnumerateSessions` and repairs the `CNetPeer` port for the selected HG IP before forwarding to `CNetLayerInternal::StartEnumerateSessions`.

The default EE target prefers the real Steam install:

`C:\Program Files (x86)\Steam\steamapps\common\Neverwinter Nights\bin\win32\nwmain.exe`

That file is used before the Google Drive workspace copy because the synced location can be slow. If an older Sinfar-cleanup layout leaves `nwmain_org.exe` present, the launcher can still fall back to it.

Useful options:

```powershell
.\build\Release\hgbridge_launcher.exe --list-servers
.\build\Release\hgbridge_launcher.exe --server 211 --dry-run
.\build\Release\hgbridge_launcher.exe --server 213 --dry-run
.\build\Release\hgbridge_launcher.exe --server 211 --inject-test
.\build\Release\hgbridge_launcher.exe --server 213 --diamond-account 5 --diamond-config-root "C:\NWN\Config"
.\build\Release\hgbridge_launcher.exe --server 213 --password a --keep-movies
.\build\Release\hgbridge_launcher.exe --ee "C:\Program Files (x86)\Steam\steamapps\common\Neverwinter Nights\bin\win32\nwmain.exe" --server 411
```

For packet-flow debugging, the test wrapper can enable raw gameplay packet dumps:

```powershell
.\tools\test-hg-bridge.ps1 -Server 213 -Launch -PacketDump
```

Only use `-PacketDump` with disposable test input, because typed chat text can appear in the raw `M...` packet log.

Summarize the latest bridge trace without dumping raw packet/chat payloads:

```powershell
.\tools\summarize-hg-bridge-log.ps1
```

## Diamond Probe Harness

To compare the 1.69 client against EE, build and launch the Diamond probe harness:

```powershell
.\tools\build-diamond-probe.ps1 -Configuration Release
.\tools\test-diamond-client.ps1 -Server 213 -Account 5 -Launch
```

The launcher stages `C:\NWN\Config\<account>.nwncdkey.ini` and `C:\NWN\Config\<account>.nwnplayer.ini` into `C:\NWN\NWN Diamond`, starts Diamond suspended, injects `diamond_probe.dll`, and then resumes `nwmain.exe +connect <server>`.

The safe default pauses on Diamond's legacy `Player Login` prompt. Click `OK` there to let the direct-connect path proceed; the account `5` druid profile is the no-character-password test account. Probe logs are written to `C:\NWN\NWN Diamond\logs\hg_diamond_probe_*.log`.

Summarize the latest trace with:

```powershell
.\tools\summarize-diamond-probe-log.ps1
```

The probe logs Diamond's old `WSOCK32` ordinal network calls plus archive/asset opens such as `chitin.key`, `xp*.key`, `data\*.bif`, `texturepacks\*.erf`, `hak`, `tlk`, `override`, and `tempclient` paths. The experimental in-process auto-submit hook is disabled by default after it proved too early/fragile; opt in only for debugging with `.\tools\test-diamond-client.ps1 -Server 213 -Account 5 -Launch -AutoProfileOk`.

## Diamond Server Injection Harness

For local 1.69 server traffic, the Diamond probe DLL can also be injected into `nwserver.exe`. The server harness defaults to the small stock `bw167demo` module on port `5122`, keeping `5121` free for `proxy2` or the legacy proxy listener:

```powershell
.\tools\build-diamond-probe.ps1 -Configuration Release
.\tools\test-diamond-server.ps1 -Launch
```

That starts `C:\NWN\NWN Diamond\nwserver.exe` suspended, injects `diamond_probe.dll`, resumes the server with `-module bw167demo -port 5122`, and writes server-side socket/file traces to `C:\NWN\NWN Diamond\logs\hg_diamond_probe_server_*.log`.

For a non-running smoke test that only proves suspended injection succeeds:

```powershell
.\tools\test-diamond-server.ps1 -InjectTest
```

Point `proxy2` at the local Diamond server with an explicit host/port target:

```powershell
cargo run -p hgbridge-proxy2 -- --listen 127.0.0.1:5121 --server 127.0.0.1:5122 --packet-dump
```

Then connect a client to `127.0.0.1:5121` when testing through the proxy, or directly to `127.0.0.1:5122` when you want raw Diamond server behavior. Summarize the latest server-side trace with:

```powershell
.\tools\summarize-diamond-probe-log.ps1 -Server -IncludePacketPrefixes
```

The launcher starts EE suspended, injects `nwncx_hg.dll`, calls the DLL's exported `HgBridgeInit` inside the target process, then resumes the client.

The DLL writes its log next to the DLL as `nwncx_hg.log`.

`--inject-test` performs the same suspended injection and DLL initialization, then terminates the client before resuming its main thread. Use it to verify signatures and logs without entering the game.

## Current Scope

The current MVP installs HG-gated protocol compatibility patches and drives a host-only EE auto-connect flow.

Diamond-compatible `BNCS` is emitted directly for known HG endpoints. HG accepts Diamond's build field `0x0003`; Sinfar's observed `0x05F8` field is server-specific and is now opt-in only through `HG_BRIDGE_ENABLE_SINFAR_BNCS_FIELD_PATCH=1`.

Additional protocol patches currently force EE's `StartConnectToSession` onto the legacy BNCS path, seed the legacy connection-state with the real session connection id, and classify legacy `BN*` plus normal `M...` gameplay packets as plain UDP.

The receive-side classifier is patched too: EE's stock `CExoNetInternal::PacketRequiresEncryption` treats legacy `BN*` control packets as encrypted, so plain 1.69 `BNCR`/`BNVR` replies were previously dropped before `NonWindowMessages`.

`BNVS` is also emitted in Diamond format. Diamond sends three 40-byte CD-key verifier blobs, one for each Diamond CD key, plus a 32-byte community/name response. EE only generated one CD-key verifier, so HG rejected the verifier with `BNVR R 0x0C`. The bridge now captures the `BNCR` challenge, uses EE's `CExoEncrypt::EncryptString` with the three staged Diamond CD keys, and sends a Diamond-shaped 162-byte `BNVS`.

EE begins sending the first reliable `M...` session packets before the 1.69 `BNVR` accept arrives. Diamond sends those packets after verifier acceptance, and HG appears to ignore them when they arrive early. The bridge now gates those initial `M...` packets, reports success back to EE, and replays them immediately after `HandleBNVRMessage` processes an accepted `BNVR`.

After the character vault loads, EE's client-side module parser has to consume one 1.69-only module-load field block that EE 8193.37 no longer reads. Diamond reads a legacy hak count byte, that many 16-byte hak resrefs, one more 16-byte module resref, and then the resource-count DWORD. EE jumps straight from the custom TLK/module resref to the resource-count DWORD, so the bridge hooks `CNWMessage::ReadDWORD` at `CNWCModule::LoadModule` return RVA `0x7CD55F`, skips `1 + hak_count * 16 + 16` bytes, and then lets EE read the real resource count. On server `213`, this skipped `385` bytes for `23` haks and produced resource count `580`.

Diamond 1.69 and EE 8193.37 both consume the post-resource `BYTE + BOOL + BOOL` tail. EE then conditionally reads two more EE-only BOOLs when `ServerSatisfiesBuild(0x2001, 0x22, 0)` or `(0x2001, 0x23, 0)` succeeds. The bridge leaves the shared tail native, forces only those two feature checks false while `CNWCModule::LoadModule` is executing for a known HG endpoint, and logs `CNWCModule::LoadModule` / `LoadModuleResources` results and resource-list snapshots.

The latest diagnostic build also traces `CNWMessage` read cursors during module load, including message pointers, read-buffer pointers, fragment pointers, read offsets, remaining bytes, and raw unread/prefix bytes when EE reports overflow or underflow. Note that EE's `MessageReadUnderflow` means unread data remains, so this trace is meant to tell us whether the parser stopped early, consumed too much, or hit an asset follow-up failure.

Resource diagnostics are enabled for known HG endpoints too. The bridge logs `CExoResMan::Exists`, `GetResObject`, and hak/key-table file-add calls, with resrefs, resource types, result pointers, and module-load depth. Startup asset diagnostics also list final paths and counts for EE user assets, Diamond `hak`, `tlk`, `override`, `custom`, base `*.key`, `dialog.tlk`, `data\*.bif*`, and `texturepacks\*.erf`, plus the workspace EE `data` keys/BIFs. Current inject-test output shows `hak`, `tlk`, and `override` visible through the EE user folder junctions, but both EE and Diamond `custom` are missing.

NWN Explorer's decompile confirms the important base-content layout difference: Diamond enumerates root key files and `texturepacks\*.erf`, while EE uses `data\*.key` and `data\txpk\*.erf` plus EE-specific `data\...` roots. `tools\compare-nwn-resources.ps1` now inventories those KEY and ERF-family containers. The 2026-04-29 base comparison produced `build\resource-compare`: Diamond `121,783` resources / `92,216` unique, EE `113,582` resources / `113,538` unique, with `1,037` Diamond unique resources missing from EE before the bridge's Diamond archive mount. The largest missing groups were `NSS` (`278`), `NCS` (`267`), `UTI` (`143`), `MDL` (`88`), `TGA` (`84`), `PLT` (`61`), and `2DA` (`29`).

Live-object item appearance is another 1.69-vs-EE protocol split. Diamond's item-appearance reader consumes only the legacy base-item appearance payload: one DWORD base item, then type-specific model/color bytes, and returns. EE `8193.37` additionally reads a `0x72` byte armor/accessory table and a visual-transform map. On HG 1.69 packets those bytes are actually the next live-object submessage, which previously caused `0x79FD43`/`0x79FD59` overreads and black/module-stalled worlds. The injected bridge hooks the EE item-appearance reader at RVA `0x79FAC0` for live HG object updates, performs the Diamond-format parse, zeroes the EE-only armor table and visual-transform output, and returns success without advancing into the next submessage. The standalone proxy now has the packet-level equivalent for complete or reassembled live-object item appearances, including neutral legacy-build visual-transform padding behind `--rewrite-live-object-item-appearance-visual-transforms`. The proxy also ports add-record visual-transform reads by inserting ten neutral LerpFloat DWORDs into legacy creature, door, and placeable adds with `--rewrite-live-object-add-visual-transforms`; the door placement follows the EE reader's pre-name visual-transform read, while placeable placement follows the EE reader's post-appearance-tail visual-transform read. The companion `--rewrite-live-object-add-records` pass then normalizes known door/placeable/trigger add bool and short-locstring layouts, preserving byte alignment by editing the CNW fragment bits rather than inventing read-buffer bytes.

Trigger add records are the exception to that normalization wording: they are now treated as wire-compatible and preserved, with explicit geometry and bit-span validation.

Current controlled live result for server `213` on 2026-04-29:

- `207.246.92.7:5121` appears in EE logs as the Beamdog master/listing endpoint.
- HG server `213` resolves as `158.69.144.21:5133`.
- EE creates connection `1` to `158.69.144.21:5133` and discovers `Higher Ground (Party 2-3)`.
- HG accepts Diamond-style `BNCS` field `0x0003` with a full 73-byte `BNCR`.
- The bridge captures the `BNCR` challenge, sends a 162-byte Diamond-style `BNVS`, and receives a 9-byte successful `BNVR`.
- The bridge queues the three early reliable `M...` startup packets, replays them after `BNVR A`, then EE sends the 37-byte login/vault request.
- HG responds with the larger reliable `M...` payloads expected during character-vault loading.
- Character vault loads.
- Pressing Play now reaches `CNWCModule::LoadModule result=0x00000000`, loads `Path of Ascension CEP Legends`, and enters the live HG module/password gate.
- The controlled run did not create any new crash reports.
- With the legacy item-appearance hook installed, Starcore5 / `Starcore-Druid [6.0]` enters and renders `Docks of Ascension` with terrain, character, portrait, hotbar, and HG chat text. The successful run logged `15` legacy item-appearance parses and `0` hits at the former extended-armor overread sites `0x79FD43` and `0x79FD59`. The summary still shows non-fatal `CExoResMan::Exists` misses, mostly `TXI`, `2DA`, and `MDL`, but no demand-load misses. One later live-object overflow remains in a different parser path and should be investigated, but it no longer blocks initial entry/rendering.

Set `HG_BRIDGE_ENABLE_SINFAR_BNCS_FIELD_PATCH=1` before launching only when deliberately testing Sinfar's `0x05F8` BNCS field mutation. Set `HG_BRIDGE_DISABLE_VERSION_PATCH=1` to skip that opt-in patch even when requested.

Connection diagnostics and repair hooks can be disabled with:

```powershell
set HG_BRIDGE_DISABLE_AUTO_CONNECT=1
set HG_BRIDGE_DISABLE_AUTO_PASSWORD=1
set HG_BRIDGE_DISABLE_ADDRESS_LOG=1
set HG_BRIDGE_DISABLE_PORT_REPAIR=1
set HG_BRIDGE_DISABLE_SESSION_PEER_REPAIR=1
set HG_BRIDGE_DISABLE_LEGACY_CONNECT_PATCH=1
set HG_BRIDGE_DISABLE_LEGACY_CONNECT_STATE_PATCH=1
set HG_BRIDGE_DISABLE_LEGACY_BNVS_PACKET=1
set HG_BRIDGE_DISABLE_LEGACY_BNVS_STATE_PATCH=1
set HG_BRIDGE_DISABLE_LEGACY_M_REPLAY=1
set HG_BRIDGE_DISABLE_LEGACY_MODULE_HAK_LIST_SKIP=1
set HG_BRIDGE_DISABLE_LEGACY_MODULE_TAIL_VERIFY=1
set HG_BRIDGE_DISABLE_LEGACY_MODULE_FEATURE_GATE=1
set HG_BRIDGE_DISABLE_LEGACY_LIVE_ITEM_APPEARANCE_READ=1
set HG_BRIDGE_DISABLE_LEGACY_LIVE_VISUAL_TRANSFORM_SKIP=1
set HG_BRIDGE_DISABLE_LEGACY_LIVE_MATERIAL_SHADER_PARAM_SKIP=1
set HG_BRIDGE_DISABLE_MODULE_LOAD_DIAGNOSTICS=1
set HG_BRIDGE_DISABLE_BNCS_ENCRYPTION_PATCH=1
set HG_BRIDGE_DISABLE_NET_FLOW_LOG=1
set HG_BRIDGE_DISABLE_SENDTO_LOG=1
set HG_BRIDGE_DISABLE_DIAMOND_IDENTITY_SEED=1
set HG_BRIDGE_DISABLE_AUTO_SPEAK_PASSWORD=1
```

Deep module-read/resource diagnostics are opt-in because those hooks sit on hot EE paths during login and character-vault loading:

```powershell
set HG_BRIDGE_ENABLE_MODULE_READ_DIAGNOSTICS=1
set HG_BRIDGE_ENABLE_RESMAN_DIAGNOSTICS=1
set HG_BRIDGE_ENABLE_RUNTIME_RESOURCE_DIAGNOSTICS=1
set HG_BRIDGE_ENABLE_AUTO_SPEAK_PASSWORD=1
set HG_BRIDGE_ENABLE_CDKEY_PASSWORD_CANDIDATES=1
set HG_BRIDGE_ENABLE_RAW_CDKEY_PASSWORD_CANDIDATES=1
```

Model/body hooks are deferred because they appear likely to be Sinfar-specific.

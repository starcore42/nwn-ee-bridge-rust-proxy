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

Latest known live HG status, as of 2026-07-01 08:45 +10: the current
gameplay-reaching capture is
`C:\nwnbridge\codex-diamond-fresh-autoplay-20260701-0632`, with packet dumps
under `diamond-client-packets`, probe log `diamond-client-probe.log`, 165
packet files, and packet window
`2026-07-01 06:32:44.905 -> 06:35:53.325 +10:00`. Gameplay was reached through
character/module entry and in-area HG gameplay messages including "You are now
in a No PVP area" and "You are now in Docks of Ascension (No PVP)." At the
latest live-data gate the newest packet was about two hours old. The strict
proxy2 replay
`C:\nwnbridge\codex-proxy2-replay-quickbar-committed-summary-automation-20260701-0849`
reported 0 quarantines, 223 strict allows, 79 captured direct live-object
frames, 14 exact live-object rewrite matches, 60 exact lifecycle claim
summaries, 10 area rewrites, 1 committed quickbar rewrite summary, 0 quickbar
item buttons, 29 blank quickbar slots, 5 spell slots, 2 preserved general
buttons, 0 general blanks, 0 unsupported blanked slots, 0 strict quarantine
decisions, and 0 fixed-width/live-object residuals. Inventory Feature-25
evidence in this shorter fresh capture showed 13 typed Feature-25 claims, 6/6
first-list refs materialized, 1/11 second-list refs materialized, 10/11
second-list refs not yet materialized at reference time, 33 second-list BOOL
bits, and 0 legacy-tail refs. Quickbar rewrite logs now mark stream-splitter
probes separately from committed translation with `trace_role` and `committed`;
the replay summary intentionally aggregates only committed quickbar rewrites.
The current production rule keeps Feature-25 refs as deferred item-context
proof, clears GUI-materialized and Feature-25 quickbar item proof when an exact
`D/06` live-object delete is observed, and reports true type-0 blank quickbar
slots separately from item/spell/general slot buckets.

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

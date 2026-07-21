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

### EE-through-proxy gameplay gate

The qualifying live gate is an EE client driven through the Release proxy into
the real HG endpoint. It assumes a working EE Steam install (override with
`-SteamRoot`), built or buildable Release bridge/proxy binaries, a local Diamond
account/CD-key profile below `-DiamondConfigRoot`, an existing character named
by `-AutoCharacter`, and either a usable native NWSync path or a local untracked
NWSync environment/cache seed. Never put an account key or real password in the
repository or command transcript; load it from a local secret source.

```powershell
$env:HG_BRIDGE_SERVER = '<host-or-ip>:<port>'
$hgPlayerPassword = '<load-from-local-secret-source>'
.\tools\test-hg-bridge.ps1 `
  -Server $env:HG_BRIDGE_SERVER `
  -Configuration Release `
  -DriverOnly -AllowDriverAutoConnect `
  -DiamondAccount 5 -DiamondConfigRoot 'C:\NWN\Config' `
  -SteamRoot 'C:\Program Files (x86)\Steam\steamapps\common\Neverwinter Nights' `
  -AutoCharacter 'starcore-druid60' `
  -Password $hgPlayerPassword -AutoSpeakPassword `
  -SeedNwsyncClientCache -NwsyncEnv 'C:\nwnbridge\hg-bridge-nwsync.env' `
  -ProxyLogRoot 'C:\nwnbridge\<descriptive-live-run>'
```

`-Server` is an endpoint, not a server-list number. The expected ordered
milestones are BN enumerate/login (`BNES`/`BNER`), key/build/session setup
(`BNK2`/`BNK3`/`BNK4`/`BNCS`), a validated character vault, typed character
selection, `PlayModule`/`Module_Info`, `Module_Loaded`, `Area_ClientArea`, a
native client `Area_AreaLoaded`, and then continuing exact live-object traffic.
Gameplay is reached only after module load plus native area completion and
sustained post-area gameplay packets; reaching BN or the vault alone does not
qualify.

Each invocation creates
`<ProxyLogRoot>\harness-proxy-<timestamp>\proxy.structured.log`,
`proxy.stdout.log`, `proxy.stderr.log`, `quickbar-item-refresh-hint.json`, and
`quarantine\` when diagnostic artifacts are written. The launcher prints the
driver/client log locations. Record the full run root, timestamps, last
milestone, post-area duration, strict decisions, exact live-object count,
quarantine contents, and stderr status.

Diamond direct capture is a separate legacy truth source used when the 1.69
wire behavior is the question; it does not by itself satisfy the EE-through-
proxy gameplay gate below.

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

Latest gate audit (`2026-07-21T22:05+10:00`): the newest gameplay-reaching
artifact remains
`C:\nwnbridge\codex-live-freshness-ack-lane-20260721-0722\harness-proxy-20260721-072045\proxy.structured.log`,
last written `2026-07-21T07:23:58.6405077+10:00` and 14 hours 41 minutes old. No
newer live proxy attempt exists. It selected typed `starcore-druid60`, reached
`Module_Loaded`, produced two native `Area_AreaLoaded` messages, accepted 76
exact live-object packets, and continued for 137.813 seconds after the final
area load. It recorded 449 strict allows and zero route conflict, quarantine,
`BNDP`, ERROR, quarantine files, or stderr; the single WARN declares the
pre-seeded NWSync cache. It therefore remains current gameplay evidence and no
fresh HG login was required.

Current-code strict replay is
`C:\nwnbridge\codex-proxy2-replay-pending-drain-final2-20260721-232328`: all 164
packet files produced 304 strict allows and 97 exact live-object claims, with
zero strict quarantines, quarantine files, output timeouts, route conflicts,
errors, or stderr. The focused root M-frame suite passes all 54 tests.
Production check/build, formatting, and the native Release build pass. Pending
session drains and direct due-packet piggyback now carry ACK, queue, semantic,
dispatch, sequence, and window effects through final strict validation. Active
module/area gates retain pending packets in their typed queue; placement-aware
gate preview keeps a source-created `LoadBar_Start` after its
`Area_ClientArea`, and direct source rejection restores both source and suffix
effects. Deflated completion uses the same pre-source transaction and restores
the partial source window if the complete reconstructed batch is rejected.

Latest known live HG proxy status: after the prior artifact crossed 24 hours,
the first current-code refresh exposed and then fixed a completed-stream
ACK-control route collision. The qualifying rerun is
`C:\nwnbridge\codex-live-freshness-ack-lane-20260721-0722\harness-proxy-20260721-072045`,
with structured log
`C:\nwnbridge\codex-live-freshness-ack-lane-20260721-0722\harness-proxy-20260721-072045\proxy.structured.log`
(last write `2026-07-21T07:23:58.6405077+10:00`). It selected typed
`starcore-druid60`, claimed `Module_Loaded`, produced two native
`Area_AreaLoaded` messages, and sustained 76 exact live-object accepts through
137.813 seconds after the final native area load. The run recorded 449 strict
allows with zero completed-route conflicts, strict/datagram quarantine,
quarantine files, `BNDP`, ERROR, or stderr. Its only WARN is the declared
pre-seeded NWSync cache. This is current gameplay evidence for the 24-hour gate.
It did not issue a door `UseObject` or configure a private v2 terminal-writer
journal, so the exact terminal proof path remains to be exercised against the
live sequence-95
interaction failure.

The `2026-07-21T10:04+10:00` gate rechecked that structured log at about 2 hours
41 minutes old, so no new HG login was required. Production now persists exact
raw reliable successors withheld by recognized quickbar/live-object/zero-fill
stream helpers in a bounded session queue. Dequeue and ordered-fence advancement
occur only after the outer strict validator accepts the completed emit; strict
rejection retains the raw event, exact retransmits refresh ACK/CRC state, and
conflicting or over-capacity merges fail transactionally. The decompile-backed
lane rule is now consistent through reassembly, coalesced shifting, hold gates,
and replay collapse: type-0 data wraps through sequence zero, while type-1 and
type-2 controls bypass at any sequence. Focused tests and both Release builds
pass. Current-code strict replay is
`C:\nwnbridge\codex-proxy2-replay-raw-successor-20260721-1045`: all 164 packet
files produced 304 strict allows, 143 generated ACK controls, 97 exact
live-object claims, 19 exact rewrites, ten Area rewrites, and one stable sealed
journal load, with zero strict/semantic quarantine, quarantine files, rewrite
failures, terminal residuals, output timeouts, warnings, errors, or stderr. The
next production boundary must stage direct semantic/synthetic/sequence effects,
then persistent-inflater completion state, until the same final validation
callback commits them.

The `2026-07-21T04:01+10:00` gate inspected that artifact at about 21 hours old;
it remained the newest live HG attempt and still satisfied the gameplay
freshness requirement. Exact Diamond and EE reader tracing disproved the prior
"primary deflated continuation plus packetized trailing records" hypothesis.
Their count-greater-than-one paths assemble one compressed member from the
complete stored first frame and every continuation's complete bytes after the
12-byte header; only count-one storage walks bytes 10..11 as queued-record
lengths. Proxy2 now follows that two-mode contract, keeps multi-frame stored
bytes out of coalesced parsing and exact in route identity, and fails closed on
count-zero appended storage. Focused dispatch/reassembly/coalesced tests pass.
Current-code strict replay is
`C:\nwnbridge\codex-proxy2-replay-full-storage-final-20260721-0501`: all 164
packet files produced 304 strict allows, 143 generated ACKs, 97 exact live-object
claims, 19 exact rewrites, ten Area rewrites, and one stable sealed-journal load.
Strict/semantic quarantine, quarantine files, rewrite failures, terminal
residuals, output timeouts, and stderr were zero. The next transport slice is the
session-owned raw successor queue for recognized stream helper families.

The `2026-07-20T21:59+10:00` gate inspected that artifact directly at about
14.98 hours old and found no newer failed live HG attempt. The current
production slice orders a direct reliable successor behind its incomplete
deflated predecessor: raw bytes and transport identity remain staged with zero
semantic effects, and only the first contiguous exact direct event commits
after an ordinary successful predecessor. Failed predecessors commit no Area
state; gaps, later/full-pipeline events, stream-helper events, and cache replay
successors remain unacknowledged for reliable retransmission. A bounded source
fence prevents a repeated future direct event overtaking a missing sequence and
includes type-0 data across `0xFFFF -> 0x0000`. Source CRC is now verified before any
reliable/semantic state mutation, active reassembly gates
coalesced dispatch, and type-1/type-2 control frames continue immediately at any
sequence. Focused Area, CRC, gap/retry,
control-lane, direct replay, reassembly, and coalesced regressions pass. The
remaining stream-family successor queue, exact completed-window cache identity,
primary-continuation/coalesced-trailing split, expanded primary-Area sequence
placement, and final-validation rollback are
tracked in `docs/active-proxy2-issues.md`.

Strict replay
`C:\nwnbridge\codex proxy2 replay ordered final 20260720-2327`
processed 164 files with 304 strict allows, 143 generated ACKs, 97 exact
live-object claims, 19 exact rewrites, ten Area rewrites, and zero quarantine,
rewrite failures, terminal residuals, output timeouts, or stderr. The configured
5,825-byte terminal journal loaded once and its length, timestamp, and SHA-256
were unchanged before and after replay.

The `2026-07-20T09:56+10:00` follow-up gate inspected that artifact directly;
its structured log was about 2.90 hours old and no newer live HG attempt
existed. Production now preserves an `Area_ClientArea` rewrite summary across
the standalone direct reliable-frame route, matching the existing
deflated/coalesced state handoff. Direct rewrites install the new
static-placeable context before later live-object reconciliation and queue the
exact one-frame Area side effects. A bounded exact cache keyed by original
sequence, reliable-origin generation, and complete source payload replays only
the rewritten wire packet with refreshed transport fields, so retransmits do
not repeat semantic Area observation, object-registry reset, LoadBar packets,
or sequence shifts even after the post-Area gate closes. Trailing records are
rejected before direct dispatch and cannot alias a cache entry.

Interleaved direct Areas are deliberately consumed while a deflated reassembly
is pending; emitting them without an ordered typed state commit would retain
stale Area context. The active next implementation is a reassembly transaction
that carries the typed Area event and commits it only after the preceding
deflated window translates successfully.

Current-code strict replay is
`C:\nwnbridge\codex proxy2 replay direct sealed final 20260720-1122`. It used a
stopped 5,825-byte v2 journal and deliberately spaced run/journal paths, proving
the native argument quoting and immutable-snapshot harness path together. It
processed all 164 source packets with 304 strict allows, 143 generated ACKs,
97 exact live-object claims, 19 exact rewrites, ten Area rewrites, both Area
contexts, one successful journal load, and identical before/after journal
length, UTC write time, and SHA-256. Strict/semantic quarantine, quarantine
files, rewrite failures, terminal residuals, output timeouts, and stderr were
zero. This baseline has no terminal quarantine artifact, so the loaded journal
correctly had no payload-selection opportunity.

The refresh was mandatory because the prior gameplay-reaching artifact had
aged past 26 hours. Its first attempt,
`C:\nwnbridge\codex-live-freshness-20260719-0128\harness-proxy-20260719-012352\proxy.structured.log`
(last write `2026-07-19T01:26:06.6136302+10:00`), reached typed character
selection, module load, native `voyage` area completion, and exact live-object
traffic, but then stalled on a second Area packet that combined missing height
with three legacy zero-count/single-resref sound rows. After the height proof
failed on the still-legacy tail, the sound normalization committed without a
height retry; EE acknowledgement stopped and HG ended the run with
`BNDP CE 16 00 00`. Production now composes those independently exact repairs
transactionally, while the captured fixture and final EE reader prove the full
byte and MSB-first fragment cursors. Strict replay
`C:\nwnbridge\codex-proxy2-replay-area-compose-20260719-0155` retained the
164-packet baseline with 304 allows, 97 exact live-object claims, 19 rewrites,
ten area rewrites, both area contexts, 143 generated ACKs, empty stderr, and
zero quarantine, rewrite failures, or terminal residuals.

Current production evidence retains the sequence-95 immutable source record
end at read buffer `229..229` plus fragment `63..76` (13 MSB-first bits,
`0x46`) and the
emitted contract at `243..243` plus `71..88` (17 bits,
`00100000001000110`, packed `0x4046`). Version 13 already has an exact EE token
for the separately materialized 259-byte candidate ending at cursor 71, but the
source token is still absent; claim, rewrite, cursor movement, fragment trim,
and wire bytes remain unchanged.

The strongest source reference is now the controlled stock Diamond writer plus
direct disassembly. `0x445160` can write a type-5 prelude before its canonical
row, so the probe captures owner-begin at the exact guarded
`WriteBYTE('U', 8)` return `0x4451E7`. `WriteOBJECTID` at
`0x508CC9..0x508CE1` preserves `0x7F000000` and otherwise ORs `0x80000000`;
the producer validates the emitted wire id rather than the raw server object id.
After the typed owner returns, the stock outer writer at
`0x44006C..0x44008C` appends three bytes (`0x57`, selector, `0x0E`) before
`GetWriteMessage` without advancing the MSB-first fragment cursor. Owner-end and
list-handoff are therefore distinct phases.

Final controlled run
`C:\nwnbridge\local-diamond-baseline-20260719-174343` produced
`terminal-writer-v2.tsv` (5,825 bytes, 18 lines, three complete blocks). Its
trace 1 records owner `309/129 -> 353/143`, list handoff `356/143`, and final
`356/143`; the two type-5 variants also captured their exact post-prelude row
boundaries. All three blocks were appended, none was poisoned, and proxy2
loaded the finished journal successfully at
`C:\nwnbridge\codex-terminal-v2-load-20260719-1746`. The deployed HG custom
writer/list producer remains required to obtain a unique sequence-95 source
block and join it to the ready EE token.

Fresh strict replay
`C:\nwnbridge\codex-proxy2-replay-terminal-ee-v13-20260719-20260719-112353`
processed all 164 source packets with 304 strict allows, 143 generated ACKs, 97
exact claims, 19 exact rewrites, ten Area rewrites, and both Area contexts. It
produced zero strict or semantic quarantines, quarantine files, rewrite
failures, terminal residuals, output timeouts, stderr, or leftover replay
processes/ports.

Writer evidence enters through one sealed bounded factory instead of a
caller-constructed observation. `--terminal-writer-trace` accepts one
private journal containing 1 through 64 immediately consecutive six-row v2
blocks. The whole journal is capped at 8,421,376 bytes and each decoded payload
at 524,288 bytes. There are no blank, comment, or separator rows. Duplicate
`(trace_id,message_id,component_sha256)` identities, partial blocks, malformed
UTF-8 or TSV/number/hex syntax, entry overflow, file overflow, malformed CNW
envelopes, invalid declared splits, non-update record offsets, and inconsistent
owner/list/finalizer cursors fail startup. If a relevant producer trace cannot
be appended or a cap is reached, the producer disables further writes and
attempts to append a reserved `incomplete` poison row through the retained
`CREATE_NEW` handle; that deliberately invalidates every earlier unique-looking
prefix without a path-reopen race. A stopped producer log containing
`poison marker failed` makes the journal unusable even if its prefix parses.

Within each block, the only admissible trace order is owner-begin, owner-end,
list-handoff, then finalizer. The factory validates the `P/05/01` envelope and
little-endian declared split, requires owner-begin plus the ten-byte update
header to fit within owner-end, requires monotonic owner-end to list-handoff,
and binds list-handoff and finalizer to the declared split. It decodes the CNW
valid-bit end, requires monotonic owner-end to list-handoff fragment cursors,
and binds list-handoff/finalizer to that end. The source residual is derived
only from owner-end to list-handoff, never from owner-begin to owner-end.
CNW writer byte counts include the seven-byte envelope and are normalized by
exactly seven; fragment coordinates already include the initial three CNW bits
and are not shifted. Exact packet correlation additionally requires the entire
finalized payload to equal the quarantined packet byte-for-byte. A digest, the
current probe's 32-byte `output_suffix`, or another partial match remains
fingerprint-only. Malformed envelopes, reordered/missing events, cross-packet
traces, identity/cursor/bit mismatches, different payload bytes, and inputs
above the existing 512-KiB live-object bound reject. The HG operator trace must
therefore persist the full finalized CNW message privately, not only the
suffix, and pair it with the writer/list bracket at `0x445160`/`0x507FC0` and
finalizer at `0x508B80`.

Each journal block is exactly six UTF-8 (no BOM) tab-separated rows in this
order; placeholders in angle brackets are values, not literal text:

```text
terminal-writer-trace\tversion\t2\ttrace_id\t<positive-decimal>\tmessage_id\t<16-hex>\tcomponent_sha256\t<64-hex>
owner-begin\ttrace_id\t<T>\tmessage_id\t<M>\tcomponent_sha256\t<H>\tabsolute_record_offset\t<decimal>\tabsolute_read_buffer_cursor\t<decimal>\tfragment_bit_cursor\t<decimal>
owner-end\ttrace_id\t<T>\tmessage_id\t<M>\tcomponent_sha256\t<H>\tabsolute_read_buffer_cursor\t<decimal>\tfragment_bit_cursor\t<decimal>
list-handoff\ttrace_id\t<T>\tmessage_id\t<M>\tcomponent_sha256\t<H>\tabsolute_read_buffer_cursor\t<decimal>\tfragment_bit_cursor\t<decimal>
finalize\ttrace_id\t<T>\tmessage_id\t<M>\tcomponent_sha256\t<H>\tabsolute_read_buffer_end\t<decimal>\tfragment_bit_cursor\t<decimal>
finalized-payload\ttrace_id\t<T>\tmessage_id\t<M>\tcomponent_sha256\t<H>\thex\t<complete-P/05/01-hex>
```

Blocks are concatenated directly with no intervening blank or separator row;
the normal final newline and CRLF line endings are accepted. `T`, `M`, and `H`
must repeat the block header identity exactly on every event, and the complete
identity tuple must be unique across the journal.
Writer byte coordinates are absolute within the finalized `P/05/01`, including
its seven-byte envelope: owner-begin record/read offsets are equal; owner-end
may precede list-handoff; list-handoff and final read offsets equal the
little-endian declared split. Fragment coordinates are the full MSB-first CNW
cursor, including the initial three valid-count bits; do not subtract three.
Owner-end may precede list-handoff, while list-handoff and final fragment
cursors agree with the valid-bit end derived from the complete payload. The
payload hex is even-length and contains every finalized byte, not a suffix or
digest.

Stop the producer and supply the finished journal before proxy startup. On
Windows, proxy2 opens the journal with read sharing only, so an extant producer
append handle or replacer causes startup status `writer-still-open`; it cannot
cache a valid-looking prefix while a later duplicate or `incomplete` poison
row can still arrive. After obtaining that deny-write/delete snapshot, proxy2
bounds, parses, and caches it once; later path changes are not observed. For each
quarantined source it first counts complete byte-for-byte payload matches.
Zero yields `selection_status=no-payload-match` with no selected identity or
source token. One yields `unique-payload-match` and only then runs the existing
record/cursor/bit requirement correlation; uniqueness alone does not mint a
token. More than one yields `ambiguous-payload-match` and fails closed before
requirement correlation, with no provenance or token even if one block would
otherwise match. Direct CLI use is:

```powershell
hgbridge_proxy2.exe --packet-dump --log C:\nwnbridge\<run>\proxy.structured.log --terminal-writer-trace C:\secure\terminal-writer-v2.tsv <normal proxy arguments>
```

Alternatively pass `-TerminalWriterTracePath C:\secure\terminal-writer-v2.tsv`
to `tools\test-hg-bridge.ps1`. A diagnostic destination is mandatory:
`--packet-dump` plus `--log`, or `NWN_BRIDGE_QUARANTINE_DIR`. Correlation is
written to the version-13 `.terminal.tsv` rows with artifact status, selection
status, journal artifact count, payload match count, verdict, selected trace
identity, and two-sided proof-join state. Journal selection and its opaque
source token remain diagnostic evidence only: they cannot claim, rewrite,
advance a cursor, trim a fragment, or mutate wire bytes. The file is private
operator evidence and must never be copied to the public repository. The
opt-in DiamondProbe v2 journal satisfies this contract for the audited stock
Diamond server; ordinary probe log suffixes do not.

Offline source-capture correlation uses the same option on the strict replay
harness:

```powershell
.\tools\replay-diamond-client-capture-through-proxy2.ps1 `
  -PacketDir C:\nwnbridge\<capture>\diamond-client-packets `
  -RunRoot C:\nwnbridge\<replay> `
  -TerminalWriterTracePath C:\secure\terminal-writer-v2.tsv
```

The harness resolves the existing leaf, verifies proxy CLI support, quotes the
complete native proxy argument line (including path values containing spaces),
records path/length/UTC-write-time/SHA-256 before and after replay, and fails if
any field changes. `replay-summary.json` retains that association plus journal-load,
terminal artifact selection, exact handoff/two-sided-proof, rewrite-count, and
owned/removed fragment-bit counters. Captures and private journals remain out
of both repositories.

The emitted final-claim observation is also subsystem-private. Sequence 95 now
earns one sealed EE token from the exact residual-removal candidate, but the
source owner is still unproven and the join remains `incomplete-source-proof`.
No packet, fragment cursor, trim, claim, rewrite, or terminal-claim registration
changed; the packet remains quarantined until the exact source journal joins
the EE proof and a dedicated writer is registered.

Strict replay
`C:\nwnbridge\codex-proxy2-replay-terminal-proof-20260718-141014`
processed all 164 packets with 304 strict allows, zero strict/semantic
quarantines or files, 97 exact live-object claims, 19 rewrites, zero rewrite
failures, zero terminal residuals, both area contexts observed, and empty
stderr. Seven writer-trace structural/cross-packet tests, the sealed emitted
final-claim branch matrix, and 27 terminal/tail9 tests pass. The next required
live test is still the door `UseObject` probe, after an actual HG server
writer/list trace has been correlated byte-for-byte and a typed EE writer
produces a final exact claim.

Server sequence 95 remains the active connection/gameplay failure: two
246-byte strict copies and one 270-byte diagnostic candidate were quarantined.
A corrected Diamond/EE decompile audit proves EE placeables consume five state
BOOLs, EE doors consume six, and both support the same mask-`0x00080000` name
grammar. Production now inserts the neutral sixth bit only for doors and
accepts exact named updates for either type. Terminal diagnostics compare
two non-mutating source interpretations at immutable source cursor 50 in the
76-bit fragment. The compact-tail9 reader ends at 59 and leaves 17 bits. The
exact stock Diamond candidate filters raw mask `0xFFFFFFF7` to reader-owned
`0x00080037` (recording ignored `0xFFF7FFC0`), then consumes position `50..52`,
scalar orientation `52..57`, five state BOOLs `57..62`, and a false direct-name
selector at 62; it ends at 63 and leaves 13 valid bits. Bounded production
failure evidence now enumerates exact Diamond reader candidates entirely inside
that unresolved suffix, requiring both an exact terminal byte walk and an exact
fragment-end handoff. Sequence 95 has one diagnostic-only direct-name candidate
at `63..76`, with no gap after the anchored reader; it does not move a cursor or
commit staged bytes. The same 13-bit span is `00 + source[40..50] + 0`, exactly
repeating the preceding `A/09` row's ten source bits in its middle. A 270-byte
speculative retry preserves that suffix while correctly advancing the fragment
count by its three inserted add-row bits, excluding transport count repair as
the explanation. Stock serializer `0x445160` ends the terminal row at cursor 63;
the continuous writer/finalizer cannot synthesize the suffix, and a later stock
row would require an absent 10-byte `U` header. Because all 13 bits are still
declared valid, do not trim them. Production diagnostics now correlate the
anchored residual against complete immutable source spans from the bounded
preceding ledger. The reduced stream identifies an exact same-object,
immediately preceding `A/09` replay from source `40..50` into residual
  `65..75`, with a two-bit prefix and one-bit suffix; a one-bit mutation rejects
  that exact candidate. Production now classifies this replay semantically only
  when the immediate same-object ledger row is `A/09`, its complete Diamond
  direct-name source span is ten BOOLs, the independently verified EE add span
  is eleven BOOLs (`+1/-0`), and the remaining suffix is exactly one false bit.
  The classifier and its machine-readable `terminal_semantic_replay` row are
  explicitly non-claiming and non-authorizing; a true one-bit suffix preserves
  raw correlation but rejects the typed envelope. Candidate count and ambiguity
  remain evidence only and cannot own or remove bits. Diamond `0x507F30` is the
  fragment-capacity growth helper, not the finalizer; the actual
  `GetWriteMessage` finalizer is `0x508B80`. Trace the predecessor handoff or an
  HG custom writer (or instrument `0x445160`, `0x507FC0`, and `0x508B80`), then
  require an exact final EE claim and rerun the live door `UseObject` probe.

The version-7 terminal artifact also records a typed reused-record reader
counterfactual for the exact end-aligned `63..76` interpretation. It is emitted
only when the stock and candidate readers match in object identity, masks, and
ordered relative field topology and widths; field values deliberately stay
independent so the evidence can narrow an owner trace without claiming replay.
The candidate must start exactly at the stock end, finish at the declared
fragment boundary,
and share an exhausted byte cursor. A second stock `U` record would require 10
header bytes but has zero available, so the artifact records
`second_stock_row_dispatch_possible=false`. It does not infer the writer:
`writer_replay_proven=false`, writer ownership is unknown, and claim, rewrite,
trim, and cursor authorization remain false.

Terminal artifacts also carry a bounded typed fragment-field provenance map.
For each stock or end-aligned Diamond reader walk it records the dialect,
object type/id, raw mask, field kind, exact source bit span/value, and matching
full probe cursor. Sequence 95's stock walk is position `50..52`, orientation
selector plus scalar low bits `52..57`, five state BOOLs `57..62`, and the
direct-name selector `62..63`; the sole end-aligned candidate repeats that
field order at `63..76`. The writer probe's cursor is the full fragment-vector
coordinate and already includes the three initial CNW message bits, so do not
subtract three when joining these artifacts. Every field row remains
`claimable=false` and `rewrite_authorized=false`.

Keep that provenance compact in retry state. The first implementation embedded
per-field bit arrays in every retained reader candidate and strict replay
`C:\nwnbridge\codex-proxy2-replay-terminal-field-provenance-20260717-1343`
overflowed the proxy thread stack at packet 122. Production now stores the
complete stock walk once as a packed 16-bit span and derives field rows only
when formatting; type-size tests guard against reintroducing nested arrays.

Diamond `sub_44EF00` calls `sub_4FBBA0` after every live-object row and loops to
read another 8-bit opcode whenever either read-buffer bytes or fragment bits
remain. EE `sub_14079BCE0` uses the same contract through
`CNWMessage::MessageMoreDataToRead`. In both clients, fragment-only residue at
the terminal row therefore triggers an opcode read from the exhausted byte
buffer; it is not legal padding. The version-7 terminal TSV records source
`245..245` plus fragment `63..76` and emitted `245..245` plus fragment `71..88`
as `fragment-only`, with `next_opcode_read_overflows=true` for both views.
Retain strict quarantine until the source writer/list owner is proven.

Production now keeps the terminal evidence model in
`live_object_update/terminal_evidence.rs` and applies one typed ownership gate
at the final exact-claim boundary. Only exact read-buffer and MSB-first fragment
cursor equality is claimable; a remaining fragment is
`fragment-writer-owner-unproven`, while cursor overruns are rejected explicitly.
The version-7 artifact and structured failure log expose the source and emitted
verdicts. Sequence 95 remains quarantined without changing any packet bits,
BOOL order, or cursor movement.

The final trim implementation now stores its five existing owner candidates in
the fixed typed claim set at `live_object_update/terminal_claim.rs` and resolves
them through one evaluator. Every registered owner must match the final cursor;
family-specific exact truncated-packet validators retain their prior scope and
run only under the reliable-residual gate. Unresolved tail9 has no direct claim
or registration path, while already-typed tail9 retains the existing generic
family completion path. Three claim-set tests, focused ownership/family-trim
coverage, all 29 `tail9` tests, `cargo check`, and strict replay
`C:\nwnbridge\codex-proxy2-replay-terminal-claim-set-20260717-225751` passed. The
replay processed 164 packets with 304 strict allows, 97 exact live-object
claims, 19 rewrites, and zero strict/semantic quarantines, quarantine files,
rewrite failures, terminal residuals, or stderr.

The supplied `Hgx.Server.dll` is not that owner. Exact host and import evidence
shows a Diamond `nwmain` client overlay: it has no socket send/receive imports,
no references to the server writer/list addresses, and its client-reader detour
at `0x455940` only writes named-pipe notification type `0x7D4` before resuming
the stock reader. No HG server or NWNX protocol component is present in the HGX
source tree. The next evidence target is the actual HG custom server binary or
a runtime server-side writer/list handoff trace.

A bounded local owner search found no such server component. The only other
writer-address-bearing binary was the generic Community Patch
`nwnx_patch.dll`; it was not loaded by the controlled harness, is not HG-specific,
and its stock-address references do not prove a hook or suffix owner. Obtaining
the deployed HG component or HG operator-side trace remains required.

The controlled stock-writer instrumentation is now available behind the
opt-in `-TraceServerWriter` server-harness switch (environment contract
`HG_DIAMOND_PROBE_SERVER_WRITER_TRACE=1`). The probe first requires the exact
checked Diamond `nwserver.exe` bytes and SHA-256, then brackets typed update
writer `0x445160`, captures the exact `WriteBYTE('U', 8)` row boundary at
`0x4451E7`, records every `WriteBOOL` call at `0x507FC0`, and snapshots the
owner-end, list-handoff, and `GetWriteMessage` finalizer phases. Set an explicit
private output with `-WriterJournalPath` (environment
`HG_DIAMOND_PROBE_SERVER_WRITER_JOURNAL`); otherwise the harness creates a new
`terminal-writer-v2.tsv` under the run root. Normal probe runs do not patch
these functions. Use either:

```powershell
.\tools\test-diamond-server.ps1 -SkipBuild -TraceServerWriter -Launch -Module bw167demo -Port <port>
.\tools\test-local-diamond-baseline.ps1 -SkipBuild -TraceServerWriter -ServerPort <port>
```

Journal preparation is lazy on the first exact outer runtime writer call, after
`DllMain`; hashing and `CREATE_NEW` file creation never run under loader lock.
Blocks are append-only and are not deduplicated. Stop the server before loading
the journal in proxy2. Never reuse an existing output path: the producer
preserves it and disables capture rather than truncating prior evidence.

Controlled Diamond-to-Diamond run
`C:\nwnbridge\codex-local-diamond-writer-delta-20260717-0734` entered the
module and captured 48 client packets. Its server artifact
`C:\NWN\NWN Diamond\logs\hg_diamond_probe_server_20260717-073411_modbw167demo_port64724.log`
traced six stock `0xFFFFFFF7` door/placeable updates in one message. The final
placeable update ended with read-buffer byte count 353 and fragment cursor
143; the finalizer began at byte count 356 and the same fragment cursor 143
(`tail_byte_delta=3`, `tail_fragment_delta=0`). Intermediate row helpers did
write BOOLs between earlier updates, so the new caller/cursor trace is needed
instead of assuming every inter-row span is empty. The exact terminal stock
handoff nevertheless adds no fragment bits and therefore does not own HG
sequence 95's declared `63..76` suffix.

Current-code strict replay
`C:\nwnbridge\codex-proxy2-replay-terminal-claim-set-20260717-225751`
processed all 164 packet files with 304 strict allows, zero strict/semantic
quarantines or files, 97 exact live-object claims, 19 live-object rewrites, and
zero terminal live-object residuals on isolated ports 65421/65433. Its stderr
was empty, and all 29 focused `tail9` tests passed. Some older private
capture-only exact-claim tests now reject under the corrected five-bit
placeable reader; keep those streams quarantined until their real source
writer/handoff owns the residual bits.

The earlier zero-quarantine account-5 capture
`C:\nwnbridge\codex-live-freshness-account5-20260715-0855\harness-proxy-20260715-084947`
remains the current inventory-response reference: it reached native
`Area_AreaLoaded`, admitted the frame-local 52-record materialization at raw
peer ACK 81, completed as `materialized_current_player_inventory`, and ended at
`2026-07-15T08:53:19+10:00` with zero quarantine files.

Current-code strict replay
`C:\nwnbridge\codex-proxy2-replay-tail9-evidence-20260716-0310`
processed the 164-packet Diamond gameplay set with 304 strict allows, zero
strict or semantic quarantines/files, one exact 36-slot quickbar, two area
context checks, and zero terminal live-object residuals. It used isolated
replay ports 60221/60233.

Area translation performance is now anchored by the same strict 164-packet
Diamond replay used for the current two-area regression. Profiling proved that
the remaining 14.26-second `voyage` and 8.55-second `docksofascension` delays
were repeated full ARE/GIT expansion during module-resource discovery. The
production lookup now uses the bounded ERF key table and IFO area-order list as
an exact-resref prefilter before retaining the full identity, ambiguity, tile,
static-row, packet-reader/writer, and cursor validations. Current-code replay
`C:\nwnbridge\codex-proxy2-replay-area-index-final-20260715-1230` reduced the
context-to-rewrite intervals to 0.489 and 0.113 seconds, with 304 strict allows,
zero quarantines or files, 10 area rewrite summaries, two area context checks,
one 36-slot quickbar, and zero terminal live-object residuals. Packet bytes and
bit order are unchanged.

Current production code routes structurally valid CNW-declared quickbars to the
exact direct reader before source-form normalization in both gameplay boundary
discovery and committed materialization. Zero-declared prefixed fragment forms
still normalize first. This is retry ordering only: the exact 36-slot byte
reader, MSB-first fragment cursor, typed writer, and EE validator remain
mandatory. Strict replay
`C:\nwnbridge\codex-proxy2-replay-direct-quickbar-complete-20260713-2110`
processed 164 packet files with 304 strict allows, zero quarantine decisions or
files, one 36-slot quickbar profile, and zero terminal live-object residuals.

The pre-fix recurrence
`C:\nwnbridge\codex-live-bard50-bounded-20260714-0245\harness-proxy-20260714-024358`
reached native gameplay with zero quarantine files, then stalled after a
651-byte live-object candidate. Production now bounds a nonzero exact-validator
reject to the last exact-owned source-record boundary and shares the resulting
candidate set between the special appearance/update proof and ordinary repair
loop. The captured dispatcher call fell from more than a minute to 1.87 ms;
offset-zero failures still use the general decompile-backed search. The current
03:12 capture above live-confirms continued traffic past that point.
Strict replay `C:\nwnbridge\codex-proxy2-replay-exact-prefix-20260714-0321`
processed 164 files with 304 strict allows, zero quarantine decisions/files,
and zero terminal live-object residuals.

Current inventory-harness finding: bounded pending-claim reconsideration is
live-confirmed by the 06:03 capture above. Its final hint reports three
ClientGui handoff events (two blocked, one ready), one handoff state update, one
queued status packet, two response-window live-object packets, and one
26-record materialized response. EE's exact minor-1 handler reads the open BOOL
and current-player inventory OBJECTID only; the request cannot carry diagnostic
ready candidate `0x800164E8`. Production therefore completes that exact update
window only after the legacy server acknowledges its synthetic reliable request
and a nonempty typed live-GUI materialization arrives; it reports
`materialized_current_player_inventory` separately from candidate association.
Candidate containment is still required before replaying any retained server
Inventory claim. Focused tests and strict replay
`C:\nwnbridge\codex-proxy2-replay-status-completion-summary-20260714-0904`
passes with zero quarantines and exports completion `none` for its no-request
baseline.

The same capture proves the transport boundary for that request window. Proxy2
sent synthetic client reliable sequence 82. A generic live-object packet then
arrived with raw server ACK 81; the 26-record materialization arrived with raw
ACK 82. Current production code preserves that peer-facing ACK before
unshifting it to EE's sequence space, ignores live-object packets until the
server ACK covers the exact synthetic request, and exports acknowledgement plus
pre-ACK exclusion counters. Each typed response snapshot also retains its own
raw peer-facing ACK beside the EE-unshifted ACK, and equal-strength response
selection uses wrapping reliable order. Wrapping sequence arithmetic, one-shot
ACK ownership, raw-ACK response serialization, and the live-shaped raw-82 / EE-80
pair have focused coverage. Strict replay
`C:\nwnbridge\codex-proxy2-replay-status-ack-gate-final-20260714-1205` processed 164
packets with 304 strict allows, zero quarantines/files, and zero terminal
live-object residuals. Current-code replay
`C:\nwnbridge\codex-proxy2-replay-response-peer-ack-20260714-1515` repeated those
counts and exported zero for both new raw-response ACK fields in its no-request
baseline. The next credentialed HG run must report one raw sequence-82
acknowledgement, one excluded raw-ACK-81 live-object packet, a materialized
response whose own raw ACK is 82, and
`materialized_current_player_inventory`; the current automation environment
again exposed no account secret, so no password was guessed.

Current production also requires the materialization packet's own raw ACK to
cover the exact synthetic status sequence. A historical ACK 82 elsewhere in
the session cannot admit a reordered or retransmitted live-object packet whose
current raw ACK is 81. The accepted snapshot and the admission decision use the
same wrapping-order raw ACK; the proven ClientGuiInventory BOOL/OBJECTID body
and fragment cursor are unchanged. Focused tests cover the live ACK 81/82
window, a historical-82/current-81 reorder, and wrapping order. Strict replay
`C:\nwnbridge\codex-proxy2-replay-current-packet-ack-20260714-1820` processed
164 packet files with 304 strict allows, zero strict/semantic quarantines or
files, and zero terminal live-object residuals. The next credentialed Bard50
probe should retain the prior expectations and additionally confirm that no
materialization whose own raw ACK precedes sequence 82 enters the response
window.

Current transport plumbing also makes the two ACK spaces explicit instead of
letting response admission consult mutable session-level "last observed" state.
Direct packets and coalesced records carry a typed raw-peer/EE-unshifted frame
context into semantic side effects. Multi-frame deflated reassembly retains both
ACK values on every buffered source frame and uses the final source frame's raw
ACK for response ownership. This is state provenance only; the exact
ClientGuiInventory BOOL/OBJECTID body and fragment cursor are unchanged.
Focused tests prove explicit raw ACK 81 overrides stale historical ACK 90 and
that reassembly preserves the live-shaped raw-82/EE-80 pair. Strict replay
`C:\nwnbridge\codex-proxy2-replay-explicit-peer-ack-20260714-2115` processed
164 packet files with 304 strict allows, zero strict/semantic quarantines or
files, one 36-slot quickbar, and zero terminal live-object residuals. The next
credentialed Bard50 probe must still provide the direct live confirmation above;
no account-secret environment source was available in this run.

The response window now makes semantic provenance frame-local as well. Direct,
coalesced, and completed deflated server paths record whether reducing the
exact current frame produced a live-object inventory-materialization
observation. A proof that names LiveObject but has no matching current gameplay
unit cannot pair the previous materialization summary with its own newer raw
ACK. The exact ClientGuiInventory BOOL/OBJECTID body and MSB-first fragment
cursor remain unchanged. Focused stale-summary coverage and strict replay
`C:\nwnbridge\codex-proxy2-replay-frame-local-materialization-20260714-2359`
passed; replay processed 164 packet files with 304 strict allows, zero
strict/semantic quarantines or files, one exact 36-slot quickbar, and zero
terminal live-object residuals. The next credentialed Bard50 run still must
confirm that the raw-ACK-82 materialization is observed and completed from that
same frame while the raw-ACK-81 packet remains excluded.

The server-frame handoff now carries the exact typed live-object materialization
summary beside its raw peer ACK. Direct, coalesced, and completed deflated paths
no longer pass only a boolean and then reread mutable session history during
response association. The gameplay splitter's conservative live-object
ownership disproved a proposed multi-top-level-live-object collection, so that
unsupported rule was removed. No ClientGuiInventory BOOL, OBJECTID, fragment
bit, cursor, or writer byte changed. Eleven focused status tests passed,
including stale-summary rejection. Strict replay
`C:\nwnbridge\codex-proxy2-replay-typed-frame-materialization-20260715-0345`
processed 164 packet files with 304 strict allows, zero strict quarantines or
files, and zero terminal live-object residuals. The latest live HG capture
remains the gameplay-reaching, zero-quarantine 06:03 Bard50 artifact above;
the raw-ACK-81/82 post-change confirmation is still the next credentialed run.

Coalesced queued records now preserve that transport context when their stored
sequence/ACK fields are zero. EE `FrameReceive` owns the primary reliable frame,
while `UnpacketizeFullMessages` walks queued records in the shared 12-byte
storage shape; all checked-in Diamond coalesced fixtures use zero queued
sequence/ACK sentinels. Production now resolves those fields once and supplies
the inherited primary sequence and EE-unshifted ACK to direct and deflated
semantic side effects, login waypoint handling, module-resource transitions,
and the ClientGui status response window. The primary raw peer ACK remains
separate. No wire byte or CNW bit cursor changed. Focused zero-header and resource
insertion tests passed; strict replay
`C:\nwnbridge\codex-proxy2-replay-coalesced-context-20260715-0624` processed
164 packet files with 304 strict allows, zero strict/semantic quarantines or
files, one exact quickbar, and zero terminal live-object residuals. At this
run's 05:48 +10 gate the latest 06:07 gameplay artifact was 23.687 hours old,
reached gameplay, and remained zero-quarantine. It has since crossed 24 hours,
so the next run must obtain a fresh credentialed Bard50 HG capture before
ordinary code work and retain the raw-ACK-81/82 completion expectations above.

The 2026-07-15 fresh account-5 capture above supersedes that freshness and
completion requirement. Its exact ACK-80/81 window and 52-record response
live-confirm the shared rule. Current production also avoids repeating the
entire area semantic parser during gameplay boundary discovery when one
complete declared `Area_ClientArea` owns all input and its remaining CNW
fragment storage is bounded with the decompile-proven 3-bit MSB-first final
valid-count header. The dispatcher still performs the full exact translation
once; stale declarations and competing top-level boundaries retain the exact
boundary probe. Strict replay
`C:\nwnbridge\codex-proxy2-replay-area-single-pass-20260715-0920` processed 164
packet files with 304 strict allows, zero strict/semantic quarantines or files,
and zero terminal residuals. For the 10,543-byte docks area, boundary handoff
fell from about 10.17 seconds in the live pre-fix trace to 0.35 ms in replay;
the full semantic rewrite still ran once.

The three preceding account-4 gameplay captures were
`C:\nwnbridge\codex-live-account4-bard-pi-action-20260713-1145\harness-proxy-20260713-114158`,
`C:\nwnbridge\codex-live-account4-buffbot-scout-20260713-1200\harness-proxy-20260713-115241`,
and
`C:\nwnbridge\codex-live-account4-reincth-scout-20260713-1205\harness-proxy-20260713-115641`.
All reached sustained zero-quarantine gameplay. `starcore-bard-pi` retained
four item slots `[1,5,6,8]`; `starcore-buffbot` retained one at slot `35`.
Every preserved item acquired matching durable GQ state, so both classified as
`all_preserved_items_have_use_count_state` and correctly sent no action.
`starcore-reincth` had no preserved items and its unrelated ready inventory
candidate remained suppressed as `candidate_not_preserved_active_item`.
Strict replay
`C:\nwnbridge\codex-proxy2-replay-profile-scouting-20260713-1200` processed
164 packet files with 304 strict allows, zero quarantine decisions/files, and
zero terminal live-object residuals.

The same run verified the new typed vault diagnostic: after exact
`CharList_ListResponse` validation proxy2 logs all fixed-width BIC resrefs in
server order. Account 3 discovery artifact
`C:\nwnbridge\codex-live-account3-vault-typed-retry-20260713-0900\harness-proxy-20260713-084841`
reported five resrefs. Corrected account 4 discovery artifact
`C:\nwnbridge\codex-live-account4-vault-typed-corrected-20260713-0915\harness-proxy-20260713-085456`
reported `starcore-bard-pi`, `starcore-buffbot`, `starcore-reincth`,
`starcore-bard50`, `starcore-helper`, and `amithraliatest`, with zero
quarantines. Use a placeholder no longer than 16 characters (for example
`vaultprobe`) to solicit the typed list without selecting a real character:

```powershell
$hgPassword = '<load account password without printing it>'
.\tools\test-hg-bridge.ps1 -SkipBuild -SkipAssets -SkipInjectTest -DiamondAccount 4 -AutoCharacter vaultprobe -Password $hgPassword -AutoSpeakPassword -ProxyExe C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe -ProxyLogRoot C:\nwnbridge\codex-live-account4-vault-typed
```

After the bard50 ClientGui response blocker is understood, continue account-4
scouting with `starcore-helper` and `amithraliatest`.

The immediately preceding gameplay-reaching proxy harness is
`C:\nwnbridge\codex-live-inventory-prepump-20260713-0242\harness-proxy-20260713-024110`.
It was launched with:

```powershell
$env:HG_BRIDGE_DRIVER_ONLY_TRACE_BNK_HANDLERS = '1'
$hgPassword = '<load account password without printing it>'
.\tools\test-hg-bridge.ps1 -SkipBuild -SkipAssets -SkipInjectTest -DiamondAccount 1 -AutoCharacter starcore-stormre -Password $hgPassword -AutoSpeakPassword -AutoOpenInventory -AutoOpenInventoryDelayMilliseconds 5000 -AutoQuickbarItemRefreshUseItem -ProxyExe C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe -ProxyLogRoot C:\nwnbridge\codex-live-inventory-prepump-20260713-0242
```

The wrapper bound proxy2 and the launcher to account 1 before proxy startup;
the proxy log must show the selected account's CD-key path and the launcher log
must show its matching player name. The run reached `Module_Loaded`, strictly
owned the opt-in one-character password send as `ClientChat`, reached
`Area_ClientArea`, proxy-generated `Area_AreaLoaded`, and sustained exact
`GameObjUpdate_LiveObject` gameplay through `2026-07-13T02:43:20+10:00`. It
wrote zero quarantine files. The bridge dispatched delayed auto-inventory from
the pre-pump edge of EE's client main loop; proxy2 then strictly claimed real
`ClientGuiInventory` sequences 80/81, HG returned one 31-item materialized
response, and proxy2 dispatched one confirmed Inventory replay. All 14
preserved active quickbar slots gained matching durable GQ state before a
client item action, so the optional `UseItem` was correctly suppressed as
`resolved_by_prior_quickbar_use_count_state`. This run is current clean
gameplay and inventory-driver evidence. The next functional probe still needs
a character/module state with an active quickbar slot missing matching GQ after
inventory materialization.

The immediately preceding clean gameplay harness was
`C:\nwnbridge\codex-live-u5-004f-current-20260712-0010\harness-proxy-20260712-234624`.
It reached gameplay through `2026-07-12T23:48:17+10:00` with zero quarantine
files, but its post-pump delayed inventory call never reached proxy2 and
therefore did not provide forced-inventory or item-action evidence.

The immediately preceding clean gameplay harness was
`C:\nwnbridge\codex-live-chat-padding-20260712-1945\harness-proxy-20260712-190730`.
It reached gameplay through `2026-07-12T19:11:05+10:00`; its one diagnostic
candidate later exact-claimed and was not a dropped-packet blocker.

The immediately preceding gameplay harness was
`C:\nwnbridge\codex-live-gui-add-boundary-20260712-171117\harness-proxy-20260712-171118`.
It reached gameplay through `2026-07-12T17:13:44+10:00` but repeatedly rejected
the three-span PlayerList/Chat window. Diamond `CNWMessage::GetWriteMessage`
(`sub_4FC920`) proves the writer preserves the unused low five bits while
installing the valid-bit count in bits 7..5; EE `SetReadMessage` and `ReadBits`
consume only the count and requested valid bits. Proxy2 now validates the exact
speaker/string boundary and three-bit count, clears only the five unused bits,
then requires the canonical `0x60` tail under strict typed proof. Strict replay
`C:\nwnbridge\codex-proxy2-replay-chat-padding-20260712-1930` processed 164
packet files with 304 allows, zero quarantines/files, and zero terminal
live-object residuals.

The gameplay harness before that was
`C:\nwnbridge\codex-live-u5-4408-boundary-20260712-1512\harness-proxy-20260712-151148`.
It ran through `2026-07-12T15:14:43+10:00`, exposed the now-fixed 1,987-byte
top-level `G I/R A` stream and the unresolved 88-byte `U/5 0x0000004F` update,
and wrote three dump files for those two logical quarantines.

The immediately preceding clean gameplay harness was
`C:\nwnbridge\codex-live-chat-talk-active-slots-retry-20260712-0107\harness-proxy-20260712-010711`.
It first proved the 21-slot active-signature set and directly recovered from the
intermittent BNK handoff stall. The prior gameplay run exposed two `Chat_Talk`
`0x09/0x01` quarantines; current code claims only the exact decompile-backed
OBJECTID/string/three-header-bit shape. The talk family has not recurred since,
so a future recurrence is still required as direct live confirmation.

The first current-code verification attempt at
`C:\nwnbridge\codex-live-chat-talk-active-slots-20260712-0105\harness-proxy-20260712-010525`
stalled after deferred BNK2 with no BNK3. It did not reach gameplay and must not
be used as gameplay evidence. Stopping that client/proxy pair and rerunning with
`HG_BRIDGE_DRIVER_ONLY_TRACE_BNK_HANDLERS=1` recovered on the next attempt; this
matches the documented intermittent BNK handoff failure mode below.

The immediately preceding gameplay-reaching capture was
`C:\nwnbridge\codex-live-clientgui-response-window-20260711-205328\harness-proxy-20260711-205330`.
It confirmed that one matching materialized set terminates a queued
ClientGui-status response window at the first response and that later gameplay
does not re-enter the completed window.

The earlier gameplay-reaching capture was
`C:\nwnbridge\codex-live-item-action-current-20260711-0442\harness-proxy-20260711-044124`.
It reached gameplay and produced zero quarantine files, but exposed the fixed
failure mode: its second status request kept attributing 49 later live-object
packets after the matching materialized response at server sequence 58. Use the
fresh 20:56 run as current gameplay and response-window evidence. The next live
target is a real EE item action after materialization in a run where no genuine
`GQ` use-count row has already resolved the pending refresh.

Previous clean live HG inventory-replay status, as of 2026-07-10 23:18 +10,
is
`C:\nwnbridge\codex-live-visible-equipment-cost-boundary-20260710-231503\harness-proxy-20260710-231505`.
It reached sustained gameplay, materialized 52 item rows, dispatched exactly
one retained Inventory replay, reached exact visible-equipment EE shape, and
produced zero quarantine files.

Latest clean live HG proxy status, as of 2026-07-10 07:12 +10, is
`C:\nwnbridge\codex-live-clientgui-refresh-confirmed-current-20260710-0710\harness-proxy-20260710-070818`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, proxy-generated
`Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject` traffic. It wrote
`quickbar-item-refresh-hint.json` and `proxy.structured.log` through
`2026-07-10T07:12:44+10:00`, and produced no quarantine directory. This run
exercised the unproven server Inventory claim fallback and the materialized
ClientGui status-response association on the current build: the final hint
reported
`inventory_equipment_bridge_output_status="client_gui_status_refresh_confirmed"`,
`inventory_equipment_bridge_output_client_gui_status_refresh_confirmed=true`,
1 queued proxy-owned `ClientGuiInventory_Status` request, exact status payload
`700D010B0000000000007F90`, response outcome `materialized_items`, best
response association `matches_queued_status_candidate`,
`...matches_queued_status_candidate=true`, and
`...materialized_item_object_ids_contain_queued_candidate=true` across 52 best
materialized item ids. Treat this capture as proof that HG answers the
proxy-owned current-player inventory status refresh with the materialized set
needed by the bridge. The next production target is the generalized inventory
UI refresh or visible-equipment output rule, with a follow-up live probe that
checks whether the EE client state actually updates after this confirmed
refresh.

Previous live HG proxy server-Inventory fallback evidence, as of
2026-07-10 03:17 +10:
`C:\nwnbridge\codex-live-inventory-clientgui-fallback-current-20260710-031303\harness-proxy-20260710-031307`.
It reached gameplay, wrote artifacts through `2026-07-10T03:17:07+10:00`,
produced no quarantine directory, and proved that the fallback reaches the
materialized ClientGui path. The final hint in that earlier build reported
`inventory_equipment_bridge_output_status="queued_client_gui_status_output"`,
1 queued proxy-owned `ClientGuiInventory_Status` request, server Inventory
claim `0x8001543E`, ready candidate `0x8001538E`, synthetic ClientGui claim
`0x7F000000`, and 85 post-status live-object responses including 1
live-GUI/materialized-item response. The retained compact current candidate
still differed from the queued candidate in that older run; the 07:12 +10 run
above confirms the generalized materialized-set containment association.

Previous live HG proxy ClientGui status-response evidence, as of
2026-07-09 23:01 +10:
`C:\nwnbridge\codex-live-client-gui-status-association-current-20260709-225811\harness-proxy-20260709-225914`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, proxy-generated
`Area_AreaLoaded`, and sustained `GameObjUpdate_LiveObject` traffic. It wrote
`quickbar-item-refresh-hint.json` and `proxy.structured.log` through
`2026-07-09T23:01:47+10:00`, and produced no quarantine directory. The delayed
inventory action fired: 21 `ClientGuiInventory` handoffs, 17 ready handoffs,
17 queued proxy-owned `ClientGuiInventory_Status` requests, and 18 post-status
live-object responses. The final hint reported candidate `0x80015379`,
`inventory_equipment_bridge_output_status="queued_client_gui_status_output"`,
`inventory_equipment_bridge_output_client_gui_status_response_outcome="live_object_only"`,
18 live-object response packets, 0 counted live-GUI response packets, and 0
counted materialized-item response packets. The exact-validator log also
reported a separate deflated `live_gui_records=1` /
`materialized_item_object_ids=21` live-object packet between the first and
later status bursts, but the bridge state did not attribute that packet to the
proxy-owned status response. As of 2026-07-10 01:04 +10, proxy2 records
ClientGui status live-object responses during deflated M reassembly semantic
observation, before recompression, using the reassembly first sequence and
last ACK sequence. Strict replay
`C:\nwnbridge\codex-proxy2-replay-deflated-clientgui-hook-20260710-010459`
over the 164-packet Diamond autoplay baseline reported 304 strict allows,
0 strict quarantines, no quarantine directory, and 0 live-object terminal
residuals. The next live proof needs a run that queues proxy-owned
`ClientGuiInventory_Status` again after resolving the current candidate/claim
mismatch.

Previous live HG proxy status, as of 2026-07-09 19:01 +10:
`C:\nwnbridge\codex-live-c008-delayed-inventory-confirm-20260709-185755\harness-proxy-20260709-185759`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, reached
gameplay through `Module_Loaded`, `Area_ClientArea`, proxy-generated
`Area_AreaLoaded`, the post-area hold gate opening, held post-area packet
release, and sustained `GameObjUpdate_LiveObject`/quickbar traffic. It wrote
`quickbar-item-refresh-hint.json` through `2026-07-09T19:01:19+10:00` and
`proxy.structured.log` through `2026-07-09T19:01:19+10:00`, and produced no
quarantine files. This run also exercised the delayed inventory target:
`AutoOpenInventory` produced 20 ready `ClientGuiInventory` handoffs, proxy2
queued 20 exact current-player `ClientGuiInventory_Status` requests with
payload `700D010B0000000000007F90`, and HG answered with 52 live-object
packets after the queued requests, including 9 live-GUI/materialized-item
response packets. Its final hint reported candidate `0x80015211`,
`inventory_equipment_bridge_output_status="queued_client_gui_status_output"`,
`inventory_equipment_bridge_output_client_gui_status_response_live_object_packets=52`,
`inventory_equipment_bridge_output_client_gui_status_response_live_gui_record_packets=9`,
and `inventory_equipment_bridge_output_client_gui_status_response_materialized_item_packets=9`.
The previous delayed-inventory seq51 C008 strict-family quarantine did not
recur, so the active live-object blocker is confirmed fixed; use this capture
as current gameplay freshness and ClientGui status-response evidence.

As of 2026-07-09 21:10 +10, proxy2 also preserves the ready item-state
candidate attached to each queued proxy-owned `ClientGuiInventory_Status`
request and reports whether the best retained ClientGui status live-object
response matches that queued request. Hints and replay summaries now include
`inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_*`,
`inventory_equipment_bridge_output_best_client_gui_status_response_association`,
`..._matches_queued_status_candidate`, and
`..._candidate_delta_from_queued_status_candidate`. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-client-gui-response-association-20260709-2104`
over the 164-packet Diamond autoplay baseline reported 304 strict allows,
0 strict quarantines, no quarantine directory, 0 live-object terminal
residuals, and inactive ClientGui association defaults because the baseline
does not exercise proxy-owned ClientGui status. The next delayed
forced-inventory live HG probe should use the current build and verify that the
best materialized response reports `matches_queued_status_candidate` before
choosing a concrete inventory UI refresh or visible-equipment output rule.

As of 2026-07-10 05:13 +10, proxy2 also records the exact materialized item-id
set for retained ClientGui status responses. The bridge response state now
exports first/last/min/max materialized item ids and whether the materialized
set contains the queued ClientGui status candidate; response association uses
that containment before falling back to compact-current-candidate equality.
Focused ClientGui response and inventory/equipment tests passed, and strict
replay
`C:\nwnbridge\codex-proxy2-replay-clientgui-materialized-association-20260710-051009`
over the 164-packet Diamond autoplay baseline ran with strict translation,
0 quarantine files, and 0 live-object terminal residuals. The replay baseline
does not exercise proxy-owned ClientGui status, so the required next live proof
is a delayed forced-inventory HG capture whose final hint reports
`...materialized_item_object_ids_contain_queued_candidate=true` and
`matches_queued_status_candidate`.

Previous clean current-code gameplay freshness evidence:
`C:\nwnbridge\codex-live-current-live-object-diagnostics-20260709-125914\harness-proxy-20260709-125919`.
It reached gameplay with no quarantine directory and wrote
`proxy.structured.log` through `2026-07-09T13:01:31+10:00`, but
`AutoOpenInventory` did not produce `ClientGuiInventory` events before the
client/proxy stopped advancing, so its final hint remained
`inventory_equipment_bridge_output_status="awaiting_bridge_state_update"` with
0 queued ClientGui status packets and 0 ClientGui status response counters.
Use it only as clean-live-object evidence, not as ClientGui writer validation.

Previous forced-inventory live evidence for the active live-object blocker:
`C:\nwnbridge\codex-live-client-gui-status-delayed-inventory-20260709-105516\harness-proxy-20260709-105528`.
It selected `C:\nwnbridge\cargo-target\debug\hgbridge_proxy2.exe`, observed
`BNK3` after deferred `BNK2`, reached gameplay through `Module_Loaded`,
`Area_ClientArea`, proxy-generated `Area_AreaLoaded`, the post-area hold gate
opening, held post-area packet release, and sustained `GameObjUpdate_LiveObject`
traffic. It wrote `quickbar-item-refresh-hint.json` and `proxy.structured.log`
through `2026-07-09T10:58:21+10:00`, but produced a
`live-object-unclaimed-strict-family` quarantine for server seq51
(`quarantine\live-object-unclaimed-strict-family-GameObjUpdate_LiveObject-seq51-frames1-1783558701965.bin`,
322 bytes, `P/05/01` declared `0x013D`). This run counts as current gameplay
freshness evidence because gameplay was reached, but it is not a clean bridge
run. Its final hint had two `ClientGuiInventory` handoffs blocked before ready
item context and one ready server `Inventory` handoff blocked by candidate/claim
mismatch: candidate `0x80015854`, claim `0x80015977`, closest proven item
`0x800158CD` at distance 170. The active proxy2 issue is to diagnose and fix
that live-object strict-family miss before treating the ClientGui inventory
writer path as the top blocker.

Previous clean current-code gameplay freshness evidence:
`C:\nwnbridge\codex-live-client-gui-status-response-20260709-091913\harness-proxy-20260709-091918`
reached the same gameplay milestones, wrote
`quickbar-item-refresh-hint.json` and `proxy.structured.log` through
`2026-07-09T09:21:38+10:00`, and produced no quarantine directory. That run did
not exercise inventory opening: the EE client exited after gameplay with 0
`ClientGuiInventory` events, so
`inventory_equipment_bridge_output_status="awaiting_bridge_state_update"` and
the queued-status/response counters stayed at 0.

The newest successful forced-inventory live evidence for the ClientGui status
writer remains
`C:\nwnbridge\codex-live-client-gui-status-output-20260709-085527\harness-proxy-20260709-085537`
(`proxy.structured.log` through `2026-07-09T08:58:14+10:00`, no quarantine
directory). It reached gameplay, observed real `ClientGuiInventory_Status`
traffic, queued one proxy-owned current-player
`ClientGuiInventory_Status` request with payload `700D010B0000000000007F90`
for object `0x7F000000`, and HG answered with
`GameObjUpdate_LiveObjectCombinedRecords` containing 51 live-GUI records,
348 live-GUI fragment bits, and 51 materialized item object ids.

As of 2026-07-09 09:22 +10, proxy2 has exact decompile-backed
`ClientGuiInventory` EE payload builders for status and select-panel claims and
queues a bounded proxy-owned current-player `ClientGuiInventory_Status` request
when live state proves a status/self inventory claim for `0x7F000000`. The
queued payload is the exact EE shape `700D010B0000000000007F90`, validated by
the typed ClientGui parser before insertion; select-panel claims,
non-current-player status claims, and missing client sequence state still
defer. Hints and replay summaries now expose the queued status metadata plus
typed live-object response counters for HG traffic observed after a queued
ClientGui status request:
`inventory_equipment_bridge_output_client_gui_status_response_live_object_packets`,
`..._live_gui_record_packets`, `..._materialized_item_packets`, and the
last-response sequence/live-GUI/materialized-item/candidate fields. Bounded
strict replay
`C:\nwnbridge\codex-proxy2-replay-client-gui-status-response-20260709-091641`
over the 164-packet Diamond autoplay baseline used alternate ports
`-ListenPort 56321 -ServerPort 56333`, ran with strict translation, and
correctly left the queued/response counters at 0 because that replay source has
no ready ClientGui inventory handoff. The next live HG forced-inventory probe
should use the diagnostic build so any repeat `P/05/01` quarantine logs
declared/read/fragment lengths, decoded fragment bit count, claim reject
stage/cursor, and declared-repair candidate counts. As of 2026-07-09 13:10 +10,
proxy2 also logs the same exact-claim diagnostics on the intermediate
`live-object-semantic-candidate-rejected-exact-validator` rewrite path, which is
the path that dumped the 516-byte candidate from the 10:58 seq51 quarantine.
As of 2026-07-09 15:14 +10, those diagnostics also include
`claim_reject_record_*` fields for the exact reject-row window: length,
opcode/ascii, object type, object id, and the first WORD/DWORD after the object
id. Bounded strict replay
`C:\nwnbridge\codex-proxy2-replay-reject-record-preview-20260709-1505` over
the 164-packet Diamond autoplay baseline reported 304 strict allows, 0 strict
quarantines, 0 quarantine files, and 0 live-object terminal residuals. The
next run used those `claim_reject_record_*` fields to identify and implement
the C008 status/self cursor repair below. If the live-object path stays clean
and server `Inventory` traffic returns first, continue the claim-neighborhood
provenance path instead.

As of 2026-07-09 17:14 +10, the seq51 reject-row evidence reduced to `U/5`
mask `0x0000_C008` on object `0xFFFFFFDE`, with the scanner split at the
12-byte header/count cursor before embedded status-effect `A` rows. Proxy2 now
models C008 as the C408 status/self suffix family without the 0x0400 four-WORD
scalar branch: compact legacy rows get a count-derived scan floor and typed
EE identity-map insertion, already-EE-shaped rows claim through the byte
boundary helper, and the exact validator consumes the ten suffix fragment BOOLs.
Focused C008/status-effect tests passed, and strict replay
`C:\nwnbridge\codex-proxy2-replay-c008-status-self-20260709-171155` over the
2026-07-03 Diamond autoplay packets ran with strict translation, 0 quarantine
files, and 0 live-object terminal residuals. The delayed forced-inventory live
confirmation at
`C:\nwnbridge\codex-live-c008-delayed-inventory-confirm-20260709-185755\harness-proxy-20260709-185759`
then reached the inventory path with no quarantine files, so server seq51 is no
longer the active blocker. ClientGui inventory response/candidate association
is now the primary blocker.

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
| After a deflated character list commits at server sequence zero, proxy2 repeatedly logs `reliable slot already committed different immutable transport bytes`; pre-module traffic repeats and gameplay is never reached | A completed type-0 stream route was incorrectly allowed to claim the independent type-1 ACK-control lane because both carry sequence zero | Fixed 2026-07-21: completed stream/coalesced route identity accepts only decompile-backed frame type 0. Diamond `sub_5F3940` and EE `FrameReceive` both keep type-1 ACK controls out of receive-data storage. Require the next run to pass typed `Module_Loaded`, native `Area_AreaLoaded`, sustained live-object traffic, and zero route-conflict/quarantine rows. |
| First-area gameplay succeeds, then a following `Area_ClientArea` reports width 11 / packet height 0 / inferred height 14 with three legacy zero-count/single-resref sound rows; EE ACK progress stops, HG retransmits the following window, and eventually sends `BNDP CE 16 00 00` without a quarantine file | The exact missing-height repair ran before sound-row normalization. It correctly rejected the still-legacy tail; the sound repair later committed, but no height retry rebuilt the final EE area shape | Fixed 2026-07-19: after a nonzero independently proven sound repair, retry only the exact missing-height repair once when height is still zero. Diagnose recurrences by comparing `packet_height`, `inferred_height`, `sound_count_zero_one_repairs`, client ACK progression, and the final EE cursor proof. The combined captured fixture proves both repairs and full fragment exhaustion; the post-fix live run sustained Docks gameplay with no disconnect/quarantine, although its source arrived already normalized. |
| Automation starts in an empty Google Drive folder | Wrong cwd | Switch to `D:\Codex Projects\NWN EE Bridge` and fail visibly if the populated checkout is absent. |
| Packet dumps stop at BN/login/vault traffic | Harness did not reach character/module/gameplay | Treat as a harness blocker, record the stage, and fix or instrument the connection path before unrelated proxy work. |
| A 735-byte `Area_ClientArea` unit with a stale 220-byte declared read window is logged or quarantined as a persistent-zlib continuation after `Module_Info` | Transport correctly proved the direct CNW window incomplete, but continuation ownership ran before the bounded typed Area repair and swallowed the real three-byte final fragment | Fixed 2026-07-13: before continuation ownership, the coalesced path tries only the strict incomplete-Area translator. It accepts a replacement boundary only when one bounded final-fragment candidate has an exact legacy parse and exact EE LoadArea cursor. Context-free candidate proof prevents repeated module-resource scans. The private stale-window fixture passes; require a live recurrence of the stale declaration for direct live confirmation. |
| A vault-scouting run reaches `BNVR A` but never sends `CharList_Request` | `-AutoCharacter` was empty or the discovery placeholder exceeded the engine's 16-byte `CResRef` limit, so auto-character was disabled | Use a non-real placeholder of at most 16 characters such as `vaultprobe`. Require proxy2's `validated character vault list` row and use only its typed resrefs for the later gameplay run. |
| `-DiamondAccount 1` selects an account-1 character, but proxy startup logs `C:\NWN\Config\5.nwncdkey.ini` or the launcher injects the wrong player name | The proxy starts before the launcher and inherited stale/default account-5 identity; native app-manager state can also retain the previous name | Fixed 2026-07-12: `test-hg-bridge.ps1` resolves and exports the selected account's CD-key/player paths before proxy startup, restores the prior environment afterward, and the bridge prefers the launcher-selected name. Require matching proxy and launcher identity rows on every alternate-account run. |
| HG sends “speak your password” feedback, proxy logs an unowned 81-byte `ClientSideMessage_Feedback` id `0x5E`, then the run stops after `Module_Loaded` | EE's case-11 feedback reader expects a build-gated BOOL that the legacy writer omitted; the coalesced prompt can also bypass the bridge's high-level text detector | Fixed 2026-07-12: proxy2 inserts only the decompile-proven default-false BOOL at the exact string boundary, and opt-in `-AutoSpeakPassword` falls back after successful `Module_Loaded` only if no prompt-triggered attempt ran. Load the secret without printing it and require a strict `ClientChat` allow. |
| Account-1 gameplay emits two `live-object-unclaimed-strict-family` files for one 962-byte payload beginning `50 05 01 AE 03 00 00 55 05 C3 FF FF FF 08 44` | Fixed 2026-07-12: the typed `U/5 0x4408` rewrite succeeded, but transport boundary selection split inside the five inserted effect-row identity maps before the four-WORD scalar suffix | The scanner now owns the exact decompile-backed byte span and leaves the seven `0x4000` BOOLs to the exact cursor validator. The private live fixture rewrites and claims the following inventory through bit 153; strict replay is clean. Require a future live recurrence before calling it live-confirmed. |
| Account-1 gameplay quarantines a 1,987-byte payload beginning `50 05 01 AD 07 00 00 47 49 41 01 00 00 00` | Fixed 2026-07-12: the pre-GUI add-map walker split inside a fragment-proven nested item and mistook active-property bytes for a top-level creature add | The walker now uses the exact focused `G I/R A` row end and Diamond fragment cursor, and stops on an unproven GUI row. The original SHA-256 `7AF84AEE4E7923BA17FE9CFCA822AAFEC60F7D060D2107BD3B9ACA4A69613D13` stream rewrites all 30 rows to an exact EE claim; require a future live recurrence before calling it live-confirmed. |
| Account-1 gameplay quarantines an 88-byte payload beginning `50 05 01 56 00 00 00 55 05 C3 FF FF FF 4F 00 00 00` | Fixed 2026-07-12: proxy2 treated only exact mask `0x000F` as status-before-action-state, so mask `0x004F` consumed the status count as the action-state byte and lost the 43-byte boundary | Diamond `sub_44ADD0` and EE `sub_140781E80` prove action code, status list, action state/follow-up, then the `0x0040` `WORD, BYTE, WORD, BYTE, BOOL` tail. The typed cursor now follows that order at the existing bounded record candidate. SHA-256 `34DC5631894403EEF8479D94E344F2A18C8898A57624721C9BEBA5133F01A6B5` rewrites and exact-claims through the following inventory; focused tests and strict replay are clean. Require a future live recurrence before calling it live-confirmed. |
| Gameplay reaches module/area/live-object traffic, then strict decisions repeatedly reject one three-span `PlayerList_Add` / `PlayerList_All` / `Chat_Talk` datagram as `coalesced-record-proof-invalid` without writing a quarantine file | Fixed 2026-07-12: Diamond `GetWriteMessage` preserves stale scratch data in the unused low five bits while storing valid-bit count 3 in bits 7..5; EE `SetReadMessage`/`ReadBits` ignore unread padding | Proxy2 now validates the exact OBJECTID/string boundary and three-bit count, canonicalizes only the low five bits, and strictly reclaims the resulting `0x60` tail. Focused/coalesced tests and the 164-packet strict replay are clean; the fresh HG run reached sustained zero-quarantine gameplay, but the exact server echo did not recur, so direct live source confirmation remains pending. |
| Driver logs `auto-inventory send end ... result=1 local-opened=1`, but proxy2 sees no `ClientGuiInventory` frame and the hint stays `no_post_committed_item_context` | The delayed request was queued after EE's client main-loop network pump had already completed; an idle client could leave the CNWMessage scratch state without an engine-owned flush | Fixed and live-confirmed 2026-07-13: dispatch the delayed request at the pre-pump edge of the same game-thread callback. Capture `codex-live-inventory-prepump-20260713-0242` strictly claimed the real client frames, materialized 31 items, dispatched one confirmed Inventory replay, and wrote zero quarantines. |
| Gameplay reaches `Party_GetList` and logs `auto-inventory scheduled`, but the due time passes with no `ClientGuiInventory` event before disconnect | The 2026-07-10 17:00 build checked delayed auto-inventory only from a later server dispatch; an idle gameplay connection supplied none | Fixed and live-confirmed 2026-07-10 20:41: the driver retains the scheduling CNWMessage and services the action from EE's client main loop on the game thread. The 5-second run logged `source=client main loop` at the exact due tick and a successful real `ClientGuiInventory` call. If it recurs, verify that main-loop servicing remains installed before changing server-dispatch timing. |
| Gameplay continues through a synthetic `Area_AreaLoaded`, while proxy2 quarantines a 430-byte `PlayerList_All` payload beginning `50 0A 01 AA 01 00 00 06` | Three of six legacy rows have a zero player-name CExoString length followed by printable name bytes and the same row's creature object id | Fixed and live-confirmed 2026-07-10 20:41: current code repairs only that exact boundary and then requires the complete decompile-backed typed body and all 28 MSB-first fragment bits. The fresh six-row shape recurred twice and both units translated without PlayerList quarantine. |
| A successful forced-inventory run releases one confirmed Inventory replay, then quarantines a 417-byte live-object payload beginning `50 05 01 9B 01 00 00` (often under two dump names for one inflated unit) | Fixed 2026-07-10: the bare-inline `Militia Shield` name was followed by cost DWORD `0x00000032`, and its printable low byte was greedily consumed as a trailing `2`; the exact fragment cursor was already correct | The parser now tries bounded printable endpoints longest-first and accepts only a complete decompile-backed active-property suffix. The private fixture exact-translates with item-name widths 6/6/7 and U/5 at cursor 28. Fresh live capture `codex-live-visible-equipment-cost-boundary-20260710-231503` reached gameplay, dispatched one confirmed replay, and produced zero quarantine files. |
| A live item-action probe commits a valid item quickbar, then the hint reports many `quickbar_events_before_first_client_action`, suppresses the action as `server_quickbar_response_before_first_client_action`, and repeats ClientGui status queue update indices within milliseconds | Fixed 2026-07-11: cached direct records were reparsed and split deflated records were observed again after typed replay | Coalesced direct/deflated caches now replay translated bytes with current transport fields but no semantic/bridge effects. Fresh live capture `codex-live-coalesced-side-effects-20260711-025757` exercised 47 typed replay hits, retained one semantic server Inventory claim, replaced the false 18 quickbar events with one genuine GQ row, and produced zero quarantine files. |
| Live HG reaches gameplay, then the final hint stays `inventory_equipment_bridge_output_status="awaiting_bridge_state_update"` with 0 `ClientGuiInventory` events despite `-AutoOpenInventory` | The driver/client exited or missed the inventory-open timing before the GUI action was emitted | Count the artifact as gameplay freshness evidence only, not forced-inventory evidence. Rerun with manual inventory opening or an explicit post-area `-AutoOpenInventoryDelayMilliseconds` value, and require `ClientGuiInventory` log rows before using the run to validate ClientGui writer/response counters. The 2026-07-09 09:19 current-code run hit this timing miss after reaching gameplay. |
| Live HG receives raw `BNK2` but no `BNK3`, `BNK4`, or `BNCS`; driver log has no `NonWindow` BNK2 begin/result and EE writes a fresh `nwmain-crash-*.nwcrash.txt` | Intermittent EE crypto handoff stall/crash before `HandleBNK2Message` processes the deferred BNK2, or a stale client/proxy state that makes the BNK2 handler unsafe | Stop stale `nwmain`/`hgbridge_proxy2` processes, rerun with `HG_BRIDGE_DRIVER_ONLY_TRACE_BNK_HANDLERS=1`, and inspect proxy `observed EE BNK3 after deferred BNK2` versus `EE crypto handshake stalled after BNK2; no BNK3 received` alongside driver `NonWindow` BNK2 rows. The 2026-07-13 12:00 bard50 attempt stalled; the 12:02 traced retry observed BNK3 after 124 ms and reached sustained gameplay. |
| A delayed `ClientGuiInventory` request reaches proxy2, then the same reliable-M client sequences retransmit and each replay increments bridge update indices or queues the same synthetic status request | Fixed in production code 2026-07-13: semantic inventory/quickbar effects were rerun for an exact reliable sequence/payload while expensive quickbar stream candidate probing delayed ACK progress | Exact sequence/payload pairs now apply typed semantic/bridge effects once while retransmissions and ACK changes remain transport-visible; the cache is bounded for sequence wrap. Require a live recurrence to confirm suppression, then continue bounding independent candidate-analysis latency. Bard50 capture `codex-live-bard50-stale-area-fast-20260713` resolved the missing-GQ context, showed repeats through bridge update 40, and ended with HG `BNDP`; focused tests and strict replay `codex-proxy2-replay-client-reliable-effects-20260713-175105` pass. |
| `BNK3`/`BNK4`/`BNCS` succeed, then proxy logs `server BNCR reject result parsed` with `detail=6` and `detail_hint="observed-hg-rapid-reconnect-or-name-reservation"` before the client sends `BNDM` | HG still has a rapid-reconnect or player-name/session reservation for the account/character, usually after a live harness rerun too soon after stopping the previous client | Do not count the failed artifact as gameplay evidence. Stop stale `nwmain` and `hgbridge_proxy2`, wait 2-5 minutes for the HG reservation to clear, and rerun the same harness command. The 2026-07-08 23:06 run failed this way and the 23:13 rerun reached gameplay after cooldown. |
| Gameplay reaches movement, then sequence 95 quarantines an alternating `A/0A,U/0A,A/09,U/09,A/09,U/09` live-object stream and `UseObject` never completes | The terminal stock `U/09` reader ends at fragment cursor 63 while 13 additional bits are declared valid; they repeat a neighboring add-row span but have no proven stock owner, indicating an earlier cursor handoff error or an HG custom fragment extension | Keep the packet quarantined. Compare its `.terminal.tsv` with a controlled stock trace from `-TraceServerWriter`, then trace/reproduce the HG writer or list handoff. Rerun the same door interaction only after one exact owner explains `63..76`; never trim from the duplicate pattern alone. |
| Capture reaches `BNVR A` and one `P/01/03` response, but never sends client `P/11/01` | Driver fell back to native DirectConnect after missing or discarding the server-list path | Keep using the server-list DirectConnect path; if Diamond's app-state server-list slot is empty, retry with the remembered `SERVERLIST_PANEL` from the constructor hook before native fallback. |
| `PRE_PLAYMOD` selection fires with `entries=0 count=0` | Auto-character path is too early or lacks refresh/retry | Add wait/refresh/retry instrumentation and rerun until the character list is populated or a new blocker is proven. |
| Player-password prompt or native connect overlay appears | Harness regressed to the wrong login path or password handling | Keep the old driver connect path; do not pass native `+password`; seed the player password internally with default `A`. |
| No probe log or packet directory is written | Probe build/injection/run-root setup failed | Rebuild the probe, check run-root permissions, and verify the Diamond process was injected before calling the run useful. |
| HG endpoint is unreachable or the server is down | External live-server blocker | Record the exact network/server failure and retry later; do not claim fresh gameplay evidence. |
| Strict replay fails before launch with `Access is denied` while replacing `target\debug\hgbridge_proxy2.exe` | A stale replay proxy is still holding the debug executable | List `hgbridge_proxy2.exe` processes, stop only the stale debug replay process, or pass `-ProxyExe` with an isolated build output. Leave unrelated live/public proxy processes alone. |
| Strict replay reaches only part of a long capture before the automation timeout, often during `drain dummy server` | Empty UDP receive waits are too expensive for 3k+ packet captures | Use `-DrainReceiveTimeoutMilliseconds 5` or another bounded value for automation replays; keep the default higher value for manual diagnosis when delayed UDP output is under investigation. |
| Strict replay proxy exits before packet replay with `Access is denied. (os error 10013)` while binding the default listen endpoint, such as `127.0.0.1:55121` | Local port reservation, policy, or a stale process owns the default proxy listen port | Retry with an explicit free port pair, for example `-ListenPort 40021 -ServerPort 40033`, `-ListenPort 56121 -ServerPort 56133`, or `-ListenPort 56221 -ServerPort 56233`, and keep `-DrainReceiveTimeoutMilliseconds 5` for automation replays. The 2026-07-08 inventory/equipment writer replay and 2026-07-09 ClientGui status-output replay passed on alternate ports after the default port was denied. |
| Live HG reaches gameplay but writes identical `unclaimed-unknown-high-level` quarantine files for payloads that logs call incomplete/non-header stream continuations | A coalesced zlib stream tail is being passed to high-level packet ownership instead of the stream-continuation path | Fixed 2026-07-06 by classifying single incomplete inflated stream units before high-level parse fallback. If this recurs, inspect `coalesced` stream-continuation handling and require a no-quarantine live rerun before new packet-family work. |
| Live wrapper proxy exits with `unexpected argument --quickbar-item-refresh-hint` before EE launch, or `-SkipBuild` uses an older proxy than the one just built | The wrapper selected a stale proxy2 executable before a fresher compatible build | Use the resolver that checks `--help` for the hint flag, skips stale candidates, selects the newest compatible executable by `LastWriteTime`, rejects stale explicit paths, and honors `-SkipBuild` when no compatible binary exists. |
| GUI-event notify probe reaches BNK/BNCS/character list/login/`Module_Info` and `LoadModuleResources`, but not `Module_Loaded`, `Area_ClientArea`, live-object traffic, or GUI-event dispatch | Historical proxy/module-load handoff blocker: Rust was parsing the EE `Device_AdvertiseProperty` name length where the CNW declared read-buffer length lives | Use the shared `translate::client_device` classifier. Fresh 2026-07-04 14:27 rerun consumed 70 device-property frames and reached gameplay; if this recurs, verify those logs before unrelated action-family work. |
| GUI-event notify probe reaches gameplay but final hint says `stream_probe_quickbar_item_candidates_without_committed_profile` | Proxy2 can parse stream-probe `GuiQuickbar_SetAllButtons` candidates, but semantic state has no committed quickbar profile/candidate | Inspect quickbar stream commitment and profile promotion before injecting GUI-event/UseItem actions. The 2026-07-04 16:22 run added a guarded promotion path; if this recurs, confirm whether `promoted_committed_profile=true` is absent and whether normal `GuiQuickbar` proof was also absent. |
| Subtype-low UseItem probe reaches gameplay and stream-probe quickbar summaries show preserved item buttons, but the hint stays `stream_probe_quickbar_item_candidates_without_committed_profile` or `no_post_committed_item_context` | A focused quickbar stream path observed the profile but did not promote the completed stream-probe slot profile into committed quickbar semantic state | Confirm whether `quickbar_stream` logged `promoted_committed_profile=true`. If absent, fix the stream-probe promotion path before rerunning action-family probes. The 2026-07-05 18:39 rerun confirms the focused stream path can now commit profiles and progress to a pending item-refresh candidate. |
| GUI-event notify probe reaches gameplay, final hint has `first_client_action="client_gui_event_notify"` and `first_client_action_matches_candidate=true`, but `quickbar_events_after_first_client_action=0` and `server_quickbar_item_use_count_events_after_first_client_action=0` | The hinted GUI-event payload lands, but it is not sufficient to make HG emit the original item-refresh quickbar update as either full `GuiQuickbar` or live-object `GQ` item-use-count rows | Trace original-client active-property action semantics/timing before changing broad translation rules. Compare event id/body/vector/timing against Diamond/EE decompiles and live client action captures. |
| UseObject probe reaches gameplay, final hint has `first_client_action_match_class="recommended_use_object"`, but `quickbar_events_after_first_client_action=0` and `server_quickbar_item_use_count_events_after_first_client_action=0` | The bounded `Input_UseObject` payload lands, but it is not sufficient to make HG emit the original item-refresh quickbar update as either full `GuiQuickbar` or live-object `GQ` item-use-count rows | Stop retesting exact probe identity. Trace original-client active-property item action/state semantics beyond SetButton, GuiEvent_Notify, UseItem, and UseObject before changing broad translation rules. |
| UseItem subtype-low probe reaches gameplay, final hint has `first_client_action_match_class="recommended_use_item_first_property_subtype_low"`, but `quickbar_events_after_first_client_action=0` and `server_quickbar_item_use_count_events_after_first_client_action=0` | The decompile-ordered subtype-low `Input_UseItem` payload lands, but it is not sufficient to make HG emit the original item-refresh quickbar update as either full `GuiQuickbar` or live-object `GQ` item-use-count rows | Stop retesting exact probe identity. Trace original-client active-property item action/state semantics beyond SetButton, GuiEvent_Notify, UseObject, zero-byte UseItem, and subtype-low UseItem before changing broad translation rules. |
| Live auto-UseItem hint reports `stream_probe_quickbar_item_candidates_without_committed_profile` | Proxy2 can parse stream-probe `GuiQuickbar_SetAllButtons` candidates, but no accepted committed quickbar profile has reached semantic state | Inspect splitter/stream commitment and quickbar buffering before trying to inject UseItem; the driver should wait for a pending hint or a committed profile. |
| A committed quickbar with zero preserved item buttons later emits a ready generic inventory candidate | Inventory readiness alone does not prove that an object is an authentic quickbar action target; the old fallback could recommend an unrelated active inventory object | Fixed 2026-07-13: dispatch requires the candidate to match a typed preserved active-item signature and otherwise reports `candidate_not_preserved_active_item`. Use the observed actionable missing-GQ slot union to choose another profile rather than forcing the generic object. |

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

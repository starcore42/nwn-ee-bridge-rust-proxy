# NWN EE Bridge Rust Proxy

Experimental Rust proxy tooling for connecting Neverwinter Nights: Enhanced
Edition clients to legacy NWN 1.69 Higher Ground endpoints.

This is not a finished bridge or a user-ready release. It is a second proxy
attempt focused on stricter packet classification and smaller translation
modules.

## Current State

This proxy is proof-of-concept code and does not work properly yet. It is useful
for developers and testers who want to inspect the newer Rust translation path,
compare it with the earlier C++ proxy, or run controlled local experiments.

Known rough areas include, but are not limited to:

- placeable and live-object packet translation
- area loading and area transitions
- quickbar, inventory, and module packet alignment
- NWSync/content advertisement details
- encrypted EE session lifecycle handling
- crashes and desyncs under real play
- multi-client/account handling

The CD-key path also still needs real design work. The proxy can read a local
Diamond-format `nwncdkey.ini` and derive the legacy verifier expected by HG.
That makes it a local testing mechanism for one player/account at a time, not a
proper public multi-user authentication system.

Expect to debug it.

## Requirements

- Windows x64.
- Rust toolchain with Cargo.
- A C compiler available to Cargo's `cc` crate, such as Visual Studio 2022 Build
  Tools with the C++ workload.
- NWN:EE for local client testing.
- A local Diamond-format `nwncdkey.ini` when testing against HG authentication.

The proxy builds a vendored legacy `libhydrogen` compatibility copy from
`third_party/libhydrogen-legacy-nwn`.

## Build

```powershell
cargo build --release
```

The release executable is written to:

```text
target\release\hgbridge_proxy2.exe
```

Show available options:

```powershell
.\target\release\hgbridge_proxy2.exe --help
```

## Run

For local testing, start the proxy on the same machine as the NWN:EE client:

```powershell
.\target\release\hgbridge_proxy2.exe `
  --listen 127.0.0.1:5121 `
  --server 213 `
  --diamond-cdkey C:\path\to\nwncdkey.ini `
  --strict-translate
```

Then direct-connect the NWN:EE client to:

```text
127.0.0.1:5121
```

`--server 213` is a built-in shortcut for the HG 213 endpoint. You can also pass
an explicit `host:port` value.

Useful options:

```text
--listen <ADDR:PORT>            Listen address. Defaults to 0.0.0.0:5121.
--server <SERVER>               HG shortcut or explicit upstream host:port.
--diamond-cdkey <PATH>          Diamond nwncdkey.ini used for legacy auth.
--strict-translate              Drop packets that are not classified as known.
--packet-dump                   Emit more packet logging.
--log <PATH>                    Write logs to a file.
--allow-remote-clients          Accept non-loopback clients.
--session-timeout-ms <MS>       Expire inactive UDP sessions.
```

`--allow-remote-clients` is only for controlled tests. This proxy is not ready
to expose as a public service.

## CD-Key Source

Prefer passing the CD-key file explicitly:

```powershell
.\target\release\hgbridge_proxy2.exe --diamond-cdkey C:\path\to\nwncdkey.ini
```

The proxy also checks `HG_BRIDGE_DIAMOND_CDKEY_PATH`. Do not place CD-key files
inside this repo.

## Assets And NWSync

No asset bundle is included here. Keep local HAK, TLK, override, and generated
NWSync content outside the repo, for example under `hg-bridge-assets`.

The Rust proxy does not currently provide a complete public NWSync/content
solution. Treat asset setup as part of local test environment preparation.

## Progress Log

- Initial public Rust proxy snapshot: strict translation structure, EE crypto handling, Diamond CD-key derivation, module/area/live-object/quickbar translation experiments.
- Expanded proxy2 translation work: module resources, quickbar payloads, area context, live-object updates, and M-frame sequencing/reassembly helpers.
- Split proxy2 translation internals into focused M-frame, live-object update, custom token, and profile modules while preserving the experimental proxy build.

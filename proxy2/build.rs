fn main() {
    // The live-object translator deliberately retains several bounded,
    // decompile-backed retry candidates. Windows' default 1 MiB executable
    // stack is too small for the largest observed HG area-load packet even
    // though every individual evidence value is size-capped by tests. Reserve
    // enough stack for that finite parser pipeline; other targets keep their
    // platform defaults.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=hgbridge_proxy2=/STACK:8388608");
    }

    println!("cargo:rerun-if-changed=fixtures");
    println!("cargo:rustc-check-cfg=cfg(hgbridge_private_fixtures)");
    if std::path::Path::new("fixtures/live_object/player_appearance_false_u09.bin").is_file() {
        println!("cargo:rustc-cfg=hgbridge_private_fixtures");
    }

    println!("cargo:rerun-if-changed=../third_party/libhydrogen-legacy-nwn/hydrogen.c");
    println!("cargo:rerun-if-changed=../third_party/libhydrogen-legacy-nwn/hydrogen.h");
    cc::Build::new()
        .file("../third_party/libhydrogen-legacy-nwn/hydrogen.c")
        .include("../third_party/libhydrogen-legacy-nwn")
        .warnings(false)
        .compile("hydrogen_legacy_nwn");
}

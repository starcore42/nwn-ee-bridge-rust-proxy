fn main() {
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

fn main() {
    println!("cargo:rerun-if-changed=../third_party/libhydrogen-legacy-nwn/hydrogen.c");
    println!("cargo:rerun-if-changed=../third_party/libhydrogen-legacy-nwn/hydrogen.h");
    cc::Build::new()
        .file("../third_party/libhydrogen-legacy-nwn/hydrogen.c")
        .include("../third_party/libhydrogen-legacy-nwn")
        .warnings(false)
        .compile("hydrogen_legacy_nwn");
}

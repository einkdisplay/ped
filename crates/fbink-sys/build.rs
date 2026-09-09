use std::env;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").expect("missing Cargo target");
    assert_eq!(
        target, "armv7-unknown-linux-musleabihf",
        "fbink-sys currently supports only the Kindle ARMv7 MUSL target"
    );

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let fbink_dir = manifest_dir.join("vendor/fbink");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let sysroot = "/usr/local/musl/armv7-unknown-linux-musleabihf";

    for rel in [
        "fbink.h",
        "fbink.c",
        "fbink_internal.h",
        "cutef8/utf8.c",
        "cutef8/dfa.c",
        "qimagescale/qimagescale.c",
        "../wrapper.h",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            if rel.starts_with("../") {
                manifest_dir.join(rel.trim_start_matches("../")).display().to_string()
            } else {
                fbink_dir.join(rel).display().to_string()
            }
        );
    }
    // wrapper lives next to build.rs
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("wrapper.h").display()
    );

    // Build only the Kindle IMAGE source set. FBInk's top-level Makefile
    // unconditionally initializes its Kobo-only i2c-tools submodule, which
    // is neither present nor relevant for the Kindle framebuffer backend.
    cc::Build::new()
        .file(fbink_dir.join("fbink.c"))
        .file(fbink_dir.join("cutef8/utf8.c"))
        .file(fbink_dir.join("cutef8/dfa.c"))
        .file(fbink_dir.join("qimagescale/qimagescale.c"))
        .include(&fbink_dir)
        .include(fbink_dir.join("cutef8"))
        .include(fbink_dir.join("qimagescale"))
        .define("FBINK_FOR_KINDLE", None)
        .define("FBINK_MINIMAL", None)
        .define("FBINK_WITH_DRAW", None)
        .define("FBINK_WITH_IMAGE", None)
        .flag("-march=armv7-a")
        .flag("-mfpu=vfpv3-d16")
        .flag("-mfloat-abi=hard")
        .flag("-O2")
        .compile("fbink");

    let bindings = bindgen::Builder::default()
        .header(manifest_dir.join("wrapper.h").display().to_string())
        .clang_arg(format!("-I{}", fbink_dir.display()))
        .clang_arg("--target=armv7-unknown-linux-musleabihf")
        .clang_arg(format!("--sysroot={sysroot}"))
        .clang_arg("-DFBINK_FOR_KINDLE")
        .clang_arg("-DFBINK_MINIMAL")
        .clang_arg("-DFBINK_WITH_DRAW")
        .clang_arg("-DFBINK_WITH_IMAGE")
        .allowlist_type("FBInkConfig")
        .allowlist_type("FBInkRect")
        .allowlist_function(
            "fbink_(open|init|print_raw_data|refresh_rect|wait_for_submission|wait_for_complete|close)",
        )
        .generate()
        .expect("failed to generate FBInk bindings");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write FBInk bindings");
}

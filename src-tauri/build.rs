use std::path::Path;

fn main() {
    tauri_build::build();
    build_lame();
}

/// Compile the vendored libmp3lame (LAME 3.100) via the `cc` crate with the
/// same explicit defines as the known-good MSVC path, so it builds identically
/// on Linux/gcc and Windows/MSVC (the autotools build of the sys crate
/// miscompiles and crashes on encode).
fn build_lame() {
    let lame = Path::new("vendor/lame");
    let src = lame.join("libmp3lame");

    let files = [
        "bitstream", "encoder", "fft", "gain_analysis", "id3tag", "lame",
        "newmdct", "presets", "psymodel", "quantize_pvt", "quantize", "reservoir",
        "set_get", "tables", "takehiro", "util", "vbrquantize", "VbrTag", "version",
    ];

    let mut cc = cc::Build::new();
    cc.warnings(false).extra_warnings(false);
    for f in files {
        cc.file(src.join(format!("{f}.c")));
    }
    cc.file(src.join("vector/xmm_quantize_sub.c"));
    cc.include(lame.join("include"))
        .include(&src)
        .include(lame) // for config.h (from configMS.h)
        .define("TAKEHIRO_IEEE754_HACK", None)
        .define("FLOAT8", Some("float"))
        .define("REAL_IS_FLOAT", Some("1"))
        .define("BS_FORMAT", Some("BINARY"))
        .define("HAVE_CONFIG_H", None)
        .define("STDC_HEADERS", None);
    cc.compile("mp3lame");

    println!("cargo:rerun-if-changed=vendor/lame");
}

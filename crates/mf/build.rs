fn main() {
    println!("cargo:rerun-if-changed=src/nvenc.c");
    println!("cargo:rerun-if-changed=../../vendor/nv-codec-headers/nvEncodeAPI.h");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
    {
        cc::Build::new()
            .file("src/nvenc.c")
            .include("../../vendor/nv-codec-headers")
            .compile("noni_nvenc");
    }
}

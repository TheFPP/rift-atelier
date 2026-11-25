use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let header = format!(
        "#define RIFT_VERSION_NUM {},{},{},0\n#define RIFT_VERSION_STR \"{}\"\n",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH"),
        env!("CARGO_PKG_VERSION"),
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build directory"));
    fs::write(out.join("rift_version.h"), header).expect("write Windows version metadata");
    embed_resource::compile("app.rc", embed_resource::ParamsIncludeDirs([out]))
        .manifest_required()
        .expect("compile Rift Atelier Windows resources");
}

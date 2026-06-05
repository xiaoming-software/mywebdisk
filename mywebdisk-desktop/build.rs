use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let sdk_dir = manifest_dir.join("../sdk");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");

    let (lib_file, link_name) = match target_os.as_str() {
        "macos" => ("libwebrpc-Mac.a", "webrpc-Mac"),
        "windows" => ("libwebrpc-Windows.a", "webrpc-Windows"),
        other => panic!("unsupported target OS for static webrpc link: {other}"),
    };

    let lib_path = sdk_dir.join(lib_file);
    if !lib_path.exists() {
        panic!("missing static sdk library: {}", lib_path.display());
    }

    println!("cargo:rerun-if-changed={}", lib_path.display());
    println!("cargo:rustc-link-search=native={}", sdk_dir.display());
    println!("cargo:rustc-link-lib=static={link_name}");

    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }

    if target_os == "windows" {
        println!("cargo:rustc-link-lib=ws2_32");
        println!("cargo:rustc-link-lib=userenv");
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=bcrypt");
    }
}

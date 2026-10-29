fn main() {
    // macOS: ONNX Runtime's prebuilt static library calls clang's availability helper
    // (___isPlatformVersionAtLeast), which lives in clang's compiler runtime. rustc links with
    // -nodefaultlibs, so add that runtime explicitly or the release link fails.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let Ok(out) = std::process::Command::new("clang").arg("--print-resource-dir").output() else { return };
        let dir = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()).join("lib/darwin");
        if dir.join("libclang_rt.osx.a").exists() {
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
    }
}

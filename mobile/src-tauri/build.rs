fn main() {
    // transcribe-cpp asks to link pthread, which Android keeps inside libc:
    // an empty libpthread.a satisfies the flag. Its C++ needs libc++ linked
    // statically into the app's library.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let stub = out.join("libpthread.a");
        // An empty ar archive is just its magic header.
        std::fs::write(&stub, b"!<arch>\n").unwrap();
        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-arg=-lc++_static");
        println!("cargo:rustc-link-arg=-lc++abi");
    }
    tauri_build::build()
}

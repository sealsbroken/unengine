//! sherpa-onnx ships as shared libraries next to the executable; let the binary find them there
//! (and in cargo's `deps/` during development) without DYLD/LD_LIBRARY_PATH.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("apple") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path");
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/deps");
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN/deps");
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN/lib");
    }
}

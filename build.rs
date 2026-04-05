fn main() {
    // Try pkg-config first (Linux/macOS)
    if pkg_config::probe_library("espeak-ng").is_ok() {
        return;
    }

    // Fallback: try linking directly
    // On Windows, espeak-ng is typically installed to C:\Program Files\eSpeak NG
    if cfg!(target_os = "windows") {
        // Try common Windows install paths
        let paths = [
            r"C:\Program Files\eSpeak NG",
            r"C:\Program Files (x86)\eSpeak NG",
        ];
        for path in &paths {
            let lib_path = format!("{}\\lib", path);
            if std::path::Path::new(&lib_path).exists() {
                println!("cargo:rustc-link-search=native={}", lib_path);
                println!("cargo:rustc-link-lib=dylib=espeak-ng");
                return;
            }
        }
        // Try vcpkg-installed path
        if let Ok(vcpkg_root) = std::env::var("VCPKG_ROOT") {
            let lib_path = format!("{}\\installed\\x64-windows\\lib", vcpkg_root);
            if std::path::Path::new(&lib_path).exists() {
                println!("cargo:rustc-link-search=native={}", lib_path);
                println!("cargo:rustc-link-lib=dylib=espeak-ng");
                return;
            }
        }
    }

    println!("cargo:rustc-link-lib=dylib=espeak-ng");
}

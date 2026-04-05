fn main() {
    // If ESPEAK_LIB_DIR is set (e.g. from vcpkg CI), use it directly
    if let Ok(lib_dir) = std::env::var("ESPEAK_LIB_DIR") {
        println!("cargo:rustc-link-search=native={}", lib_dir);
        println!("cargo:rustc-link-lib=dylib=espeak-ng");
        return;
    }

    // Try pkg-config (Linux/macOS)
    if pkg_config::probe_library("espeak-ng").is_ok() {
        return;
    }

    // Windows fallback: try common install paths
    if cfg!(target_os = "windows") {
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
        // Try vcpkg default
        if let Ok(vcpkg_root) = std::env::var("VCPKG_INSTALLATION_ROOT") {
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

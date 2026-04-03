fn main() {
    if cfg!(target_os = "windows") {
        // Delay-load WinDivert.dll so extract_windivert() in main() can
        // write the DLL to disk before it's actually needed.
        println!("cargo:rustc-link-arg=/DELAYLOAD:WinDivert.dll");
        println!("cargo:rustc-link-lib=delayimp");

        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Gos3lih");
        res.set("FileDescription", "Real-time network monitor and bandwidth throttler");
        res.set("LegalCopyright", "Copyright © 2026 Gos3lih");
        res.compile().unwrap_or_else(|e| {
            eprintln!("winres failed (non-fatal): {e}");
        });
    }
}

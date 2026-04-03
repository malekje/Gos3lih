fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "NetFlow-Pro");
        res.set("FileDescription", "Real-time network monitor and bandwidth throttler");
        res.set("LegalCopyright", "Copyright © 2026 NetFlow-Pro");
        res.compile().unwrap_or_else(|e| {
            eprintln!("winres failed (non-fatal): {e}");
        });
    }
}

fn main() {
    #[cfg(target_family = "windows")]
    {
        const MAJOR: u64 = 0;
        const MINOR: u64 = 0;
        const PATCH: u64 = 1;
        const RELEASE: u64 = 0;
        let mut res = winres::WindowsResource::new();

        res.set("CompanyName", "Your Company Name");
        res.set("FileDescription", "TUI based audio client for Vox Oxide");
        res.set("ProductName", "Vox Oxide");
        res.set("OriginalFilename", "vox-oxide.exe");
        res.set("LegalCopyright", "MIT License");

        res.set_version_info(
            winres::VersionInfo::FILEVERSION,
            MAJOR << 48 | MINOR << 32 | PATCH << 16 | RELEASE,
        );
        res.set_version_info(
            winres::VersionInfo::PRODUCTVERSION,
            MAJOR << 48 | MINOR << 32 | PATCH << 16 | RELEASE,
        );

        res.set_icon("vox-oxide.ico");

        return res.compile().unwrap();
    }
}

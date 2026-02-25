fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os == "windows" {
        const MAJOR: u64 = 0;
        const MINOR: u64 = 0;
        const PATCH: u64 = 1;
        const RELEASE: u64 = 0;

        winres::WindowsResource::new()
            .set_toolkit_path("/usr/bin")
            .set_windres_path("x86_64-w64-mingw32-windres")
            .set_language(0x0409)
            .set("FileDescription", "TUI based audio client for Vox Oxide")
            .set("ProductName", "Vox Oxide")
            .set("OriginalFilename", "vox-oxide.exe")
            .set("LegalCopyright", "MIT License")
            .set("InternalName", "TEST.EXE")
            .set_version_info(
                winres::VersionInfo::FILEVERSION,
                MAJOR << 48 | MINOR << 32 | PATCH << 16 | RELEASE,
            )
            .set_version_info(
                winres::VersionInfo::PRODUCTVERSION,
                MAJOR << 48 | MINOR << 32 | PATCH << 16 | RELEASE,
            )
            .set_icon("vox-oxide.ico")
            .compile()
            .unwrap();
    }
}

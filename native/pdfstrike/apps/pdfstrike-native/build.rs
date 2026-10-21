//! Embed PDFStrike's original app icon, version info, and per-monitor DPI manifest on Windows.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `PDFSTRIKE_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

#[path = "src/windows_manifest.rs"]
mod windows_manifest;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/pdfstrike-geometric.ico");
    println!("cargo:rerun-if-changed=src/windows_manifest.rs");
    println!("cargo:rerun-if-env-changed=PDFSTRIKE_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/pdfstrike-geometric.ico")
        .set_manifest(windows_manifest::WINDOWS_MANIFEST)
        .set("ProductName", "PDFStrike")
        .set("FileDescription", "PDFStrike local PDF editor")
        .set("LegalCopyright", "PDFStrike adaptation. PdfCraft contributors credited in NOTICE.")
        .set("OriginalFilename", "pdfstrike-native.exe")
        .set("InternalName", "pdfstrike-native");
    if let Err(e) = res.compile() {
        if std::env::var_os("PDFSTRIKE_REQUIRE_WINRES").is_some() {
            println!("cargo::error=embedding Windows resources failed: {e}");
            return;
        }
        println!("cargo:warning=pdfstrike-native.exe built without icon/version resources: {e}");
    }
}

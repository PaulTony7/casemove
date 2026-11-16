//! Embeds the window icon and version metadata into the Windows executable.
//! Build-script only; nothing here is part of the app on other platforms.

fn main() {
    println!("cargo:rerun-if-changed=assets/casemove.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/casemove.ico");
    resource.set("ProductName", "Casemove");
    resource.set("FileDescription", "CS2 storage-unit and trade-up manager");
    resource.set("LegalCopyright", "Casemove contributors; see NOTICE");
    resource.set("Comments", "GPL-3.0-or-later; see LICENSE and NOTICE");
    if let Err(error) = resource.compile() {
        // A missing resource compiler should not fail the build; the app just
        // ships without the embedded icon.
        println!("cargo:warning=could not embed the Windows icon: {error}");
    }
}

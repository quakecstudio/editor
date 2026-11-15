// SPDX-License-Identifier: GPL-3.0-or-later

//! Embeds the icon and the version information in `quakec-studio.exe` on Windows, as vouch
//! does: Explorer, the taskbar and the file types the MSI registers show the icon, and
//! Windows Defender's heuristics score an executable with no version information as more
//! suspect.

fn main() -> std::io::Result<()> {
    println!("cargo::rerun-if-changed=../../packaging/windows/quakec-studio.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../packaging/windows/quakec-studio.ico")
            .set("ProductName", "QuakeC Studio")
            .set("FileDescription", "QuakeC Studio")
            .set("CompanyName", "QuakeC Studio contributors")
            .set(
                "LegalCopyright",
                "Copyright (C) 2026 QuakeC Studio contributors. GPL-3.0-or-later.",
            )
            .set("OriginalFilename", "quakec-studio.exe")
            .set("InternalName", "quakec-studio")
            .compile()?;
    }
    Ok(())
}

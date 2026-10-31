//! The tray menu (`tray-icon`, `muda`) imports `SetWindowSubclass` and other
//! functions that only Common Controls 6 exports by name, so a program
//! without a manifest asking for it does not start. The app gets that
//! manifest from gpui (`gpui.manifest.xml`, linked into every program that
//! uses gpui). The host's own test programs do not use gpui, so they get
//! theirs here. `rustc-link-arg` only reaches this package's own linked
//! programs (its tests), never the app.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources/windows");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let dir = std::path::Path::new("resources/windows");
        embed_resource::compile_for_everything(
            dir.join("tests.rc"),
            embed_resource::ParamsIncludeDirs([dir]),
        )
        .manifest_required()
        .unwrap();
    }
}

fn main() {
    tauri_build::build();
    // Cargo examples 沒有主程式的 Windows resource，mock updater 仍需 v6 controls。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref()==Ok("windows") {
        let manifest=std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("probe.manifest");
        std::fs::write(&manifest,r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency></assembly>"#).unwrap();
        println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}",manifest.display());
    }
}

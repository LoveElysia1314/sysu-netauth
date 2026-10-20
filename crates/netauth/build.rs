fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows.rc");
    println!("cargo:rerun-if-changed=../../packaging/gui.manifest");
    println!("cargo:rerun-if-changed=../../assets");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let out =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("windows.res");
        let status = std::process::Command::new("rc.exe")
            .arg("/nologo")
            .arg(format!("/fo{}", out.display()))
            .arg("../../packaging/windows.rc")
            .status()
            .expect("rc.exe is required (run in a Visual Studio developer shell)");
        assert!(status.success(), "Windows resource compilation failed");
        println!(
            "cargo:rustc-link-arg-bin=sysu_netauth_gui={}",
            out.display()
        );
    }
}

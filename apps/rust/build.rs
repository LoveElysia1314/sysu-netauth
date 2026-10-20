fn main() {
    let git_version = git_version::git_version!(
        args = ["--tags", "--match", "v[0-9]*", "--abbrev=0"],
        fallback = "0.0.0"
    );
    let version = git_version.strip_prefix('v').unwrap_or(git_version);
    println!("cargo:rustc-env=NETAUTH_APP_VERSION={version}");
    println!("cargo:rerun-if-changed=packaging/windows.rc");
    println!("cargo:rerun-if-changed=packaging/gui.manifest");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-env-changed=NETAUTH_RC");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let out =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("windows.res");
        let compiler = std::env::var_os("NETAUTH_RC").unwrap_or_else(|| {
            if cfg!(windows) {
                "rc.exe".into()
            } else {
                "llvm-rc".into()
            }
        });
        let packaging = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("packaging");
        let status = std::process::Command::new(&compiler)
            .current_dir(packaging)
            .arg("/nologo")
            .arg(format!("/fo{}", out.display()))
            .arg("windows.rc")
            .status()
            .unwrap_or_else(|e|panic!("cannot run resource compiler {compiler:?}: {e}; use a VS developer shell on Windows, or install LLVM and set NETAUTH_RC on Linux"));
        assert!(status.success(), "Windows resource compilation failed");
        println!(
            "cargo:rustc-link-arg-bin=sysu_netauth_gui={}",
            out.display()
        );
    }
}

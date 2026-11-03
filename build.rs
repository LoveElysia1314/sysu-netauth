fn main() {
    let git_version = git_version::git_version!(
        args = ["--tags", "--match", "v[0-9]*", "--abbrev=0"],
        fallback = "0.0.0"
    );
    let version = git_version.strip_prefix('v').unwrap_or(git_version);
    println!("cargo:rerun-if-env-changed=NETAUTH_RELEASE_TAG");
    if let Ok(tag) = std::env::var("NETAUTH_RELEASE_TAG") {
        if !tag.is_empty() {
            assert_eq!(
                version,
                tag.trim_start_matches('v'),
                "cached Git version differs from release tag; rebuild with a clean Cargo target directory"
            );
        }
    }
    println!("cargo:rustc-env=NETAUTH_APP_VERSION={version}");
    println!("cargo:rerun-if-changed=packaging/windows.rc");
    println!("cargo:rerun-if-changed=packaging/gui.manifest");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-env-changed=NETAUTH_RC");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let gnu = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join(if gnu {
            "windows.o"
        } else {
            "windows.res"
        });
        let compiler = std::env::var_os("NETAUTH_RC").unwrap_or_else(|| {
            if gnu {
                "x86_64-w64-mingw32-windres".into()
            } else if cfg!(windows) {
                "rc.exe".into()
            } else {
                "llvm-rc".into()
            }
        });
        let packaging = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("packaging");
        let mut command = std::process::Command::new(&compiler);
        command.current_dir(packaging);
        if gnu {
            command.args([
                "--input",
                "windows.rc",
                "--output-format",
                "coff",
                "--output",
            ]);
            command.arg(&out);
        } else {
            command
                .arg("/nologo")
                .arg(format!("/fo{}", out.display()))
                .arg("windows.rc");
        }
        let status = command
            .status()
            .unwrap_or_else(|e|panic!("cannot run resource compiler {compiler:?}: {e}; install the Windows SDK (MSVC), LLVM (cross MSVC), or MinGW binutils (GNU), or set NETAUTH_RC"));
        assert!(status.success(), "Windows resource compilation failed");
        println!(
            "cargo:rustc-link-arg-bin=sysu_netauth_gui={}",
            out.display()
        );
    }
}

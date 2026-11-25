//! The launch scripts are shell; nothing else watches them. These tests
//! keep `proton/` honest without needing a prefix or a Proton install.

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask lives in the workspace")
            .into()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("victory-xtask-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// A stand-in Proton: the verbs `run.sh` uses, with no Wine behind them.
    /// `probe` false makes `getnativepath` fail, which exercises the plain
    /// prefix fallback.
    fn fake_proton(dir: &Path, prefix: &Path, probe: bool) -> PathBuf {
        fs::create_dir_all(dir).expect("proton dir");
        let script = dir.join("proton");
        let native = if probe {
            format!("printf '%s\\n' '{}/drive_c'", prefix.display())
        } else {
            "exit 1".to_string()
        };
        let body = format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
               getnativepath) {native} ;;\n\
               getcompatpath) printf 'Z:%s\\n' \"$2\" ;;\n\
               waitforexitandrun)\n\
                 shift\n\
                 printf 'launched %s %s umu=%s overrides=%s\\n' \"$0\" \"$*\" \"${{UMU_ID:-}}\" \"${{WINEDLLOVERRIDES:-}}\"\n\
                 ;;\n\
               *) printf 'unexpected verb %s\\n' \"$1\" >&2; exit 2 ;;\n\
             esac\n"
        );
        fs::write(&script, body).expect("fake proton");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        dir.to_path_buf()
    }

    fn launch(prefix: &Path, proton: &Path, exe: &Path) -> Output {
        Command::new("bash")
            .args(["proton/run.sh", "--prefix"])
            .arg(prefix)
            .arg("--proton")
            .arg(proton)
            .arg("--exe")
            .arg(exe)
            .args(["--", "config"])
            .current_dir(root())
            .output()
            .expect("bash")
    }

    #[test]
    fn proton_scripts_parse_and_are_executable() {
        let root = root();
        for script in ["proton/run.sh", "proton/smoke.sh"] {
            let status = Command::new("bash")
                .args(["-n", script])
                .current_dir(&root)
                .status()
                .expect("bash");
            assert!(status.success(), "{script} does not parse");
            let mode = fs::metadata(root.join(script))
                .expect("script exists")
                .permissions();
            assert_ne!(mode.mode() & 0o111, 0, "{script} is not executable");
        }
    }

    #[test]
    fn proton_launcher_help_needs_nothing_installed() {
        let out = Command::new("bash")
            .args(["proton/run.sh", "--help"])
            .current_dir(root())
            .output()
            .expect("bash");
        assert!(out.status.success(), "run.sh --help failed");
        let text = String::from_utf8_lossy(&out.stdout);
        for flag in [
            "--prefix",
            "--exe",
            "--proton",
            "--z",
            "--config",
            "--d3dcompiler",
            "--texture-wait",
        ] {
            assert!(text.contains(flag), "usage misses {flag}");
        }
    }

    #[test]
    fn proton_launcher_creates_a_fresh_prefix() {
        let tmp = temp_dir("fresh-prefix");
        let prefix = tmp.join("prefix");
        let proton = fake_proton(&tmp.join("proton"), &prefix, false);
        let exe = tmp.join("victory.exe");
        fs::write(&exe, b"not a real exe").expect("stand-in exe");

        let out = launch(&prefix, &proton, &exe);
        assert!(
            out.status.success(),
            "first run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let z = prefix.join("dosdevices/z:");
        assert_eq!(
            fs::read_link(&z).expect("z: is a symlink"),
            Path::new("../drive_z"),
            "z: must be relative to dosdevices"
        );
        assert!(prefix.join("drive_z").is_dir(), "drive_z is missing");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("umu=victory"),
            "the UMU launch is what runs wine directly: {stdout}"
        );
        assert!(
            stdout.contains("Z:"),
            "the exe must be passed as a Windows path: {stdout}"
        );

        let out = launch(&prefix, &proton, &exe);
        assert!(
            out.status.success(),
            "second run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn proton_launcher_pins_a_native_d3dcompiler() {
        let tmp = temp_dir("d3dcompiler");
        let prefix = tmp.join("prefix");
        let proton = fake_proton(&tmp.join("proton"), &prefix, true);
        let exe = tmp.join("victory.exe");
        let dll = tmp.join("d3dcompiler_47.dll");
        fs::write(&exe, b"not a real exe").expect("stand-in exe");
        fs::write(&dll, b"a stand-in DLL").expect("stand-in DLL");

        let out = Command::new("bash")
            .args(["proton/run.sh", "--prefix"])
            .arg(&prefix)
            .arg("--proton")
            .arg(&proton)
            .arg("--exe")
            .arg(&exe)
            .arg("--d3dcompiler")
            .arg(&dll)
            .args(["--", "config"])
            .current_dir(root())
            .output()
            .expect("bash");
        assert!(
            out.status.success(),
            "run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let pinned = prefix.join("drive_c/windows/system32/d3dcompiler_47.dll");
        assert_eq!(
            fs::read(&pinned).expect("pinned DLL"),
            b"a stand-in DLL".as_slice()
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("d3dcompiler_47=n"),
            "the native DLL must be forced: {stdout}"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn proton_launcher_prefers_ge_proton() {
        let tmp = temp_dir("ge-first");
        let tools = tmp.join("compatibilitytools.d");
        let prefix = tmp.join("prefix");
        let exe = tmp.join("victory.exe");
        fs::write(&exe, b"not a real exe").expect("stand-in exe");
        for name in ["GE-Proton10-9", "GE-Proton10-11", "Proton-9.0"] {
            let dir = tools.join(name);
            fs::create_dir_all(&dir).expect("tool dir");
            fake_proton(&dir, &prefix, true);
        }

        let run = || {
            Command::new("bash")
                .args(["proton/run.sh", "--prefix"])
                .arg(&prefix)
                .arg("--exe")
                .arg(&exe)
                .args(["--", "config"])
                .current_dir(root())
                .env("STEAM_COMPAT_TOOLS", &tools)
                .env_remove("PROTON")
                .output()
                .expect("bash")
        };
        let out = run();
        assert!(
            out.status.success(),
            "run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("GE-Proton10-11/proton"),
            "GE-Proton must win: {stdout}"
        );
        assert!(
            !stdout.contains("Proton-9.0/proton"),
            "plain Proton must not win over GE: {stdout}"
        );

        for name in ["GE-Proton10-9", "GE-Proton10-11"] {
            fs::remove_dir_all(tools.join(name)).expect("remove GE");
        }
        let out = run();
        assert!(
            out.status.success(),
            "fallback run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("Proton-9.0/proton"),
            "plain Proton must be the fallback: {stdout}"
        );
        let _ = fs::remove_dir_all(&tmp);
    }
}

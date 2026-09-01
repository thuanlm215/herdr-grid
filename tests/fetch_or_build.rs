#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Outcome {
    output: Output,
    installed: Vec<u8>,
    urls: String,
    mode: u32,
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn sha256(path: &Path) -> String {
    for (program, args) in [("sha256sum", vec![]), ("shasum", vec!["-a", "256"])] {
        if let Ok(output) = Command::new(program).args(args).arg(path).output() {
            if output.status.success() {
                return String::from_utf8(output.stdout)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_string();
            }
        }
    }
    panic!("tests require sha256sum or shasum");
}

fn run(os: &str, arch: &str, asset: &str, missing: bool, corrupt: bool) -> Outcome {
    let root = tempfile::tempdir().unwrap();
    let bin_dir = root.path().join("bin");
    let fixture_dir = root.path().join("fixture");
    let output_path = root.path().join("target/release/herdr-grid");
    let urls_path = root.path().join("urls");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&fixture_dir).unwrap();

    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"herdr-grid\"\nversion = \"9.8.7\"\n",
    )
    .unwrap();
    fs::write(fixture_dir.join("binary"), b"verified prebuilt\n").unwrap();
    let digest = if corrupt {
        "0".repeat(64)
    } else {
        sha256(&fixture_dir.join("binary"))
    };
    fs::write(
        fixture_dir.join("SHA256SUMS"),
        format!("{digest}  herdr-grid-{asset}\n"),
    )
    .unwrap();

    write_executable(
        &bin_dir.join("uname"),
        "#!/bin/sh\ncase \"$1\" in -s) printf '%s\\n' \"$TEST_OS\" ;; -m) printf '%s\\n' \"$TEST_ARCH\" ;; esac\n",
    );
    write_executable(
        &bin_dir.join("curl"),
        r#"#!/bin/sh
dest=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) dest="$2"; shift 2 ;;
    -*) shift ;;
    *) url="$1"; shift ;;
  esac
done
printf '%s\n' "$url" >> "$URL_LOG"
case "$url" in
  */SHA256SUMS) cp "$FIXTURE_DIR/SHA256SUMS" "$dest" ;;
  *)
    [ "${MISSING_BINARY:-0}" = 1 ] && exit 22
    cp "$FIXTURE_DIR/binary" "$dest"
    ;;
esac
"#,
    );
    write_executable(
        &bin_dir.join("cargo"),
        "#!/bin/sh\nmkdir -p \"$(dirname \"$HERDR_GRID_OUT\")\"\nprintf 'source build\\n' > \"$HERDR_GRID_OUT\"\nprintf 'FAKE_CARGO %s\\n' \"$*\"\n",
    );

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/fetch-or-build.sh");
    let output = Command::new("sh")
        .arg(script)
        .env("PATH", path)
        .env("HOME", root.path())
        .env("TEST_OS", os)
        .env("TEST_ARCH", arch)
        .env("FIXTURE_DIR", &fixture_dir)
        .env("URL_LOG", &urls_path)
        .env("MISSING_BINARY", if missing { "1" } else { "0" })
        .env("HERDR_GRID_REPO_ROOT", root.path())
        .env("HERDR_GRID_CARGO_TOML", root.path().join("Cargo.toml"))
        .env("HERDR_GRID_OUT", &output_path)
        .env(
            "HERDR_GRID_RELEASE_BASE_URL",
            "https://example.invalid/releases/download",
        )
        .output()
        .unwrap();

    let installed = fs::read(&output_path).unwrap_or_default();
    let mode = fs::metadata(&output_path)
        .map(|metadata| metadata.mode())
        .unwrap_or_default();
    let urls = fs::read_to_string(&urls_path).unwrap_or_default();
    Outcome {
        output,
        installed,
        urls,
        mode,
    }
}

#[test]
fn downloads_and_verifies_every_release_platform() {
    for (os, arch, triple) in [
        ("Darwin", "arm64", "aarch64-apple-darwin"),
        ("Darwin", "x86_64", "x86_64-apple-darwin"),
        ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
        ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
    ] {
        let outcome = run(os, arch, triple, false, false);
        assert!(
            outcome.output.status.success(),
            "{}",
            String::from_utf8_lossy(&outcome.output.stderr)
        );
        assert_eq!(outcome.installed, b"verified prebuilt\n");
        assert_ne!(
            outcome.mode & 0o111,
            0,
            "installed binary must be executable"
        );
        assert!(
            outcome
                .urls
                .contains(&format!("/v9.8.7/herdr-grid-{triple}")),
            "{}",
            outcome.urls
        );
        assert!(outcome.urls.contains("/v9.8.7/SHA256SUMS"));
        assert!(!String::from_utf8_lossy(&outcome.output.stdout).contains("FAKE_CARGO"));
    }
}

#[test]
fn missing_asset_falls_back_to_locked_source_build() {
    let outcome = run("Linux", "x86_64", "x86_64-unknown-linux-musl", true, false);
    assert!(outcome.output.status.success());
    assert_eq!(outcome.installed, b"source build\n");
    assert!(String::from_utf8_lossy(&outcome.output.stdout)
        .contains("FAKE_CARGO build --release --locked"));
}

#[test]
fn checksum_mismatch_never_installs_the_download() {
    let outcome = run(
        "Linux",
        "aarch64",
        "aarch64-unknown-linux-musl",
        false,
        true,
    );
    assert!(outcome.output.status.success());
    assert_eq!(outcome.installed, b"source build\n");
    assert!(String::from_utf8_lossy(&outcome.output.stderr).contains("checksum mismatch"));
}

#[test]
fn unsupported_platform_falls_back_without_downloading() {
    let outcome = run("Linux", "riscv64", "unused", false, false);
    assert!(outcome.output.status.success());
    assert_eq!(outcome.installed, b"source build\n");
    assert!(outcome.urls.is_empty());
}

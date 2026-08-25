use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn isolated_home() -> TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd
}

fn plugin_library_name(name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("starforge_{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("libstarforge_{name}.dylib")
    } else {
        format!("libstarforge_{name}.so")
    }
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{} failed\nstdout:\n{}\nstderr:\n{}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, context: &str) {
    assert!(
        !output.status.success(),
        "{} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_signed_plugin_fixture(
    dir: &Path,
    name: &str,
    starforge_version: &str,
) -> (PathBuf, SigningKey, String, String) {
    fs::create_dir_all(dir).expect("create plugin fixture dir");
    let lib = dir.join(plugin_library_name(name));
    let lib_bytes = b"sample dynamic library payload for plugin signature test";
    fs::write(&lib, lib_bytes).expect("write plugin library");

    let mut rng = rand::thread_rng();
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();
    let pk_bytes = verifying_key.to_bytes();
    let g_addr = stellar_strkey::ed25519::PublicKey(pk_bytes).to_string();

    let digest: [u8; 32] = Sha256::digest(lib_bytes).into();
    let signature = signing_key.sign(&digest);
    let sig_hex = hex::encode(signature.to_bytes());

    fs::write(
        dir.join("starforge-plugin.toml"),
        format!(
            r#"
name = "{name}"
version = "1.0.0"
starforge_version = "{starforge_version}"
description = "signed test plugin"
publisher = "StarForge Test Publisher"
publisher_key = "{g_addr}"
signature = "{sig_hex}"
"#
        ),
    )
    .expect("write plugin manifest");

    (lib, signing_key, g_addr, sig_hex)
}

fn create_unsigned_plugin_fixture(dir: &Path, name: &str, starforge_version: &str) -> PathBuf {
    fs::create_dir_all(dir).expect("create plugin fixture dir");
    let lib = dir.join(plugin_library_name(name));
    fs::write(&lib, b"unsigned dynamic library payload").expect("write plugin library");
    fs::write(
        dir.join("starforge-plugin.toml"),
        format!(
            r#"
name = "{name}"
version = "1.0.0"
starforge_version = "{starforge_version}"
description = "unsigned test plugin"
"#
        ),
    )
    .expect("write plugin manifest");
    lib
}

// ── Primary Flow Test ─────────────────────────────────────────────────────────

#[test]
fn test_primary_flow_signed_plugin_verification() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("signed_primary");
    let (lib, _sk, g_addr, _sig) =
        create_signed_plugin_fixture(&fixture_dir, "signed_primary", env!("CARGO_PKG_VERSION"));

    let install = starforge(home.path())
        .args([
            "plugin",
            "install",
            "signed_primary",
            "--path",
            lib.to_str().unwrap(),
            "--source",
            "https://github.com/StarForge-Labs/signed_primary",
        ])
        .output()
        .expect("run plugin install");
    assert_success(&install, "install signed plugin");

    let stdout_install = String::from_utf8_lossy(&install.stdout);
    assert!(stdout_install.contains("Verification"));
    assert!(stdout_install.contains("verified"));
    assert!(stdout_install.contains("Publisher"));

    let list = starforge(home.path())
        .args(["plugin", "list"])
        .output()
        .expect("run plugin list");
    assert_success(&list, "list signed plugin");
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(list_stdout.contains("signed_primary"));
    assert!(list_stdout.contains("verified"));

    let list_json = starforge(home.path())
        .args(["plugin", "list", "--json"])
        .output()
        .expect("run plugin list --json");
    assert_success(&list_json, "list signed plugin json");
    let json_str = String::from_utf8_lossy(&list_json.stdout);
    assert!(json_str.contains("\"verification_status\": \"verified\""));
    assert!(json_str.contains(&g_addr));

    let audit = starforge(home.path())
        .args(["plugin", "audit", "signed_primary"])
        .output()
        .expect("run plugin audit");
    assert_success(&audit, "audit signed plugin");
    let audit_stdout = String::from_utf8_lossy(&audit.stdout);
    assert!(audit_stdout.contains("signature"));
    assert!(audit_stdout.contains("verified"));
}

// ── Boundary Case Tests ───────────────────────────────────────────────────────

#[test]
fn test_boundary_unsigned_plugin_allowed_by_default() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("unsigned_default");
    let lib =
        create_unsigned_plugin_fixture(&fixture_dir, "unsigned_default", env!("CARGO_PKG_VERSION"));

    let install = starforge(home.path())
        .args([
            "plugin",
            "install",
            "unsigned_default",
            "--path",
            lib.to_str().unwrap(),
        ])
        .output()
        .expect("install unsigned plugin");
    assert_success(&install, "install unsigned plugin under default config");

    let audit = starforge(home.path())
        .args(["plugin", "audit", "unsigned_default"])
        .output()
        .expect("audit unsigned plugin");
    assert_success(&audit, "audit unsigned plugin under default config");
    let stdout = String::from_utf8_lossy(&audit.stdout);
    assert!(stdout.contains("unsigned"));
}

#[test]
fn test_boundary_trusted_publisher_in_config() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("trusted_pub");
    let (lib, _sk, g_addr, _sig) =
        create_signed_plugin_fixture(&fixture_dir, "trusted_pub", env!("CARGO_PKG_VERSION"));

    // Add publisher to config
    let config_dir = home.path().join(".starforge");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
version = "1"
network = "testnet"

[plugin_trust]
trusted_publishers = ["{g_addr}"]
"#
        ),
    )
    .unwrap();

    let install = starforge(home.path())
        .args([
            "plugin",
            "install",
            "trusted_pub",
            "--path",
            lib.to_str().unwrap(),
        ])
        .output()
        .expect("install signed plugin with publisher in config");
    assert_success(&install, "install signed plugin with publisher in config");
}

// ── Failure Case Tests ────────────────────────────────────────────────────────

#[test]
fn test_failure_tampered_binary_rejected() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("tampered");
    let (lib, _sk, _g_addr, _sig) =
        create_signed_plugin_fixture(&fixture_dir, "tampered", env!("CARGO_PKG_VERSION"));

    // Tamper with binary content after signing
    fs::write(&lib, b"TAMPERED DATA THAT DOES NOT MATCH SIGNATURE").unwrap();

    let install = starforge(home.path())
        .args([
            "plugin",
            "install",
            "tampered",
            "--path",
            lib.to_str().unwrap(),
        ])
        .output()
        .expect("install tampered plugin");
    assert_failure(&install, "install tampered plugin");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(combined.contains("Signature verification failed") || combined.contains("invalid"));
}

#[test]
fn test_failure_malformed_publisher_key_rejected() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("bad_key");
    fs::create_dir_all(&fixture_dir).unwrap();
    let lib = fixture_dir.join(plugin_library_name("bad_key"));
    fs::write(&lib, b"dummy payload").unwrap();

    fs::write(
        fixture_dir.join("starforge-plugin.toml"),
        format!(
            r#"
name = "bad_key"
version = "1.0.0"
starforge_version = "{}"
publisher_key = "INVALID_NOT_A_STELLAR_KEY"
signature = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
"#,
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();

    let install = starforge(home.path())
        .args([
            "plugin",
            "install",
            "bad_key",
            "--path",
            lib.to_str().unwrap(),
        ])
        .output()
        .expect("install bad key plugin");
    assert_failure(&install, "install bad key plugin");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(combined.contains("invalid 'publisher_key'") || combined.contains("publisher_key"));
}

#[test]
fn test_failure_untrusted_publisher_key_rejected_when_list_configured() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("untrusted_pub");
    let (lib, _sk, _g_addr, _sig) =
        create_signed_plugin_fixture(&fixture_dir, "untrusted_pub", env!("CARGO_PKG_VERSION"));

    // Configure a DIFFERENT trusted publisher
    let config_dir = home.path().join(".starforge");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        r#"
version = "1"
network = "testnet"

[plugin_trust]
trusted_publishers = ["GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFXYCTXLWFM2AC2DELDW2"]
"#,
    )
    .unwrap();

    let install = starforge(home.path())
        .args([
            "plugin",
            "install",
            "untrusted_pub",
            "--path",
            lib.to_str().unwrap(),
        ])
        .output()
        .expect("install plugin from untrusted publisher key");
    assert_failure(&install, "install plugin from untrusted publisher key");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(combined.contains("untrusted publisher") || combined.contains("Untrusted Publisher"));
}

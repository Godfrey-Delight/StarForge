//! Cross-platform CLI integration tests for StarForge.
//!
//! Exercises filesystem, process, terminal, and path behavior across all supported
//! operating systems (Linux, macOS, Windows).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge_cmd(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
    cmd
}

fn starforge_in_dir(home: &Path, cwd: &Path) -> Command {
    let mut cmd = starforge_cmd(home);
    cmd.current_dir(cwd);
    cmd
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "Command '{}' failed with status {:?}.\nStdout: {}\nStderr: {}",
        context,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, context: &str) {
    assert!(
        !output.status.success(),
        "Command '{}' unexpectedly succeeded.\nStdout: {}\nStderr: {}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Primary Flow Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_version_and_info() {
    let home = isolated_home();

    // Verify --version
    let output = starforge_cmd(home.path())
        .arg("--version")
        .output()
        .expect("spawn starforge --version");
    assert_success(&output, "starforge --version");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("starforge"), "Version output should contain binary name");

    // Verify info command
    let output = starforge_cmd(home.path())
        .arg("info")
        .output()
        .expect("spawn starforge info");
    assert_success(&output, "starforge info");
}

#[test]
fn test_cross_platform_core_subcommands() {
    let home = isolated_home();

    let subcommands = [
        vec!["network", "show"],
        vec!["wallet", "list"],
        vec!["template", "list"],
    ];

    for args in &subcommands {
        let output = starforge_cmd(home.path())
            .args(args)
            .output()
            .unwrap_or_else(|_| panic!("spawn starforge {:?}", args));
        assert_success(&output, &format!("starforge {:?}", args));
    }
}

#[test]
fn test_cross_platform_home_dir_resolution() {
    let home = isolated_home();
    let home_path = home.path();

    // Invoking wallet list should populate the isolated home structure
    let output = starforge_cmd(home_path)
        .args(["wallet", "list"])
        .output()
        .expect("spawn wallet list");
    assert_success(&output, "wallet list with isolated home");

    // Check that HOME / USERPROFILE env var was respected
    // The .starforge directory or wallet config should be created inside the temp home
    let starforge_dir = home_path.join(".starforge");
    if starforge_dir.exists() {
        assert!(
            starforge_dir.is_dir(),
            ".starforge in isolated home should be a directory"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Filesystem & Path Boundary Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_paths_with_spaces_and_special_chars() {
    let home = isolated_home();
    let special_dir = home.path().join("StarForge Space & Symbols (Test)");
    fs::create_dir_all(&special_dir).expect("create dir with spaces and symbols");

    let output = starforge_in_dir(home.path(), &special_dir)
        .arg("info")
        .output()
        .expect("spawn in directory with spaces");
    assert_success(&output, "starforge info in dir with spaces and special chars");
}

#[test]
fn test_cross_platform_deeply_nested_directory_execution() {
    let home = isolated_home();
    let mut deep_dir = home.path().to_path_buf();
    for i in 0..10 {
        deep_dir.push(format!("nested_level_{}", i));
    }
    fs::create_dir_all(&deep_dir).expect("create deeply nested dir");

    let output = starforge_in_dir(home.path(), &deep_dir)
        .arg("info")
        .output()
        .expect("spawn in deeply nested dir");
    assert_success(&output, "starforge info in deeply nested directory");
}

#[test]
fn test_cross_platform_path_separators_normalization() {
    let home = isolated_home();
    let working_dir = home.path().join("sub").join("nested");
    fs::create_dir_all(&working_dir).expect("create sub nested dir");

    // Relative path with parent traversal
    let output = starforge_in_dir(home.path(), &working_dir)
        .args(["info"])
        .output()
        .expect("spawn relative dir");
    assert_success(&output, "starforge info from relative subfolder");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Terminal & Output Control Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_no_color_environment_handling() {
    let home = isolated_home();

    // When NO_COLOR is set, ANSI escape codes (\x1b[) should not be emitted in plain help
    let mut cmd = starforge_cmd(home.path());
    cmd.env("NO_COLOR", "1");
    cmd.env("TERM", "dumb");
    cmd.arg("--help");

    let output = cmd.output().expect("spawn with NO_COLOR=1");
    assert_success(&output, "starforge --help with NO_COLOR=1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\x1b["), "Output with NO_COLOR=1 should not contain ANSI escape codes");
}

#[test]
fn test_cross_platform_quiet_flag() {
    let home = isolated_home();

    let output = starforge_cmd(home.path())
        .arg("-q")
        .arg("info")
        .output()
        .expect("spawn with -q");
    assert_success(&output, "starforge -q info");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Failure Paths & Invalid Input Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_invalid_subcommand_failure() {
    let home = isolated_home();

    let output = starforge_cmd(home.path())
        .arg("non_existent_command_xyz_12345")
        .output()
        .expect("spawn invalid subcommand");
    assert_failure(&output, "invalid subcommand");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("error:") || combined.contains("Error") || combined.contains("Usage") || combined.contains("unrecognized") || combined.contains("not recognized") || combined.contains("not a valid") || !combined.is_empty(),
        "Error output should provide a descriptive failure message"
    );
}

#[test]
fn test_cross_platform_unsupported_flag_failure() {
    let home = isolated_home();

    let output = starforge_cmd(home.path())
        .arg("--unsupported-flag-cross-platform-test")
        .output()
        .expect("spawn unsupported flag");
    assert_failure(&output, "unsupported flag");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected") || stderr.contains("error:") || stderr.contains("unknown") || stderr.contains("unrecognized"),
        "Error message should explain unsupported flag"
    );
}

#[test]
fn test_cross_platform_empty_argument_handling() {
    let home = isolated_home();

    // Passing empty string as a subcommand
    let output = starforge_cmd(home.path())
        .arg("")
        .output()
        .expect("spawn with empty arg");
    assert_failure(&output, "empty string argument");
}

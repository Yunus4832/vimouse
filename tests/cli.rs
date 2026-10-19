use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
use vimouse::status::Runtime;

static NUMBER: AtomicUsize = AtomicUsize::new(0);
fn directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "vimouse-test-{}-{}",
        std::process::id(),
        NUMBER.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn scripted_demo_runs_without_any_device_access() {
    let path = directory();
    let mut child = Command::new(env!("CARGO_BIN_EXE_vimouse"))
        .args(["demo", "--no-hud", "--fast", "--runtime-dir"])
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"caps l:100 m a g g ' a v j:100 v : q enter\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Quit"));
    assert!(text.contains("\"marks\":1"));
    assert!(!path.join("status.json").exists());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn runtime_lock_excludes_second_controller_and_can_be_reacquired() {
    let path = directory();
    let first = Runtime::acquire(path.clone()).unwrap();
    assert!(Runtime::acquire(path.clone()).is_err());
    drop(first);
    drop(Runtime::acquire(path.clone()).unwrap());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn runtime_rejects_a_symlink_instead_of_following_it() {
    use std::os::unix::fs::symlink;
    let real = directory();
    let alias = directory();
    drop(Runtime::acquire(real.clone()).unwrap());
    symlink(&real, &alias).unwrap();
    assert!(Runtime::acquire(alias.clone()).is_err());
    fs::remove_file(alias).unwrap();
    fs::remove_dir_all(real).unwrap();
}

#[test]
fn config_roundtrips_and_unknown_cli_arguments_fail() {
    let binary = env!("CARGO_BIN_EXE_vimouse");
    let output = Command::new(binary).arg("config").output().unwrap();
    assert!(output.status.success());
    let config: vimouse::config::Config =
        toml::from_str(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    assert!(config.validate().is_ok());
    let output = Command::new(binary)
        .args(["run", "--typo"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
}

#[test]
fn missing_wayland_session_is_reported_before_starting_a_controller() {
    let path = directory();
    fs::create_dir(&path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vimouse"))
        .args(["demo", "--fast"])
        .env("PATH", &path)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Wayland") || error.contains("without sudo"),
        "{error}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Demo:"));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn qt_is_never_launched_by_the_native_hud() {
    use std::os::unix::fs::PermissionsExt;
    let path = directory();
    fs::create_dir(&path).unwrap();
    let executable = path.join("qml6");
    let sentinel = path.join("qt-was-launched");
    fs::write(
        &executable,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", sentinel.display()),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vimouse"))
        .args(["hud", "--preview", "--runtime-dir"])
        .arg(path.join("runtime"))
        .env("PATH", &path)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!sentinel.exists());
    fs::remove_dir_all(path).unwrap();
}

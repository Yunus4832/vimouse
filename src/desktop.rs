//! Bounded read-only native probes, isolated from the input controller.
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn probe(command: &str) -> Result<String, String> {
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                return if output.status.success() {
                    String::from_utf8(output.stdout).map_err(|e| e.to_string())
                } else {
                    Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => e.to_string(),
                    _ => "Wayland probe timed out; check the desktop session".into(),
                });
            }
        }
    }
}
pub fn check_hud() -> Result<(), String> {
    probe("check-hud").map(|_| ())
}

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

/// Owns the overlay child. Health checks reap an exited child and fail closed;
/// the backend releases input before the user service restarts the application.
pub struct HudProcess {
    state: HudState,
}
enum HudState {
    Disabled,
    Running(std::process::Child),
    Exited(std::process::ExitStatus),
}
impl HudProcess {
    pub fn new(child: Option<std::process::Child>) -> Self {
        Self {
            state: match child {
                Some(child) => HudState::Running(child),
                None => HudState::Disabled,
            },
        }
    }
    pub fn check(&mut self) -> std::io::Result<()> {
        if let HudState::Running(child) = &mut self.state {
            if let Some(status) = child.try_wait()? {
                self.state = HudState::Exited(status);
            }
        }
        if let HudState::Exited(status) = self.state {
            return Err(std::io::Error::other(format!(
                "HUD exited ({status}); stopped input control"
            )));
        }
        Ok(())
    }
}
impl Drop for HudProcess {
    fn drop(&mut self) {
        if let HudState::Running(child) = &mut self.state {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_and_exited_huds_are_distinct_and_failure_is_sticky() {
        let mut disabled = HudProcess::new(None);
        assert!(disabled.check().is_ok());
        let mut child = std::process::Command::new("/bin/true").spawn().unwrap();
        child.wait().unwrap();
        let mut exited = HudProcess::new(Some(child));
        assert!(exited.check().is_err());
        assert!(exited.check().is_err());
    }
}

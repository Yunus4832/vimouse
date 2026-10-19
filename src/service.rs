//! User service installation and control. Never runs systemctl as root.
use crate::setup::atomic_write;
use std::{
    env,
    ffi::OsStr,
    fs, io,
    os::{
        linux::net::SocketAddrExt,
        unix::{
            ffi::OsStrExt,
            net::{SocketAddr, UnixDatagram},
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const UNIT: &str = "vimouse.service";
const SESSION: &[&str] = &[
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "NIRI_SOCKET",
];

fn user_only() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        Err("Service commands must run as your desktop user, without sudo".into())
    } else {
        Ok(())
    }
}
fn paths() -> Result<(PathBuf, PathBuf), String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or("HOME must be an absolute path")?;
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if !config.is_absolute() {
        return Err("XDG_CONFIG_HOME must be an absolute path".into());
    }
    Ok((home.join(".local/bin/vimouse"), config.join("systemd/user")))
}
fn systemctl(args: &[&str]) -> Result<(), String> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .map_err(|e| {
            format!(
                "Cannot run systemctl --user: {e}. This command requires a systemd user session"
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "systemctl --user {} failed ({status}). Inspect `journalctl --user -u vimouse.service -n 40`",
            args.join(" ")
        ))
    }
}
fn manager_ready() -> Result<(), String> {
    let status = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("Cannot find systemctl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("Cannot connect to the systemd user manager. Run in your desktop login session, without sudo".into())
    }
}
fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn unit(binary: &Path) -> Result<String, String> {
    let path = binary.to_str().ok_or("Executable path must be UTF-8")?;
    Ok(include_str!("../packaging/vimouse.service").replace("%h/.local/bin/vimouse", &quote(path)))
}
fn save_session() -> Result<(), String> {
    // Store only the display connection settings, scoped to this unit. Do not
    // import the whole shell environment or change other services' environment.
    if env::var_os("WAYLAND_DISPLAY").is_none() {
        return Ok(());
    }
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })));
    if !runtime.is_absolute() {
        return Err("XDG_RUNTIME_DIR must be absolute".into());
    }
    let directory = runtime.join("systemd/user");
    let mut text =
        String::from("# Managed by vimouse; desktop connection settings only.\n[Service]\n");
    for key in SESSION {
        if let Some(value) = env::var_os(key) {
            let value = value
                .to_str()
                .ok_or_else(|| format!("{key} must be UTF-8"))?;
            text.push_str(&format!(
                "Environment={}\n",
                quote(&format!("{key}={value}"))
            ));
        } else {
            // Clear stale compositor-specific variables when switching desktops.
            text.push_str(&format!("UnsetEnvironment={key}\n"));
        }
    }
    atomic_write(
        &directory.join("vimouse.service.d/session.conf"),
        text.as_bytes(),
        0o600,
    )
}

fn install_binary(binary: &Path) -> Result<(), String> {
    let source =
        fs::read(env::current_exe().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    // Read before replacing, so updating the installed executable itself is safe.
    atomic_write(binary, &source, 0o755)
}

pub fn execute(command: &str, args: Vec<String>) -> Result<(), String> {
    user_only()?;
    if !args.is_empty() {
        return Err(format!("Usage: vimouse {command} (no options)"));
    }
    if command == "install" {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or("HOME must be an absolute path")?;
        let binary = home.join(".local/bin/vimouse");
        install_binary(&binary)?;
        println!(
            "Installed {}. Permissions and services are unchanged.",
            binary.display()
        );
        if !env::var_os("PATH")
            .is_some_and(|value| env::split_paths(&value).any(|p| p == home.join(".local/bin")))
        {
            println!(
                "Add {} to PATH to use the `vimouse` command.",
                home.join(".local/bin").display()
            );
        }
        return Ok(());
    }
    let (binary, directory) = paths()?;
    match command {
        "install-service" => {
            manager_ready()?;
            install_binary(&binary)?;
            atomic_write(&directory.join(UNIT), unit(&binary)?.as_bytes(), 0o644)?;
            save_session()?;
            // Migrate the former two-service layout without creating duplicate HUDs.
            if directory.join("vimouse-hud.service").exists() {
                systemctl(&["disable", "--now", "vimouse-hud.service"])?;
            }
            systemctl(&["daemon-reload"])?;
            systemctl(&["enable", UNIT])?;
            println!(
                "Installed {} and enabled {UNIT} for graphical login.",
                binary.display()
            );
            println!(
                "Not started. Run `vimouse start`; if already running, use `vimouse stop` then `vimouse start` to load this version."
            );
        }
        "start" => {
            if !directory.join(UNIT).is_file() {
                return Err(
                    "User service is not installed. Run `vimouse install-service` first".into(),
                );
            }
            save_session()?;
            systemctl(&["daemon-reload"])?;
            systemctl(&["reset-failed", UNIT])?;
            systemctl(&["start", UNIT])?;
            println!("Vimouse started (or was already running). Stop: vimouse stop");
        }
        "stop" => {
            systemctl(&["stop", UNIT])?;
            // Also stop a legacy split HUD when present.
            if directory.join("vimouse-hud.service").exists() {
                systemctl(&["stop", "vimouse-hud.service"])?;
            }
            println!("Vimouse stopped. Login autostart is still enabled.");
        }
        "status" => systemctl(&["--no-pager", "status", UNIT])?,
        _ => return Err(format!("Unknown service command: {command}")),
    }
    Ok(())
}

fn notify(socket: Option<&OsStr>) -> io::Result<()> {
    let Some(socket) = socket else { return Ok(()) };
    let bytes = socket.as_bytes();
    let address = if bytes.first() == Some(&b'@') {
        SocketAddr::from_abstract_name(&bytes[1..])?
    } else {
        SocketAddr::from_pathname(socket)?
    };
    UnixDatagram::unbound()?.send_to_addr(
        b"READY=1\nSTATUS=Keyboard capture and mouse control ready (Insert)",
        &address,
    )?;
    Ok(())
}
pub fn notify_ready() -> io::Result<()> {
    notify(env::var_os("NOTIFY_SOCKET").as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unit_quotes_paths_and_disables_expansion_in_literal_names() {
        let text = unit(Path::new("/home/space and%percent/$dollar/vimouse")).unwrap();
        assert!(text.contains("ExecStart=\"/home/space and%%percent/$dollar/vimouse\" run\n"));
        assert!(text.contains("Type=notify"));
        assert!(!text.contains("--no-hud"));
        assert_eq!(
            quote("DISPLAY=a\nEnvironment=bad"),
            "\"DISPLAY=a\\nEnvironment=bad\""
        );
    }
    #[test]
    fn readiness_uses_a_datagram_without_systemd_libraries() {
        let name = format!("vimouse-notify-test-{}", std::process::id());
        let address = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        let socket = UnixDatagram::bind_addr(&address).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        notify(Some(OsStr::new(&format!("@{name}")))).unwrap();
        let mut bytes = [0u8; 256];
        let count = socket.recv(&mut bytes).unwrap();
        assert!(bytes[..count].starts_with(b"READY=1\n"));
        notify(None).unwrap();
    }
}

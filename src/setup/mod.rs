//! One-time privileged device access setup. Normal input and HUD stay unprivileged.
mod acl;
use std::{
    env,
    ffi::{CStr, OsStr},
    fs::{self, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
};

fn executable(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|p| p.join(name))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}
fn admin_tool(name: &str) -> Result<PathBuf, String> {
    ["/usr/sbin", "/usr/bin", "/sbin", "/bin"]
        .iter()
        .map(|p| Path::new(p).join(name))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
        .ok_or_else(|| format!("Required system setup tool `{name}` is missing"))
}
fn admin_run(name: &str, args: &[&OsStr]) -> Result<(), String> {
    let status = Command::new(admin_tool(name)?)
        .args(args)
        .status()
        .map_err(|e| format!("{name}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed ({status}); init stopped"))
    }
}

pub(crate) fn atomic_write(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    let parent = path.parent().ok_or("path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".vimouse-install-{}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(data).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn username(uid: u32) -> Result<String, String> {
    if uid == 0 {
        return Err("init must grant access to a normal desktop account, not root".into());
    }
    let mut data = vec![0u8; 65536];
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    // SAFETY: all buffers outlive getpwuid_r and are writable for their given lengths.
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            data.as_mut_ptr().cast(),
            data.len(),
            &mut result,
        )
    };
    if code != 0 || result.is_null() {
        return Err(format!("Cannot resolve desktop account UID {uid}"));
    }
    // SAFETY: success returned a passwd whose string data is inside the live buffer.
    let name = unsafe { CStr::from_ptr((*result).pw_name) }
        .to_str()
        .map_err(|e| e.to_string())?
        .to_owned();
    if name.is_empty() || name.starts_with('-') {
        return Err("Invalid account name".into());
    }
    Ok(name)
}

fn kernel_release() -> Result<String, String> {
    let mut uts = std::mem::MaybeUninit::<libc::utsname>::uninit();
    // SAFETY: uts points to storage large enough for uname.
    if unsafe { libc::uname(uts.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: uname succeeded and release is a NUL-terminated string.
    Ok(unsafe { CStr::from_ptr((*uts.as_ptr()).release.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}
fn require_matching_modules(release: &str, available: bool) -> Result<(), String> {
    if available {
        Ok(())
    } else {
        Err(format!(
            "uinput is unavailable and modules for running kernel {release} are missing. After a kernel upgrade, reboot into the installed kernel and retry `vimouse init`. In a container, expose the host uinput device. No permission changes were made."
        ))
    }
}
fn configure(uid: u32) -> Result<(), String> {
    let user = username(uid)?;
    for name in ["groupadd", "usermod", "udevadm"] {
        admin_tool(name)?;
    }
    if crate::linux::check_uinput().is_err() {
        let release = kernel_release()?;
        require_matching_modules(&release, Path::new("/lib/modules").join(&release).is_dir())?;
        admin_run("modprobe", &[OsStr::new("uinput")])?;
        crate::linux::check_uinput().map_err(|e| {
            format!("uinput kernel API is still unavailable: {e}; permission setup has not started")
        })?;
    }
    println!("Configuring input access for {user} (UID {uid}).");
    admin_run("groupadd", &[OsStr::new("-f"), OsStr::new("vimouse")])?;
    admin_run(
        "usermod",
        &[
            OsStr::new("-aG"),
            OsStr::new("vimouse"),
            OsStr::new("--"),
            OsStr::new(&user),
        ],
    )?;
    atomic_write(
        Path::new("/etc/udev/rules.d/70-vimouse.rules"),
        include_bytes!("../../packaging/70-vimouse.rules"),
        0o644,
    )?;
    atomic_write(
        Path::new("/etc/modules-load.d/vimouse.conf"),
        b"uinput\n",
        0o644,
    )?;
    for args in [
        vec!["control", "--reload-rules"],
        vec!["trigger", "--subsystem-match=input"],
        vec!["trigger", "--subsystem-match=misc"],
        vec!["settle", "--timeout=5"],
    ] {
        admin_run("udevadm", &args.iter().map(OsStr::new).collect::<Vec<_>>())?;
    }
    let uinput = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open("/dev/uinput")
        .map_err(|e| e.to_string())?;
    acl::grant_fd(uinput.as_raw_fd(),uid).map_err(|e|format!("Persistent group/rules installed, but immediate uinput ACL failed: {e}. Log out and back in before running Vimouse."))?;
    let keyboards = crate::linux::keyboards();
    for (path, device) in &keyboards {
        acl::grant_fd(device.as_raw_fd(),uid).map_err(|e|format!("Immediate keyboard ACL failed on {}: {e}. Log out and back in to apply group access.",path.display()))?;
    }
    println!(
        "Input access configured: uinput and {} keyboard(s). No setfacl/libacl needed.",
        keyboards.len()
    );
    println!(
        "Log out/in once so group membership also covers newly connected devices and user services."
    );
    println!("The vimouse group can read keyboards and inject input; only add trusted accounts.");
    Ok(())
}

pub fn init(args: Vec<String>) -> Result<(), String> {
    let root = unsafe { libc::geteuid() } == 0;
    if root {
        let uid=match args.as_slice() {
            []=>env::var("SUDO_UID").or_else(|_|env::var("PKEXEC_UID")).map_err(|_|"Run `vimouse init` as your normal desktop user; it requests administrator authentication automatically")?.parse::<u32>().map_err(|_|"Invalid invoking UID")?,
            [flag,uid] if flag=="--uid"=>uid.parse::<u32>().map_err(|_|"Invalid UID")?,
            _=>return Err("Usage: vimouse init (run as your normal desktop user)".into()),
        };
        return configure(uid);
    }
    if !args.is_empty() {
        return Err(
            "Usage: vimouse init; the current desktop account is selected automatically".into(),
        );
    }
    let uid = unsafe { libc::geteuid() };
    let user = username(uid)?;
    let helper=executable("sudo").or_else(||executable("pkexec"))
        .ok_or("Administrator authentication requires sudo or pkexec. Install one, then run `vimouse init` again")?;
    println!("Administrator authentication will configure keyboard/uinput access for {user} only.");
    let status = Command::new(helper)
        .arg(env::current_exe().map_err(|e| e.to_string())?)
        .args(["init", "--uid", &uid.to_string()])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "Permission setup failed or authentication was cancelled ({status})"
        ));
    }
    println!("Ready. Run `vimouse doctor`, or `vimouse install-service` then `vimouse start`.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_root_target_and_kernel_mismatch_fail_before_mutations() {
        assert!(username(0).is_err());
        assert!(
            require_matching_modules("missing-test-kernel", false)
                .unwrap_err()
                .contains("reboot")
        );
        assert!(require_matching_modules("present", true).is_ok());
    }
}

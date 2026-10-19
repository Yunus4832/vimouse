use evdev::KeyCode as K;
use std::{
    env,
    io::{self, BufRead},
    path::PathBuf,
    process::{Child, Command, ExitCode},
    time::{Duration, Instant},
};
use vimouse::{
    config::Config,
    engine::{Action, Engine},
    linux,
    status::{self, Runtime},
};

const HELP: &str = "Vimouse — modal mouse control for Linux

Usage: vimouse <command> [options]
  install        Copy this executable to ~/.local/bin/vimouse (no service changes)
  init           Request administrator authentication and configure input permissions
  install-service Install and enable the user service (does not start it)
  start          Start the installed user service
  stop           Stop the user service and its overlay
  status         Show user service status
  run            Capture keyboards and control the system mouse (device access required)
  demo           Exercise the same controller without capturing or injecting any input
  hud            Show the native status / command bar
  doctor         Check Linux input access (does not grab or create devices)
  check-uinput   Probe the existing uinput kernel API only (does not create devices)
  displays       Show logical display geometry and the active output
  list-devices   List readable physical keyboards
  config         Print the default TOML configuration
  licenses       Print embedded third-party license notices

Options:
  --config PATH       Load configuration (otherwise use $XDG_CONFIG_HOME/vimouse/config.toml if present)
  --runtime-dir DIR   Private 0700 directory for the lock and status.json
  --device PATH       Capture only this keyboard; repeat for multiple keyboards
  --no-hud            Run without launching the native Wayland overlay
  --preview           Show a sample HUD without running the controller (hud only)
  --fast              Skip real-time delays (demo only)

Demo input (one or more space-separated tokens per line):
  caps l:500 a j:200 m a g g ' a v l:200 v : q enter
  Hold a key/chord: h:500, alt+j:200, ctrl+enter, shift+i
  Idle: wait:1000. Quit: : q enter or Ctrl+D.
  Quick jump: caps f a (5x5 a-y, then immediately back to Normal).
  Precision: tab p h tab j space (4x4 a-p centered at the pointer, then hjkl / Tab).
  Normal mode: Space cycles displays and centers the pointer; f jumps, Tab refines.

Emergency: Ctrl+Alt+Esc returns to insert; Ctrl+Alt+Backspace quits.
";

#[derive(Default)]
struct Options {
    config: Option<PathBuf>,
    runtime: Option<PathBuf>,
    devices: Vec<PathBuf>,
    no_hud: bool,
    preview: bool,
    fast: bool,
}

fn options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                options.config = Some(args.next().ok_or("--config requires a path")?.into())
            }
            "--runtime-dir" => {
                options.runtime = Some(args.next().ok_or("--runtime-dir requires a path")?.into())
            }
            "--device" => options
                .devices
                .push(args.next().ok_or("--device requires a path")?.into()),
            "--no-hud" => options.no_hud = true,
            "--preview" => options.preview = true,
            "--fast" => options.fast = true,
            _ => return Err(format!("unknown option: {arg}")),
        }
    }
    Ok(options)
}

fn load_config(explicit: Option<PathBuf>) -> Result<Config, String> {
    if let Some(path) = explicit {
        return Config::load(&path);
    }
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(base) = base {
        let path = base.join("vimouse/config.toml");
        if path.exists() {
            return Config::load(&path);
        }
    }
    Ok(Config::default())
}

// Use the same executable for the independent, unprivileged native overlay.
fn hud(directory: &std::path::Path, preview: bool) -> Result<Child, String> {
    let mut command = Command::new(env::current_exe().map_err(|e| e.to_string())?);
    command.arg("hud").arg("--runtime-dir").arg(directory);
    if preview {
        command.arg("--preview");
    }
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start the HUD: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(stdout).read_line(&mut line);
        let _ = send.send(result.is_ok() && line.trim() == "Vimouse HUD ready (native Wayland)");
    });
    if receive
        .recv_timeout(Duration::from_secs(4))
        .unwrap_or(false)
    {
        let _ = reader.join();
        Ok(child)
    } else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        Err("Native HUD did not become ready; input capture was not started".into())
    }
}

struct Hud(Option<Child>);
impl Drop for Hud {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn parse_key(text: &str) -> Result<Vec<K>, String> {
    let mut keys = Vec::new();
    for part in text.split('+') {
        let key = match part {
            "ctrl" => K::KEY_LEFTCTRL,
            "alt" => K::KEY_LEFTALT,
            "shift" => K::KEY_LEFTSHIFT,
            "super" => K::KEY_LEFTMETA,
            "caps" | "capslock" => K::KEY_CAPSLOCK,
            "enter" => K::KEY_ENTER,
            "esc" => K::KEY_ESC,
            "backspace" => K::KEY_BACKSPACE,
            "tab" => K::KEY_TAB,
            "space" => K::KEY_SPACE,
            "f8" => K::KEY_F8,
            "f9" => K::KEY_F9,
            "f10" => K::KEY_F10,
            "f11" => K::KEY_F11,
            "f12" => K::KEY_F12,
            ":" => {
                keys.push(K::KEY_LEFTSHIFT);
                K::KEY_SEMICOLON
            }
            "'" => K::KEY_APOSTROPHE,
            "`" => K::KEY_GRAVE,
            _ if part.len() == 1 => {
                let ch = part.chars().next().unwrap();
                if ch.is_ascii_uppercase() {
                    keys.push(K::KEY_LEFTSHIFT);
                }
                (1..=255)
                    .map(K)
                    .find(|key| {
                        vimouse::engine::character(*key, false) == Some(ch.to_ascii_lowercase())
                    })
                    .ok_or_else(|| format!("unknown key: {part}"))?
            }
            _ => return Err(format!("unknown key: {part}")),
        };
        keys.push(key);
    }
    Ok(keys)
}

fn demo(config: Config, mut runtime: Runtime, fast: bool) -> Result<(), String> {
    let tick = Duration::from_secs_f64(1.0 / f64::from(config.tick_hz));
    let (layout, source) = vimouse::displays::detect(config.screen);
    let mut engine = Engine::with_layout(config, layout);
    if let Some(name) = vimouse::displays::active_output(source) {
        engine.select_display(&name);
    }
    let mut last_status = Instant::now();
    runtime.write(&engine.status()).map_err(|e| e.to_string())?;
    println!(
        "Demo: no system input is captured or injected. Type `caps l:500 a j:200`, then Enter."
    );
    println!("{}", serde_json::to_string(&engine.status()).unwrap());
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim_start().starts_with('#') {
            continue;
        }
        for token in line.split_whitespace() {
            let (name, ms) = match token.rsplit_once(':') {
                Some((name, millis)) if !name.is_empty() && !millis.is_empty() => (
                    name,
                    millis
                        .parse::<u64>()
                        .map_err(|_| format!("invalid duration: {token}"))?,
                ),
                _ => (token, 0),
            };
            if ms > 86400000 {
                return Err("demo duration must be at most one day".into());
            }
            let keys = if name == "wait" {
                Vec::new()
            } else {
                parse_key(name)?
            };
            let mut actions = Vec::new();
            for key in &keys {
                actions.extend(engine.key(*key, 1));
            }
            let mut remaining = Duration::from_millis(ms);
            while !remaining.is_zero() {
                let elapsed = remaining.min(tick);
                actions.extend(engine.tick(elapsed));
                if !fast && last_status.elapsed() >= Duration::from_millis(40) {
                    runtime.write(&engine.status()).map_err(|e| e.to_string())?;
                    last_status = Instant::now();
                }
                if !fast {
                    std::thread::sleep(elapsed);
                }
                remaining = remaining.saturating_sub(elapsed);
            }
            for key in keys.iter().rev() {
                actions.extend(engine.key(*key, 0));
            }
            let quit = actions.contains(&Action::Quit);
            let pointers = actions
                .iter()
                .filter(|a| matches!(a, Action::Pointer(_)))
                .count();
            let discrete: Vec<_> = actions
                .iter()
                .filter(|a| !matches!(a, Action::Pointer(_)))
                .collect();
            println!("{token}: {pointers} pointer updates; {discrete:?}");
            println!("{}", serde_json::to_string(&engine.status()).unwrap());
            runtime.write(&engine.status()).map_err(|e| e.to_string())?;
            if quit {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn execute() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "--help".into());
    if ["--help", "-h", "help"].contains(&command.as_str()) {
        print!("{HELP}");
        return Ok(());
    }
    if command == "--version" {
        println!("vimouse {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if command == "init" {
        return vimouse::setup::init(args.collect());
    }
    if ["install", "install-service", "start", "stop", "status"].contains(&command.as_str()) {
        return vimouse::service::execute(&command, args.collect());
    }
    let options = options(args)?;
    if options.preview && command != "hud" {
        return Err("--preview is only valid for hud".into());
    }
    if options.fast && command != "demo" {
        return Err("--fast is only valid for demo".into());
    }
    if !options.devices.is_empty() && command != "run" {
        return Err("--device is only valid for run".into());
    }
    match command.as_str() {
        "licenses" => {
            print!("{}", include_str!("../packaging/THIRD-PARTY-NOTICES.txt"));
            Ok(())
        }
        "check-hud" => vimouse::hud::check(),
        "native-displays" => {
            let layout = vimouse::hud::layout()?;
            println!("{}", serde_json::to_string(&layout.outputs).unwrap());
            Ok(())
        }
        "displays" => {
            let config = load_config(options.config)?;
            let (layout, source) = vimouse::displays::detect(config.screen);
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({"source": format!("{source:?}"), "active": vimouse::displays::active_output(source), "layout": layout})).unwrap());
            Ok(())
        }
        "check-uinput" => {
            let version =
                linux::check_uinput().map_err(|e| format!("uinput kernel API unavailable: {e}"))?;
            println!("uinput kernel API {version}: ready");
            Ok(())
        }
        "doctor" => {
            if linux::doctor(!options.no_hud) {
                Ok(())
            } else {
                Err("Input access or HUD dependencies are not ready; see the checks above. Controller-only demo: vimouse demo --no-hud".into())
            }
        }
        "list-devices" => {
            linux::list();
            Ok(())
        }
        "config" => {
            print!("{}", toml::to_string_pretty(&Config::default()).unwrap());
            Ok(())
        }
        "run" | "demo" => {
            let config = load_config(options.config)?;
            if !options.no_hud {
                vimouse::desktop::check_hud()?;
            }
            let directory = options.runtime.unwrap_or_else(status::default_directory);
            let runtime = Runtime::acquire(directory.clone()).map_err(|e| e.to_string())?;
            let _hud = if options.no_hud {
                Hud(None)
            } else {
                match hud(&directory, false) {
                    Ok(child) => Hud(Some(child)),
                    Err(error) => return Err(error),
                }
            };
            if command == "demo" {
                demo(config, runtime, options.fast)
            } else {
                linux::run(config, options.devices, runtime).map_err(|e| e.to_string())
            }
        }
        "hud" => {
            let directory = options.runtime.unwrap_or_else(status::default_directory);
            status::prepare_directory(&directory).map_err(|e| e.to_string())?;
            vimouse::hud::run(&directory, options.preview)
        }
        _ => Err(format!("unknown command: {command}\nRun vimouse --help")),
    }
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("vimouse: {error}");
            ExitCode::FAILURE
        }
    }
}

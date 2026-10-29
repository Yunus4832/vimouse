//! evdev input capture + separate uinput keyboard and absolute pointer.
//! No X server dependency; the compositor consumes standard Linux input devices.
use crate::{
    config::Config,
    displays::{self, ABS_MAX, Layout},
    engine::{Action, Engine, Point},
    status::Runtime,
};
use evdev::{
    AbsInfo, AbsoluteAxisCode as Abs, AttributeSet, Device, EventType, InputEvent, KeyCode as K,
    PropType, RelativeAxisCode as Rel, UinputAbsSetup, uinput::VirtualDevice,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const PREFIX: &str = "Vimouse ";

pub fn keyboards() -> Vec<(PathBuf, Device)> {
    let mut devices: Vec<_> = evdev::enumerate()
        .filter(|(_, device)| {
            !device.name().unwrap_or("").starts_with(PREFIX)
                && device.supported_keys().is_some_and(|keys| {
                    [K::KEY_A, K::KEY_Z, K::KEY_ENTER, K::KEY_SPACE]
                        .iter()
                        .all(|key| keys.contains(*key))
                })
        })
        .collect();
    devices.sort_by(|a, b| a.0.cmp(&b.0));
    devices
}

pub fn list() {
    let devices = keyboards();
    if devices.is_empty() {
        println!("No readable keyboards. Check /dev/input permissions; see README.md.");
    }
    for (path, device) in devices {
        println!(
            "{}\t{}",
            path.display(),
            device.name().unwrap_or("Unknown keyboard")
        );
    }
}

/// Probe the existing kernel interface without creating or grabbing any device.
pub fn check_uinput() -> io::Result<u32> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")?;
    uinput_version(&file)
}

fn uinput_version(file: &std::fs::File) -> io::Result<u32> {
    let mut version: libc::c_uint = 0;
    // SAFETY: file owns a valid fd and UI_GET_VERSION writes exactly one c_uint.
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<libc::c_uint>(b'U' as u32, 45),
            &mut version,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if version < 5 {
        return Err(io::Error::other(format!(
            "uinput API {version} is too old; version 5 or newer is required"
        )));
    }
    Ok(version)
}

#[cfg(test)]
mod probe_tests {
    #[test]
    fn a_character_device_node_is_not_proof_of_a_working_uinput_interface() {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
            .unwrap();
        assert_eq!(
            super::uinput_version(&file).unwrap_err().raw_os_error(),
            Some(libc::ENOTTY)
        );
    }
}

pub fn doctor(include_hud: bool) -> bool {
    let mut okay = true;
    println!(
        "Session: {} / {}",
        std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()
    );
    for path in ["/dev/input", "/dev/uinput"] {
        if Path::new(path).exists() {
            println!("FOUND {path}");
        } else {
            println!("MISSING {path}");
            okay = false;
        }
    }
    match check_uinput() {
        Ok(version) => println!("uinput kernel API {version}: ready."),
        Err(error) => {
            println!(
                "uinput kernel API unavailable: {error}. Check device access and matching kernel modules (README.md)."
            );
            okay = false;
        }
    }
    if keyboards().is_empty() {
        println!("No readable physical keyboards.");
        okay = false;
    } else {
        list();
    }
    if include_hud {
        match crate::desktop::check_hud() {
            Ok(()) => println!("HUD: native Wayland layer-shell is available (no Qt required)."),
            Err(error) => {
                println!("HUD: {error}");
                okay = false;
            }
        }
    }
    if !okay {
        println!("Run `vimouse init` as your normal desktop user to configure device access.");
    }
    okay
}

struct Output {
    keyboard: VirtualDevice,
    pointer: VirtualDevice,
    keys: BTreeSet<K>,
    buttons: BTreeSet<K>,
    last_point: Option<Point>,
    layout: Layout,
}

impl Output {
    fn new(devices: &[(PathBuf, Device)], layout: &Layout) -> io::Result<Self> {
        let mut keys = AttributeSet::new();
        for (_, device) in devices {
            if let Some(supported) = device.supported_keys() {
                for key in supported.iter() {
                    // Keep mouse/gamepad buttons out of the virtual keyboard.
                    if key.0 < 0x100 || (0x160..0x2c0).contains(&key.0) {
                        keys.insert(key);
                    }
                }
            }
        }
        for key in [
            K::KEY_LEFT,
            K::KEY_RIGHT,
            K::KEY_UP,
            K::KEY_DOWN,
            K::KEY_ESC,
            K::KEY_ENTER,
        ] {
            keys.insert(key);
        }
        let keyboard = VirtualDevice::builder()?
            .name("Vimouse Keyboard")
            .with_keys(&keys)?
            .build()?;
        let mut buttons = AttributeSet::new();
        for key in [K::BTN_LEFT, K::BTN_RIGHT, K::BTN_MIDDLE] {
            buttons.insert(key);
        }
        let mut axes = AttributeSet::new();
        axes.insert(Rel::REL_WHEEL);
        axes.insert(Rel::REL_HWHEEL);
        let mut properties = AttributeSet::new();
        properties.insert(PropType::POINTER);
        let pointer = VirtualDevice::builder()?
            .name("Vimouse Absolute Pointer")
            .with_keys(&buttons)?
            .with_properties(&properties)?
            .with_relative_axes(&axes)?
            .with_absolute_axis(&UinputAbsSetup::new(
                Abs::ABS_X,
                AbsInfo::new(0, 0, ABS_MAX, 0, 0, 0),
            ))?
            .with_absolute_axis(&UinputAbsSetup::new(
                Abs::ABS_Y,
                AbsInfo::new(0, 0, ABS_MAX, 0, 0, 0),
            ))?
            .build()?;
        Ok(Self {
            keyboard,
            pointer,
            keys: BTreeSet::new(),
            buttons: BTreeSet::new(),
            last_point: None,
            layout: layout.clone(),
        })
    }

    fn emit(&mut self, actions: Vec<Action>) -> io::Result<bool> {
        for action in actions {
            match action {
                Action::Key(key, value) => {
                    self.keyboard
                        .emit(&[InputEvent::new(EventType::KEY.0, key.0, value)])?;
                    if value == 1 {
                        self.keys.insert(key);
                    } else if value == 0 {
                        self.keys.remove(&key);
                    }
                }
                Action::Button(key, down) => {
                    self.pointer.emit(&[InputEvent::new(
                        EventType::KEY.0,
                        key.0,
                        i32::from(down),
                    )])?;
                    if down {
                        self.buttons.insert(key);
                    } else {
                        self.buttons.remove(&key);
                    }
                }
                Action::Pointer(point) => {
                    let point = self.layout.absolute(point);
                    // The input core drops unchanged ABS values. Reasserting a
                    // saved virtual position after physical mouse use therefore
                    // needs a one-unit intermediate frame, followed by the target.
                    if self.last_point == Some(point) {
                        let adjacent = if point.x == 0 { 1 } else { point.x - 1 };
                        self.pointer.emit(&[InputEvent::new(
                            EventType::ABSOLUTE.0,
                            Abs::ABS_X.0,
                            adjacent,
                        )])?;
                    }
                    self.pointer.emit(&[
                        InputEvent::new(EventType::ABSOLUTE.0, Abs::ABS_X.0, point.x),
                        InputEvent::new(EventType::ABSOLUTE.0, Abs::ABS_Y.0, point.y),
                    ])?;
                    self.last_point = Some(point);
                }
                Action::Scroll {
                    horizontal,
                    vertical,
                } => self.pointer.emit(&[
                    InputEvent::new(EventType::RELATIVE.0, Rel::REL_HWHEEL.0, horizontal),
                    InputEvent::new(EventType::RELATIVE.0, Rel::REL_WHEEL.0, vertical),
                ])?,
                Action::Quit => return Ok(true),
            }
        }
        Ok(false)
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        for key in &self.keys {
            let _ = self
                .keyboard
                .emit(&[InputEvent::new(EventType::KEY.0, key.0, 0)]);
        }
        for button in &self.buttons {
            let _ = self
                .pointer
                .emit(&[InputEvent::new(EventType::KEY.0, button.0, 0)]);
        }
    }
}

/// Merges physical keyboards: releasing H on one must not stop H held on another.
#[derive(Default)]
pub struct KeyMerger {
    sources: BTreeSet<(usize, K)>,
    counts: BTreeMap<K, usize>,
}

impl KeyMerger {
    pub fn event(&mut self, source: usize, key: K, value: i32) -> Option<i32> {
        match value {
            1 if self.sources.insert((source, key)) => {
                let count = self.counts.entry(key).or_default();
                *count += 1;
                (*count == 1).then_some(1)
            }
            0 if self.sources.remove(&(source, key)) => {
                let count = self.counts.get_mut(&key)?;
                *count -= 1;
                (*count == 0).then_some(0)
            }
            2 if self.sources.contains(&(source, key)) => Some(2),
            _ => None,
        }
    }
}

pub fn run(
    config: Config,
    selected: Vec<PathBuf>,
    mut runtime: Runtime,
    mut check_health: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    check_health()?;
    // Load state before creating virtual devices or grabbing a physical keyboard.
    let (mark_store, saved_marks) = crate::marks::Store::open(crate::marks::default_path()?)?;
    eprintln!(
        "Named marks: {} loaded from {}",
        saved_marks.len(),
        mark_store.path().display()
    );
    let mut mark_worker = crate::marks::SaveWorker::spawn(mark_store)?;
    // All captured devices live in this inner call. They are dropped before the
    // outer worker is joined, even on an input error or HUD failure.
    run_input(
        config,
        selected,
        &mut runtime,
        saved_marks,
        &mut mark_worker,
        check_health,
    )
}

fn run_input(
    config: Config,
    selected: Vec<PathBuf>,
    runtime: &mut Runtime,
    saved_marks: crate::marks::Marks,
    mark_worker: &mut crate::marks::SaveWorker,
    mut check_health: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let mut devices = if selected.is_empty() {
        keyboards()
    } else {
        let mut devices = Vec::new();
        let mut unique = BTreeSet::new();
        for path in selected {
            let canonical = std::fs::canonicalize(&path)?;
            if !unique.insert(canonical) {
                return Err(io::Error::other("duplicate keyboard path"));
            }
            let device = Device::open(&path)?;
            if device.name().unwrap_or("").starts_with(PREFIX)
                || !device
                    .supported_keys()
                    .is_some_and(|k| k.contains(K::KEY_A) && k.contains(K::KEY_ENTER))
            {
                return Err(io::Error::other(format!(
                    "{} is not a physical keyboard",
                    path.display()
                )));
            }
            devices.push((path, device));
        }
        devices
    };
    if devices.is_empty() {
        return Err(io::Error::other(
            "no readable keyboards; run `vimouse doctor` and see README.md",
        ));
    }
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&stop))?;
    }
    let fallback_screen = config.screen;
    let (layout, mut display_source) = displays::detect(fallback_screen);
    eprintln!(
        "Displays: {} ({display_source:?}), desktop {}×{}",
        layout.outputs.len(),
        layout.bounds.width,
        layout.bounds.height
    );
    let mut output = Output::new(&devices, &layout).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("creating uinput devices: {e}; run `vimouse doctor`"),
        )
    })?;
    // Let udev/libinput discover virtual devices BEFORE grabbing a real keyboard.
    std::thread::sleep(Duration::from_secs(1));
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    check_health()?;
    for (path, device) in &mut devices {
        if device.get_key_state()?.iter().next().is_some() {
            return Err(io::Error::other(format!(
                "release all keys on {} and start again",
                path.display()
            )));
        }
        device.set_nonblocking(true)?;
        device
            .grab()
            .map_err(|e| io::Error::new(e.kind(), format!("grabbing {}: {e}", path.display())))?;
        eprintln!(
            "Capturing {} ({})",
            path.display(),
            device.name().unwrap_or("keyboard")
        );
    }
    // evdev::Device ungrabs in Drop, including partial startup failure and panic.
    let tick = Duration::from_secs_f64(1.0 / f64::from(config.tick_hz));
    let mut engine = Engine::with_layout(config, layout);
    engine.restore_marks(saved_marks);
    if let Some(name) = displays::active_output(display_source) {
        engine.select_display(&name);
    }
    runtime.write(&engine.status())?;
    let mut merger = KeyMerger::default();
    let mut last_tick = Instant::now();
    let mut last_status = Instant::now();
    let mut next_tick = Instant::now() + tick;
    let mut last_display_check = Instant::now();
    let mut last_input = Instant::now();
    eprintln!("Vimouse ready (insert). Ctrl+Alt+Esc releases control; Ctrl+Alt+Backspace quits.");
    crate::service::notify_ready()?;
    while !stop.load(Ordering::Relaxed) {
        check_health()?;
        for result in mark_worker.poll() {
            engine.mark_save_finished(result.revision, result.result);
        }
        let now = Instant::now();
        let timeout = if engine.is_moving() {
            next_tick.saturating_duration_since(now)
        } else {
            Duration::from_millis(250)
        };
        let mut fds: Vec<_> = devices
            .iter()
            .map(|(_, d)| libc::pollfd {
                fd: d.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        // SAFETY: fds is a live contiguous buffer of pollfd; poll only updates revents.
        let result = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                timeout.as_millis().max(1) as i32,
            )
        };
        if result < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        let now = Instant::now();
        // Integrate using the OLD held state, then process new events. This avoids
        // a large first movement after an idle poll or losing movement on key-up.
        if output.emit(engine.tick(now.duration_since(last_tick)))? {
            break;
        }
        last_tick = now;
        next_tick = now + tick;
        let mut quit = false;
        if !engine.is_moving()
            && last_input.elapsed() >= Duration::from_millis(500)
            && last_display_check.elapsed() >= Duration::from_secs(3)
        {
            let (layout, source) = displays::detect(fallback_screen);
            // A transient compositor query failure must not erase a known topology.
            if source != displays::Source::Config || display_source == displays::Source::Config {
                output.layout = layout.clone();
                output.emit(engine.replace_layout(layout))?;
                display_source = source;
            }
            last_display_check = Instant::now();
            last_tick = Instant::now();
        }
        for (source, (path, device)) in devices.iter_mut().enumerate() {
            if fds[source].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err(io::Error::other(format!(
                    "keyboard {} disconnected; stopped and released control (restart to reconnect)",
                    path.display()
                )));
            }
            if fds[source].revents & libc::POLLIN == 0 {
                continue;
            }
            let events: Vec<_> = match device.fetch_events() {
                Ok(events) => events.collect(),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            };
            for event in events {
                if event.event_type() == EventType::KEY {
                    last_input = Instant::now();
                    if let Some(value) = merger.event(source, K(event.code()), event.value()) {
                        if engine.needs_active_display(K(event.code()), value) {
                            if let Some(name) = displays::active_output(display_source) {
                                output.emit(engine.select_display(&name))?;
                            }
                            last_tick = Instant::now();
                        }
                        let actions = engine.key(K(event.code()), value);
                        let revision = engine.marks_revision();
                        match mark_worker.submit(revision, engine.marks()) {
                            Ok(true) => engine.mark_save_started(revision),
                            Ok(false) => {}
                            Err(error) => {
                                engine.mark_save_started(revision);
                                engine.mark_save_finished(revision, Err(error.to_string()));
                            }
                        }
                        quit |= output.emit(actions)?;
                    }
                }
                if quit {
                    break;
                }
            }
            if quit {
                break;
            }
        }
        if quit {
            break;
        }
        if last_status.elapsed() >= Duration::from_millis(40) || !engine.is_moving() {
            runtime.write(&engine.status())?;
            last_status = Instant::now();
        }
    }
    output.emit(engine.reset())?;
    Ok(())
}

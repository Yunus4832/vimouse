use evdev::KeyCode as K;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use vimouse::{
    config::Config,
    displays::{Display, Layout},
    engine::{Engine, Point},
    marks::{Mark, Marks, Store},
};
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "vimouse-marks-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("vimouse/marks.json")
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn display(name: &str, x: i32, y: i32, width: i32, height: i32) -> Display {
    Display {
        name: name.into(),
        x,
        y,
        width,
        height,
    }
}
fn engine(display: Display) -> Engine {
    let mut e = Engine::with_layout(Config::default(), Layout::new(vec![display]).unwrap());
    tap(&mut e, K::KEY_CAPSLOCK);
    e
}
fn tap(e: &mut Engine, key: K) {
    e.key(key, 1);
    e.key(key, 0);
}
fn save_a(e: &mut Engine) {
    tap(e, K::KEY_M);
    tap(e, K::KEY_A);
}
fn jump_a(e: &mut Engine) {
    tap(e, K::KEY_APOSTROPHE);
    tap(e, K::KEY_A);
}

#[test]
fn named_marks_survive_restart_and_follow_logical_display_geometry() {
    let temp = Temporary::new();
    let (mut store, saved) = Store::open(temp.file()).unwrap();
    assert!(saved.is_empty());
    assert!(!temp.file().exists());
    let mut e = engine(display("DP-1", -1920, 100, 1920, 1080));
    save_a(&mut e);
    store.sync(e.marks_revision(), e.marks()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&fs::read(temp.file()).unwrap()).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["marks"]["a"]["display"], "DP-1");
    assert_eq!(json["marks"]["a"]["x"].as_f64().unwrap(), 960.0 / 1919.0);
    assert!(json.get("previous").is_none());
    assert_eq!(
        fs::metadata(temp.file()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(temp.file().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let inode = fs::metadata(temp.file()).unwrap().ino();
    e.key(K::KEY_L, 1);
    e.tick(Duration::from_millis(20));
    e.key(K::KEY_L, 0);
    store.sync(e.marks_revision(), e.marks()).unwrap();
    assert_eq!(
        fs::metadata(temp.file()).unwrap().ino(),
        inode,
        "moving must not rewrite marks"
    );
    drop(e);
    drop(store);
    let (_, saved) = Store::open(temp.file()).unwrap();
    let mut restarted = engine(display("DP-1", 3000, -400, 960, 540));
    restarted.restore_marks(saved);
    tap(&mut restarted, K::KEY_APOSTROPHE);
    tap(&mut restarted, K::KEY_APOSTROPHE);
    assert!(restarted.status().message.contains("No previous jump"));
    tap(&mut restarted, K::KEY_G);
    tap(&mut restarted, K::KEY_G);
    jump_a(&mut restarted);
    assert_eq!(restarted.point(), Point { x: 3480, y: -130 });
    assert_eq!(restarted.status().marks, 1);
}

#[test]
fn disconnected_displays_keep_marks_and_cannot_redirect_to_another_output() {
    let original = display("DP-1", 0, 0, 1920, 1080);
    let mut e = engine(original.clone());
    save_a(&mut e);
    let revision = e.marks_revision();
    e.replace_layout(Layout::new(vec![display("eDP-1", 0, 0, 1560, 1040)]).unwrap());
    let before = e.point();
    jump_a(&mut e);
    assert_eq!(e.point(), before);
    assert!(e.status().message.contains("DP-1"));
    assert!(e.status().message.contains("unavailable"));
    assert_eq!(e.status().marks, 1);
    assert_eq!(e.marks_revision(), revision);
    let returned = display("DP-1", -1920, 40, 1920, 1080);
    e.replace_layout(Layout::new(vec![returned.clone()]).unwrap());
    jump_a(&mut e);
    assert_eq!(e.point(), Point { x: -960, y: 580 });
    let tiny = display("tiny", -1, -1, 1, 1);
    let mark = Mark::capture(&tiny, Point { x: -1, y: -1 });
    assert_eq!((mark.x, mark.y), (0.0, 0.0));
    assert_eq!(
        mark.resolve(&Layout::new(vec![tiny]).unwrap()),
        Some(Point { x: -1, y: -1 })
    );
}

#[test]
fn invalid_state_is_not_silently_replaced() {
    let temp = Temporary::new();
    fs::create_dir(temp.file().parent().unwrap()).unwrap();
    for bytes in [
        b"not json".to_vec(),
        b"{\"version\":2,\"marks\":{}}".to_vec(),
        b"{\"version\":1,\"marks\":{\"a\":{\"display\":\"DP-1\",\"x\":1.1,\"y\":0.5}}}".to_vec(),
        b"{\"version\":1,\"marks\":{\"A\":{\"display\":\"DP-1\",\"x\":0.5,\"y\":0.5}}}".to_vec(),
        vec![b' '; 65_537],
    ] {
        fs::write(temp.file(), &bytes).unwrap();
        assert!(Store::open(temp.file()).is_err());
        assert_eq!(fs::read(temp.file()).unwrap(), bytes);
    }
    fs::remove_file(temp.file()).unwrap();
    let real = temp.0.join("original.json");
    fs::write(&real, b"keep me").unwrap();
    std::os::unix::fs::symlink(&real, temp.file()).unwrap();
    assert!(Store::open(temp.file()).is_err());
    assert_eq!(fs::read(real).unwrap(), b"keep me");
}

#[test]
fn a_save_error_keeps_the_in_memory_mark_and_the_next_edit_can_retry() {
    let temp = Temporary::new();
    let (mut store, _) = Store::open(temp.file()).unwrap();
    let mut e = engine(display("DP-1", 0, 0, 1920, 1080));
    fs::write(temp.file().parent().unwrap(), b"block directory creation").unwrap();
    save_a(&mut e);
    assert!(store.sync(e.marks_revision(), e.marks()).is_err());
    assert_eq!(e.status().marks, 1);
    fs::remove_file(temp.file().parent().unwrap()).unwrap();
    store.sync(e.marks_revision(), e.marks()).unwrap();
    assert!(
        !temp.file().exists(),
        "failed edits must not retry on each input tick"
    );
    save_a(&mut e);
    store.sync(e.marks_revision(), e.marks()).unwrap();
    let (_, saved) = Store::open(temp.file()).unwrap();
    assert_eq!(saved, e.marks().clone());
}

#[test]
fn relative_coordinates_preserve_corners_and_empty_fallback_identity_is_not_guessed() {
    let d = display("DP-1", -400, -100, 101, 51);
    let l = Layout::new(vec![d.clone()]).unwrap();
    for point in [
        Point { x: -400, y: -100 },
        Point { x: -300, y: -50 },
        Point { x: -350, y: -75 },
    ] {
        assert_eq!(Mark::capture(&d, point).resolve(&l), Some(point));
    }
    let unnamed = Mark {
        display: String::new(),
        x: 0.5,
        y: 0.5,
    };
    assert!(unnamed.resolve(&l).is_none());
    let mut marks = Marks::new();
    marks.insert('a', unnamed);
    let mut e = engine(d);
    e.restore_marks(marks);
    jump_a(&mut e);
    assert!(e.status().message.contains("unavailable"));
}

#[test]
fn background_saver_flushes_the_latest_complete_snapshot_on_drop() {
    let temp = Temporary::new();
    let (store, _) = Store::open(temp.file()).unwrap();
    let mut worker = vimouse::marks::SaveWorker::spawn(store).unwrap();
    let mut marks = Marks::new();
    for revision in 1..=50 {
        marks.insert(
            'a',
            Mark {
                display: "DP-1".into(),
                x: revision as f64 / 50.0,
                y: 0.5,
            },
        );
        assert!(worker.submit(revision, &marks).unwrap());
        assert!(!worker.submit(revision, &marks).unwrap());
    }
    drop(worker);
    let (_, saved) = Store::open(temp.file()).unwrap();
    assert_eq!(saved, marks);
}

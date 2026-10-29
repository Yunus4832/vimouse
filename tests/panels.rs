use evdev::KeyCode as K;
use std::time::Duration;
use vimouse::{
    config::Config,
    displays::{Display, Layout},
    engine::{Action, Engine, Mode, Point},
    marks::{Mark, Marks},
};
fn tap(e: &mut Engine, k: K) -> Vec<Action> {
    let mut out = e.key(k, 1);
    out.extend(e.key(k, 0));
    out
}
fn shifted(e: &mut Engine, k: K) -> Vec<Action> {
    e.key(K::KEY_LEFTSHIFT, 1);
    let out = tap(e, k);
    e.key(K::KEY_LEFTSHIFT, 0);
    out
}
fn engine() -> Engine {
    let mut e = Engine::with_layout(
        Config::default(),
        Layout::new(vec![
            Display {
                name: "DP-1".into(),
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            Display {
                name: "eDP-1".into(),
                x: 0,
                y: 0,
                width: 1560,
                height: 1040,
            },
        ])
        .unwrap(),
    );
    tap(&mut e, K::KEY_CAPSLOCK);
    e
}
#[test]
fn mark_view_publishes_all_outputs_and_selects_without_clicking_or_saving() {
    let mut e = engine();
    let origin = e.point();
    let mut marks = Marks::new();
    marks.insert(
        'a',
        Mark {
            display: "DP-1".into(),
            x: 0.0,
            y: 1.0,
        },
    );
    marks.insert(
        'b',
        Mark {
            display: "eDP-1".into(),
            x: 1.0,
            y: 0.0,
        },
    );
    marks.insert(
        'c',
        Mark {
            display: "missing".into(),
            x: 0.5,
            y: 0.5,
        },
    );
    e.restore_marks(marks);
    assert!(shifted(&mut e, K::KEY_M).is_empty());
    assert_eq!(e.mode(), Mode::Marks);
    assert_eq!(e.point(), origin);
    let state = e.status();
    assert_eq!(state.mark_targets.len(), 3);
    assert_eq!(
        state.mark_targets[0].position,
        Some(Point { x: -1920, y: 1079 })
    );
    assert_eq!(
        state.mark_targets[1].position,
        Some(Point { x: 1559, y: 0 })
    );
    assert!(state.mark_targets[2].position.is_none());
    assert!(tap(&mut e, K::KEY_C).is_empty());
    assert_eq!(e.point(), origin);
    assert_eq!(e.mode(), Mode::Marks);
    assert!(e.status().message.contains("offline"));
    assert_eq!(
        tap(&mut e, K::KEY_B),
        vec![Action::Pointer(Point { x: 1559, y: 0 })]
    );
    assert_eq!(e.mode(), Mode::Normal);
    assert_eq!(e.display().name, "eDP-1");
    assert_eq!(e.marks_revision(), 0);
    assert!(e.status().mark_targets.is_empty());
    tap(&mut e, K::KEY_APOSTROPHE);
    tap(&mut e, K::KEY_APOSTROPHE);
    assert_eq!(e.point(), origin);
}
#[test]
fn modal_overlays_consume_input_cancel_without_moving_and_clear_held_motion() {
    let mut e = engine();
    e.key(K::KEY_L, 1);
    e.tick(Duration::from_millis(10));
    let point = e.point();
    shifted(&mut e, K::KEY_M);
    assert!(e.tick(Duration::from_millis(100)).is_empty());
    assert_eq!(e.point(), point);
    assert!(tap(&mut e, K::KEY_Z).is_empty());
    assert_eq!(e.mode(), Mode::Marks);
    shifted(&mut e, K::KEY_M);
    assert_eq!(e.mode(), Mode::Normal);
    shifted(&mut e, K::KEY_SLASH);
    assert_eq!(e.mode(), Mode::Help);
    assert!(tap(&mut e, K::KEY_F).is_empty());
    tap(&mut e, K::KEY_3);
    assert_eq!(e.status().speed, 0);
    assert_eq!(e.key(K::KEY_SLASH, 2), Vec::<Action>::new());
    assert_eq!(e.mode(), Mode::Help);
    tap(&mut e, K::KEY_ENTER);
    assert_eq!(e.mode(), Mode::Normal);
    assert_eq!(e.point(), point);
    shifted(&mut e, K::KEY_SLASH);
    shifted(&mut e, K::KEY_SLASH);
    assert_eq!(e.mode(), Mode::Normal);
    shifted(&mut e, K::KEY_M);
    tap(&mut e, K::KEY_ESC);
    assert_eq!(e.mode(), Mode::Normal);
}
#[test]
fn numeric_speed_keys_are_explicit_normal_mode_actions_and_do_not_leak() {
    let mut e = engine();
    let point = e.point();
    for (key, speed) in [(K::KEY_3, 2), (K::KEY_1, 0), (K::KEY_2, 1), (K::KEY_2, 1)] {
        assert!(tap(&mut e, key).is_empty());
        assert_eq!(e.status().speed, speed);
        assert_eq!(e.point(), point);
    }
    shifted(&mut e, K::KEY_1);
    assert_eq!(e.status().speed, 1);
    e.key(K::KEY_LEFTCTRL, 1);
    tap(&mut e, K::KEY_3);
    e.key(K::KEY_LEFTCTRL, 0);
    assert_eq!(e.status().speed, 1);
    tap(&mut e, K::KEY_M);
    tap(&mut e, K::KEY_3);
    assert_eq!(e.status().speed, 1);
    assert_eq!(e.status().marks, 0);
    shifted(&mut e, K::KEY_SEMICOLON);
    tap(&mut e, K::KEY_1);
    assert_eq!(e.status().pending, ":1");
    assert_eq!(e.status().speed, 1);
    tap(&mut e, K::KEY_ESC);
    tap(&mut e, K::KEY_I);
    assert_eq!(
        tap(&mut e, K::KEY_3),
        vec![Action::Key(K::KEY_3, 1), Action::Key(K::KEY_3, 0)]
    );
}
#[test]
fn command_aliases_show_the_same_views_and_emergency_escape_works() {
    for (keys, mode) in [
        (
            vec![K::KEY_M, K::KEY_A, K::KEY_R, K::KEY_K, K::KEY_S],
            Mode::Marks,
        ),
        (vec![K::KEY_H, K::KEY_E, K::KEY_L, K::KEY_P], Mode::Help),
    ] {
        let mut e = engine();
        shifted(&mut e, K::KEY_SEMICOLON);
        for key in keys {
            tap(&mut e, key);
        }
        tap(&mut e, K::KEY_ENTER);
        assert_eq!(e.mode(), mode);
        e.key(K::KEY_LEFTCTRL, 1);
        e.key(K::KEY_LEFTALT, 1);
        tap(&mut e, K::KEY_ESC);
        assert_eq!(e.mode(), Mode::Insert);
    }
}

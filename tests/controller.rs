use evdev::KeyCode as K;
use std::time::Duration;
use vimouse::{
    config::Config,
    engine::{Action, Engine, Mode, Point},
    linux::KeyMerger,
};

fn tap(engine: &mut Engine, key: K) -> Vec<Action> {
    let mut result = engine.key(key, 1);
    result.extend(engine.key(key, 0));
    result
}

fn normal() -> Engine {
    let mut engine = Engine::new(Config::default());
    tap(&mut engine, K::KEY_CAPSLOCK);
    engine
}

fn advance(engine: &mut Engine, millis: u64) -> Vec<Action> {
    (0..millis / 10)
        .flat_map(|_| engine.tick(Duration::from_millis(10)))
        .collect()
}

fn command(engine: &mut Engine) {
    engine.key(K::KEY_LEFTSHIFT, 1);
    tap(engine, K::KEY_SEMICOLON);
    engine.key(K::KEY_LEFTSHIFT, 0);
}

#[test]
fn insert_passes_balanced_keys_and_repeat_without_activating_mouse() {
    let mut engine = Engine::new(Config::default());
    assert_eq!(engine.mode(), Mode::Insert);
    assert_eq!(engine.key(K::KEY_H, 1), vec![Action::Key(K::KEY_H, 1)]);
    assert_eq!(engine.key(K::KEY_H, 2), vec![Action::Key(K::KEY_H, 2)]);
    assert_eq!(engine.key(K::KEY_H, 0), vec![Action::Key(K::KEY_H, 0)]);
    assert!(engine.key(K::KEY_H, 0).is_empty());
    assert!(advance(&mut engine, 1000).is_empty());
}

#[test]
fn double_caps_toggles_native_caps_while_a_slow_pair_only_changes_modes() {
    let mut engine = normal();
    advance(&mut engine, 100);
    assert_eq!(
        tap(&mut engine, K::KEY_CAPSLOCK),
        vec![
            Action::Key(K::KEY_CAPSLOCK, 1),
            Action::Key(K::KEY_CAPSLOCK, 0)
        ]
    );
    assert_eq!(engine.mode(), Mode::Insert);
    tap(&mut engine, K::KEY_CAPSLOCK);
    advance(&mut engine, 400);
    assert!(tap(&mut engine, K::KEY_CAPSLOCK).is_empty());
}

#[test]
fn entering_normal_releases_insert_modifiers_and_consumes_toggle() {
    let mut engine = Engine::new(Config::default());
    engine.key(K::KEY_LEFTCTRL, 1);
    let actions = engine.key(K::KEY_CAPSLOCK, 1);
    assert!(actions.contains(&Action::Key(K::KEY_LEFTCTRL, 0)));
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Key(K::KEY_CAPSLOCK, _)))
    );
    assert!(engine.key(K::KEY_CAPSLOCK, 2).is_empty());
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(engine.key(K::KEY_LEFTCTRL, 0).is_empty());
}

#[test]
fn movement_is_continuous_time_based_and_stops_on_release() {
    let mut engine = normal();
    engine.key(K::KEY_L, 1);
    advance(&mut engine, 100);
    assert_eq!(engine.point(), Point { x: 1120, y: 540 });
    engine.key(K::KEY_L, 0);
    advance(&mut engine, 100);
    assert_eq!(engine.point().x, 1120);
    engine.key(K::KEY_H, 1);
    advance(&mut engine, 1000);
    assert_eq!(engine.point().x, 0);
}

#[test]
fn diagonal_motion_has_same_speed_and_opposing_keys_cancel() {
    let mut engine = normal();
    engine.key(K::KEY_L, 1);
    engine.key(K::KEY_J, 1);
    advance(&mut engine, 100);
    let point = engine.point();
    assert!((point.x - 1073).abs() <= 1);
    assert!((point.y - 653).abs() <= 1);
    engine.key(K::KEY_H, 1);
    engine.key(K::KEY_K, 1);
    advance(&mut engine, 100);
    assert_eq!(point, engine.point());
}

#[test]
fn speed_switch_does_not_repeat_on_kernel_autorepeat() {
    let mut engine = normal();
    tap(&mut engine, K::KEY_A);
    assert_eq!(engine.status().speed, 1);
    engine.key(K::KEY_A, 2);
    assert_eq!(engine.status().speed, 1);
    engine.key(K::KEY_LEFTCTRL, 1);
    tap(&mut engine, K::KEY_A);
    engine.key(K::KEY_LEFTCTRL, 0);
    assert_eq!(engine.status().speed, 2);
    engine.key(K::KEY_LEFTALT, 1);
    tap(&mut engine, K::KEY_A);
    assert_eq!(engine.status().speed, 0);
}

#[test]
fn marks_track_named_positions_and_previous_jump_is_a_swap() {
    let mut engine = normal();
    tap(&mut engine, K::KEY_M);
    tap(&mut engine, K::KEY_A);
    tap(&mut engine, K::KEY_G);
    tap(&mut engine, K::KEY_G);
    assert_eq!(engine.point(), Point { x: 0, y: 0 });
    tap(&mut engine, K::KEY_APOSTROPHE);
    tap(&mut engine, K::KEY_A);
    assert_eq!(engine.point(), Point { x: 960, y: 540 });
    tap(&mut engine, K::KEY_APOSTROPHE);
    tap(&mut engine, K::KEY_APOSTROPHE);
    assert_eq!(engine.point(), Point { x: 0, y: 0 });
    tap(&mut engine, K::KEY_GRAVE);
    tap(&mut engine, K::KEY_GRAVE);
    assert_eq!(engine.point(), Point { x: 960, y: 540 });
    tap(&mut engine, K::KEY_APOSTROPHE);
    tap(&mut engine, K::KEY_B);
    assert!(engine.status().message.contains("not set"));
}

#[test]
fn jumps_use_screen_edges_and_vertical_jumps_preserve_x() {
    let mut engine = normal();
    for key in [K::KEY_G, K::KEY_E] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), Point { x: 1919, y: 1079 });
    for key in [K::KEY_Z, K::KEY_T] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), Point { x: 1919, y: 0 });
    for key in [K::KEY_Z, K::KEY_Z] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), Point { x: 1919, y: 540 });
    for key in [K::KEY_Z, K::KEY_B] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), Point { x: 1919, y: 1079 });
}

#[test]
fn every_exit_path_releases_a_held_drag() {
    for exit in [K::KEY_V, K::KEY_I, K::KEY_ESC, K::KEY_CAPSLOCK, K::KEY_S] {
        let mut engine = normal();
        assert_eq!(
            tap(&mut engine, K::KEY_V),
            vec![Action::Button(K::BTN_LEFT, true)]
        );
        assert!(
            tap(&mut engine, exit).contains(&Action::Button(K::BTN_LEFT, false)),
            "{exit:?}"
        );
        assert!(!engine.reset().contains(&Action::Button(K::BTN_LEFT, false)));
    }
    let mut engine = normal();
    engine.key(K::KEY_LEFTCTRL, 1);
    assert_eq!(
        tap(&mut engine, K::KEY_V),
        vec![Action::Button(K::BTN_MIDDLE, true)]
    );
    assert!(
        engine
            .reset()
            .contains(&Action::Button(K::BTN_MIDDLE, false))
    );
}

#[test]
fn idle_timeout_releases_drag_but_never_interrupts_held_movement() {
    let mut engine = normal();
    tap(&mut engine, K::KEY_V);
    assert!(
        engine
            .tick(Duration::from_secs(61))
            .contains(&Action::Button(K::BTN_LEFT, false))
    );
    assert_eq!(engine.mode(), Mode::Insert);
    let mut engine = normal();
    engine.key(K::KEY_L, 1);
    advance(&mut engine, 61000);
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn right_click_select_maps_arrows_and_balances_confirm() {
    let mut engine = normal();
    engine.key(K::KEY_LEFTALT, 1);
    let actions = tap(&mut engine, K::KEY_ENTER);
    engine.key(K::KEY_LEFTALT, 0);
    assert_eq!(
        actions,
        vec![
            Action::Button(K::BTN_RIGHT, true),
            Action::Button(K::BTN_RIGHT, false)
        ]
    );
    assert_eq!(engine.mode(), Mode::Select);
    assert_eq!(
        tap(&mut engine, K::KEY_J),
        vec![Action::Key(K::KEY_DOWN, 1), Action::Key(K::KEY_DOWN, 0)]
    );
    assert_eq!(
        tap(&mut engine, K::KEY_ENTER),
        vec![Action::Key(K::KEY_ENTER, 1), Action::Key(K::KEY_ENTER, 0)]
    );
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn ctrl_enter_sends_one_middle_click_without_forwarding_ctrl() {
    for ctrl in [K::KEY_LEFTCTRL, K::KEY_RIGHTCTRL] {
        let mut engine = normal();
        assert!(engine.key(ctrl, 1).is_empty());
        assert_eq!(
            tap(&mut engine, K::KEY_ENTER),
            vec![
                Action::Button(K::BTN_MIDDLE, true),
                Action::Button(K::BTN_MIDDLE, false)
            ]
        );
        assert_eq!(engine.mode(), Mode::Normal);
        assert!(engine.key(ctrl, 0).is_empty());
        assert!(engine.reset().is_empty());
    }
}

#[test]
fn selection_quit_can_dismiss_or_leave_the_menu_open() {
    let mut engine = normal();
    tap(&mut engine, K::KEY_S);
    assert_eq!(
        tap(&mut engine, K::KEY_Q),
        vec![Action::Key(K::KEY_ESC, 1), Action::Key(K::KEY_ESC, 0)]
    );
    tap(&mut engine, K::KEY_S);
    engine.key(K::KEY_LEFTSHIFT, 1);
    assert!(tap(&mut engine, K::KEY_Q).is_empty());
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn scroll_has_a_separate_repeat_rate_and_correct_axis() {
    let mut engine = normal();
    engine.key(K::KEY_LEFTALT, 1);
    assert_eq!(
        engine.key(K::KEY_L, 1),
        vec![Action::Scroll {
            horizontal: -1,
            vertical: 0
        }]
    );
    assert_eq!(advance(&mut engine, 160).len(), 2);
    engine.key(K::KEY_L, 0);
    assert!(advance(&mut engine, 100).is_empty());
}

#[test]
fn command_editing_errors_cancel_and_quit_are_deterministic() {
    let mut engine = normal();
    command(&mut engine);
    assert_eq!(engine.mode(), Mode::Command);
    tap(&mut engine, K::KEY_X);
    tap(&mut engine, K::KEY_ENTER);
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(engine.status().message.contains("Unknown command"));
    command(&mut engine);
    tap(&mut engine, K::KEY_Q);
    tap(&mut engine, K::KEY_BACKSPACE);
    assert_eq!(engine.status().pending, ":");
    tap(&mut engine, K::KEY_ESC);
    assert_eq!(engine.mode(), Mode::Normal);
    command(&mut engine);
    tap(&mut engine, K::KEY_Q);
    assert!(tap(&mut engine, K::KEY_ENTER).contains(&Action::Quit));
    assert_eq!(engine.mode(), Mode::Insert);
}

#[test]
fn emergency_chord_cancels_command_and_drag_and_releases_modifiers() {
    for quit in [false, true] {
        let mut engine = normal();
        tap(&mut engine, K::KEY_V);
        engine.key(K::KEY_LEFTCTRL, 1);
        engine.key(K::KEY_LEFTALT, 1);
        let actions = engine.key(if quit { K::KEY_BACKSPACE } else { K::KEY_ESC }, 1);
        assert!(actions.contains(&Action::Button(K::BTN_LEFT, false)));
        assert_eq!(actions.contains(&Action::Quit), quit);
        assert_eq!(engine.mode(), Mode::Insert);
        assert!(engine.key(K::KEY_LEFTCTRL, 0).is_empty());
    }
}

#[test]
fn native_tab_switching_holds_modifiers_until_physical_release() {
    let mut engine = normal();
    engine.key(K::KEY_LEFTALT, 1);
    assert_eq!(
        tap(&mut engine, K::KEY_TAB),
        vec![
            Action::Key(K::KEY_LEFTALT, 1),
            Action::Key(K::KEY_TAB, 1),
            Action::Key(K::KEY_TAB, 0)
        ]
    );
    assert_eq!(
        tap(&mut engine, K::KEY_TAB),
        vec![Action::Key(K::KEY_TAB, 1), Action::Key(K::KEY_TAB, 0)]
    );
    assert_eq!(
        engine.key(K::KEY_LEFTALT, 0),
        vec![Action::Key(K::KEY_LEFTALT, 0)]
    );
}

#[test]
fn multiple_keyboards_have_reference_counted_key_ownership() {
    let mut merger = KeyMerger::default();
    assert_eq!(merger.event(0, K::KEY_H, 1), Some(1));
    assert_eq!(merger.event(1, K::KEY_H, 1), None);
    assert_eq!(merger.event(0, K::KEY_H, 0), None);
    assert_eq!(merger.event(1, K::KEY_H, 2), Some(2));
    assert_eq!(merger.event(1, K::KEY_H, 0), Some(0));
    assert_eq!(merger.event(1, K::KEY_H, 0), None);
}

#[test]
fn mouse_action_after_native_tab_releases_the_forwarded_modifier_first() {
    let mut engine = normal();
    engine.key(K::KEY_LEFTALT, 1);
    tap(&mut engine, K::KEY_TAB);
    let actions = tap(&mut engine, K::KEY_ENTER);
    assert_eq!(actions[0], Action::Key(K::KEY_LEFTALT, 0));
    assert_eq!(actions[1], Action::Button(K::BTN_RIGHT, true));
}

#[test]
fn invalid_config_is_rejected_before_input_capture() {
    let mut config = Config::default();
    assert!(config.validate().is_ok());
    config.speeds[1] = f64::NAN;
    assert!(config.validate().is_err());
    config = Config::default();
    config.tick_hz = 0;
    assert!(config.validate().is_err());
    config = Config::default();
    config.screen.width = 0;
    assert!(config.validate().is_err());
    config = Config::default();
    config.toggle = "not-a-key".into();
    assert!(config.validate().is_err());
}

#[test]
fn resetting_and_releasing_old_motion_key_cannot_leak_it_into_insert() {
    let mut engine = normal();
    engine.key(K::KEY_L, 1);
    advance(&mut engine, 100);
    tap(&mut engine, K::KEY_I);
    assert!(!engine.is_moving());
    assert!(engine.key(K::KEY_L, 0).is_empty());
    let point = engine.point();
    advance(&mut engine, 100);
    assert_eq!(point, engine.point());
}

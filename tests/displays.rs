use evdev::KeyCode as K;
use serde_json::json;
use vimouse::{
    config::Config,
    displays::{ABS_MAX, Display, Layout, parse_kscreen, parse_niri},
    engine::{Action, Engine, Mode, Point},
    grid::Rect,
};

fn layout() -> Layout {
    Layout::new(vec![
        Display {
            name: "DP-1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        Display {
            name: "eDP-1".into(),
            x: 1920,
            y: 0,
            width: 1560,
            height: 1040,
        },
    ])
    .unwrap()
}
fn tap(engine: &mut Engine, key: K) -> Vec<Action> {
    let mut out = engine.key(key, 1);
    out.extend(engine.key(key, 0));
    out
}
fn normal() -> Engine {
    let mut engine = Engine::with_layout(Config::default(), layout());
    tap(&mut engine, K::KEY_CAPSLOCK);
    engine
}

#[test]
fn kscreen_handles_mixed_scaling_and_disabled_outputs() {
    let value = json!({"outputs": [
        {"name":"DP-1", "enabled":true, "connected":true, "pos":{"x":0,"y":0}, "size":{"width":1920,"height":1080}, "scale":1.0},
        {"name":"eDP-1", "enabled":true, "connected":true, "pos":{"x":1920,"y":0}, "size":{"width":3120,"height":2080}, "scale":2.0},
        {"name":"off", "enabled":false, "connected":true}
    ]});
    assert_eq!(parse_kscreen(&value).unwrap(), layout());
    let mut invalid = value;
    invalid["outputs"][0]["scale"] = json!(0);
    assert!(parse_kscreen(&invalid).is_err());
}

#[test]
fn niri_uses_logical_dimensions_and_supports_negative_coordinates() {
    let value = json!({
        "DP-1":{"name":"DP-1","logical":{"x":-1920,"y":0,"width":1920,"height":1080,"scale":1.0}},
        "eDP-1":{"name":"eDP-1","logical":{"x":0,"y":0,"width":1560,"height":1040,"scale":2.0}},
        "off":{"logical":null}
    });
    let layout = parse_niri(&value).unwrap();
    assert_eq!(
        layout.bounds,
        Rect {
            x: -1920,
            y: 0,
            width: 3480,
            height: 1080
        }
    );
    assert_eq!(layout.outputs[1].width, 1560);
    let mut engine = Engine::with_layout(Config::default(), layout);
    tap(&mut engine, K::KEY_CAPSLOCK);
    tap(&mut engine, K::KEY_F);
    assert_eq!(engine.status().grid.unwrap().bounds.x, -1920);
    tap(&mut engine, K::KEY_A);
    assert_eq!(engine.point(), Point { x: -1728, y: 108 });
}

#[test]
fn space_cycles_outputs_and_centers_without_entering_grid_mode() {
    let mut engine = normal();
    assert_eq!(
        tap(&mut engine, K::KEY_SPACE),
        vec![Action::Pointer(Point { x: 2700, y: 520 })]
    );
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(engine.status().grid.is_none());
    assert_eq!(engine.display().name, "eDP-1");
    assert!(engine.key(K::KEY_SPACE, 2).is_empty());
    assert_eq!(
        tap(&mut engine, K::KEY_SPACE),
        vec![Action::Pointer(Point { x: 960, y: 540 })]
    );
    assert_eq!(engine.display().name, "DP-1");
}

#[test]
fn insert_space_is_passed_through_and_grid_space_still_finishes() {
    let mut engine = Engine::with_layout(Config::default(), layout());
    assert_eq!(
        tap(&mut engine, K::KEY_SPACE),
        vec![Action::Key(K::KEY_SPACE, 1), Action::Key(K::KEY_SPACE, 0)]
    );
    tap(&mut engine, K::KEY_CAPSLOCK);
    tap(&mut engine, K::KEY_SPACE);
    tap(&mut engine, K::KEY_TAB);
    tap(&mut engine, K::KEY_A);
    let position = engine.point();
    assert!(tap(&mut engine, K::KEY_SPACE).is_empty());
    assert_eq!(engine.mode(), Mode::Normal);
    assert_eq!(engine.point(), position);
    assert_eq!(engine.display().name, "eDP-1");
}

#[test]
fn f_is_confined_to_current_output_with_global_coordinates() {
    let mut engine = normal();
    tap(&mut engine, K::KEY_SPACE);
    tap(&mut engine, K::KEY_F);
    assert_eq!(
        engine.status().grid.unwrap().bounds,
        Rect {
            x: 1920,
            y: 0,
            width: 1560,
            height: 1040
        }
    );
    tap(&mut engine, K::KEY_A);
    assert_eq!(engine.point(), Point { x: 2076, y: 104 });
    assert_eq!(engine.mode(), Mode::Normal);
    tap(&mut engine, K::KEY_TAB);
    assert_eq!(
        engine.status().grid.as_ref().unwrap().bounds,
        Rect {
            x: 1920,
            y: 0,
            width: 312,
            height: 208
        }
    );
    for key in [K::KEY_P, K::KEY_L, K::KEY_J, K::KEY_TAB, K::KEY_H] {
        tap(&mut engine, key);
    }
    assert!(engine.display().contains(engine.point()));
    assert_eq!(engine.display().name, "eDP-1");
    tap(&mut engine, K::KEY_ESC);
    assert_eq!(engine.point(), Point { x: 2076, y: 104 });
}

#[test]
fn screen_relative_jumps_and_saved_global_positions_cross_outputs_correctly() {
    let mut engine = normal();
    tap(&mut engine, K::KEY_M);
    tap(&mut engine, K::KEY_A);
    tap(&mut engine, K::KEY_SPACE);
    for key in [K::KEY_G, K::KEY_G] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), Point { x: 1920, y: 0 });
    for key in [K::KEY_G, K::KEY_E] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), Point { x: 3479, y: 1039 });
    for key in [K::KEY_APOSTROPHE, K::KEY_A] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), Point { x: 960, y: 540 });
    assert_eq!(engine.display().name, "DP-1");
}

#[test]
fn compositor_active_output_selects_the_correct_grid_screen() {
    let mut engine = normal();
    assert_eq!(
        engine.select_display("eDP-1"),
        vec![Action::Pointer(Point { x: 2700, y: 520 })]
    );
    assert!(engine.select_display("missing").is_empty());
    assert!(engine.needs_active_display(K::KEY_F, 1));
    tap(&mut engine, K::KEY_F);
    assert_eq!(
        engine.status().grid.unwrap().bounds,
        engine.display().rect()
    );
}

#[test]
fn desktop_normalization_roundtrips_without_per_display_stretching() {
    for layout in [layout(), parse_niri(&json!({"left":{"logical":{"x":-1920,"y":-120,"width":1920,"height":1080}}, "right":{"logical":{"x":0,"y":0,"width":1560,"height":1040}}})).unwrap()] {
        for output in &layout.outputs {
            for point in [output.rect().center(), Point { x: output.x, y: output.y }, Point { x: output.x + output.width - 1, y: output.y + output.height - 1 }] {
                let normalized = layout.absolute(point);
                let x = f64::from(normalized.x) / f64::from(ABS_MAX) * f64::from(layout.bounds.width) + f64::from(layout.bounds.x);
                let y = f64::from(normalized.y) / f64::from(ABS_MAX) * f64::from(layout.bounds.height) + f64::from(layout.bounds.y);
                assert!((x - f64::from(point.x)).abs() < 0.01);
                assert!((y - f64::from(point.y)).abs() < 0.01);
            }
        }
    }
}

#[test]
fn display_gaps_and_hot_unplug_do_not_leave_cursor_or_grid_outside_outputs() {
    let gapped = Layout::new(vec![
        Display {
            name: "a".into(),
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        },
        Display {
            name: "b".into(),
            x: 1200,
            y: 100,
            width: 1000,
            height: 800,
        },
    ])
    .unwrap();
    let (_, point) = gapped.locate(Point { x: 1100, y: 50 });
    assert!(gapped.outputs.iter().any(|o| o.contains(point)));
    let mut engine = normal();
    tap(&mut engine, K::KEY_SPACE);
    tap(&mut engine, K::KEY_F);
    let single = Layout::new(vec![layout().outputs[0].clone()]).unwrap();
    assert_eq!(
        engine.replace_layout(single.clone()),
        vec![Action::Pointer(Point { x: 960, y: 540 })]
    );
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(engine.status().grid.is_none());
    assert!(single.outputs[0].contains(engine.point()));
    assert!(engine.replace_layout(single).is_empty());
}

#[test]
fn changed_outputs_release_a_held_drag_before_repositioning() {
    let mut engine = normal();
    assert_eq!(
        tap(&mut engine, K::KEY_V),
        vec![Action::Button(K::BTN_LEFT, true)]
    );
    let single = Layout::new(vec![layout().outputs[1].clone()]).unwrap();
    let actions = engine.replace_layout(single);
    assert_eq!(actions[0], Action::Button(K::BTN_LEFT, false));
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(engine.display().contains(engine.point()));
}

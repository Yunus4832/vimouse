use evdev::KeyCode as K;
use std::time::Duration;
use vimouse::{
    config::Config,
    engine::{Action, Engine, Mode, Point},
    grid::{Grid, GridTool, Rect},
};

fn tap(engine: &mut Engine, key: K) -> Vec<Action> {
    let mut actions = engine.key(key, 1);
    actions.extend(engine.key(key, 0));
    actions
}
fn normal() -> Engine {
    let mut engine = Engine::new(Config::default());
    tap(&mut engine, K::KEY_CAPSLOCK);
    engine
}
fn block() -> Engine {
    let mut engine = normal();
    tap(&mut engine, K::KEY_F);
    engine
}
fn precision() -> Engine {
    let mut engine = normal();
    assert!(tap(&mut engine, K::KEY_TAB).is_empty());
    engine
}
fn fine() -> Engine {
    let mut engine = precision();
    tap(&mut engine, K::KEY_P);
    engine
}
fn advance(engine: &mut Engine, millis: u64) {
    for _ in 0..millis / 10 {
        engine.tick(Duration::from_millis(10));
    }
}

#[test]
fn f_selects_any_of_twenty_five_blocks_then_immediately_returns_to_normal() {
    for index in 0..25 {
        let mut engine = block();
        let root = engine.status().grid.unwrap();
        assert_eq!(root.tool, GridTool::Block);
        assert_eq!(root.columns, 5);
        let key = (1..=255)
            .map(K)
            .find(|k| vimouse::engine::character(*k, false) == Some(char::from(b'a' + index as u8)))
            .unwrap();
        let expected = root.bounds.cell(index, 5).center();
        assert_eq!(tap(&mut engine, key), vec![Action::Pointer(expected)]);
        assert_eq!(engine.mode(), Mode::Normal);
        assert!(engine.status().grid.is_none());
        assert_eq!(engine.point(), expected);
    }
}

#[test]
fn f_then_hjkl_is_continuous_normal_movement_without_an_extra_confirmation() {
    let mut engine = block();
    tap(&mut engine, K::KEY_A);
    assert_eq!(engine.point(), Point { x: 192, y: 108 });
    engine.key(K::KEY_L, 1);
    advance(&mut engine, 100);
    engine.key(K::KEY_L, 0);
    assert_eq!(engine.point(), Point { x: 352, y: 108 });
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(engine.status().grid.is_none());
}

#[test]
fn tab_can_start_without_f_and_does_not_warp_the_pointer() {
    let mut engine = normal();
    let point = engine.point();
    assert_eq!(tap(&mut engine, K::KEY_TAB), vec![]);
    assert_eq!(engine.mode(), Mode::Grid);
    assert_eq!(engine.point(), point);
    let grid = engine.status().grid.unwrap();
    assert_eq!(grid.tool, GridTool::Precision);
    assert_eq!(grid.columns, 4);
    assert_eq!(grid.depth, 1);
    assert!(grid.path.is_empty());
    assert_eq!(
        grid.bounds,
        Rect {
            x: 768,
            y: 432,
            width: 384,
            height: 216
        }
    );
}

#[test]
fn tab_recenters_after_hjkl_without_snapping_to_f_boundaries() {
    let mut engine = block();
    tap(&mut engine, K::KEY_A);
    engine.key(K::KEY_L, 1);
    advance(&mut engine, 200);
    engine.key(K::KEY_L, 0);
    assert_eq!(engine.point(), Point { x: 512, y: 108 });
    assert!(tap(&mut engine, K::KEY_TAB).is_empty());
    assert_eq!(engine.point(), Point { x: 512, y: 108 });
    let grid = engine.status().grid.unwrap();
    assert!(grid.path.is_empty());
    assert_eq!(
        grid.bounds,
        Rect {
            x: 320,
            y: 0,
            width: 384,
            height: 216
        }
    );
}

#[test]
fn precision_uses_sixteen_labels_and_stays_open_until_finished() {
    for index in 0..16 {
        let mut grid = Grid::precision(
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            Point { x: 192, y: 108 },
        );
        assert_eq!(grid.status().tool, GridTool::Precision);
        assert_eq!(grid.status().columns, 4);
        assert_eq!(
            grid.choose(char::from(b'a' + index as u8)),
            Some(Point {
                x: (index % 4) * 96 + 48,
                y: (index / 4) * 54 + 27
            })
        );
        assert!(grid.status().fine);
        assert_eq!(grid.status().columns, 3);
        assert_eq!(grid.status().selected, Some(4));
    }
    let mut engine = precision();
    for key in [K::KEY_Q, K::KEY_R, K::KEY_Y] {
        assert!(tap(&mut engine, key).is_empty());
    }
    tap(&mut engine, K::KEY_P);
    assert_eq!(engine.mode(), Mode::Grid);
    assert_eq!(engine.status().grid.unwrap().depth, 2);
    assert!(tap(&mut engine, K::KEY_SPACE).is_empty());
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn fine_hjkl_navigates_nine_positions_with_no_letter_selection() {
    let mut engine = fine();
    let bounds = engine.status().grid.unwrap().bounds;
    for key in [
        K::KEY_A,
        K::KEY_D,
        K::KEY_F,
        K::KEY_I,
        K::KEY_M,
        K::KEY_Q,
        K::KEY_V,
    ] {
        assert!(tap(&mut engine, key).is_empty());
        assert_eq!(engine.status().grid.as_ref().unwrap().selected, Some(4));
    }
    assert_eq!(
        tap(&mut engine, K::KEY_H),
        vec![Action::Pointer(bounds.cell(3, 3).center())]
    );
    assert!(tap(&mut engine, K::KEY_H).is_empty());
    tap(&mut engine, K::KEY_K);
    assert_eq!(engine.status().grid.unwrap().selected, Some(0));
    assert!(tap(&mut engine, K::KEY_K).is_empty());
    for key in [K::KEY_J, K::KEY_J, K::KEY_L, K::KEY_L] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.status().grid.unwrap().selected, Some(8));
}

#[test]
fn tab_refines_three_to_two_then_stops_and_z_restores_the_parent_selection() {
    let mut engine = fine();
    tap(&mut engine, K::KEY_L);
    tap(&mut engine, K::KEY_J);
    let parent = engine.status().grid.unwrap();
    let point = engine.point();
    assert!(matches!(
        tap(&mut engine, K::KEY_TAB).as_slice(),
        [Action::Pointer(_)]
    ));
    let last = engine.status().grid.unwrap();
    assert_eq!(last.columns, 2);
    assert_eq!(last.depth, 3);
    assert_eq!(last.bounds, parent.bounds.cell(8, 3));
    assert_eq!(last.selected, Some(3));
    for _ in 0..10 {
        assert!(tap(&mut engine, K::KEY_TAB).is_empty());
    }
    assert_eq!(engine.status().grid.unwrap().depth, 3);
    tap(&mut engine, K::KEY_H);
    assert_eq!(tap(&mut engine, K::KEY_Z), vec![Action::Pointer(point)]);
    assert_eq!(engine.status().grid.unwrap().selected, Some(8));
    assert_eq!(engine.status().grid.unwrap().columns, 3);
}

#[test]
fn z_returns_to_tab_entry_without_reopening_the_f_tool() {
    let mut engine = normal();
    engine.key(K::KEY_L, 1);
    advance(&mut engine, 50);
    engine.key(K::KEY_L, 0);
    let origin = engine.point();
    tap(&mut engine, K::KEY_TAB);
    tap(&mut engine, K::KEY_A);
    assert_eq!(tap(&mut engine, K::KEY_Z), vec![Action::Pointer(origin)]);
    let grid = engine.status().grid.unwrap();
    assert_eq!(grid.tool, GridTool::Precision);
    assert_eq!(grid.columns, 4);
    assert_eq!(grid.depth, 1);
    assert!(tap(&mut engine, K::KEY_Z).is_empty());
    assert_eq!(engine.status().grid.unwrap().depth, 1);
}

#[test]
fn space_commits_precision_without_clicking_or_changing_displays() {
    let mut engine = fine();
    tap(&mut engine, K::KEY_H);
    let target = engine.point();
    assert!(tap(&mut engine, K::KEY_SPACE).is_empty());
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(engine.status().grid.is_none());
    assert_eq!(engine.point(), target);
}

#[test]
fn escape_restores_each_tools_own_entry_position_and_preserves_previous_jump() {
    let mut engine = block();
    let origin = engine.point();
    tap(&mut engine, K::KEY_ESC);
    assert_eq!(engine.mode(), Mode::Normal);
    assert_eq!(engine.point(), origin);
    tap(&mut engine, K::KEY_F);
    tap(&mut engine, K::KEY_A);
    let block_center = engine.point();
    tap(&mut engine, K::KEY_TAB);
    tap(&mut engine, K::KEY_P);
    tap(&mut engine, K::KEY_L);
    tap(&mut engine, K::KEY_TAB);
    tap(&mut engine, K::KEY_ESC);
    assert_eq!(engine.point(), block_center);
    for key in [K::KEY_APOSTROPHE, K::KEY_APOSTROPHE] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.point(), origin);
}

#[test]
fn explicit_enter_can_click_precision_with_existing_button_modifiers() {
    for (modifier, button, mode) in [
        (None, K::BTN_LEFT, Mode::Normal),
        (Some(K::KEY_LEFTCTRL), K::BTN_MIDDLE, Mode::Normal),
        (Some(K::KEY_LEFTALT), K::BTN_RIGHT, Mode::Select),
    ] {
        let mut engine = fine();
        tap(&mut engine, K::KEY_J);
        let target = engine.point();
        if let Some(key) = modifier {
            engine.key(key, 1);
        }
        assert_eq!(
            tap(&mut engine, K::KEY_ENTER),
            vec![Action::Button(button, true), Action::Button(button, false)]
        );
        assert_eq!(engine.mode(), mode);
        assert_eq!(engine.point(), target);
    }
}

#[test]
fn starting_tools_stops_held_motion_and_repeats_do_not_reenter_or_zoom() {
    let mut engine = normal();
    engine.key(K::KEY_L, 1);
    advance(&mut engine, 100);
    let point = engine.point();
    tap(&mut engine, K::KEY_TAB);
    assert!(!engine.is_moving());
    advance(&mut engine, 100);
    assert_eq!(engine.point(), point);
    assert!(engine.key(K::KEY_TAB, 2).is_empty());
    assert_eq!(engine.status().grid.unwrap().columns, 4);
    tap(&mut engine, K::KEY_P);
    assert!(engine.key(K::KEY_TAB, 2).is_empty());
    assert_eq!(engine.status().grid.unwrap().columns, 3);
    tap(&mut engine, K::KEY_ESC);
    tap(&mut engine, K::KEY_F);
    tap(&mut engine, K::KEY_H);
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(!engine.is_moving());
    assert!(engine.key(K::KEY_H, 2).is_empty());
    assert!(!engine.is_moving());
}

#[test]
fn native_tab_shortcuts_and_insert_tab_are_preserved() {
    let mut engine = Engine::new(Config::default());
    assert_eq!(
        tap(&mut engine, K::KEY_TAB),
        vec![Action::Key(K::KEY_TAB, 1), Action::Key(K::KEY_TAB, 0)]
    );
    tap(&mut engine, K::KEY_CAPSLOCK);
    for modifier in [K::KEY_LEFTCTRL, K::KEY_LEFTALT] {
        engine.key(modifier, 1);
        assert_eq!(
            tap(&mut engine, K::KEY_TAB),
            vec![
                Action::Key(modifier, 1),
                Action::Key(K::KEY_TAB, 1),
                Action::Key(K::KEY_TAB, 0)
            ]
        );
        assert_eq!(engine.mode(), Mode::Normal);
        engine.key(modifier, 0);
    }
    engine.key(K::KEY_LEFTCTRL, 1);
    assert!(matches!(
        engine.key(K::KEY_F, 1).as_slice(),
        [Action::Scroll { .. }]
    ));
}

#[test]
fn timeout_toggle_and_emergency_release_both_tools() {
    for precision_tool in [false, true] {
        for exit in [K::KEY_CAPSLOCK, K::KEY_ESC] {
            let mut engine = if precision_tool { precision() } else { block() };
            tap(&mut engine, exit);
            assert!(engine.status().grid.is_none());
        }
        let mut engine = if precision_tool { precision() } else { block() };
        engine.tick(Duration::from_secs(61));
        assert_eq!(engine.mode(), Mode::Insert);
        assert!(engine.status().grid.is_none());
        let mut engine = if precision_tool { precision() } else { block() };
        engine.key(K::KEY_LEFTCTRL, 1);
        engine.key(K::KEY_LEFTALT, 1);
        assert!(tap(&mut engine, K::KEY_BACKSPACE).contains(&Action::Quit));
        assert_eq!(engine.mode(), Mode::Insert);
        assert!(engine.status().grid.is_none());
    }
}

#[test]
fn centered_precision_preserves_size_and_stays_inside_offset_outputs() {
    for width in 1..40 {
        let rect = Rect {
            x: -37,
            y: 19,
            width,
            height: 23,
        };
        for x in rect.x..rect.x + rect.width {
            for y in rect.y..rect.y + rect.height {
                let grid = Grid::precision(rect, Point { x, y });
                let region = grid.status().bounds;
                assert_eq!(region.width, (width / 5).max(1));
                assert_eq!(region.height, (rect.height / 5).max(1));
                assert!(region.x >= rect.x && region.y >= rect.y);
                assert!(region.x + region.width <= rect.x + rect.width);
                assert!(region.y + region.height <= rect.y + rect.height);
                assert!(x >= region.x && x < region.x + region.width);
                assert!(y >= region.y && y < region.y + region.height);
                assert_eq!(grid.status().columns, 4);
            }
        }
    }
}

#[test]
fn centered_precision_moves_smoothly_across_old_f_boundaries() {
    let screen = Rect {
        x: 0,
        y: 0,
        width: 1921,
        height: 1081,
    };
    let a = Grid::precision(screen, Point { x: 767, y: 500 })
        .status()
        .bounds;
    let b = Grid::precision(screen, Point { x: 768, y: 500 })
        .status()
        .bounds;
    assert_eq!(a.width, 384);
    assert_eq!(a.height, 216);
    assert_eq!(b.x, a.x + 1);
    assert_eq!(b.y, a.y);
    assert_eq!(a.center(), Point { x: 767, y: 500 });
    assert_eq!(b.center(), Point { x: 768, y: 500 });
}

#[test]
fn edge_correction_keeps_precision_on_current_display_without_resizing() {
    let screen = Rect {
        x: -1560,
        y: 100,
        width: 1560,
        height: 1040,
    };
    for point in [
        Point { x: -1560, y: 100 },
        Point { x: -1, y: 100 },
        Point { x: -1560, y: 1139 },
        Point { x: -1, y: 1139 },
    ] {
        let region = Grid::precision(screen, point).status().bounds;
        assert_eq!((region.width, region.height), (312, 208));
        assert!(region.x >= screen.x && region.y >= screen.y);
        assert!(region.x + region.width <= 0 && region.y + region.height <= 1140);
    }
    assert_eq!(
        Grid::precision(screen, Point { x: -1, y: 1139 })
            .status()
            .bounds,
        Rect {
            x: -312,
            y: 932,
            width: 312,
            height: 208
        }
    );
}

#[test]
fn integer_partitioning_never_produces_empty_or_outside_cells() {
    for width in 1..32 {
        for height in 1..32 {
            let parent = Rect {
                x: 27,
                y: 19,
                width,
                height,
            };
            for columns in [2, 3, 4, 5] {
                let mut area = 0;
                for index in 0..columns * columns {
                    let cell = parent.cell(index, columns);
                    area += cell.width * cell.height;
                    assert!(cell.width >= 1 && cell.height >= 1);
                    assert!(cell.x >= parent.x && cell.y >= parent.y);
                    assert!(
                        cell.x + cell.width <= parent.x + width
                            && cell.y + cell.height <= parent.y + height
                    );
                }
                if width >= columns as i32 && height >= columns as i32 {
                    assert_eq!(area, width * height);
                }
            }
        }
    }
}

#[test]
fn old_grid_settings_do_not_override_the_separate_tools() {
    let config: Config =
        toml::from_str("[grid]\nadaptive=false\nfive_min_size=150\nthree_min_size=40\n").unwrap();
    assert!(config.validate().is_ok());
    let mut engine = Engine::new(config);
    for key in [K::KEY_CAPSLOCK, K::KEY_F, K::KEY_A] {
        tap(&mut engine, key);
    }
    assert_eq!(engine.mode(), Mode::Normal);
    tap(&mut engine, K::KEY_TAB);
    assert_eq!(engine.status().grid.unwrap().tool, GridTool::Precision);
    assert_eq!(engine.status().grid.unwrap().columns, 4);
}

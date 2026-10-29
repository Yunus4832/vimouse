//! On-demand help and named-mark overlays, drawn on the existing input-transparent surfaces.
use super::render::Canvas;
use crate::{engine::Status, grid::Rect};
use std::collections::BTreeMap;
const ACCENT: u32 = 0xffffdf00;

pub(super) fn help(canvas: &mut Canvas<'_>, s: &Status, width: i32, height: i32) {
    let toggle = if s.toggle.is_empty() {
        "CAPSLOCK"
    } else {
        &s.toggle
    };
    let toggle = toggle.to_uppercase();
    let entries = [
        (toggle.as_str(), "Mouse / typing mode"),
        ("hjkl", "Hold to move; diagonals work"),
        ("f + a-y", "Jump to a 5x5 block center"),
        ("Tab + a-p", "Refine around the pointer"),
        ("hjkl / Tab", "Fine grid: move / subdivide"),
        ("z / Backspace", "Grid: back one level"),
        ("Space", "Next display; grid: finish"),
        ("1 / 2 / 3", "FAST / MED / FINE"),
        ("a", "Cycle speed"),
        ("Alt+a / Ctrl+a", "Fastest / finest speed"),
        ("Enter", "Left click"),
        ("Alt+Enter", "Right click and menu mode"),
        ("Ctrl+Enter", "Middle click"),
        ("v / Ctrl+v", "Left / middle drag; repeat to end"),
        ("i / Shift+i", "Typing / click then typing"),
        ("Alt+hjkl", "Scroll horizontally / vertically"),
        ("Ctrl+b / Ctrl+f", "Scroll up / down"),
        ("m + a-z", "Save a persistent mark"),
        ("' + a-z", "Jump to a saved mark"),
        ("M (Shift+m)", "Show marks; a-z jumps"),
        ("''", "Swap with previous jump"),
        ("gg / gc / ge", "Top-left / center / bottom-right"),
        ("zt / zz / zb", "Top / middle / bottom, keep x"),
        ("s", "Menu: hjkl arrows, Enter/q close"),
        ("Alt+Tab / Ctrl+Tab", "Native window / tab switching"),
        (":", "Command input; :q quits"),
        ("? (Shift+/)", "Toggle this help"),
        ("Ctrl+Alt+Esc", "Release control; typing mode"),
        ("Ctrl+Alt+Backspace", "Quit Vimouse immediately"),
    ];
    let w = 860.min(width - 32).max(1);
    let columns = if w >= 700 { 2 } else { 1 };
    let rows = entries.len().div_ceil(columns);
    let row_height = ((height - 160) / rows as i32).clamp(14, 23);
    let error_height = if s.mark_save_error.is_some() { 28 } else { 0 };
    let h = (rows as i32 * row_height + 116 + error_height)
        .min(height - 32)
        .max(1);
    let panel = Rect {
        x: (width - w) / 2,
        y: (height - h) / 2,
        width: w,
        height: h,
    };
    canvas.rounded(panel, 16.0, 0xf21a1e27);
    canvas.text(
        (panel.x + 24, panel.y + 20),
        "VIMOUSE  /  KEYBOARD HELP",
        17,
        0xffe9edf5,
        false,
        panel.x + w - 20,
    );
    canvas.text(
        (panel.x + 24, panel.y + 50),
        "Normal mode bindings. Grid and menu keys follow their active mode.",
        11,
        0xffa9b1c2,
        false,
        panel.x + w - 20,
    );
    canvas.fill(
        Rect {
            x: panel.x + 24,
            y: panel.y + 76,
            width: (w - 48).max(1),
            height: 1,
        },
        0xff3e4655,
    );
    let column_width = (w - 48) / columns as i32;
    for (index, (key, description)) in entries.iter().enumerate() {
        let x = panel.x + 24 + (index / rows) as i32 * column_width;
        let y = panel.y + 92 + (index % rows) as i32 * row_height;
        if y + 12 > panel.y + h - 35 {
            continue;
        }
        canvas.text((x, y), key, 12, 0xffa4e5ba, false, x + column_width - 10);
        canvas.text(
            (x + 148, y),
            description,
            11,
            0xffc3cbd9,
            false,
            x + column_width - 12,
        );
    }
    if s.mark_save_error.is_some() {
        canvas.text(
            (panel.x + 24, panel.y + h - 52),
            "Marks not saved (memory only). Retry m + letter; see service log.",
            11,
            0xffff7a85,
            false,
            panel.x + w - 20,
        );
    }
    canvas.text(
        (panel.x + 24, panel.y + h - 26),
        "Esc / ? / Enter  close",
        11,
        0xff7dd3fc,
        false,
        panel.x + w - 20,
    );
}

fn line(canvas: &mut Canvas<'_>, from: (i32, i32), to: (i32, i32)) {
    let (mut x, mut y) = from;
    let dx = (to.0 - x).abs();
    let sx = if x < to.0 { 1 } else { -1 };
    let dy = -(to.1 - y).abs();
    let sy = if y < to.1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        canvas.fill(
            Rect {
                x,
                y,
                width: 1,
                height: 1,
            },
            0xa6a69100,
        );
        if (x, y) == to {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
    }
}

pub(super) fn marks(canvas: &mut Canvas<'_>, s: &Status, output: &str, width: i32, height: i32) {
    let geometry = s
        .displays
        .iter()
        .find(|d| d.name == output)
        .or_else(|| s.display.name.is_empty().then_some(&s.display));
    let Some(geometry) = geometry else {
        return;
    };
    let mut groups: BTreeMap<(i32, i32), Vec<char>> = BTreeMap::new();
    for mark in s.mark_targets.iter().filter(|m| m.display == geometry.name) {
        if let Some(point) = mark.position {
            let x = (point.x - geometry.x) * width / geometry.width;
            let y = (point.y - geometry.y) * height / geometry.height;
            groups.entry((x, y)).or_default().push(mark.label);
        }
    }
    for ((x, y), labels) in groups {
        let text = labels
            .iter()
            .map(char::to_string)
            .collect::<Vec<_>>()
            .join("/");
        let w = (text.len() as i32 * 7 + 12).min(width).max(1);
        let tag = Rect {
            x: (x + 10).clamp(0, (width - w).max(0)),
            y: (if y >= 28 { y - 28 } else { y + 10 }).clamp(0, (height - 22).max(0)),
            width: w,
            height: 22,
        };
        line(
            canvas,
            (x, y),
            (
                x.clamp(tag.x, tag.x + tag.width - 1),
                y.clamp(tag.y, tag.y + tag.height - 1),
            ),
        );
        // The cross is the exact landing point; connected letter badges stay readable at edges.
        canvas.fill(
            Rect {
                x: x - 4,
                y: y - 1,
                width: 9,
                height: 3,
            },
            0xff171300,
        );
        canvas.fill(
            Rect {
                x: x - 1,
                y: y - 4,
                width: 3,
                height: 9,
            },
            0xff171300,
        );
        canvas.fill(
            Rect {
                x: x - 3,
                y,
                width: 7,
                height: 1,
            },
            ACCENT,
        );
        canvas.fill(
            Rect {
                x,
                y: y - 3,
                width: 1,
                height: 7,
            },
            ACCENT,
        );
        canvas.rounded(tag, 5.0, 0xf21a1e27);
        canvas.text(
            (tag.x + 6, tag.y + 4),
            &text,
            12,
            ACCENT,
            false,
            tag.x + w - 4,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config,
        displays::Display,
        engine::{Engine, MarkTarget, Mode, Point},
    };
    #[test]
    fn marks_render_on_other_outputs_but_help_stays_on_the_active_output() {
        let mut state = Engine::new(Config::default()).status();
        state.display.name = "DP-1".into();
        state.displays = vec![
            state.display.clone(),
            Display {
                name: "eDP-1".into(),
                x: 1920,
                y: 0,
                width: 1560,
                height: 1040,
            },
        ];
        state.mode = Mode::Marks;
        state.mark_targets = vec![MarkTarget {
            label: 'a',
            display: "eDP-1".into(),
            position: Some(Point { x: 2700, y: 520 }),
        }];
        let mut bytes = vec![0; 1560 * 1040 * 4];
        let mut canvas = Canvas {
            bytes: &mut bytes,
            width: 1560,
            height: 1040,
            scale: 1,
        };
        super::super::render::render(&mut canvas, Some(&state), "eDP-1", 1560, 1040);
        let offset = (520 * 1560 + 780) * 4;
        assert_eq!(&canvas.bytes[offset..offset + 4], &ACCENT.to_ne_bytes());
        state.mode = Mode::Help;
        super::super::render::render(&mut canvas, Some(&state), "eDP-1", 1560, 1040);
        assert!(canvas.bytes.iter().all(|byte| *byte == 0));
        super::super::render::render(&mut canvas, Some(&state), "DP-1", 1560, 1040);
        assert!(canvas.bytes.iter().any(|byte| *byte != 0));
        assert_eq!(
            &canvas.bytes[..4],
            &[0; 4],
            "help must not dim the entire display"
        );
    }
}

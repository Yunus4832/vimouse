//! Deterministic modal controller. No I/O, threads, clocks or desktop APIs.
mod context;
mod global;
mod tools;
use global::InputState;
mod pointer;
use context::Context;

use crate::config::Config;
use crate::displays::{Display, Layout};
use crate::grid::{Grid, GridStatus, GridTool};
use crate::marks::{Mark, Marks};
use evdev::KeyCode as K;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Insert,
    Normal,
    Visual,
    Select,
    Command,
    Grid,
    Marks,
    Help,
}

pub use crate::geometry::Point;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Key(K, i32),
    Pointer(Point),
    Button(K, bool),
    Scroll { horizontal: i32, vertical: i32 },
    Quit,
}

#[derive(Clone, Copy, Debug)]
enum Motion {
    Move(i32, i32),
    Scroll(i32, i32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarkTarget {
    pub label: char,
    pub display: String,
    pub position: Option<Point>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub mode: Mode,
    pub speed: usize,
    pub pixels_per_second: f64,
    pub position: Point,
    pub pending: String,
    pub message: String,
    pub marks: usize,
    #[serde(default)]
    pub mark_save_error: Option<String>,
    #[serde(default)]
    pub marks_pending: bool,
    #[serde(default)]
    pub mark_targets: Vec<MarkTarget>,
    #[serde(default)]
    pub toggle: String,
    pub screen: crate::config::Screen,
    pub grid: Option<GridStatus>,
    pub display: Display,
    pub displays: Vec<Display>,
    pub desktop: crate::grid::Rect,
}

pub struct Engine {
    config: Config,
    context: Context,
    speed: usize,
    x: f64,
    y: f64,
    input: InputState,
    held: BTreeMap<K, Motion>,
    marks: Marks,
    marks_revision: u64,
    mark_save_revision: Option<u64>,
    mark_save_error: Option<String>,
    previous: Option<Point>,
    message: String,
    idle: Duration,
    scroll_elapsed: Duration,
    toggle_elapsed: Option<Duration>,
    layout: Layout,
    output: usize,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        let layout = Layout::single(config.screen);
        Self::with_layout(config, layout)
    }

    pub fn with_layout(config: Config, layout: Layout) -> Self {
        let center = layout.outputs[0].rect().center();
        Self {
            x: f64::from(center.x),
            y: f64::from(center.y),
            config,
            context: Context::Insert,
            speed: 0,
            input: InputState::default(),
            held: BTreeMap::new(),
            marks: Marks::new(),
            marks_revision: 0,
            mark_save_revision: None,
            mark_save_error: None,
            previous: None,
            message: "Ready".into(),
            idle: Duration::ZERO,
            scroll_elapsed: Duration::ZERO,
            toggle_elapsed: None,
            layout,
            output: 0,
        }
    }

    pub fn status(&self) -> Status {
        Status {
            mode: self.mode(),
            speed: self.speed,
            pixels_per_second: self.config.speeds[self.speed],
            position: self.point(),
            pending: if self.mode() == Mode::Command {
                format!(":{}", self.context.command_text())
            } else {
                self.context.prefix().to_owned()
            },
            message: self.message.clone(),
            marks: self.marks.len(),
            mark_save_error: self.mark_save_error.clone(),
            marks_pending: self.mark_save_revision.is_some(),
            mark_targets: if self.mode() == Mode::Marks {
                self.marks
                    .iter()
                    .map(|(&label, mark)| MarkTarget {
                        label,
                        display: mark.display.clone(),
                        position: mark.resolve(&self.layout),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            toggle: self.config.toggle.clone(),
            screen: crate::config::Screen {
                width: self.display().width,
                height: self.display().height,
            },
            grid: self.context.grid().map(Grid::status),
            display: self.display().clone(),
            displays: self.layout.outputs.clone(),
            desktop: self.layout.bounds,
        }
    }

    pub fn restore_marks(&mut self, marks: Marks) {
        self.marks = marks;
        self.marks_revision = 0;
        self.mark_save_revision = None;
        self.mark_save_error = None;
    }
    pub fn marks(&self) -> &Marks {
        &self.marks
    }
    pub fn marks_revision(&self) -> u64 {
        self.marks_revision
    }
    pub fn mark_save_started(&mut self, revision: u64) {
        self.mark_save_revision = Some(revision);
    }
    pub fn mark_save_finished(&mut self, revision: u64, result: Result<(), String>) {
        if self.mark_save_revision != Some(revision) {
            return;
        }
        self.mark_save_revision = None;
        self.mark_save_error = result.err();
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    pub fn display(&self) -> &Display {
        &self.layout.outputs[self.output]
    }
    fn display_rect(&self) -> crate::grid::Rect {
        self.display().rect()
    }

    pub fn select_display(&mut self, name: &str) -> Vec<Action> {
        let Some(index) = self.layout.outputs.iter().position(|o| o.name == name) else {
            return Vec::new();
        };
        if index == self.output && self.display().contains(self.point()) {
            return Vec::new();
        }
        self.output = index;
        self.held.clear();
        let mut actions = Vec::new();
        self.place(self.display_rect().center(), &mut actions);
        actions
    }

    pub fn replace_layout(&mut self, layout: Layout) -> Vec<Action> {
        if layout == self.layout {
            return Vec::new();
        }
        let name = self.display().name.clone();
        let point = self.point();
        let mut actions = Vec::new();
        if self.mode() == Mode::Grid || self.mode() == Mode::Visual {
            self.enter(Context::normal(), &mut actions);
        }
        self.layout = layout;
        self.output = self
            .layout
            .outputs
            .iter()
            .position(|o| o.name == name)
            .unwrap_or_else(|| self.layout.locate(point).0);
        self.previous = None;
        self.held.clear();
        let target = if self.display().contains(point) {
            point
        } else {
            self.display_rect().center()
        };
        self.place(target, &mut actions);
        self.message = "Display layout changed".into();
        actions
    }

    pub fn needs_active_display(&self, key: K, value: i32) -> bool {
        value == 1
            && ((self.mode() == Mode::Insert && Some(key) == self.config.toggle_key())
                || (self.mode() == Mode::Normal
                    && key == K::KEY_F
                    && !self.ctrl()
                    && !self.alt()
                    && !self.shift()
                    && self.context.prefix().is_empty()))
    }

    pub fn mode(&self) -> Mode {
        self.context.mode()
    }
    pub fn is_moving(&self) -> bool {
        !self.held.is_empty()
    }
    pub fn point(&self) -> Point {
        Point {
            x: self.x.round() as i32,
            y: self.y.round() as i32,
        }
    }

    fn shift(&self) -> bool {
        self.input.down.contains(&K::KEY_LEFTSHIFT) || self.input.down.contains(&K::KEY_RIGHTSHIFT)
    }
    fn ctrl(&self) -> bool {
        self.input.down.contains(&K::KEY_LEFTCTRL) || self.input.down.contains(&K::KEY_RIGHTCTRL)
    }
    fn alt(&self) -> bool {
        self.input.down.contains(&K::KEY_LEFTALT) || self.input.down.contains(&K::KEY_RIGHTALT)
    }

    fn enter(&mut self, context: Context, actions: &mut Vec<Action>) {
        let previous = std::mem::replace(&mut self.context, context);
        if let Some(origin) = previous.exit(actions) {
            self.previous = Some(origin);
        }
        for key in std::mem::take(&mut self.input.forwarded) {
            actions.push(Action::Key(key, 0));
        }
        self.held.clear();
        self.idle = Duration::ZERO;
        self.message.clear();
    }

    fn cancel_context(&mut self, actions: &mut Vec<Action>) {
        if let Some(grid) = self.context.grid() {
            let origin = grid.origin;
            let previous = self.previous;
            self.enter(Context::normal(), actions);
            self.previous = previous;
            self.place(origin, actions);
        } else {
            let dismiss_menu = self.mode() == Mode::Select;
            self.enter(Context::normal(), actions);
            if dismiss_menu {
                Self::tap(K::KEY_ESC, actions);
            }
        }
    }

    pub fn reset(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        self.enter(Context::Insert, &mut actions);
        self.input.down.clear();
        actions
    }

    fn tap(key: K, actions: &mut Vec<Action>) {
        actions.extend([Action::Key(key, 1), Action::Key(key, 0)]);
    }

    fn click(button: K, actions: &mut Vec<Action>) {
        actions.extend([Action::Button(button, true), Action::Button(button, false)]);
    }

    fn jump(&mut self, point: Point, actions: &mut Vec<Action>) {
        self.previous = Some(self.point());
        self.place(point, actions);
    }

    fn place(&mut self, point: Point, actions: &mut Vec<Action>) {
        let (output, point) = self.layout.locate(point);
        self.output = output;
        self.x = f64::from(point.x);
        self.y = f64::from(point.y);
        actions.push(Action::Pointer(self.point()));
    }

    /// Linux key values: release=0, press=1, kernel autorepeat=2.
    pub fn key(&mut self, key: K, value: i32) -> Vec<Action> {
        let mut out = Vec::new();
        if !(0..=2).contains(&value) {
            return out;
        }
        if value == 0 {
            self.input.down.remove(&key);
            self.held.remove(&key);
        } else {
            self.input.down.insert(key);
            self.idle = Duration::ZERO;
        }

        if self.handle_global_key(key, value, &mut out) {
            return out;
        }
        let actions = match self.mode() {
            Mode::Grid => self.handle_grid(key, value),
            Mode::Help => self.handle_help(key, value),
            Mode::Marks => self.handle_marks(key, value),
            Mode::Command => self.handle_command(key, value),
            Mode::Select => self.handle_select(key),
            Mode::Normal | Mode::Visual => self.handle_pointer(key, value),
            Mode::Insert => unreachable!("Insert forwarding is handled before dispatch"),
        };
        if out.is_empty() {
            return actions;
        }
        out.extend(actions);
        out
    }

    pub fn tick(&mut self, elapsed: Duration) -> Vec<Action> {
        let mut out = Vec::new();
        if let Some(since) = &mut self.toggle_elapsed {
            *since = since.saturating_add(elapsed);
        }
        if self.mode() == Mode::Insert {
            return out;
        }
        // Held motions count as activity; an idle timeout must never end a drag
        // while the user is actively moving.
        if self.held.is_empty() {
            self.idle += elapsed;
        } else {
            self.idle = Duration::ZERO;
        }
        if self.config.idle_seconds > 0
            && self.idle >= Duration::from_secs(self.config.idle_seconds)
        {
            self.enter(Context::Insert, &mut out);
            self.message = "Idle · returned to insert".into();
            return out;
        }
        let (mut dx, mut dy, mut sx, mut sy) = (0, 0, 0, 0);
        for motion in self.held.values() {
            match *motion {
                Motion::Move(x, y) => {
                    dx += x;
                    dy += y;
                }
                Motion::Scroll(x, y) => {
                    sx += x;
                    sy += y;
                }
            }
        }
        if dx != 0 || dy != 0 {
            let previous = self.point();
            let length = f64::from(dx * dx + dy * dy).sqrt();
            let distance = self.config.speeds[self.speed] * elapsed.as_secs_f64().min(0.05);
            self.x += f64::from(dx) / length * distance;
            self.y += f64::from(dy) / length * distance;
            let (output, point) = self.layout.locate(self.point());
            self.output = output;
            let pressed_edge = point != self.point();
            if pressed_edge {
                self.x = f64::from(point.x);
                self.y = f64::from(point.y);
            }
            // Keep reporting an outward held motion at the boundary. Compositors
            // can push the real pointer back and recheck edges after a dwell;
            // stopping events here would require an unrelated sideways motion.
            if previous != self.point() || pressed_edge {
                out.push(Action::Pointer(self.point()));
            }
        }
        if sx != 0 || sy != 0 {
            self.scroll_elapsed += elapsed;
            if self.scroll_elapsed >= Duration::from_millis(self.config.scroll_interval_ms) {
                self.scroll_elapsed = Duration::ZERO;
                out.push(Action::Scroll {
                    horizontal: sx.signum() * self.config.scroll_lines,
                    vertical: sy.signum() * self.config.scroll_lines,
                });
            }
        }
        out
    }
}

pub fn is_modifier(key: K) -> bool {
    matches!(
        key,
        K::KEY_LEFTCTRL
            | K::KEY_RIGHTCTRL
            | K::KEY_LEFTALT
            | K::KEY_RIGHTALT
            | K::KEY_LEFTSHIFT
            | K::KEY_RIGHTSHIFT
            | K::KEY_LEFTMETA
            | K::KEY_RIGHTMETA
    )
}

/// Command notation uses physical US-layout keys, independent of the input method.
pub fn character(key: K, shift: bool) -> Option<char> {
    let pair = match key {
        K::KEY_A => ('a', 'A'),
        K::KEY_B => ('b', 'B'),
        K::KEY_C => ('c', 'C'),
        K::KEY_D => ('d', 'D'),
        K::KEY_E => ('e', 'E'),
        K::KEY_F => ('f', 'F'),
        K::KEY_G => ('g', 'G'),
        K::KEY_H => ('h', 'H'),
        K::KEY_I => ('i', 'I'),
        K::KEY_J => ('j', 'J'),
        K::KEY_K => ('k', 'K'),
        K::KEY_L => ('l', 'L'),
        K::KEY_M => ('m', 'M'),
        K::KEY_N => ('n', 'N'),
        K::KEY_O => ('o', 'O'),
        K::KEY_P => ('p', 'P'),
        K::KEY_Q => ('q', 'Q'),
        K::KEY_R => ('r', 'R'),
        K::KEY_S => ('s', 'S'),
        K::KEY_T => ('t', 'T'),
        K::KEY_U => ('u', 'U'),
        K::KEY_V => ('v', 'V'),
        K::KEY_W => ('w', 'W'),
        K::KEY_X => ('x', 'X'),
        K::KEY_Y => ('y', 'Y'),
        K::KEY_Z => ('z', 'Z'),
        K::KEY_SLASH => ('/', '?'),
        K::KEY_SEMICOLON => (';', ':'),
        K::KEY_APOSTROPHE => ('\'', '"'),
        K::KEY_GRAVE => ('`', '~'),
        K::KEY_SPACE => (' ', ' '),
        K::KEY_ENTER => ('\n', '\n'),
        K::KEY_1 => ('1', '!'),
        K::KEY_2 => ('2', '@'),
        K::KEY_3 => ('3', '#'),
        K::KEY_4 => ('4', '$'),
        K::KEY_5 => ('5', '%'),
        K::KEY_6 => ('6', '^'),
        K::KEY_7 => ('7', '&'),
        K::KEY_8 => ('8', '*'),
        K::KEY_9 => ('9', '('),
        K::KEY_0 => ('0', ')'),
        K::KEY_MINUS => ('-', '_'),
        K::KEY_EQUAL => ('=', '+'),
        _ => return None,
    };
    Some(if shift { pair.1 } else { pair.0 })
}

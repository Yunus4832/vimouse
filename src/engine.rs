//! Deterministic modal controller. No I/O, threads, clocks or desktop APIs.
use crate::config::Config;
use crate::displays::{Display, Layout};
use crate::grid::{Grid, GridStatus, GridTool};
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

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
pub struct Status {
    pub mode: Mode,
    pub speed: usize,
    pub pixels_per_second: f64,
    pub position: Point,
    pub pending: String,
    pub message: String,
    pub marks: usize,
    pub screen: crate::config::Screen,
    pub grid: Option<GridStatus>,
    pub display: Display,
    pub displays: Vec<Display>,
    pub desktop: crate::grid::Rect,
}

pub struct Engine {
    config: Config,
    mode: Mode,
    speed: usize,
    x: f64,
    y: f64,
    keys: BTreeSet<K>,
    forwarded: BTreeSet<K>,
    held: BTreeMap<K, Motion>,
    drag_button: Option<K>,
    pending: String,
    command: String,
    marks: BTreeMap<char, Point>,
    previous: Option<Point>,
    message: String,
    idle: Duration,
    scroll_elapsed: Duration,
    toggle_elapsed: Option<Duration>,
    grid: Option<Grid>,
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
            mode: Mode::Insert,
            speed: 0,
            keys: BTreeSet::new(),
            forwarded: BTreeSet::new(),
            held: BTreeMap::new(),
            drag_button: None,
            pending: String::new(),
            command: String::new(),
            marks: BTreeMap::new(),
            previous: None,
            message: "Ready".into(),
            idle: Duration::ZERO,
            scroll_elapsed: Duration::ZERO,
            toggle_elapsed: None,
            grid: None,
            layout,
            output: 0,
        }
    }

    pub fn status(&self) -> Status {
        Status {
            mode: self.mode,
            speed: self.speed,
            pixels_per_second: self.config.speeds[self.speed],
            position: self.point(),
            pending: if self.mode == Mode::Command {
                format!(":{}", self.command)
            } else {
                self.pending.clone()
            },
            message: self.message.clone(),
            marks: self.marks.len(),
            screen: crate::config::Screen {
                width: self.display().width,
                height: self.display().height,
            },
            grid: self.grid.as_ref().map(Grid::status),
            display: self.display().clone(),
            displays: self.layout.outputs.clone(),
            desktop: self.layout.bounds,
        }
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
        if self.mode == Mode::Grid || self.mode == Mode::Visual {
            self.transition(Mode::Normal, &mut actions);
        }
        self.layout = layout;
        self.output = self
            .layout
            .outputs
            .iter()
            .position(|o| o.name == name)
            .unwrap_or_else(|| self.layout.locate(point).0);
        self.marks.clear();
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
            && ((self.mode == Mode::Insert && Some(key) == self.config.toggle_key())
                || (self.mode == Mode::Normal
                    && key == K::KEY_F
                    && !self.ctrl()
                    && !self.alt()
                    && !self.shift()
                    && self.pending.is_empty()))
    }

    pub fn mode(&self) -> Mode {
        self.mode
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
        self.keys.contains(&K::KEY_LEFTSHIFT) || self.keys.contains(&K::KEY_RIGHTSHIFT)
    }
    fn ctrl(&self) -> bool {
        self.keys.contains(&K::KEY_LEFTCTRL) || self.keys.contains(&K::KEY_RIGHTCTRL)
    }
    fn alt(&self) -> bool {
        self.keys.contains(&K::KEY_LEFTALT) || self.keys.contains(&K::KEY_RIGHTALT)
    }

    fn release_drag(&mut self, actions: &mut Vec<Action>) {
        if let Some(button) = self.drag_button.take() {
            actions.push(Action::Button(button, false));
        }
    }

    fn transition(&mut self, mode: Mode, actions: &mut Vec<Action>) {
        if let Some(grid) = self.grid.take() {
            self.previous = Some(grid.origin);
        }
        self.release_drag(actions);
        for key in std::mem::take(&mut self.forwarded) {
            actions.push(Action::Key(key, 0));
        }
        self.held.clear();
        self.pending.clear();
        self.command.clear();
        self.idle = Duration::ZERO;
        self.mode = mode;
        self.message.clear();
    }

    pub fn reset(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        self.transition(Mode::Insert, &mut actions);
        self.keys.clear();
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
            self.keys.remove(&key);
            self.held.remove(&key);
        } else {
            self.keys.insert(key);
            self.idle = Duration::ZERO;
        }

        // An unconditional escape route, including during command entry and drags.
        if value == 1 && self.ctrl() && self.alt() && (key == K::KEY_ESC || key == K::KEY_BACKSPACE)
        {
            self.transition(Mode::Insert, &mut out);
            if key == K::KEY_BACKSPACE {
                out.push(Action::Quit);
            }
            return out;
        }
        if Some(key) == self.config.toggle_key() {
            if value == 1 {
                let native_caps = key == K::KEY_CAPSLOCK
                    && self.mode == Mode::Normal
                    && self
                        .toggle_elapsed
                        .is_some_and(|elapsed| elapsed <= Duration::from_millis(350));
                let mode = if self.mode == Mode::Insert {
                    Mode::Normal
                } else {
                    Mode::Insert
                };
                self.transition(mode, &mut out);
                self.toggle_elapsed = if mode == Mode::Normal {
                    Some(Duration::ZERO)
                } else {
                    None
                };
                if native_caps {
                    Self::tap(K::KEY_CAPSLOCK, &mut out);
                }
                // Wayland does not expose global pointer position. Reclaim our last
                // virtual position on activation so marks and jumps stay consistent.
                if mode == Mode::Normal {
                    out.push(Action::Pointer(self.point()));
                }
            }
            return out;
        }
        if value == 0 && self.forwarded.remove(&key) {
            out.push(Action::Key(key, 0));
            return out;
        }
        if self.mode == Mode::Insert {
            match value {
                1 => {
                    self.forwarded.insert(key);
                    out.push(Action::Key(key, 1));
                }
                2 if self.forwarded.contains(&key) => out.push(Action::Key(key, 2)),
                _ => {}
            }
            return out;
        }
        if value == 0 || is_modifier(key) {
            return out;
        }
        // Preserve native Alt+Tab / Ctrl+Tab, including holding the modifier
        // across multiple tabs. Releases are balanced even outside insert mode.
        if key == K::KEY_TAB && (self.ctrl() || self.alt()) && self.mode != Mode::Command {
            for modifier in [
                K::KEY_LEFTCTRL,
                K::KEY_RIGHTCTRL,
                K::KEY_LEFTALT,
                K::KEY_RIGHTALT,
                K::KEY_LEFTSHIFT,
                K::KEY_RIGHTSHIFT,
            ] {
                if self.keys.contains(&modifier) && self.forwarded.insert(modifier) {
                    out.push(Action::Key(modifier, 1));
                }
            }
            Self::tap(K::KEY_TAB, &mut out);
            return out;
        }
        // Do not leak a native switching modifier into the next mouse action.
        for key in std::mem::take(&mut self.forwarded) {
            out.push(Action::Key(key, 0));
        }
        if self.mode == Mode::Grid {
            // One physical press selects one level. Autorepeat must not zoom
            // through several levels while the user is reading the overlay.
            if value != 1 {
                return out;
            }
            if key == K::KEY_ESC {
                let origin = self.grid.as_ref().unwrap().origin;
                let previous = self.previous;
                self.transition(Mode::Normal, &mut out);
                self.previous = previous;
                self.place(origin, &mut out);
            } else if key == K::KEY_SPACE || key == K::KEY_ENTER {
                self.transition(Mode::Normal, &mut out);
                if key == K::KEY_ENTER {
                    let button = if self.alt() {
                        K::BTN_RIGHT
                    } else if self.ctrl() {
                        K::BTN_MIDDLE
                    } else {
                        K::BTN_LEFT
                    };
                    Self::click(button, &mut out);
                    if button == K::BTN_RIGHT {
                        self.transition(Mode::Select, &mut out);
                    }
                }
            } else if !self.ctrl() && !self.alt() {
                let label = character(key, false);
                let back = key == K::KEY_BACKSPACE || label == Some('z');
                let point = if back {
                    self.grid.as_mut().unwrap().back()
                } else if key == K::KEY_TAB {
                    let point = self.grid.as_mut().unwrap().refine();
                    if point.is_none() && self.grid.as_ref().unwrap().status().fine {
                        self.message = "Final 2×2 level reached".into();
                    }
                    point
                } else if self.grid.as_ref().unwrap().status().fine {
                    let direction = match key {
                        K::KEY_H => Some((-1, 0)),
                        K::KEY_L => Some((1, 0)),
                        K::KEY_K => Some((0, -1)),
                        K::KEY_J => Some((0, 1)),
                        _ => None,
                    };
                    direction.and_then(|(dx, dy)| self.grid.as_mut().unwrap().nudge(dx, dy))
                } else {
                    label.and_then(|c| self.grid.as_mut().unwrap().choose(c))
                };
                if let Some(point) = point {
                    if self.grid.as_ref().unwrap().tool() == GridTool::Block {
                        self.transition(Mode::Normal, &mut out);
                    } else {
                        self.message.clear();
                    }
                    self.place(point, &mut out);
                } else if back {
                    self.message = "Already at the top level".into();
                }
            }
            return out;
        }
        if key == K::KEY_ESC {
            self.transition(Mode::Normal, &mut out);
            return out;
        }
        let ch = character(key, self.shift());
        if self.mode == Mode::Command {
            match key {
                K::KEY_ENTER if value == 1 => {
                    let command = self.command.clone();
                    self.transition(Mode::Normal, &mut out);
                    match command.as_str() {
                        "q" | "quit" => { out.extend(self.reset()); out.push(Action::Quit); },
                        "i" | "insert" => self.transition(Mode::Insert, &mut out),
                        "marks" => self.message = self.marks.keys().collect(),
                        "help" => self.message = "hjkl move · Enter click · a speed · i insert · v drag · m{a-z} mark · :q quit".into(),
                        _ => self.message = format!("Unknown command: {command}"),
                    }
                }
                K::KEY_BACKSPACE => {
                    self.command.pop();
                }
                _ => {
                    if let Some(ch) = ch {
                        if self.command.len() < 128 {
                            self.command.push(ch);
                        }
                    }
                }
            }
            return out;
        }
        if self.mode == Mode::Select {
            let mapped = match key {
                K::KEY_H => Some(K::KEY_LEFT),
                K::KEY_J => Some(K::KEY_DOWN),
                K::KEY_K => Some(K::KEY_UP),
                K::KEY_L => Some(K::KEY_RIGHT),
                K::KEY_ENTER => {
                    self.transition(Mode::Normal, &mut out);
                    Some(K::KEY_ENTER)
                }
                K::KEY_Q => {
                    let escape = !self.shift();
                    self.transition(Mode::Normal, &mut out);
                    escape.then_some(K::KEY_ESC)
                }
                _ => None,
            };
            if let Some(key) = mapped {
                Self::tap(key, &mut out);
            }
            return out;
        }
        // Only motions repeat continuously. Discrete commands execute once per press.
        if value == 2 {
            return out;
        }
        if self.mode == Mode::Normal && key == K::KEY_SPACE && !self.ctrl() && !self.alt() {
            self.held.clear();
            self.pending.clear();
            self.output = (self.output + 1) % self.layout.outputs.len();
            self.jump(self.display_rect().center(), &mut out);
            self.message = format!(
                "Display {}/{}: {}",
                self.output + 1,
                self.layout.outputs.len(),
                self.display().name
            );
            return out;
        }
        if self.mode == Mode::Normal
            && key == K::KEY_TAB
            && !self.ctrl()
            && !self.alt()
            && !self.shift()
        {
            let grid = Grid::precision(self.display_rect(), self.point());
            self.transition(Mode::Grid, &mut out);
            self.grid = Some(grid);
            return out;
        }
        if !self.pending.is_empty() {
            let prefix = std::mem::take(&mut self.pending);
            match (prefix.as_str(), ch) {
                ("m", Some(c @ 'a'..='z')) => {
                    self.marks.insert(c, self.point());
                    self.message = format!("Mark {c} saved");
                }
                ("'" | "`", Some(c)) => {
                    let target = if c == '\'' || c == '`' {
                        self.previous
                    } else {
                        self.marks.get(&c).copied()
                    };
                    if let Some(p) = target {
                        self.jump(p, &mut out);
                    } else {
                        self.message = format!("Mark {c} is not set");
                    }
                }
                ("g", Some('g')) => self.jump(
                    Point {
                        x: self.display().x,
                        y: self.display().y,
                    },
                    &mut out,
                ),
                ("g", Some('c')) => self.jump(
                    Point {
                        x: self.display().x + self.display().width / 2,
                        y: self.display().y + self.display().height / 2,
                    },
                    &mut out,
                ),
                ("g", Some('e')) => self.jump(
                    Point {
                        x: self.display().x + self.display().width - 1,
                        y: self.display().y + self.display().height - 1,
                    },
                    &mut out,
                ),
                ("z", Some('t')) | ("z", Some('\n')) => self.jump(
                    Point {
                        x: self.point().x,
                        y: self.display().y,
                    },
                    &mut out,
                ),
                ("z", Some('z')) => self.jump(
                    Point {
                        x: self.point().x,
                        y: self.display().y + self.display().height / 2,
                    },
                    &mut out,
                ),
                ("z", Some('b')) => self.jump(
                    Point {
                        x: self.point().x,
                        y: self.display().y + self.display().height - 1,
                    },
                    &mut out,
                ),
                _ => self.message = "Unknown key sequence".into(),
            }
            return out;
        }
        self.message.clear();
        if self.ctrl() && key == K::KEY_A {
            self.speed = 2;
            return out;
        }
        if self.alt() && key == K::KEY_A {
            self.speed = 0;
            return out;
        }

        let direction = match key {
            K::KEY_H => Some((-1, 0)),
            K::KEY_L => Some((1, 0)),
            K::KEY_K => Some((0, -1)),
            K::KEY_J => Some((0, 1)),
            _ => None,
        };
        let scroll = if self.alt() {
            direction.map(|(x, y)| (-x, -y))
        } else if self.ctrl() && key == K::KEY_B {
            Some((0, 1))
        } else if self.ctrl() && key == K::KEY_F {
            Some((0, -1))
        } else {
            None
        };
        if let Some((x, y)) = scroll {
            self.held.insert(key, Motion::Scroll(x, y));
            self.scroll_elapsed = Duration::ZERO;
            out.push(Action::Scroll {
                horizontal: x * self.config.scroll_lines,
                vertical: y * self.config.scroll_lines,
            });
        } else if let Some((x, y)) = direction {
            if !self.ctrl() && !self.alt() {
                self.held.insert(key, Motion::Move(x, y));
            }
        } else if key == K::KEY_ENTER {
            self.release_drag(&mut out);
            if self.mode == Mode::Visual {
                self.mode = Mode::Normal;
            }
            let button = if self.alt() {
                K::BTN_RIGHT
            } else if self.ctrl() {
                K::BTN_MIDDLE
            } else {
                K::BTN_LEFT
            };
            Self::click(button, &mut out);
            if button == K::BTN_RIGHT {
                self.transition(Mode::Select, &mut out);
            }
        } else if key == K::KEY_V && !self.alt() {
            if self.mode == Mode::Visual {
                self.transition(Mode::Normal, &mut out);
            } else {
                self.transition(Mode::Visual, &mut out);
                let button = if self.ctrl() {
                    K::BTN_MIDDLE
                } else {
                    K::BTN_LEFT
                };
                self.drag_button = Some(button);
                out.push(Action::Button(button, true));
            }
        } else if !self.ctrl() && !self.alt() {
            match ch {
                Some('i' | 'I') => {
                    self.release_drag(&mut out);
                    if self.shift() {
                        Self::click(K::BTN_LEFT, &mut out);
                    }
                    self.transition(Mode::Insert, &mut out);
                }
                Some('a') => self.speed = (self.speed + 1) % 3,
                Some('s') => self.transition(Mode::Select, &mut out),
                Some('f') if self.mode == Mode::Normal => {
                    self.transition(Mode::Grid, &mut out);
                    self.grid = Some(Grid::in_rect(self.display_rect(), self.point()));
                }
                Some(':') => self.transition(Mode::Command, &mut out),
                Some(c @ ('g' | 'z' | 'm' | '\'' | '`')) => self.pending.push(c),
                _ => {}
            }
        }
        out
    }

    pub fn tick(&mut self, elapsed: Duration) -> Vec<Action> {
        let mut out = Vec::new();
        if let Some(since) = &mut self.toggle_elapsed {
            *since = since.saturating_add(elapsed);
        }
        if self.mode == Mode::Insert {
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
            self.transition(Mode::Insert, &mut out);
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
            if point != self.point() {
                self.x = f64::from(point.x);
                self.y = f64::from(point.y);
            }
            if previous != self.point() {
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

use super::*;

impl Engine {
    pub(super) fn handle_grid(&mut self, key: K, value: i32) -> Vec<Action> {
        let mut out = Vec::new();

        // One physical press selects one level. Autorepeat must not zoom
        // through several levels while the user is reading the overlay.
        if value != 1 {
            return out;
        }
        if key == K::KEY_SPACE || key == K::KEY_ENTER {
            self.enter(Context::normal(), &mut out);
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
                    self.enter(Context::Select, &mut out);
                }
            }
        } else if !self.ctrl() && !self.alt() {
            let label = character(key, false);
            let back = key == K::KEY_BACKSPACE || label == Some('z');
            let point = if back {
                self.context.grid_mut().back()
            } else if key == K::KEY_TAB {
                let point = self.context.grid_mut().refine();
                if point.is_none() && self.context.grid().expect("Grid dispatch").status().fine {
                    self.message = "Final 2×2 level reached".into();
                }
                point
            } else if self.context.grid().expect("Grid dispatch").status().fine {
                let direction = match key {
                    K::KEY_H => Some((-1, 0)),
                    K::KEY_L => Some((1, 0)),
                    K::KEY_K => Some((0, -1)),
                    K::KEY_J => Some((0, 1)),
                    _ => None,
                };
                direction.and_then(|(dx, dy)| self.context.grid_mut().nudge(dx, dy))
            } else {
                label.and_then(|c| self.context.grid_mut().choose(c))
            };
            if let Some(point) = point {
                if self.context.grid().expect("Grid dispatch").tool() == GridTool::Block {
                    self.enter(Context::normal(), &mut out);
                } else {
                    self.message.clear();
                }
                self.place(point, &mut out);
            } else if back {
                self.message = "Already at the top level".into();
            }
        }
        out
    }
    pub(super) fn handle_help(&mut self, key: K, value: i32) -> Vec<Action> {
        let mut out = Vec::new();
        let ch = character(key, self.shift());

        if value == 1 && !self.ctrl() && !self.alt() && (key == K::KEY_ENTER || ch == Some('?')) {
            self.enter(Context::normal(), &mut out);
        }
        out
    }
    pub(super) fn handle_marks(&mut self, key: K, value: i32) -> Vec<Action> {
        let mut out = Vec::new();
        let ch = character(key, self.shift());

        if value == 1 && !self.ctrl() && !self.alt() {
            if ch == Some('M') {
                self.enter(Context::normal(), &mut out);
            } else if let Some(label @ 'a'..='z') = character(key, false) {
                if let Some(mark) = self.marks.get(&label) {
                    if let Some(point) = mark.resolve(&self.layout) {
                        self.enter(Context::normal(), &mut out);
                        self.jump(point, &mut out);
                    } else {
                        self.message = format!("Mark {label}: display {} is offline", mark.display);
                    }
                } else {
                    self.message = format!("Mark {label} is not set");
                }
            }
        }
        out
    }
    pub(super) fn handle_command(&mut self, key: K, value: i32) -> Vec<Action> {
        let mut out = Vec::new();
        let ch = character(key, self.shift());

        match key {
            K::KEY_ENTER if value == 1 => {
                let command = self.context.command_text().to_owned();
                self.enter(Context::normal(), &mut out);
                match command.as_str() {
                    "q" | "quit" => {
                        out.extend(self.reset());
                        out.push(Action::Quit);
                    }
                    "i" | "insert" => self.enter(Context::Insert, &mut out),
                    "marks" => self.enter(Context::Marks, &mut out),
                    "help" => self.enter(Context::Help, &mut out),
                    _ => self.message = format!("Unknown command: {command}"),
                }
            }
            K::KEY_BACKSPACE => {
                self.context.command_text_mut().pop();
            }
            _ => {
                if let Some(ch) = ch {
                    if self.context.command_text().len() < 128 {
                        self.context.command_text_mut().push(ch);
                    }
                }
            }
        }
        out
    }
    pub(super) fn handle_select(&mut self, key: K) -> Vec<Action> {
        let mut out = Vec::new();

        let mapped = match key {
            K::KEY_H => Some(K::KEY_LEFT),
            K::KEY_J => Some(K::KEY_DOWN),
            K::KEY_K => Some(K::KEY_UP),
            K::KEY_L => Some(K::KEY_RIGHT),
            K::KEY_ENTER => {
                self.enter(Context::normal(), &mut out);
                Some(K::KEY_ENTER)
            }
            K::KEY_Q => {
                let escape = !self.shift();
                self.enter(Context::normal(), &mut out);
                escape.then_some(K::KEY_ESC)
            }
            _ => None,
        };
        if let Some(key) = mapped {
            Self::tap(key, &mut out);
        }
        out
    }
}

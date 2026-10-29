use super::*;

impl Engine {
    pub(super) fn handle_pointer(&mut self, key: K, value: i32) -> Vec<Action> {
        let mut out = Vec::new();
        let ch = character(key, self.shift());
        // Only motions repeat continuously. Discrete commands execute once per press.
        if value == 2 {
            return out;
        }
        if self.mode() == Mode::Normal && key == K::KEY_SPACE && !self.ctrl() && !self.alt() {
            self.held.clear();
            self.context.prefix_mut().clear();
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
        if self.mode() == Mode::Normal
            && key == K::KEY_TAB
            && !self.ctrl()
            && !self.alt()
            && !self.shift()
        {
            let grid = Grid::precision(self.display_rect(), self.point());
            self.enter(Context::Grid(grid), &mut out);
            return out;
        }
        if !self.context.prefix().is_empty() {
            let prefix = std::mem::take(self.context.prefix_mut());
            match (prefix.as_str(), ch) {
                ("m", Some(c @ 'a'..='z')) => {
                    let mark = Mark::capture(self.display(), self.point());
                    self.marks.insert(c, mark);
                    self.marks_revision += 1;
                    self.message = format!("Mark {c} saved");
                }
                ("'" | "`", Some(c)) => {
                    if c == '\'' || c == '`' {
                        if let Some(point) = self.previous {
                            self.jump(point, &mut out);
                        } else {
                            self.message = "No previous jump".into();
                        }
                    } else if let Some(mark) = self.marks.get(&c) {
                        if let Some(point) = mark.resolve(&self.layout) {
                            self.jump(point, &mut out);
                        } else {
                            self.message =
                                format!("Display {} for mark {c} is unavailable", mark.display);
                        }
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
            if self.mode() == Mode::Visual {
                self.enter(Context::normal(), &mut out);
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
                self.enter(Context::Select, &mut out);
            }
        } else if key == K::KEY_V && !self.alt() {
            if self.mode() == Mode::Visual {
                self.enter(Context::normal(), &mut out);
            } else {
                let button = if self.ctrl() {
                    K::BTN_MIDDLE
                } else {
                    K::BTN_LEFT
                };
                self.enter(
                    Context::Visual {
                        button,
                        prefix: String::new(),
                    },
                    &mut out,
                );
                out.push(Action::Button(button, true));
            }
        } else if !self.ctrl() && !self.alt() {
            match ch {
                Some('i' | 'I') => {
                    let click = self.shift();
                    self.enter(Context::Insert, &mut out);
                    if click {
                        Self::click(K::BTN_LEFT, &mut out);
                    }
                }
                Some('M') if self.mode() == Mode::Normal => self.enter(Context::Marks, &mut out),
                Some('?') if self.mode() == Mode::Normal => self.enter(Context::Help, &mut out),
                Some(c @ '1'..='3') if self.mode() == Mode::Normal => {
                    self.speed = (c as u8 - b'1') as usize
                }
                Some('a') => self.speed = (self.speed + 1) % 3,
                Some('s') => self.enter(Context::Select, &mut out),
                Some('f') if self.mode() == Mode::Normal => {
                    self.enter(
                        Context::Grid(Grid::in_rect(self.display_rect(), self.point())),
                        &mut out,
                    );
                }
                Some(':') => self.enter(Context::command(), &mut out),
                Some(c @ ('g' | 'z' | 'm' | '\'' | '`')) => self.context.prefix_mut().push(c),
                _ => {}
            }
        }
        out
    }
}

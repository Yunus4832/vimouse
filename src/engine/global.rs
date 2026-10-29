//! Physical key tracking and the explicitly shared escape/switching policy.
use super::*;

#[derive(Default)]
pub(super) struct InputState {
    pub(super) down: BTreeSet<K>,
    pub(super) forwarded: BTreeSet<K>,
}
impl Engine {
    pub(super) fn handle_global_key(&mut self, key: K, value: i32, out: &mut Vec<Action>) -> bool {
        // An unconditional escape route, including during command entry and drags.
        if value == 1 && self.ctrl() && self.alt() && (key == K::KEY_ESC || key == K::KEY_BACKSPACE)
        {
            self.enter(Context::Insert, out);
            if key == K::KEY_BACKSPACE {
                out.push(Action::Quit);
            }
            return true;
        }
        if Some(key) == self.config.toggle_key() {
            if value == 1 {
                let native_caps = key == K::KEY_CAPSLOCK
                    && self.mode() == Mode::Normal
                    && self
                        .toggle_elapsed
                        .is_some_and(|elapsed| elapsed <= Duration::from_millis(350));
                let mode = if self.mode() == Mode::Insert {
                    Mode::Normal
                } else {
                    Mode::Insert
                };
                self.enter(
                    if mode == Mode::Normal {
                        Context::normal()
                    } else {
                        Context::Insert
                    },
                    out,
                );
                self.toggle_elapsed = if mode == Mode::Normal {
                    Some(Duration::ZERO)
                } else {
                    None
                };
                if native_caps {
                    Self::tap(K::KEY_CAPSLOCK, out);
                }
                // Wayland does not expose global pointer position. Reclaim our last
                // virtual position on activation so marks and jumps stay consistent.
                if mode == Mode::Normal {
                    out.push(Action::Pointer(self.point()));
                }
            }
            return true;
        }
        if value == 0 && self.input.forwarded.remove(&key) {
            out.push(Action::Key(key, 0));
            return true;
        }
        if self.mode() == Mode::Insert {
            match value {
                1 => {
                    self.input.forwarded.insert(key);
                    out.push(Action::Key(key, 1));
                }
                2 if self.input.forwarded.contains(&key) => out.push(Action::Key(key, 2)),
                _ => {}
            }
            return true;
        }
        if value == 0 || is_modifier(key) {
            return true;
        }
        // Preserve native Alt+Tab / Ctrl+Tab, including holding the modifier
        // across multiple tabs. Releases are balanced even outside insert mode.
        if key == K::KEY_TAB && (self.ctrl() || self.alt()) && self.mode() != Mode::Command {
            for modifier in [
                K::KEY_LEFTCTRL,
                K::KEY_RIGHTCTRL,
                K::KEY_LEFTALT,
                K::KEY_RIGHTALT,
                K::KEY_LEFTSHIFT,
                K::KEY_RIGHTSHIFT,
            ] {
                if self.input.down.contains(&modifier) && self.input.forwarded.insert(modifier) {
                    out.push(Action::Key(modifier, 1));
                }
            }
            Self::tap(K::KEY_TAB, out);
            return true;
        }
        // Do not leak a native switching modifier into the next mouse action.
        for key in std::mem::take(&mut self.input.forwarded) {
            out.push(Action::Key(key, 0));
        }
        if key == K::KEY_ESC {
            if value == 1 {
                self.cancel_context(out);
            }
            return true;
        }
        false
    }
}

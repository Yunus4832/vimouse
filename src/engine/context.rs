//! Context-owned state. A grid/command/drag cannot exist without its context.
use super::{Action, K, Mode, Point};
use crate::grid::Grid;

pub(super) enum Context {
    Insert,
    Normal { prefix: String },
    Visual { button: K, prefix: String },
    Select,
    Command { text: String },
    Grid(Grid),
    Marks,
    Help,
}
impl Context {
    pub(super) fn normal() -> Self {
        Self::Normal {
            prefix: String::new(),
        }
    }
    pub(super) fn command() -> Self {
        Self::Command {
            text: String::new(),
        }
    }
    pub(super) fn mode(&self) -> Mode {
        match self {
            Self::Insert => Mode::Insert,
            Self::Normal { .. } => Mode::Normal,
            Self::Visual { .. } => Mode::Visual,
            Self::Select => Mode::Select,
            Self::Command { .. } => Mode::Command,
            Self::Grid(_) => Mode::Grid,
            Self::Marks => Mode::Marks,
            Self::Help => Mode::Help,
        }
    }
    pub(super) fn prefix(&self) -> &str {
        match self {
            Self::Normal { prefix } | Self::Visual { prefix, .. } => prefix,
            _ => "",
        }
    }
    pub(super) fn prefix_mut(&mut self) -> &mut String {
        match self {
            Self::Normal { prefix } | Self::Visual { prefix, .. } => prefix,
            _ => unreachable!("prefix input is dispatched only to pointer contexts"),
        }
    }
    pub(super) fn command_text(&self) -> &str {
        match self {
            Self::Command { text } => text,
            _ => "",
        }
    }
    pub(super) fn command_text_mut(&mut self) -> &mut String {
        match self {
            Self::Command { text } => text,
            _ => unreachable!("command input is dispatched only to Command"),
        }
    }
    pub(super) fn grid(&self) -> Option<&Grid> {
        if let Self::Grid(grid) = self {
            Some(grid)
        } else {
            None
        }
    }
    pub(super) fn grid_mut(&mut self) -> &mut Grid {
        match self {
            Self::Grid(grid) => grid,
            _ => unreachable!("grid input is dispatched only to Grid"),
        }
    }
    pub(super) fn exit(self, actions: &mut Vec<Action>) -> Option<Point> {
        match self {
            Self::Visual { button, .. } => {
                actions.push(Action::Button(button, false));
                None
            }
            Self::Grid(grid) => Some(grid.origin),
            _ => None,
        }
    }
}

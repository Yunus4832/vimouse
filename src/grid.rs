//! Integer, half-open rectangles: every selection stays inside its parent.
use crate::engine::Point;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn center(self) -> Point {
        Point {
            x: self.x + self.width / 2,
            y: self.y + self.height / 2,
        }
    }

    pub fn cell(self, index: usize, columns: usize) -> Self {
        self.cell_in(index, columns, columns)
    }

    pub fn cell_in(self, index: usize, columns: usize, rows: usize) -> Self {
        assert!((1..=5).contains(&columns) && (1..=5).contains(&rows) && index < columns * rows);
        let col = (index % columns) as i32;
        let row = (index / columns) as i32;
        let columns = columns as i32;
        let rows = rows as i32;
        let x = col * self.width / columns;
        let y = row * self.height / rows;
        // Below five pixels, some labels intentionally refer to the same pixel.
        // This keeps all labels valid without producing empty/outside rectangles.
        let width = (((col + 1) * self.width / columns) - x).max(1);
        let height = (((row + 1) * self.height / rows) - y).max(1);
        Self {
            x: self.x + x,
            y: self.y + y,
            width,
            height,
        }
    }

    pub fn centered_region(self, point: Point, width: i32, height: i32) -> Self {
        let width = width.clamp(1, self.width);
        let height = height.clamp(1, self.height);
        Self {
            x: (point.x - width / 2).clamp(self.x, self.x + self.width - width),
            y: (point.y - height / 2).clamp(self.y, self.y + self.height - height),
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GridTool {
    Block,
    Precision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridStatus {
    pub tool: GridTool,
    pub bounds: Rect,
    pub path: String,
    pub depth: usize,
    pub columns: usize,
    pub rows: usize,
    pub fine: bool,
    pub selected: Option<usize>,
}

pub struct Grid {
    pub origin: Point,
    history: Vec<Frame>,
    path: String,
    tool: GridTool,
    screen: Rect,
}

struct Frame {
    bounds: Rect,
    selected: usize,
    divisions: usize,
}

const LAST_DEPTH: usize = 3;

impl Frame {
    fn new(bounds: Rect, divisions: usize) -> Self {
        Self {
            bounds,
            selected: (divisions / 2) * divisions + divisions / 2,
            divisions,
        }
    }

    fn point(&self) -> Point {
        self.bounds.cell(self.selected, self.divisions).center()
    }
}

impl Grid {
    pub fn new(width: i32, height: i32, origin: Point) -> Self {
        Self::in_rect(
            Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            origin,
        )
    }

    pub fn in_rect(bounds: Rect, origin: Point) -> Self {
        Self {
            origin,
            history: vec![Frame::new(bounds, 5)],
            path: String::new(),
            tool: GridTool::Block,
            screen: bounds,
        }
    }

    /// Center an independent 4x4 viewport at the pointer, keeping its previous
    /// screen/5 extent. Translate at output edges; never warp the pointer.
    pub fn precision(screen: Rect, origin: Point) -> Self {
        let region = screen.centered_region(
            origin,
            (screen.width / 5).max(1),
            (screen.height / 5).max(1),
        );
        Self {
            origin,
            history: vec![Frame::new(region, 4)],
            path: String::new(),
            tool: GridTool::Precision,
            screen,
        }
    }

    pub fn tool(&self) -> GridTool {
        self.tool
    }

    pub fn status(&self) -> GridStatus {
        let frame = self.history.last().unwrap();
        let bounds = frame.bounds;
        let depth = self.history.len() - 1 + usize::from(self.tool == GridTool::Precision);
        let fine = depth >= 2;
        let columns = frame.divisions;
        GridStatus {
            tool: self.tool,
            bounds,
            path: self.path.clone(),
            depth,
            columns,
            rows: columns,
            fine,
            selected: fine.then_some(frame.selected),
        }
    }

    pub fn choose(&mut self, label: char) -> Option<Point> {
        let status = self.status();
        if status.fine
            || !('a'..='y').contains(&label)
            || (label as u8 - b'a') as usize >= status.columns * status.rows
        {
            return None;
        }
        let parent = self.history.last().unwrap().bounds;
        let child = parent.cell((label as u8 - b'a') as usize, status.columns);
        if self.tool == GridTool::Block {
            return Some(child.center());
        }
        if child == parent {
            return None;
        }
        self.history.push(Frame::new(child, 5 - (status.depth + 1)));
        self.path.push(label);
        Some(if self.status().fine {
            self.history.last().unwrap().point()
        } else {
            child.center()
        })
    }

    /// Move within the current fine grid without changing its bounds.
    pub fn nudge(&mut self, dx: i32, dy: i32) -> Option<Point> {
        let status = self.status();
        if !status.fine {
            return None;
        }
        let frame = self.history.last_mut().unwrap();
        let col = (frame.selected as i32 % status.columns as i32 + dx)
            .clamp(0, status.columns as i32 - 1);
        let row =
            (frame.selected as i32 / status.columns as i32 + dy).clamp(0, status.rows as i32 - 1);
        let selected = row as usize * status.columns + col as usize;
        if selected == frame.selected {
            return None;
        }
        frame.selected = selected;
        Some(frame.point())
    }

    /// Tab descends into the selected cell, preserving the parent's selection.
    pub fn refine(&mut self) -> Option<Point> {
        let status = self.status();
        if !status.fine || status.depth == LAST_DEPTH {
            return None;
        }
        let selected_cell =
            status
                .bounds
                .cell_in(status.selected.unwrap(), status.columns, status.rows);
        let child = self.screen.centered_region(
            self.history.last().unwrap().point(),
            selected_cell.width,
            selected_cell.height,
        );
        if child == status.bounds {
            return None;
        }
        self.history.push(Frame::new(child, 5 - (status.depth + 1)));
        Some(self.history.last().unwrap().point())
    }

    pub fn back(&mut self) -> Option<Point> {
        if self.history.len() <= 1 {
            return None;
        }
        self.history.pop();
        let parent = self.history.last().unwrap();
        if parent.divisions == 4 {
            self.path.clear();
        }
        Some(if parent.divisions < 4 {
            parent.point()
        } else {
            self.origin
        })
    }
}

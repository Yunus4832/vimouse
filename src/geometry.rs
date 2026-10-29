//! Logical desktop geometry, shared by control, layout and persisted positions.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

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

//! Small, font-independent ARGB renderer. Coordinates are logical output pixels.
use crate::{
    engine::{Mode, Status},
    grid::Rect,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

struct Glyph {
    metrics: fontdue::Metrics,
    pixels: Vec<u8>,
}
fn glyph(ch: char, size: i32, bold: bool) -> Arc<Glyph> {
    static REGULAR: OnceLock<fontdue::Font> = OnceLock::new();
    static BOLD: OnceLock<fontdue::Font> = OnceLock::new();
    type Cache = Mutex<BTreeMap<(char, i32, bool), Arc<Glyph>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap();
    cache
        .entry((ch, size, bold))
        .or_insert_with(|| {
            let font = if bold {
                BOLD.get_or_init(|| {
                    fontdue::Font::from_bytes(
                        include_bytes!("../../assets/DejaVuSansMono-Bold.ttf") as &[u8],
                        fontdue::FontSettings::default(),
                    )
                    .expect("embedded font")
                })
            } else {
                REGULAR.get_or_init(|| {
                    fontdue::Font::from_bytes(
                        include_bytes!("../../assets/DejaVuSansMono.ttf") as &[u8],
                        fontdue::FontSettings::default(),
                    )
                    .expect("embedded font")
                })
            };
            let (metrics, pixels) = font.rasterize(ch, size as f32);
            Arc::new(Glyph { metrics, pixels })
        })
        .clone()
}

pub struct Canvas<'a> {
    pub bytes: &'a mut [u8],
    pub width: i32,
    pub height: i32,
    pub scale: i32,
}

impl Canvas<'_> {
    fn fill(&mut self, r: Rect, color: u32) {
        let pixel = color.to_ne_bytes(); // wl_shm ARGB8888 is native-endian, premultiplied.
        for y in (r.y * self.scale).max(0)..((r.y + r.height) * self.scale).min(self.height) {
            let start = (r.x * self.scale).clamp(0, self.width);
            let end = ((r.x + r.width) * self.scale).clamp(start, self.width);
            let row = &mut self.bytes
                [((y * self.width + start) as usize) * 4..((y * self.width + end) as usize) * 4];
            for p in row.chunks_exact_mut(4) {
                p.copy_from_slice(&pixel);
            }
        }
    }
    fn border(&mut self, r: Rect, color: u32) {
        for edge in [
            Rect { height: 1, ..r },
            Rect {
                y: r.y + r.height - 1,
                height: 1,
                ..r
            },
            Rect { width: 1, ..r },
            Rect {
                x: r.x + r.width - 1,
                width: 1,
                ..r
            },
        ] {
            self.fill(edge, color);
        }
    }
    fn text(
        &mut self,
        position: (i32, i32),
        text: &str,
        size: i32,
        color: u32,
        outline: bool,
        limit: i32,
    ) {
        let (mut x, y) = (position.0 * self.scale, position.1 * self.scale);
        let size = size * self.scale;
        for ch in text.chars() {
            let glyph = glyph(ch, size, outline);
            if x + glyph.metrics.advance_width.ceil() as i32 > limit * self.scale {
                break;
            }
            let gx = x + glyph.metrics.xmin;
            let gy = y + size - glyph.metrics.height as i32 - glyph.metrics.ymin;
            if outline {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        self.glyph(
                            gx + dx * self.scale,
                            gy + dy * self.scale,
                            &glyph,
                            0xff171300,
                        );
                    }
                }
            }
            self.glyph(gx, gy, &glyph, color);
            x += glyph.metrics.advance_width.round() as i32;
        }
    }
    fn glyph(&mut self, x: i32, y: i32, glyph: &Glyph, color: u32) {
        for (index, coverage) in glyph.pixels.iter().copied().enumerate() {
            if coverage == 0 {
                continue;
            }
            let px = x + (index % glyph.metrics.width) as i32;
            let py = y + (index / glyph.metrics.width) as i32;
            if px < 0 || py < 0 || px >= self.width || py >= self.height {
                continue;
            }
            let offset = ((py * self.width + px) * 4) as usize;
            let dst = u32::from_ne_bytes(self.bytes[offset..offset + 4].try_into().unwrap());
            let alpha = (color >> 24) * u32::from(coverage) / 255;
            let mut out = 0;
            for shift in [0, 8, 16, 24] {
                let source = ((color >> shift) & 255) * u32::from(coverage) / 255;
                out |= (source + ((dst >> shift) & 255) * (255 - alpha) / 255).min(255) << shift;
            }
            self.bytes[offset..offset + 4].copy_from_slice(&out.to_ne_bytes());
        }
    }
}

/// Output name selects exactly one surface, including monitors with negative origins.
pub fn render(
    canvas: &mut Canvas<'_>,
    status: Option<&Status>,
    output: &str,
    width: i32,
    height: i32,
) {
    canvas.bytes.fill(0);
    let Some(s) = status.filter(|s| s.display.name == output || s.display.name.is_empty()) else {
        return;
    };
    if s.mode == Mode::Grid {
        let Some(grid) = &s.grid else { return };
        // The engine and compositor use logical pixels; project only if fallback geometry differs.
        let project = |r: Rect| Rect {
            x: (r.x - s.display.x) * width / s.display.width,
            y: (r.y - s.display.y) * height / s.display.height,
            width: (r.width * width / s.display.width).max(1),
            height: (r.height * height / s.display.height).max(1),
        };
        for index in 0..grid.columns * grid.rows {
            let cell = project(grid.bounds.cell_in(index, grid.columns, grid.rows));
            canvas.border(cell, 0xa6a69100);
            if !grid.fine {
                canvas.text(
                    (cell.x + 4, cell.y + 3),
                    &char::from(b'a' + index as u8).to_string(),
                    13,
                    0xffffdf00,
                    true,
                    width,
                );
            }
        }
        if let Some(index) = grid.selected.filter(|_| grid.fine) {
            canvas.border(
                project(grid.bounds.cell_in(index, grid.columns, grid.rows)),
                0xffffdf00,
            );
        }
        canvas.border(project(grid.bounds), 0xd9d9be00);
    } else if s.mode != Mode::Insert {
        let w = 920.min(width - 24).max(1);
        let bar = Rect {
            x: (width - w) / 2,
            y: (height - 92).max(0),
            width: w,
            height: 56,
        };
        canvas.fill(bar, 0xed171b23);
        canvas.border(bar, 0xff3e4655);
        let accent = match s.mode {
            Mode::Visual => 0xffc4a7ff,
            Mode::Select => 0xffffc97e,
            Mode::Command => 0xff7dd3fc,
            _ => 0xffa4e5ba,
        };
        canvas.fill(
            Rect {
                x: bar.x,
                y: bar.y + 16,
                width: 3,
                height: 24,
            },
            accent,
        );
        let mode = format!("vi  {:?}", s.mode).to_uppercase();
        canvas.text(
            (bar.x + 18, bar.y + 21),
            &mode,
            12,
            accent,
            false,
            bar.x + w - 12,
        );
        let hint = if !s.pending.is_empty() {
            &s.pending
        } else if !s.message.is_empty() {
            &s.message
        } else {
            match s.mode {
                Mode::Visual => "hjkl drag  v release  Esc cancel",
                Mode::Select => "hjkl navigate  Enter confirm  q dismiss",
                _ => "hjkl move  Enter click  f jump  Tab refine",
            }
        };
        canvas.text(
            (bar.x + 140, bar.y + 22),
            hint,
            11,
            0xffa9b1c2,
            false,
            bar.x + w - 185,
        );
        let metrics = format!(
            "{}  {},{}",
            ["FAST", "MED", "FINE"][s.speed.min(2)],
            s.position.x,
            s.position.y
        );
        canvas.text(
            (bar.x + w - 175, bar.y + 22),
            &metrics,
            10,
            0xffa9b1c2,
            false,
            bar.x + w - 8,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, engine::Engine};
    use evdev::KeyCode as K;
    #[test]
    fn grid_is_transparent_inside_cells_and_clears_on_exit() {
        let mut engine = Engine::new(Config::default());
        engine.key(K::KEY_CAPSLOCK, 1);
        engine.key(K::KEY_CAPSLOCK, 0);
        engine.key(K::KEY_F, 1);
        engine.key(K::KEY_F, 0);
        let mut bytes = vec![0; 1920 * 1080 * 4];
        let mut c = Canvas {
            bytes: &mut bytes,
            width: 1920,
            height: 1080,
            scale: 1,
        };
        render(&mut c, Some(&engine.status()), "", 1920, 1080);
        assert!(c.bytes[..4].iter().any(|b| *b != 0));
        assert_eq!(
            &c.bytes[(100 * 1920 + 100) * 4..(100 * 1920 + 100) * 4 + 4],
            &[0; 4]
        );
        assert!(
            c.bytes[(3 * 1920 + 4) * 4..(17 * 1920) * 4]
                .iter()
                .any(|b| *b != 0)
        );
        render(&mut c, Some(&engine.status()), "other", 1920, 1080); // unnamed fallback applies to all
        render(&mut c, None, "", 1920, 1080);
        assert!(c.bytes.iter().all(|b| *b == 0));
    }
    #[test]
    fn output_selection_and_hidpi_use_logical_geometry() {
        let mut engine = Engine::new(Config::default());
        engine.key(K::KEY_CAPSLOCK, 1);
        engine.key(K::KEY_CAPSLOCK, 0);
        engine.key(K::KEY_F, 1);
        let mut s = engine.status();
        s.display.name = "DP-1".into();
        let mut bytes = vec![0; 3840 * 2160 * 4];
        let mut c = Canvas {
            bytes: &mut bytes,
            width: 3840,
            height: 2160,
            scale: 2,
        };
        render(&mut c, Some(&s), "eDP-1", 1920, 1080);
        assert!(c.bytes.iter().all(|b| *b == 0));
        render(&mut c, Some(&s), "DP-1", 1920, 1080);
        assert_eq!(
            &c.bytes[(10 * 3840 + 768) * 4..(10 * 3840 + 768) * 4 + 4],
            &0xa6a69100u32.to_ne_bytes()
        );
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::{config::Config, engine::Engine};
    use evdev::KeyCode as K;
    use std::io::Write;
    #[test]
    fn render_all_tools() {
        let mut engine = Engine::new(Config::default());
        let scenarios = [
            ("normal", vec![K::KEY_CAPSLOCK]),
            ("block", vec![K::KEY_F]),
            ("precision", vec![K::KEY_M, K::KEY_TAB]),
            ("fine3", vec![K::KEY_P]),
            ("fine2", vec![K::KEY_TAB]),
            ("insert", vec![K::KEY_ESC, K::KEY_I]),
        ];
        for (name, keys) in scenarios {
            for key in keys {
                engine.key(key, 1);
                engine.key(key, 0);
            }
            let s = engine.status();
            let mut bytes = vec![0; 1920 * 1080 * 4];
            render(
                &mut Canvas {
                    bytes: &mut bytes,
                    width: 1920,
                    height: 1080,
                    scale: 1,
                },
                Some(&s),
                "",
                1920,
                1080,
            );
            if name == "insert" {
                assert!(bytes.iter().all(|b| *b == 0));
            } else {
                assert!(bytes.iter().any(|b| *b != 0), "{name}");
            }
            if s.mode == Mode::Grid {
                assert_eq!(
                    &bytes[(1020 * 1920 + 960) * 4..(1020 * 1920 + 960) * 4 + 4],
                    &[0; 4]
                );
            }
            if let Some(directory) = std::env::var_os("VIMOUSE_RENDER_DIR") {
                let path = std::path::Path::new(&directory).join(format!("{name}.ppm"));
                let mut file = std::fs::File::create(path).unwrap();
                write!(file, "P6\n1920 1080\n255\n").unwrap();
                // Composite over a light checkerboard for visual inspection.
                let mut rgb = Vec::with_capacity(1920 * 1080 * 3);
                for (index, p) in bytes.chunks_exact(4).enumerate() {
                    let argb = u32::from_ne_bytes(p.try_into().unwrap());
                    let alpha = (argb >> 24) & 255;
                    let bg = if (index % 1920 / 48 + index / 1920 / 48) % 2 == 0 {
                        220
                    } else {
                        245
                    };
                    for shift in [16, 8, 0] {
                        rgb.push(
                            (((argb >> shift) & 255) + bg * (255 - alpha) / 255).min(255) as u8,
                        );
                    }
                }
                file.write_all(&rgb).unwrap();
            }
        }
    }
}

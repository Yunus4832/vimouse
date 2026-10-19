//! Logical output geometry and bounded, read-only compositor discovery.
use crate::{config::Screen, engine::Point, grid::Rect};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const ABS_MAX: i32 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Display {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Display {
    pub fn rect(&self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.width && p.y < self.y + self.height
    }
    pub fn clamp(&self, p: Point) -> Point {
        Point {
            x: p.x.clamp(self.x, self.x + self.width - 1),
            y: p.y.clamp(self.y, self.y + self.height - 1),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Layout {
    pub outputs: Vec<Display>,
    pub bounds: Rect,
}

impl Layout {
    pub fn single(screen: Screen) -> Self {
        Self::new(vec![Display {
            name: String::new(),
            x: 0,
            y: 0,
            width: screen.width,
            height: screen.height,
        }])
        .unwrap()
    }
    pub fn new(mut outputs: Vec<Display>) -> Result<Self, String> {
        if outputs.is_empty() || outputs.len() > 32 {
            return Err("expected 1..32 enabled displays".into());
        }
        let mut names = std::collections::BTreeSet::new();
        for output in &outputs {
            if output.width < 1
                || output.height < 1
                || output.width > 131072
                || output.height > 131072
                || output.x.abs_diff(0) > 1_000_000
                || output.y.abs_diff(0) > 1_000_000
                || !names.insert(&output.name)
            {
                return Err("invalid or duplicate display geometry".into());
            }
        }
        outputs.sort_by(|a, b| (a.x, a.y, &a.name).cmp(&(b.x, b.y, &b.name)));
        let x = outputs.iter().map(|o| o.x).min().unwrap();
        let y = outputs.iter().map(|o| o.y).min().unwrap();
        let right = outputs.iter().map(|o| o.x + o.width).max().unwrap();
        let bottom = outputs.iter().map(|o| o.y + o.height).max().unwrap();
        Ok(Self {
            outputs,
            bounds: Rect {
                x,
                y,
                width: right - x,
                height: bottom - y,
            },
        })
    }
    pub fn locate(&self, p: Point) -> (usize, Point) {
        if let Some(index) = self.outputs.iter().position(|o| o.contains(p)) {
            return (index, p);
        }
        self.outputs
            .iter()
            .enumerate()
            .map(|(index, o)| {
                let q = o.clamp(p);
                let dx = i64::from(p.x) - i64::from(q.x);
                let dy = i64::from(p.y) - i64::from(q.y);
                (dx * dx + dy * dy, index, q)
            })
            .min_by_key(|(distance, index, _)| (*distance, *index))
            .map(|(_, index, q)| (index, q))
            .unwrap()
    }
    /// libinput transforms normalized absolute motion over the whole workspace.
    pub fn absolute(&self, p: Point) -> Point {
        fn axis(value: i32, origin: i32, extent: i32) -> i32 {
            ((f64::from(value - origin) / f64::from(extent)) * f64::from(ABS_MAX))
                .round()
                .clamp(0.0, f64::from(ABS_MAX)) as i32
        }
        Point {
            x: axis(p.x, self.bounds.x, self.bounds.width),
            y: axis(p.y, self.bounds.y, self.bounds.height),
        }
    }
}

fn integer(value: &Value) -> Result<i32, String> {
    value
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| "missing display coordinate".into())
}

pub fn parse_kscreen(value: &Value) -> Result<Layout, String> {
    let outputs = value["outputs"]
        .as_array()
        .ok_or("missing KScreen outputs")?;
    let mut enabled = Vec::new();
    for output in outputs {
        if output["enabled"] != true || output["connected"] != true {
            continue;
        }
        let scale = output["scale"]
            .as_f64()
            .filter(|s| s.is_finite() && *s > 0.0)
            .ok_or("invalid display scale")?;
        // KScreen size already includes rotation, but is expressed in device pixels.
        let width = (f64::from(integer(&output["size"]["width"])?) / scale).round() as i32;
        let height = (f64::from(integer(&output["size"]["height"])?) / scale).round() as i32;
        enabled.push(Display {
            name: output["name"].as_str().ok_or("missing output name")?.into(),
            x: integer(&output["pos"]["x"])?,
            y: integer(&output["pos"]["y"])?,
            width,
            height,
        });
    }
    Layout::new(enabled)
}

pub fn parse_niri(value: &Value) -> Result<Layout, String> {
    let outputs = value.as_object().ok_or("missing niri outputs")?;
    let mut enabled = Vec::new();
    for (name, output) in outputs {
        let logical = &output["logical"];
        if logical.is_null() {
            continue;
        }
        enabled.push(Display {
            name: output["name"].as_str().unwrap_or(name).into(),
            x: integer(&logical["x"])?,
            y: integer(&logical["y"])?,
            width: integer(&logical["width"])?,
            height: integer(&logical["height"])?,
        });
    }
    Layout::new(enabled)
}

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        stdout.take(2_000_000).read_to_end(&mut data).map(|_| data)
    });
    let deadline = Instant::now() + Duration::from_millis(1000);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let data = reader
        .join()
        .map_err(|_| "display query reader failed")?
        .map_err(|e| e.to_string())?;
    if !status.is_some_and(|s| s.success()) {
        return Err(format!("{program} display query failed or timed out"));
    }
    String::from_utf8(data).map_err(|e| e.to_string())
}

fn niri(request: &str) -> Result<Value, String> {
    let socket = std::env::var_os("NIRI_SOCKET").ok_or("NIRI_SOCKET is not set")?;
    let mut stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(300)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(300)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(format!("\"{request}\"\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    std::io::BufRead::read_line(&mut reader, &mut line).map_err(|e| e.to_string())?;
    let reply: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    reply["Ok"]
        .get(request)
        .cloned()
        .ok_or_else(|| "niri returned an unexpected reply".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Kde,
    Niri,
    Wayland,
    Config,
}

pub fn detect(screen: Screen) -> (Layout, Source) {
    if std::env::var_os("NIRI_SOCKET").is_some() {
        if let Ok(layout) = niri("Outputs").and_then(|v| parse_niri(&v)) {
            return (layout, Source::Niri);
        }
    }
    if std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .any(|s| s.eq_ignore_ascii_case("kde"))
    {
        if let Ok(layout) = command_output("kscreen-doctor", &["-j"])
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .and_then(|v| parse_kscreen(&v))
        {
            return (layout, Source::Kde);
        }
    }
    if let Ok(layout) = crate::desktop::probe("native-displays")
        .and_then(|s| serde_json::from_str::<Vec<Display>>(&s).map_err(|e| e.to_string()))
        .and_then(Layout::new)
    {
        return (layout, Source::Wayland);
    }
    (Layout::single(screen), Source::Config)
}

pub fn active_output(source: Source) -> Option<String> {
    match source {
        Source::Kde => command_output(
            "qdbus6",
            &["org.kde.KWin", "/KWin", "org.kde.KWin.activeOutputName"],
        )
        .ok()
        .map(|s| s.trim().into())
        .filter(|s: &String| !s.is_empty()),
        Source::Niri => niri("FocusedOutput")
            .ok()
            .and_then(|v| v["name"].as_str().map(str::to_owned)),
        Source::Config | Source::Wayland => None,
    }
}

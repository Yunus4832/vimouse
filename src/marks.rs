//! Durable named marks, separate from configuration and transient jump history.
mod worker;
use crate::{
    displays::{Display, Layout},
    geometry::Point,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub use worker::{SaveResult, SaveWorker};

pub type Marks = BTreeMap<char, Mark>;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    pub display: String,
    /// Normalized logical position, inclusive 0..=1, not physical pixels.
    pub x: f64,
    pub y: f64,
}
impl Mark {
    pub fn capture(display: &Display, point: Point) -> Self {
        let point = display.clamp(point);
        Self {
            display: display.name.clone(),
            x: f64::from(point.x - display.x) / f64::from((display.width - 1).max(1)),
            y: f64::from(point.y - display.y) / f64::from((display.height - 1).max(1)),
        }
    }
    fn valid(&self) -> bool {
        self.display.len() <= 256
            && self.x.is_finite()
            && self.y.is_finite()
            && (0.0..=1.0).contains(&self.x)
            && (0.0..=1.0).contains(&self.y)
    }
    pub fn resolve(&self, layout: &Layout) -> Option<Point> {
        if !self.valid() {
            return None;
        }
        let display = layout.outputs.iter().find(|d| d.name == self.display)?;
        Some(Point {
            x: display.x + (self.x * f64::from(display.width - 1)).round() as i32,
            y: display.y + (self.y * f64::from(display.height - 1)).round() as i32,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    marks: Marks,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn validate(marks: &Marks) -> io::Result<()> {
    if marks.len() > 26
        || marks
            .iter()
            .any(|(key, mark)| !key.is_ascii_lowercase() || !mark.valid())
    {
        return Err(invalid(
            "marks must use a-z, display names up to 256 bytes and finite relative coordinates in 0..=1",
        ));
    }
    Ok(())
}
pub fn default_path() -> io::Result<PathBuf> {
    let base = if let Some(base) = env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        PathBuf::from(base)
    } else {
        PathBuf::from(env::var_os("HOME").ok_or_else(|| {
            io::Error::other("HOME or XDG_STATE_HOME is required for named marks")
        })?)
        .join(".local/state")
    };
    if !base.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mark state directory must be absolute",
        ));
    }
    Ok(base.join("vimouse/marks.json"))
}

pub struct Store {
    path: PathBuf,
    last_revision: u64,
}
impl Store {
    /// Invalid or unsupported files are left untouched; callers load before grabbing input.
    pub fn open(path: PathBuf) -> io::Result<(Self, Marks)> {
        let result=Self::load(&path).map_err(|e|io::Error::new(e.kind(),format!("Cannot load {}: {e}. The file has been left unchanged; repair it or move it aside to start with empty marks.",path.display())))?;
        Ok((
            Self {
                path,
                last_revision: 0,
            },
            result,
        ))
    }
    fn load(path: &Path) -> io::Result<Marks> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Marks::new()),
            Err(e) => return Err(e),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(invalid(
                "mark file must be a regular file owned by this user",
            ));
        }
        let mut data = Vec::new();
        file.take(65_537).read_to_end(&mut data)?;
        if data.len() > 65_536 {
            return Err(invalid("mark file exceeds 64 KiB"));
        }
        let document: Document =
            serde_json::from_slice(&data).map_err(|e| invalid(&e.to_string()))?;
        if document.version != 1 {
            return Err(invalid("unsupported mark file version"));
        }
        validate(&document.marks)?;
        Ok(document.marks)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Only mark edits write to disk. A failed edit is retried on the next edit,
    /// never on every pointer tick, so an I/O failure cannot stall movement repeatedly.
    pub fn sync(&mut self, revision: u64, marks: &Marks) -> io::Result<()> {
        if revision == self.last_revision {
            return Ok(());
        }
        self.last_revision = revision;
        self.save(marks)
    }
    fn save(&self, marks: &Marks) -> io::Result<()> {
        validate(marks)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| invalid("mark file has no parent"))?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "mark directory must be owned by this user with permissions 0700",
            ));
        }
        let mut data = serde_json::to_vec_pretty(&Document {
            version: 1,
            marks: marks.clone(),
        })
        .map_err(|e| invalid(&e.to_string()))?;
        data.push(b'\n');
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = parent.join(format!(
            ".marks-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(&data)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            File::open(parent)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

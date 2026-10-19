use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub toggle: String,
    pub speeds: [f64; 3],
    pub tick_hz: u32,
    pub idle_seconds: u64,
    pub scroll_interval_ms: u64,
    pub scroll_lines: i32,
    pub screen: Screen,
    /// Accepted from older configuration files; the two-stage grid has no knobs.
    #[serde(skip_serializing)]
    pub grid: GridConfig,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GridConfig {
    // Legacy fields accepted for compatibility, with no effect on positioning.
    pub adaptive: bool,
    pub five_min_size: i32,
    pub three_min_size: i32,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            adaptive: true,
            five_min_size: 150,
            three_min_size: 40,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Screen {
    pub width: i32,
    pub height: i32,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            toggle: "capslock".into(),
            speeds: [1600.0, 400.0, 100.0],
            tick_hz: 120,
            idle_seconds: 60,
            scroll_interval_ms: 80,
            scroll_lines: 1,
            screen: Screen::default(),
            grid: GridConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let config: Self = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.toggle_key().is_none() {
            return Err("toggle must be capslock, f8, f9, f10, f11 or f12".into());
        }
        if !(30..=240).contains(&self.tick_hz) {
            return Err("tick_hz must be 30..240".into());
        }
        if self
            .speeds
            .iter()
            .any(|n| !n.is_finite() || *n < 1.0 || *n > 20000.0)
            || self.speeds.windows(2).any(|w| w[0] < w[1])
        {
            return Err(
                "speeds must be three finite values in descending order, each 1..20000 pixels/s"
                    .into(),
            );
        }
        if !(100..=32768).contains(&self.screen.width)
            || !(100..=32768).contains(&self.screen.height)
        {
            return Err("screen width/height must be 100..32768 logical pixels".into());
        }
        if !(10..=2000).contains(&self.scroll_interval_ms) || !(1..=20).contains(&self.scroll_lines)
        {
            return Err("scroll_interval_ms must be 10..2000 and scroll_lines 1..20".into());
        }
        if self.idle_seconds > 86400 {
            return Err("idle_seconds must be 0..86400 (0 disables timeout)".into());
        }
        Ok(())
    }

    pub fn toggle_key(&self) -> Option<evdev::KeyCode> {
        use evdev::KeyCode;
        Some(match self.toggle.as_str() {
            "capslock" => KeyCode::KEY_CAPSLOCK,
            "f8" => KeyCode::KEY_F8,
            "f9" => KeyCode::KEY_F9,
            "f10" => KeyCode::KEY_F10,
            "f11" => KeyCode::KEY_F11,
            "f12" => KeyCode::KEY_F12,
            _ => return None,
        })
    }
}

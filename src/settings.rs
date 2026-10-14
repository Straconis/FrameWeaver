//! Choices remembered between runs, stored as JSON in the user's config folder.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::timing::Mode;
use crate::video::{Aspect, Fit, Format, Quality, Size};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub output_folder: PathBuf,
    pub format: Format,
    pub quality: Quality,
    pub mode: Mode,
    /// The total length, in separate hours, minutes and seconds boxes.
    pub length_hours: u32,
    pub length_minutes: u32,
    pub length_seconds: u32,
    /// Seconds each picture shows, in "time per picture" mode.
    pub per_picture: f64,
    pub fps: u32,
    pub size: Size,
    pub custom_size: (u32, u32),
    pub aspect: Aspect,
    pub fit: Fit,
    pub bar_color: [u8; 3],
    pub open_when_done: bool,
    /// A specific FFmpeg to use instead of the bundled one, if set.
    pub ffmpeg_path: Option<PathBuf>,
    /// Where pictures were last opened from, to start the file picker there.
    pub last_input_folder: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        let base = dirs::video_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        Settings {
            output_folder: base.join("FrameWeaver"),
            format: Format::Mp4H264,
            quality: Quality::Balanced,
            mode: Mode::TotalLength,
            length_hours: 0,
            length_minutes: 1,
            length_seconds: 0,
            per_picture: 0.5,
            fps: 30,
            size: Size::SameAsPictures,
            custom_size: (1920, 1080),
            aspect: Aspect::SameAsPictures,
            fit: Fit::Bars,
            bar_color: [0, 0, 0],
            open_when_done: false,
            ffmpeg_path: None,
            last_input_folder: None,
        }
    }
}

impl Settings {
    /// The total length from the three boxes.
    pub fn length(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            u64::from(self.length_hours) * 3600
                + u64::from(self.length_minutes) * 60
                + u64::from(self.length_seconds),
        )
    }

    fn path() -> Option<PathBuf> {
        Some(
            dirs::config_dir()?
                .join("FrameWeaver")
                .join("settings.json"),
        )
    }

    pub fn load() -> Settings {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"fps": 60, "length_minutes": 5}"#).unwrap();
        assert_eq!(s.fps, 60);
        assert_eq!(s.length(), std::time::Duration::from_secs(300));
        assert_eq!(s.format, Format::Mp4H264);
    }

    #[test]
    fn settings_round_trip() {
        let s = Settings {
            bar_color: [1, 2, 3],
            aspect: Aspect::Vertical9x16,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}

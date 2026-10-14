//! The frame math: how long each picture stays on screen so the video is exactly the chosen
//! length.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// The user picks the whole video's length; each picture's time is worked out.
    #[default]
    TotalLength,
    /// The user picks how long each picture shows; the length follows from that.
    PerPicture,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::TotalLength => "Total length",
            Mode::PerPicture => "Time per picture",
        }
    }
}

/// How many frames each picture gets, in order. Pictures with 0 frames are skipped.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub frames: Vec<u64>,
    pub fps: u32,
}

impl Plan {
    pub fn total_frames(&self) -> u64 {
        self.frames.iter().sum()
    }

    pub fn shown(&self) -> usize {
        self.frames.iter().filter(|&&f| f > 0).count()
    }

    pub fn skipped(&self) -> usize {
        self.frames.len() - self.shown()
    }

    pub fn length(&self) -> Duration {
        Duration::from_secs_f64(self.total_frames() as f64 / f64::from(self.fps))
    }

    /// A plain description of the math, shown before the video is made.
    pub fn summary(&self) -> String {
        let pictures = self.frames.len();
        if pictures == 0 {
            return "No pictures".into();
        }
        let total = self.total_frames();
        let per = total as f64 / self.shown().max(1) as f64;
        let mut text = format!(
            "{} pictures → {} at {} fps = {} frames. Each picture shows for about {:.1} frames ({:.2} s).",
            group(pictures as u64),
            format_hms(self.length()),
            self.fps,
            group(total),
            per,
            per / f64::from(self.fps),
        );
        if self.skipped() > 0 {
            text.push_str(&format!(
                " There are more pictures than frames, so {} pictures are skipped evenly.",
                group(self.skipped() as u64)
            ));
        }
        text
    }
}

/// Frames for a video of `length`, at least one.
pub fn total_frames(length: Duration, fps: u32) -> u64 {
    ((length.as_secs_f64() * f64::from(fps)).round() as u64).max(1)
}

/// Spreads `frames` over `pictures` as evenly as possible: the counts add up exactly, and no
/// picture gets more than one frame more than another. With more pictures than frames, each
/// frame shows a different picture, picked evenly, and the rest get 0.
pub fn spread(pictures: usize, frames: u64) -> Vec<u64> {
    // Rounding each boundary to the nearest frame (rather than down) keeps any skipping
    // symmetrical, so it doesn't favour the start or the end of the recording.
    let n = pictures as u128;
    let f = u128::from(frames);
    let boundary = |i: u128| (2 * i * f + n) / (2 * n);
    (0..n)
        .map(|i| (boundary(i + 1) - boundary(i)) as u64)
        .collect()
}

pub fn plan_total(pictures: usize, length: Duration, fps: u32) -> Plan {
    Plan {
        frames: spread(pictures, total_frames(length, fps)),
        fps,
    }
}

pub fn plan_per_picture(pictures: usize, per_picture: Duration, fps: u32) -> Plan {
    let each = total_frames(per_picture, fps);
    Plan {
        frames: vec![each; pictures],
        fps,
    }
}

/// Reads "HH:MM:SS", "MM:SS" or "SS", each part optionally with decimals on the seconds,
/// for example "1:30" or "0:00:02.5". Minutes and seconds must be under 60 when a larger
/// part is given.
pub fn parse_hms(text: &str) -> Option<Duration> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.trim().is_empty()) {
        return None;
    }
    let seconds: f64 = parts
        .last()?
        .trim()
        .parse()
        .ok()
        .filter(|s: &f64| s.is_finite() && *s >= 0.0)?;
    let whole = |s: &str| s.trim().parse::<u64>().ok();
    let (hours, minutes) = match parts.len() {
        3 => (whole(parts[0])?, whole(parts[1])?),
        2 => (0, whole(parts[0])?),
        _ => (0, 0),
    };
    if parts.len() > 1 && seconds >= 60.0 || parts.len() == 3 && minutes >= 60 {
        return None;
    }
    let total = hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds;
    (total > 0.0 && total < 100.0 * 3600.0).then(|| Duration::from_secs_f64(total))
}

/// Writes a length as "HH:MM:SS", adding hundredths only when they matter.
pub fn format_hms(d: Duration) -> String {
    let hundredths = (d.as_secs_f64() * 100.0).round() as u64;
    let (secs, frac) = (hundredths / 100, hundredths % 100);
    let base = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    if frac == 0 {
        base
    } else {
        format!("{base}.{frac:02}")
    }
}

/// 12345 → "12,345".
fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads_frames_evenly() {
        assert_eq!(spread(4, 10), vec![3, 2, 3, 2]);
        assert_eq!(spread(3, 9), vec![3, 3, 3]);
        assert_eq!(spread(5, 2), vec![0, 1, 0, 1, 0]);
        assert!(spread(0, 10).is_empty());
    }

    #[test]
    fn two_minutes_at_30_fps_is_exactly_3600_frames() {
        let plan = plan_total(487, parse_hms("00:02:00").unwrap(), 30);
        assert_eq!(plan.total_frames(), 3600);
        assert_eq!(plan.skipped(), 0);
        assert_eq!(plan.length(), Duration::from_secs(120));
        assert!(
            plan.summary()
                .starts_with("487 pictures → 00:02:00 at 30 fps = 3,600 frames."),
            "{}",
            plan.summary()
        );
    }

    #[test]
    fn more_pictures_than_frames_skips_evenly_and_says_so() {
        let plan = plan_total(1000, Duration::from_secs(10), 30);
        assert_eq!(plan.total_frames(), 300);
        assert_eq!(plan.shown(), 300);
        assert_eq!(plan.skipped(), 700);
        assert!(plan.summary().contains("700 pictures are skipped evenly"));
    }

    #[test]
    fn per_picture_mode_gives_every_picture_the_same_time() {
        let plan = plan_per_picture(10, Duration::from_millis(500), 30);
        assert_eq!(plan.frames, vec![15; 10]);
        assert_eq!(plan.length(), Duration::from_secs(5));
    }

    #[test]
    fn lengths_are_read_in_friendly_forms() {
        assert_eq!(parse_hms("00:02:00"), Some(Duration::from_secs(120)));
        assert_eq!(parse_hms("1:30"), Some(Duration::from_secs(90)));
        assert_eq!(parse_hms("45"), Some(Duration::from_secs(45)));
        assert_eq!(parse_hms(" 0:00:02.5 "), Some(Duration::from_millis(2500)));
        assert_eq!(parse_hms("90"), Some(Duration::from_secs(90)));
        for bad in [
            "",
            "1:60",
            "1:61:00",
            "a:b",
            "0",
            "1::2",
            "-5",
            "1:2:3:4",
            "100:00:00",
        ] {
            assert_eq!(parse_hms(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn lengths_are_written_as_hh_mm_ss() {
        assert_eq!(format_hms(Duration::from_secs(3725)), "01:02:05");
        assert_eq!(format_hms(Duration::from_millis(2500)), "00:00:02.50");
    }

    mod properties {
        use super::super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(3000))]

            /// The heart of FrameWeaver: frames always add up exactly, no picture gets more than
            /// one frame more than another, and skipping only happens when it must.
            #[test]
            fn spreading_is_exact_and_even(pictures in 1usize..5000, frames in 1u64..500_000) {
                let counts = spread(pictures, frames);
                prop_assert_eq!(counts.len(), pictures);
                prop_assert_eq!(counts.iter().sum::<u64>(), frames);
                let (lo, hi) = (*counts.iter().min().unwrap(), *counts.iter().max().unwrap());
                prop_assert!(hi - lo <= 1);
                let skipped = counts.iter().filter(|&&c| c == 0).count();
                prop_assert_eq!(skipped, pictures.saturating_sub(frames as usize));
            }

            /// Any length written out reads back the same.
            #[test]
            fn lengths_round_trip(centis in 1u64..(99 * 3600 * 100)) {
                let d = Duration::from_millis(centis * 10);
                let back = parse_hms(&format_hms(d)).unwrap();
                prop_assert!((back.as_secs_f64() - d.as_secs_f64()).abs() < 0.006);
            }
        }
    }
}

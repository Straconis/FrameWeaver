//! Video settings, and how they turn into instructions for FFmpeg.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::timing::Plan;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[default]
    Mp4H264,
    Mp4H265,
    WebmVp9,
    Gif,
}

impl Format {
    pub const ALL: [Format; 4] = [
        Format::Mp4H264,
        Format::Mp4H265,
        Format::WebmVp9,
        Format::Gif,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Format::Mp4H264 => "MP4 (H.264): plays everywhere",
            Format::Mp4H265 => "MP4 (H.265): about half the size",
            Format::WebmVp9 => "WebM (VP9): open format",
            Format::Gif => "Animated GIF: short clips",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Mp4H264 | Format::Mp4H265 => "mp4",
            Format::WebmVp9 => "webm",
            Format::Gif => "gif",
        }
    }

    /// The FFmpeg encoder this format needs, to check it's available.
    pub fn encoder(self) -> &'static str {
        match self {
            Format::Mp4H264 => "libx264",
            Format::Mp4H265 => "libx265",
            Format::WebmVp9 => "libvpx-vp9",
            Format::Gif => "gif",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    High,
    #[default]
    Balanced,
    Small,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::High, Quality::Balanced, Quality::Small];

    pub fn label(self) -> &'static str {
        match self {
            Quality::High => "High quality",
            Quality::Balanced => "Balanced",
            Quality::Small => "Small file",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Size {
    #[default]
    SameAsPictures,
    P720,
    P1080,
    P1440,
    P2160,
    Custom,
}

impl Size {
    pub const ALL: [Size; 6] = [
        Size::SameAsPictures,
        Size::P720,
        Size::P1080,
        Size::P1440,
        Size::P2160,
        Size::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Size::SameAsPictures => "Same as pictures",
            Size::P720 => "720p",
            Size::P1080 => "1080p (Full HD)",
            Size::P1440 => "1440p",
            Size::P2160 => "2160p (4K)",
            Size::Custom => "Custom",
        }
    }

    /// The short side in pixels for the "p" sizes (1080p is 1920×1080, or 1080×1920 upright).
    fn short_side(self) -> Option<u32> {
        match self {
            Size::P720 => Some(720),
            Size::P1080 => Some(1080),
            Size::P1440 => Some(1440),
            Size::P2160 => Some(2160),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aspect {
    #[default]
    SameAsPictures,
    Wide16x9,
    Classic4x3,
    Square,
    Vertical9x16,
    Ultrawide21x9,
}

impl Aspect {
    pub const ALL: [Aspect; 6] = [
        Aspect::SameAsPictures,
        Aspect::Wide16x9,
        Aspect::Classic4x3,
        Aspect::Square,
        Aspect::Vertical9x16,
        Aspect::Ultrawide21x9,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Aspect::SameAsPictures => "Same as pictures",
            Aspect::Wide16x9 => "16:9 widescreen",
            Aspect::Classic4x3 => "4:3 classic",
            Aspect::Square => "1:1 square",
            Aspect::Vertical9x16 => "9:16 vertical (phones)",
            Aspect::Ultrawide21x9 => "21:9 ultrawide",
        }
    }

    fn ratio(self) -> Option<f64> {
        match self {
            Aspect::SameAsPictures => None,
            Aspect::Wide16x9 => Some(16.0 / 9.0),
            Aspect::Classic4x3 => Some(4.0 / 3.0),
            Aspect::Square => Some(1.0),
            Aspect::Vertical9x16 => Some(9.0 / 16.0),
            Aspect::Ultrawide21x9 => Some(21.0 / 9.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fit {
    /// Show all of each picture, with bars filling any space.
    #[default]
    Bars,
    /// Fill the frame, cropping the picture's edges.
    Crop,
    /// Fill the frame by stretching the picture.
    Stretch,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Bars, Fit::Crop, Fit::Stretch];

    pub fn label(self) -> &'static str {
        match self {
            Fit::Bars => "Fit, with bars",
            Fit::Crop => "Fill, cropping the edges",
            Fit::Stretch => "Stretch to fill",
        }
    }
}

pub const FRAME_RATES: [u32; 5] = [24, 25, 30, 50, 60];
/// Encoders struggle above this; it's beyond 8K.
const MAX_SIDE: u32 = 8192;

/// The video's width and height. Always even, because video encoders need that.
pub fn output_size(
    pictures: (u32, u32),
    size: Size,
    custom: (u32, u32),
    aspect: Aspect,
) -> (u32, u32) {
    let even = |v: f64| (((v.round() as u32).clamp(2, MAX_SIDE)) / 2) * 2;
    let (pw, ph) = (pictures.0.max(2), pictures.1.max(2));
    if size == Size::Custom {
        return (even(f64::from(custom.0)), even(f64::from(custom.1)));
    }
    let ratio = aspect.ratio().unwrap_or(f64::from(pw) / f64::from(ph));
    match (size.short_side(), aspect) {
        (None, Aspect::SameAsPictures) => (even(f64::from(pw)), even(f64::from(ph))),
        (short, _) => {
            // Round the short side first, then work out the long side from it, so the shape
            // stays as close as possible to the chosen one even for tiny pictures.
            let short = even(f64::from(short.unwrap_or(pw.min(ph))));
            let long = |ratio: f64| even(f64::from(short) * ratio);
            if ratio >= 1.0 {
                (long(ratio), short)
            } else {
                (short, long(1.0 / ratio))
            }
        }
    }
}

/// Everything needed to make one video.
#[derive(Clone, Debug)]
pub struct Job {
    pub plan: Plan,
    pub format: Format,
    pub quality: Quality,
    pub size: (u32, u32),
    pub fit: Fit,
    pub bar_color: [u8; 3],
}

/// The list FFmpeg reads the pictures from, with exact display times. Times are worked out from
/// running frame totals, so rounding never builds up over thousands of pictures.
pub fn concat_list(pictures: &[PathBuf], plan: &Plan) -> String {
    let mut out = String::from("ffconcat version 1.0\n");
    let fps = u128::from(plan.fps);
    let micros = |frames: u128| (frames * 1_000_000 + fps / 2) / fps;
    let mut done: u128 = 0;
    let mut last = None;
    for (path, &frames) in pictures.iter().zip(&plan.frames) {
        if frames == 0 {
            continue;
        }
        let start = micros(done);
        done += u128::from(frames);
        let length = micros(done) - start;
        let _ = writeln!(
            out,
            "file {}\nduration {}.{:06}",
            quote(path),
            length / 1_000_000,
            length % 1_000_000
        );
        last = Some(path);
    }
    // FFmpeg ignores the last picture's duration unless the picture is listed once more.
    if let Some(path) = last {
        let _ = writeln!(out, "file {}", quote(path));
    }
    out
}

/// Quotes a path for FFmpeg's list format: inside single quotes, with any single quote written
/// as '\''.
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// The picture-shaping part: size, fit, frame rate, colour format.
pub fn video_filter(job: &Job) -> String {
    let (w, h) = job.size;
    let [r, g, b] = job.bar_color;
    let shape = match job.fit {
        Fit::Bars => format!(
            "scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=0x{r:02X}{g:02X}{b:02X}"
        ),
        Fit::Crop => format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}"),
        Fit::Stretch => format!("scale={w}:{h}"),
    };
    let fps = job.plan.fps;
    match job.format {
        Format::Gif => format!(
            "{shape},setsar=1,fps={fps},split[a][b];[a]palettegen=stats_mode=full[p];[b][p]paletteuse=dither=bayer:bayer_scale=3"
        ),
        _ => format!("{shape},setsar=1,fps={fps},format=yuv420p"),
    }
}

fn encoder_args(job: &Job) -> Vec<String> {
    let q = |high: u32, balanced: u32, small: u32| {
        match job.quality {
            Quality::High => high,
            Quality::Balanced => balanced,
            Quality::Small => small,
        }
        .to_string()
    };
    let args: Vec<String> = match job.format {
        Format::Mp4H264 => vec![
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            &q(18, 23, 28),
            "-movflags",
            "+faststart",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        Format::Mp4H265 => vec![
            "-c:v",
            "libx265",
            "-preset",
            "medium",
            "-crf",
            &q(22, 26, 30),
            "-tag:v",
            "hvc1",
            "-movflags",
            "+faststart",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        Format::WebmVp9 => vec![
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            &q(24, 32, 40),
            "-row-mt",
            "1",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        Format::Gif => vec!["-loop".to_string(), "0".to_string()],
    };
    args
}

/// The full FFmpeg command line (without the program name).
pub fn ffmpeg_args(job: &Job, list: &Path, output: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "-hide_banner",
        "-nostdin",
        "-y",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
        "-f",
        "concat",
        "-safe",
        "0",
        // Pictures can change size part-way (a window resized during a SnapLapse recording).
        // Normally FFmpeg rebuilds its processing chain then and loses the frames of the picture
        // before the change; the scale step handles new sizes itself, so keep the chain.
        "-reinit_filter",
        "0",
        "-i",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    args.push(list.to_string_lossy().into_owned());
    args.push("-vf".into());
    args.push(video_filter(job));
    args.extend(encoder_args(job));
    // Stop at exactly the planned number of frames, so the length is exact.
    args.push("-frames:v".into());
    args.push(job.plan.total_frames().to_string());
    args.push("-an".into());
    args.push(output.to_string_lossy().into_owned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_chosen_shape_and_are_even() {
        let pictures = (1567, 883);
        assert_eq!(
            output_size(
                pictures,
                Size::SameAsPictures,
                (0, 0),
                Aspect::SameAsPictures
            ),
            (1566, 882)
        );
        assert_eq!(
            output_size(pictures, Size::P1080, (0, 0), Aspect::Wide16x9),
            (1920, 1080)
        );
        assert_eq!(
            output_size(pictures, Size::P1080, (0, 0), Aspect::Vertical9x16),
            (1080, 1920)
        );
        assert_eq!(
            output_size(pictures, Size::P720, (0, 0), Aspect::Square),
            (720, 720)
        );
        assert_eq!(
            output_size(pictures, Size::P2160, (0, 0), Aspect::Ultrawide21x9),
            (5040, 2160)
        );
        assert_eq!(
            output_size(pictures, Size::Custom, (1001, 777), Aspect::Wide16x9),
            (1000, 776)
        );
        // "Same as pictures" size with a new shape keeps the pictures' short side.
        assert_eq!(
            output_size(pictures, Size::SameAsPictures, (0, 0), Aspect::Classic4x3),
            (1176, 882)
        );
    }

    #[test]
    fn the_list_gives_exact_times_and_repeats_the_last_picture() {
        let pictures: Vec<PathBuf> = ["/p/1.png", "/p/2.png", "/p/3.png"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let plan = Plan {
            frames: vec![1, 0, 2],
            fps: 30,
        };
        let list = concat_list(&pictures, &plan);
        assert_eq!(
            list,
            "ffconcat version 1.0\nfile '/p/1.png'\nduration 0.033333\nfile '/p/3.png'\nduration 0.066667\nfile '/p/3.png'\n"
        );
    }

    #[test]
    fn awkward_file_names_are_quoted_safely() {
        assert_eq!(
            quote(Path::new("/a/it's here.png")),
            r"'/a/it'\''s here.png'"
        );
        assert_eq!(
            quote(Path::new(r"C:\Users\x\1 - a.png")),
            r"'C:\Users\x\1 - a.png'"
        );
    }

    #[test]
    fn filters_match_the_fit_and_format() {
        let mut job = Job {
            plan: Plan {
                frames: vec![1],
                fps: 25,
            },
            format: Format::Mp4H264,
            quality: Quality::Balanced,
            size: (1280, 720),
            fit: Fit::Bars,
            bar_color: [16, 32, 255],
        };
        assert_eq!(
            video_filter(&job),
            "scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2:color=0x1020FF,setsar=1,fps=25,format=yuv420p"
        );
        job.fit = Fit::Crop;
        assert!(video_filter(&job)
            .starts_with("scale=1280:720:force_original_aspect_ratio=increase,crop=1280:720"));
        job.format = Format::Gif;
        assert!(video_filter(&job).contains("palettegen"));
        let args = ffmpeg_args(&job, Path::new("list.txt"), Path::new("out.gif"));
        assert_eq!(args.last().unwrap(), "out.gif");
        assert!(args.windows(2).any(|w| w[0] == "-frames:v" && w[1] == "1"));
    }

    mod properties {
        use super::super::*;
        use crate::timing::spread;
        use proptest::prelude::*;

        fn sizes() -> impl Strategy<Value = Size> {
            prop::sample::select(Size::ALL.to_vec())
        }
        fn aspects() -> impl Strategy<Value = Aspect> {
            prop::sample::select(Aspect::ALL.to_vec())
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(3000))]

            /// Whatever the pictures and settings, the video size is even, at least 2, at most
            /// 8192, and has the chosen shape (within rounding).
            #[test]
            fn output_sizes_are_always_encodable(
                pw in 1u32..10_000, ph in 1u32..10_000, cw in 0u32..20_000, ch in 0u32..20_000,
                size in sizes(), aspect in aspects(),
            ) {
                let (w, h) = output_size((pw, ph), size, (cw, ch), aspect);
                prop_assert!(w % 2 == 0 && h % 2 == 0);
                prop_assert!((2..=MAX_SIDE).contains(&w) && (2..=MAX_SIDE).contains(&h));
                if let (Some(r), true) = (aspect.ratio(), size != Size::Custom) {
                    let fits = w < MAX_SIDE && h < MAX_SIDE && w > 2 && h > 2;
                    if fits {
                        // The long side is rounded to an even number, so it can be up to 2 pixels off.
                        let short = w.min(h) as f64;
                        let actual = (w.max(h) as f64) / short;
                        let wanted = if r >= 1.0 { r } else { 1.0 / r };
                        prop_assert!((actual - wanted).abs() <= 2.0 / short + 1e-9, "{w}x{h} vs {r}");
                    }
                }
            }

            /// The list's times always add up to exactly the planned length, to the microsecond,
            /// however many pictures there are.
            #[test]
            fn list_times_add_up_exactly(pictures in 1usize..3000, frames in 1u64..200_000, fps in prop::sample::select(FRAME_RATES.to_vec())) {
                let plan = Plan { frames: spread(pictures, frames), fps };
                let paths: Vec<PathBuf> = (0..pictures).map(|i| PathBuf::from(format!("/p/{i}.png"))).collect();
                let list = concat_list(&paths, &plan);
                let total_micros: u128 = list
                    .lines()
                    .filter_map(|l| l.strip_prefix("duration "))
                    .map(|d| {
                        let (s, us) = d.split_once('.').unwrap();
                        s.parse::<u128>().unwrap() * 1_000_000 + us.parse::<u128>().unwrap()
                    })
                    .sum();
                let expected = (u128::from(frames) * 1_000_000 + u128::from(fps) / 2) / u128::from(fps);
                prop_assert_eq!(total_micros, expected);
            }
        }
    }
}

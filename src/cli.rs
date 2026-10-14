//! Command-line mode, for scripting and testing. Running with no arguments opens the app instead.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::ffmpeg::{Encoding, Ffmpeg, State};
use crate::timing;
use crate::video::{self, Aspect, Fit, Format, Job, Quality, Size};

const USAGE: &str = "\
FrameWeaver: turn pictures into a time-lapse video of any length.

Usage:
  frameweaver                     Open the app
  frameweaver --input PATH --output FILE [options]
      PATH is a folder of pictures, a picture, or a ZIP (split ZIPs: give any part).
      FILE's extension picks the format: .mp4, .webm or .gif.

Options:
  --length HH:MM:SS     Total length of the video (default 00:01:00)
  --per-picture SECONDS Show each picture this long instead
  --fps N               24, 25, 30 (default), 50 or 60
  --size S              same (default), 720p, 1080p, 1440p, 4k, or WIDTHxHEIGHT
  --aspect A            same (default), 16:9, 4:3, 1:1, 9:16 or 21:9
  --fit F               bars (default), crop or stretch
  --quality Q           high, balanced (default) or small
  --hevc                For .mp4: use H.265 for a smaller file
  --help | --version
";

pub fn run(args: &[String]) -> i32 {
    let result = match args[0].as_str() {
        "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        "--version" | "-V" => {
            println!("FrameWeaver {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => make_video(args),
    };
    match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

fn make_video(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut output = None;
    let mut length = Duration::from_secs(60);
    let mut per_picture = None;
    let mut fps = 30;
    let mut size = (Size::SameAsPictures, (0, 0));
    let mut aspect = Aspect::SameAsPictures;
    let mut fit = Fit::Bars;
    let mut quality = Quality::Balanced;
    let mut hevc = false;

    let mut rest = args.iter();
    while let Some(option) = rest.next() {
        if option == "--hevc" {
            hevc = true;
            continue;
        }
        let value = rest
            .next()
            .ok_or_else(|| format!("{option} needs a value\n\n{USAGE}"))?;
        match option.as_str() {
            "--input" => input = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--length" => {
                length = timing::parse_hms(value).ok_or("--length must look like 00:02:00")?
            }
            "--per-picture" => {
                let secs: f64 = value
                    .parse()
                    .map_err(|_| "--per-picture must be a number of seconds")?;
                per_picture = Some(Duration::from_secs_f64(secs.max(0.001)));
            }
            "--fps" => {
                fps = value
                    .parse()
                    .ok()
                    .filter(|f| video::FRAME_RATES.contains(f))
                    .ok_or("--fps must be 24, 25, 30, 50 or 60")?
            }
            "--size" => {
                size = parse_size(value)
                    .ok_or("--size must be same, 720p, 1080p, 1440p, 4k or WIDTHxHEIGHT")?
            }
            "--aspect" => {
                aspect = match value.as_str() {
                    "same" => Aspect::SameAsPictures,
                    "16:9" => Aspect::Wide16x9,
                    "4:3" => Aspect::Classic4x3,
                    "1:1" => Aspect::Square,
                    "9:16" => Aspect::Vertical9x16,
                    "21:9" => Aspect::Ultrawide21x9,
                    _ => return Err("--aspect must be same, 16:9, 4:3, 1:1, 9:16 or 21:9".into()),
                }
            }
            "--fit" => {
                fit = match value.as_str() {
                    "bars" => Fit::Bars,
                    "crop" => Fit::Crop,
                    "stretch" => Fit::Stretch,
                    _ => return Err("--fit must be bars, crop or stretch".into()),
                }
            }
            "--quality" => {
                quality = match value.as_str() {
                    "high" => Quality::High,
                    "balanced" => Quality::Balanced,
                    "small" => Quality::Small,
                    _ => return Err("--quality must be high, balanced or small".into()),
                }
            }
            other => return Err(format!("Unknown option {other:?}\n\n{USAGE}")),
        }
    }
    let input = input.ok_or_else(|| format!("--input is needed\n\n{USAGE}"))?;
    let output = output.ok_or_else(|| format!("--output is needed\n\n{USAGE}"))?;
    let format = match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp4") if hevc => Format::Mp4H265,
        Some("mp4") => Format::Mp4H264,
        Some("webm") => Format::WebmVp9,
        Some("gif") => Format::Gif,
        _ => return Err("The output file must end in .mp4, .webm or .gif".into()),
    };

    let ffmpeg = Ffmpeg::find(None)?;
    if !ffmpeg.supports(format) {
        return Err(format!("This FFmpeg can't make {}", format.label()));
    }
    let pictures = Arc::new(crate::input::load(&input)?);
    let plan = match per_picture {
        Some(each) => timing::plan_per_picture(pictures.files.len(), each, fps),
        None => timing::plan_total(pictures.files.len(), length, fps),
    };
    println!("{}", plan.summary());
    let source = pictures.first_size.unwrap_or((1920, 1080));
    let job = Job {
        plan,
        format,
        quality,
        size: video::output_size(source, size.0, size.1, aspect),
        fit,
        bar_color: [0, 0, 0],
    };
    println!(
        "Making a {}×{} {} with {}",
        job.size.0,
        job.size.1,
        format.label(),
        ffmpeg.version
    );
    match Encoding::start(&ffmpeg, pictures, job, output).wait() {
        State::Done(path) => {
            println!("Saved {}", path.display());
            Ok(())
        }
        State::Failed(message) => Err(message),
        State::Cancelled | State::Running => Err("The video wasn't finished".into()),
    }
}

fn parse_size(text: &str) -> Option<(Size, (u32, u32))> {
    let size = match text.to_ascii_lowercase().as_str() {
        "same" => Size::SameAsPictures,
        "720p" => Size::P720,
        "1080p" => Size::P1080,
        "1440p" => Size::P1440,
        "4k" | "2160p" => Size::P2160,
        custom => {
            let (w, h) = custom.split_once('x')?;
            return Some((
                Size::Custom,
                (w.trim().parse().ok()?, h.trim().parse().ok()?),
            ));
        }
    };
    Some((size, (0, 0)))
}

/// On Windows the app has no console of its own, so print to the one it was started from.
pub fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("1080p"), Some((Size::P1080, (0, 0))));
        assert_eq!(parse_size("4K"), Some((Size::P2160, (0, 0))));
        assert_eq!(parse_size("1280x720"), Some((Size::Custom, (1280, 720))));
        assert_eq!(parse_size("big"), None);
    }

    #[test]
    fn bad_options_are_explained() {
        assert_eq!(run(&["--input".into()]), 1);
        assert_eq!(
            run(&[
                "--input".into(),
                "x".into(),
                "--output".into(),
                "x.avi".into()
            ]),
            1
        );
    }
}

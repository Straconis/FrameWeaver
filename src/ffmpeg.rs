//! Finding FFmpeg and running it to make the video.
//!
//! FrameWeaver ships with its own copy of FFmpeg next to the program, so nothing else needs
//! installing. If that copy is missing, it falls back to one installed on the system.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::input::{Pictures, TempFolder};
use crate::video::{self, Format, Job};

#[cfg(windows)]
const PROGRAM: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const PROGRAM: &str = "ffmpeg";

/// A working FFmpeg and the formats it can make.
#[derive(Clone, Debug)]
pub struct Ffmpeg {
    pub path: PathBuf,
    pub version: String,
    encoders: Vec<String>,
}

impl Ffmpeg {
    /// Looks for FFmpeg: a path chosen in the settings, then next to FrameWeaver, then on the
    /// system's PATH.
    pub fn find(chosen: Option<&Path>) -> Result<Ffmpeg, String> {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join(PROGRAM)));
        let on_path = std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths)
                    .map(|d| d.join(PROGRAM))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let candidates = chosen
            .map(Path::to_path_buf)
            .into_iter()
            .chain(beside)
            .chain(on_path);
        for candidate in candidates.filter(|p| p.is_file()) {
            if let Some(found) = Self::check(&candidate) {
                return Ok(found);
            }
        }
        Err("FFmpeg wasn't found. It normally comes with FrameWeaver; reinstalling should fix this. \
             On Linux you can also install it with: sudo apt install ffmpeg"
            .into())
    }

    fn check(path: &Path) -> Option<Ffmpeg> {
        let version = run_quietly(path, &["-hide_banner", "-version"])?;
        let version = version.lines().next()?.trim().to_string();
        let encoders = run_quietly(path, &["-hide_banner", "-encoders"])?
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
            .collect();
        Some(Ffmpeg {
            path: path.to_path_buf(),
            version,
            encoders,
        })
    }

    pub fn supports(&self, format: Format) -> bool {
        self.encoders.iter().any(|e| e == format.encoder())
    }
}

fn run_quietly(program: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    hide_window(&mut command);
    let out = command.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// No console window flashing up on Windows.
fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Lets everything else on the computer go first while encoding (Linux; Windows does this
/// through the priority flag in `hide_window`).
fn lower_priority(command: &mut Command) {
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, 10);
            Ok(())
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = command;
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Running,
    Done(PathBuf),
    Cancelled,
    Failed(String),
}

/// Progress the app reads while a video is being made.
pub struct Progress {
    pub frames_done: AtomicU64,
    pub total_frames: u64,
    started: Instant,
    state: Mutex<State>,
}

impl Progress {
    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    pub fn fraction(&self) -> f32 {
        (self.frames_done.load(Ordering::Relaxed) as f32 / self.total_frames.max(1) as f32).min(1.0)
    }

    /// A rough estimate of the time left, once there's enough progress to judge.
    pub fn time_left(&self) -> Option<Duration> {
        let done = self.frames_done.load(Ordering::Relaxed);
        let elapsed = self.started.elapsed();
        if done < 10 || elapsed < Duration::from_secs(2) {
            return None;
        }
        let per_frame = elapsed.as_secs_f64() / done as f64;
        Some(Duration::from_secs_f64(
            per_frame * self.total_frames.saturating_sub(done) as f64,
        ))
    }
}

/// A video being made in the background.
pub struct Encoding {
    pub progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Encoding {
    /// Starts making the video. `pictures` is kept alive until the end, because unpacked ZIP
    /// pictures are deleted when it's dropped.
    pub fn start(ffmpeg: &Ffmpeg, pictures: Arc<Pictures>, job: Job, output: PathBuf) -> Encoding {
        let progress = Arc::new(Progress {
            frames_done: AtomicU64::new(0),
            total_frames: job.plan.total_frames(),
            started: Instant::now(),
            state: Mutex::new(State::Running),
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let thread = {
            let (progress, cancel, program) =
                (progress.clone(), cancel.clone(), ffmpeg.path.clone());
            thread::Builder::new()
                .name("frameweaver-encode".into())
                .spawn(move || {
                    let result = encode(&program, &pictures, &job, &output, &progress, &cancel);
                    *progress.state.lock().unwrap() = result;
                })
                .expect("couldn't start the encoding thread")
        };
        Encoding {
            progress,
            cancel,
            thread: Some(thread),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Waits for the end (used by the command line).
    pub fn wait(mut self) -> State {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.progress.state()
    }
}

impl Drop for Encoding {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A file name in `folder` that isn't taken yet: "name.mp4", then "name (2).mp4" and so on.
pub fn free_name(folder: &Path, name: &str, extension: &str) -> PathBuf {
    (1u32..)
        .map(|n| {
            if n == 1 {
                format!("{name}.{extension}")
            } else {
                format!("{name} ({n}).{extension}")
            }
        })
        .map(|file| folder.join(file))
        .find(|path| !path.exists())
        .expect("there is always a free name")
}

fn encode(
    program: &Path,
    pictures: &Pictures,
    job: &Job,
    output: &Path,
    progress: &Progress,
    cancel: &AtomicBool,
) -> State {
    let work = match TempFolder::new() {
        Ok(w) => w,
        Err(e) => return State::Failed(format!("Couldn't make a temporary folder: {e}")),
    };
    let list = work.path().join("pictures.ffconcat");
    if let Err(e) = std::fs::write(&list, video::concat_list(&pictures.files, &job.plan)) {
        return State::Failed(format!("Couldn't write the picture list: {e}"));
    }
    if let Some(folder) = output.parent() {
        if let Err(e) = std::fs::create_dir_all(folder) {
            return State::Failed(format!("Couldn't create the output folder: {e}"));
        }
    }
    // Write to a temporary name and rename at the end, so a half-made video never appears
    // under the real name.
    let extension = job.format.extension();
    let partial = output.with_extension(format!("partial.{extension}"));

    let mut command = Command::new(program);
    command
        .args(video::ffmpeg_args(job, &list, &partial))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_window(&mut command);
    lower_priority(&mut command);
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return State::Failed(format!("Couldn't start FFmpeg: {e}")),
    };

    // FFmpeg reports errors on one stream and progress on the other; read both.
    let errors = read_all_later(child.stderr.take());
    let stdout = child.stdout.take();
    let watcher = {
        let progress_lines = stdout.map(BufReader::new);
        thread::scope(|scope| {
            let reader = scope.spawn(|| {
                if let Some(lines) = progress_lines {
                    for line in lines.lines().map_while(Result::ok) {
                        if let Some(frame) = line
                            .strip_prefix("frame=")
                            .and_then(|f| f.trim().parse().ok())
                        {
                            progress.frames_done.store(frame, Ordering::Relaxed);
                        }
                    }
                }
            });
            let outcome = wait_or_cancel(&mut child, cancel);
            let _ = reader.join();
            outcome
        })
    };
    let errors = errors.join().unwrap_or_default();

    match watcher {
        Outcome::Cancelled => {
            let _ = std::fs::remove_file(&partial);
            State::Cancelled
        }
        Outcome::Finished(true) => match std::fs::rename(&partial, output) {
            Ok(()) => {
                progress
                    .frames_done
                    .store(progress.total_frames, Ordering::Relaxed);
                State::Done(output.to_path_buf())
            }
            Err(e) => State::Failed(format!("Couldn't save the video: {e}")),
        },
        Outcome::Finished(false) => {
            let _ = std::fs::remove_file(&partial);
            let detail = errors
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("unknown error");
            State::Failed(format!("FFmpeg couldn't make the video: {detail}"))
        }
    }
}

enum Outcome {
    Finished(bool),
    Cancelled,
}

fn wait_or_cancel(child: &mut Child, cancel: &AtomicBool) -> Outcome {
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Outcome::Cancelled;
        }
        match child.try_wait() {
            Ok(Some(status)) => return Outcome::Finished(status.success()),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return Outcome::Finished(false),
        }
    }
}

fn read_all_later(stream: Option<impl Read + Send + 'static>) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut s) = stream {
            let _ = s.read_to_string(&mut text);
        }
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing;
    use crate::video::{Fit, Quality};

    #[test]
    fn free_names_never_overwrite() {
        let dir = crate::test_dir("free-name");
        assert_eq!(free_name(&dir, "Rec", "mp4"), dir.join("Rec.mp4"));
        std::fs::write(dir.join("Rec.mp4"), b"").unwrap();
        std::fs::write(dir.join("Rec (2).mp4"), b"").unwrap();
        assert_eq!(free_name(&dir, "Rec", "mp4"), dir.join("Rec (3).mp4"));
    }

    /// Reads a video's frame count and size with ffprobe.
    fn probe(ffmpeg: &Ffmpeg, video: &Path) -> (u64, u32, u32) {
        let probe = ffmpeg.path.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        });
        let out = Command::new(probe)
            .args([
                "-v",
                "error",
                "-count_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
            ])
            .args(["stream=nb_read_frames,width,height", "-of", "csv=p=0"])
            .arg(video)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let fields: Vec<&str> = text.trim().split(',').collect();
        (
            fields[2].parse().unwrap(),
            fields[0].parse().unwrap(),
            fields[1].parse().unwrap(),
        )
    }

    /// Which picture appears in each frame of a video, read from the red level at its centre.
    fn pictures_shown(ffmpeg: &Ffmpeg, video: &Path) -> Vec<u32> {
        let out = Command::new(&ffmpeg.path)
            .args(["-v", "error", "-i"])
            .arg(video)
            .args([
                "-vf",
                "crop=8:8:(iw-8)/2:(ih-8)/2,scale=1:1,format=rgb24",
                "-f",
                "rawvideo",
                "-",
            ])
            .output()
            .unwrap();
        out.stdout
            .as_chunks::<3>()
            .0
            .iter()
            .map(|rgb| (u32::from(rgb[0]) + 15) / 30)
            .collect()
    }

    /// Makes real videos with the FFmpeg on this machine (the build container has one) from
    /// pictures of different sizes, and checks the frame count and size are exactly as planned.
    #[test]
    fn makes_videos_with_exactly_the_planned_frames() {
        let Ok(ffmpeg) = Ffmpeg::find(None) else {
            eprintln!("FFmpeg isn't installed here; skipping the real encoding test");
            return;
        };
        let dir = crate::test_dir("encode").join("Rec");
        std::fs::create_dir(&dir).unwrap();
        for n in 1..=7u32 {
            // Pictures change size part-way, like a window resized during a SnapLapse recording.
            let (w, h) = if n < 4 { (320, 200) } else { (300, 300) };
            // Each picture is a solid colour whose red level identifies it (30, 60, 90…).
            image::RgbImage::from_pixel(w, h, image::Rgb([(n * 30) as u8, 0, 0]))
                .save(dir.join(format!("{n:06} - x.png")))
                .unwrap();
        }
        let pictures = Arc::new(crate::input::load(&dir).unwrap());
        let out = crate::test_dir("encode-out");
        for (format, fit) in [
            (Format::Mp4H264, Fit::Bars),
            (Format::WebmVp9, Fit::Crop),
            (Format::Gif, Fit::Stretch),
        ] {
            if !ffmpeg.supports(format) {
                continue;
            }
            let plan = timing::plan_total(7, Duration::from_secs(2), 25);
            let expected: Vec<u32> = plan
                .frames
                .iter()
                .enumerate()
                .flat_map(|(i, &count)| std::iter::repeat_n(i as u32 + 1, count as usize))
                .collect();
            let job = Job {
                plan,
                format,
                quality: Quality::Small,
                size: (256, 144),
                fit,
                bar_color: [0, 0, 0],
            };
            let target = free_name(&out, "video", format.extension());
            let state = Encoding::start(&ffmpeg, pictures.clone(), job, target.clone()).wait();
            assert_eq!(state, State::Done(target.clone()), "{format:?}");
            assert_eq!(probe(&ffmpeg, &target), (50, 256, 144), "{format:?}");
            assert_eq!(
                pictures_shown(&ffmpeg, &target),
                expected,
                "{format:?}: wrong picture in some frame"
            );
        }
        let leftovers: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("partial"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn cancelling_stops_and_leaves_no_partial_file() {
        let Ok(ffmpeg) = Ffmpeg::find(None) else {
            return;
        };
        let dir = crate::test_dir("cancel").join("Rec");
        std::fs::create_dir(&dir).unwrap();
        image::RgbImage::from_pixel(64, 64, image::Rgb([1, 2, 3]))
            .save(dir.join("1.png"))
            .unwrap();
        let pictures = Arc::new(crate::input::load(&dir).unwrap());
        let job = Job {
            // An hour of 4K video: long enough that cancelling happens mid-way.
            plan: timing::plan_total(1, Duration::from_secs(3600), 60),
            format: Format::Mp4H264,
            quality: Quality::High,
            size: (3840, 2160),
            fit: Fit::Bars,
            bar_color: [0, 0, 0],
        };
        let out = crate::test_dir("cancel-out");
        let encoding = Encoding::start(&ffmpeg, pictures, job, out.join("long.mp4"));
        thread::sleep(Duration::from_millis(800));
        encoding.cancel();
        assert_eq!(encoding.wait(), State::Cancelled);
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
    }
}

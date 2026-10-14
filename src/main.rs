// Release builds on Windows open without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod cli;
mod ffmpeg;
mod input;
mod settings;
mod timing;
mod video;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        cli::attach_console();
        std::process::exit(cli::run(&args));
    }
    if let Err(e) = app::run() {
        let message = format!("FrameWeaver couldn't start: {e}");
        eprintln!("{message}");
        #[cfg(windows)]
        rfd::MessageDialog::new()
            .set_title("FrameWeaver")
            .set_description(&message)
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
}

/// A fresh, empty folder for one test.
#[cfg(test)]
pub fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("frameweaver-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

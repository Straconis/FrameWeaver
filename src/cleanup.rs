//! Clearing old videos out of the output folder.
//!
//! Only video files FrameWeaver can make are touched (MP4, WebM and animated GIF), plus any
//! half-made `….partial.mp4` left behind if FrameWeaver was closed mid-encode. Folders and every
//! other kind of file are left alone, and so is anything inside a subfolder.

use std::path::{Path, PathBuf};

use crate::video::Format;

/// What a cleanup would remove from a folder.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Cleanup {
    /// Every file to delete, sorted by name.
    pub items: Vec<PathBuf>,
    /// Their total size on disk, in bytes.
    pub bytes: u64,
}

impl Cleanup {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Lists the videos in `folder`. A missing folder has nothing to clean.
pub fn scan(folder: &Path) -> Cleanup {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Cleanup::default();
    };
    let mut items: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.path())
        .filter(|path| is_video(path))
        .collect();
    items.sort();
    let bytes = items
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    Cleanup { items, bytes }
}

/// Deletes everything in `cleanup`. Returns how many files were removed and the first error, if
/// any. Files that are already gone count as removed.
pub fn delete(cleanup: &Cleanup) -> (usize, Option<String>) {
    let mut removed = 0;
    let mut first_error = None;
    for path in &cleanup.items {
        match std::fs::remove_file(path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => removed += 1,
            Err(e) => {
                first_error.get_or_insert_with(|| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    format!("couldn't delete {name}: {e}")
                });
            }
        }
    }
    (removed, first_error)
}

/// A size such as `2.4 GB`, for the confirmation message.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

/// A file with an extension FrameWeaver saves videos with.
fn is_video(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        Format::ALL
            .iter()
            .any(|f| f.extension().eq_ignore_ascii_case(e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(cleanup: &Cleanup) -> Vec<String> {
        cleanup
            .items
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn finds_only_videos_in_the_folder_itself() {
        let dir = crate::test_dir("cleanup-scan");
        for file in [
            "Session 3.mp4",
            "Session 3 (2).MP4",
            "Clip.webm",
            "Loop.gif",
            "Session 4.partial.mp4",
        ] {
            std::fs::write(dir.join(file), [0u8; 10]).unwrap();
        }
        // None of these are videos FrameWeaver makes, so they must survive.
        std::fs::create_dir(dir.join("Keep me.mp4")).unwrap();
        std::fs::create_dir(dir.join("Pictures")).unwrap();
        std::fs::write(dir.join("Pictures").join("Inside.mp4"), b"keep").unwrap();
        for file in ["notes.txt", "000001 - VTT.png", "Movie.mkv", "mp4"] {
            std::fs::write(dir.join(file), b"keep").unwrap();
        }

        let found = scan(&dir);
        assert_eq!(
            names(&found),
            [
                "Clip.webm",
                "Loop.gif",
                "Session 3 (2).MP4",
                "Session 3.mp4",
                "Session 4.partial.mp4",
            ]
        );
        assert_eq!(found.bytes, 50);

        assert_eq!(delete(&found), (5, None));
        assert!(scan(&dir).is_empty());
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "000001 - VTT.png",
                "Keep me.mp4",
                "Movie.mkv",
                "Pictures",
                "mp4",
                "notes.txt"
            ]
        );
        assert!(dir.join("Pictures").join("Inside.mp4").exists());
    }

    #[test]
    fn a_missing_folder_has_nothing_to_clean() {
        let dir = crate::test_dir("cleanup-missing").join("not here");
        assert!(scan(&dir).is_empty());
    }

    #[test]
    fn already_deleted_files_count_as_removed() {
        let dir = crate::test_dir("cleanup-gone");
        let cleanup = Cleanup {
            items: vec![dir.join("Gone.mp4")],
            bytes: 0,
        };
        assert_eq!(delete(&cleanup), (1, None));
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(human_size(0), "0 bytes");
        assert_eq!(human_size(1023), "1023 bytes");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}

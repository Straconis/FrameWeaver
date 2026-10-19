//! Finding the pictures to turn into a video: a folder, a single picture, or a SnapLapse ZIP
//! (including ZIPs split into "… - part 1.zip", "… - part 2.zip" and so on).

use std::cmp::Ordering;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

const PICTURE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// The pictures for one video, in order.
pub struct Pictures {
    pub files: Vec<PathBuf>,
    /// A name for the video, taken from the folder or ZIP.
    pub name: String,
    /// The size of the first picture, used for "same as pictures" sizes.
    pub first_size: Option<(u32, u32)>,
    /// For a split ZIP, any part numbers that aren't in the folder, such as part 2 of parts 1
    /// and 3. The video is still made from the parts that are there.
    pub missing_parts: Vec<u32>,
    /// Where ZIP contents were unpacked. Deleted when this is dropped.
    _unpacked: Option<TempFolder>,
}

impl Pictures {
    /// Explains which parts of a split ZIP are missing, if any.
    pub fn missing_parts_warning(&self) -> String {
        let list: Vec<String> = self.missing_parts.iter().map(u32::to_string).collect();
        match list.as_slice() {
            [] => String::new(),
            [one] => format!(
                "Part {one} of this split ZIP isn't in the folder, so its pictures are left out."
            ),
            many => format!(
                "Parts {} of this split ZIP aren't in the folder, so their pictures are left out.",
                many.join(", ")
            ),
        }
    }
}

impl std::fmt::Debug for Pictures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} pictures from {:?}", self.files.len(), self.name)
    }
}

pub fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PICTURE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// Loads the pictures from a folder, a picture file, or a ZIP file.
pub fn load(path: &Path) -> Result<Pictures, String> {
    // FFmpeg reads the picture list from a temporary folder, so every path must be absolute.
    let path = &std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut missing_parts = Vec::new();
    let (files, name, unpacked) = if path.is_dir() {
        (
            pictures_in(path).map_err(|e| format!("Couldn't read the folder: {e}"))?,
            file_stem(path),
            None,
        )
    } else if is_zip(path) {
        let parts = zip_parts(path);
        missing_parts = missing_part_numbers(&parts);
        let folder =
            TempFolder::new().map_err(|e| format!("Couldn't make a temporary folder: {e}"))?;
        for (i, part) in parts.iter().enumerate() {
            unzip_pictures(part, folder.path(), i)
                .map_err(|e| format!("Couldn't read {}: {e}", part.display()))?;
        }
        let files = pictures_in(folder.path()).map_err(|e| e.to_string())?;
        (files, recording_name(path), Some(folder))
    } else if is_picture(path) {
        (vec![path.to_path_buf()], file_stem(path), None)
    } else {
        return Err("Choose a folder of pictures, a picture, or a ZIP file".into());
    };
    if files.is_empty() {
        return Err("No PNG, JPG or WebP pictures were found there".into());
    }
    let first_size = files.first().and_then(|f| image::image_dimensions(f).ok());
    Ok(Pictures {
        files,
        name,
        first_size,
        missing_parts,
        _unpacked: unpacked,
    })
}

/// The pictures directly in `dir`, in natural order.
fn pictures_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_picture(p))
        .collect();
    files.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));
    Ok(files)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into())
}

/// For "Rec - part 2.zip", returns "Rec"; otherwise the name without ".zip".
fn recording_name(zip: &Path) -> String {
    let stem = file_stem(zip);
    split_part(&stem).map_or(stem.clone(), |(base, _)| base.to_string())
}

/// Splits "Rec - part 03" into ("Rec", 3).
fn split_part(stem: &str) -> Option<(&str, u32)> {
    let (base, number) = stem.rsplit_once(" - part ")?;
    let n = number.parse().ok()?;
    Some((base, n))
}

/// If `zip` is one part of a split recording, every part in order; otherwise just `zip`.
pub fn zip_parts(zip: &Path) -> Vec<PathBuf> {
    let stem = file_stem(zip);
    let (Some((base, _)), Some(dir)) = (split_part(&stem), zip.parent()) else {
        return vec![zip.to_path_buf()];
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![zip.to_path_buf()];
    };
    let mut parts: Vec<(u32, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_zip(p))
        .filter_map(|p| {
            let stem = file_stem(&p);
            let (b, n) = split_part(&stem)?;
            (b == base).then_some((n, p.clone()))
        })
        .collect();
    parts.sort_by_key(|(n, _)| *n);
    parts.into_iter().map(|(_, p)| p).collect()
}

/// Part numbers missing from a run of split ZIP parts: for parts 1, 2 and 5, that's 3 and 4.
/// A missing last part can't be noticed, because nothing says how many parts there were.
fn missing_part_numbers(parts: &[PathBuf]) -> Vec<u32> {
    let numbers: Vec<u32> = parts
        .iter()
        .filter_map(|p| split_part(&file_stem(p)).map(|(_, n)| n))
        .collect();
    let Some(&last) = numbers.iter().max() else {
        return Vec::new();
    };
    (1..last).filter(|n| !numbers.contains(n)).collect()
}

/// Unpacks the pictures in a ZIP into `into`. Only the file name of each entry is kept, so no
/// entry can write outside the folder. A name already taken gets the part number added.
fn unzip_pictures(zip: &Path, into: &Path, part: usize) -> io::Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(zip)?).map_err(io::Error::other)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(io::Error::other)?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.to_owned()))
        else {
            continue;
        };
        let mut target = into.join(&name);
        if !is_picture(&target) {
            continue;
        }
        if target.exists() {
            target = into.join(format!("part{part} {}", name.to_string_lossy()));
        }
        io::copy(&mut entry, &mut File::create(&target)?)?;
    }
    Ok(())
}

/// Compares names the way people expect: numbers by value (2 before 10), letters ignoring case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        match (a.is_empty(), b.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
        let a_digit = a.starts_with(|c: char| c.is_ascii_digit());
        let b_digit = b.starts_with(|c: char| c.is_ascii_digit());
        let take = |s: &str, digits: bool| {
            s.find(|c: char| c.is_ascii_digit() != digits)
                .unwrap_or(s.len())
        };
        let (a_len, b_len) = (take(a, a_digit), take(b, b_digit));
        let (a_chunk, b_chunk) = (&a[..a_len], &b[..b_len]);
        let order = if a_digit && b_digit {
            // Compare by value: ignore leading zeros, then the longer number is bigger.
            let (a_num, b_num) = (
                a_chunk.trim_start_matches('0'),
                b_chunk.trim_start_matches('0'),
            );
            a_num
                .len()
                .cmp(&b_num.len())
                .then_with(|| a_num.cmp(b_num))
                .then_with(|| a_len.cmp(&b_len))
        } else if a_digit != b_digit {
            // Numbers sort before letters.
            if a_digit {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        } else {
            a_chunk
                .to_lowercase()
                .cmp(&b_chunk.to_lowercase())
                .then_with(|| a_chunk.cmp(b_chunk))
        };
        if order != Ordering::Equal {
            return order;
        }
        a = &a[a_len..];
        b = &b[b_len..];
    }
}

/// A temporary folder that deletes itself when no longer needed.
pub struct TempFolder(PathBuf);

impl TempFolder {
    pub fn new() -> io::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let path = std::env::temp_dir().join(format!("frameweaver-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(TempFolder(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn natural_order_puts_2_before_10() {
        let mut names = vec![
            "10 - a.png",
            "2 - a.png",
            "000001 - a.png",
            "b.png",
            "A.png",
            "1 - a.png",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        // "1" and "000001" are the same number; the shorter is put first so the order is stable.
        assert_eq!(
            names,
            vec![
                "1 - a.png",
                "000001 - a.png",
                "2 - a.png",
                "10 - a.png",
                "A.png",
                "b.png"
            ]
        );
    }

    #[test]
    fn huge_numbers_still_compare_by_value() {
        assert_eq!(
            natural_cmp("99999999999999999999999 x", "100000000000000000000000 x"),
            Ordering::Less
        );
    }

    #[test]
    fn split_zip_parts_are_found_and_ordered() {
        let dir = crate::test_dir("parts");
        for name in [
            "Rec - part 10.zip",
            "Rec - part 2.zip",
            "Rec - part 1.zip",
            "Other - part 1.zip",
            "Rec.zip",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let parts: Vec<String> = zip_parts(&dir.join("Rec - part 2.zip"))
            .iter()
            .map(|p| file_name(p))
            .collect();
        assert_eq!(
            parts,
            vec!["Rec - part 1.zip", "Rec - part 2.zip", "Rec - part 10.zip"]
        );
        assert_eq!(zip_parts(&dir.join("Rec.zip")), vec![dir.join("Rec.zip")]);
        let found = zip_parts(&dir.join("Rec - part 1.zip"));
        assert_eq!(missing_part_numbers(&found), (3..=9).collect::<Vec<u32>>());
        assert!(missing_part_numbers(&[dir.join("Rec.zip")]).is_empty());
        assert_eq!(recording_name(&dir.join("Rec - part 2.zip")), "Rec");
    }

    fn tiny_png(path: &Path, w: u32, h: u32) {
        image::RgbImage::from_pixel(w, h, image::Rgb([10, 20, 30]))
            .save(path)
            .unwrap();
    }

    #[test]
    fn a_folder_gives_its_pictures_in_order() {
        let dir = crate::test_dir("folder").join("My Recording");
        std::fs::create_dir(&dir).unwrap();
        for n in [3, 1, 20, 2] {
            tiny_png(&dir.join(format!("{n} - VTT.png")), 8, 6);
        }
        std::fs::write(dir.join("notes.txt"), b"not a picture").unwrap();
        let pictures = load(&dir).unwrap();
        let names: Vec<String> = pictures.files.iter().map(|p| file_name(p)).collect();
        assert_eq!(
            names,
            vec!["1 - VTT.png", "2 - VTT.png", "3 - VTT.png", "20 - VTT.png"]
        );
        assert_eq!(pictures.name, "My Recording");
        assert_eq!(pictures.first_size, Some((8, 6)));
    }

    #[test]
    fn split_zips_are_unpacked_in_order_and_cleaned_up() {
        let dir = crate::test_dir("zips");
        let pngs = crate::test_dir("zips-src");
        for n in 1..=4 {
            tiny_png(&pngs.join(format!("{n:06} - VTT.png")), 4, 4);
        }
        for (part, numbers) in [(1, [1, 2]), (2, [3, 4])] {
            let file =
                File::create(dir.join(format!("2026-10-14 19-30-05 - VTT - part {part}.zip")))
                    .unwrap();
            let mut zip = zip::ZipWriter::new(file);
            for n in numbers {
                let name = format!("{n:06} - VTT.png");
                zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(&std::fs::read(pngs.join(&name)).unwrap())
                    .unwrap();
            }
            zip.finish().unwrap();
        }
        let pictures = load(&dir.join("2026-10-14 19-30-05 - VTT - part 2.zip")).unwrap();
        assert_eq!(pictures.name, "2026-10-14 19-30-05 - VTT");
        assert!(pictures.missing_parts.is_empty());
        let names: Vec<String> = pictures.files.iter().map(|p| file_name(p)).collect();
        assert_eq!(
            names,
            (1..=4)
                .map(|n| format!("{n:06} - VTT.png"))
                .collect::<Vec<_>>()
        );
        let unpacked = pictures.files[0].parent().unwrap().to_path_buf();
        assert!(unpacked.exists());
        drop(pictures);
        assert!(!unpacked.exists(), "temporary pictures should be deleted");
    }

    #[test]
    fn empty_folders_and_other_files_are_explained() {
        let dir = crate::test_dir("empty");
        assert!(load(&dir).unwrap_err().contains("No PNG"));
        std::fs::write(dir.join("a.txt"), b"").unwrap();
        assert!(load(&dir.join("a.txt")).unwrap_err().contains("Choose"));
    }

    mod properties {
        use super::super::*;
        use proptest::prelude::*;

        proptest! {
            /// Natural order is a proper ordering: consistent both ways, and numbered names
            /// always come out in number order.
            #[test]
            fn natural_order_is_consistent(a in "[a-c0-9 ]{0,8}", b in "[a-c0-9 ]{0,8}") {
                prop_assert_eq!(natural_cmp(&a, &b), natural_cmp(&b, &a).reverse());
            }

            #[test]
            fn numbered_files_sort_by_number(mut ns in proptest::collection::vec(0u64..1_000_000, 1..50)) {
                let mut names: Vec<String> = ns.iter().map(|n| format!("{n} - x.png")).collect();
                names.sort_by(|a, b| natural_cmp(a, b));
                ns.sort();
                let sorted: Vec<String> = ns.iter().map(|n| format!("{n} - x.png")).collect();
                prop_assert_eq!(names, sorted);
            }
        }
    }
}

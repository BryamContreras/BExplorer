use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

use walkdir::WalkDir;

const SNAPSHOT_FILE_INTERVAL: usize = 512;
const SNAPSHOT_TIME_INTERVAL: Duration = Duration::from_millis(250);
const CANDIDATE_SNAPSHOT_FILE_INTERVAL: usize = 4_096;
const CANDIDATE_SNAPSHOT_TIME_INTERVAL: Duration = Duration::from_secs(2);
const MINIMUM_PREFIX_LENGTH: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DuplicateKind {
    Original,
    Exact,
    Possible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicateFile {
    pub path: PathBuf,
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub kind: DuplicateKind,
}

#[derive(Clone, Debug)]
pub enum DuplicateScanEvent {
    Counting {
        files_found: usize,
    },
    Progress {
        scanned: usize,
        total: usize,
        current_path: Option<PathBuf>,
        duplicates: Option<Vec<DuplicateFile>>,
        skipped: usize,
    },
    Finished {
        scanned: usize,
        total: usize,
        duplicates: Vec<DuplicateFile>,
        skipped: usize,
    },
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug)]
struct ScannedFile {
    path: PathBuf,
    name: String,
    stem: String,
    extension: String,
    size: u64,
    created: Option<SystemTime>,
    modified: Option<SystemTime>,
}

/// Scan a local directory recursively. Directory links are not followed, so a
/// symlink cycle cannot make the cleanup scan recurse forever.
pub fn scan_folder(root: PathBuf, sender: Sender<DuplicateScanEvent>, cancel: Arc<AtomicBool>) {
    if !root.is_dir() {
        let _ = sender.send(DuplicateScanEvent::Failed(format!(
            "{} is not a directory",
            root.display()
        )));
        return;
    }

    let mut total = 0_usize;
    for entry in WalkDir::new(&root).follow_links(false) {
        if cancel.load(AtomicOrdering::Relaxed) {
            let _ = sender.send(DuplicateScanEvent::Cancelled);
            return;
        }
        if entry.is_ok_and(|entry| entry.file_type().is_file()) {
            total += 1;
            if total.is_multiple_of(SNAPSHOT_FILE_INTERVAL) {
                let _ = sender.send(DuplicateScanEvent::Counting { files_found: total });
            }
        }
    }
    let _ = sender.send(DuplicateScanEvent::Counting { files_found: total });

    let mut files = Vec::with_capacity(total);
    let mut scanned = 0_usize;
    let mut skipped = 0_usize;
    let mut last_snapshot = Instant::now();
    let mut last_candidate_snapshot = Instant::now();
    for entry in WalkDir::new(&root).follow_links(false) {
        if cancel.load(AtomicOrdering::Relaxed) {
            let _ = sender.send(DuplicateScanEvent::Cancelled);
            return;
        }
        let entry = match entry {
            Ok(entry) if entry.file_type().is_file() => entry,
            Ok(_) => continue,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        scanned += 1;
        let path = entry.into_path();
        match scanned_file(&path) {
            Ok(file) => files.push(file),
            Err(_) => skipped += 1,
        }

        let should_publish = scanned.is_multiple_of(SNAPSHOT_FILE_INTERVAL)
            || last_snapshot.elapsed() >= SNAPSHOT_TIME_INTERVAL;
        if should_publish {
            let should_refresh_candidates =
                should_refresh_candidate_snapshot(scanned, last_candidate_snapshot.elapsed());
            let duplicates = should_refresh_candidates.then(|| classify_duplicates(&files));
            let _ = sender.send(DuplicateScanEvent::Progress {
                scanned,
                total,
                current_path: Some(path),
                duplicates,
                skipped,
            });
            last_snapshot = Instant::now();
            if should_refresh_candidates {
                last_candidate_snapshot = Instant::now();
            }
        }
    }

    let duplicates = classify_duplicates(&files);
    let _ = sender.send(DuplicateScanEvent::Finished {
        scanned,
        total,
        duplicates,
        skipped,
    });
}

fn should_refresh_candidate_snapshot(scanned: usize, elapsed: Duration) -> bool {
    scanned.is_multiple_of(CANDIDATE_SNAPSHOT_FILE_INTERVAL)
        || elapsed >= CANDIDATE_SNAPSHOT_TIME_INTERVAL
}

fn scanned_file(path: &Path) -> std::io::Result<ScannedFile> {
    let metadata = std::fs::metadata(path)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let comparable_name = name.to_lowercase();
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_lowercase())
        .unwrap_or_else(|| comparable_name.clone());
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    Ok(ScannedFile {
        path: path.to_path_buf(),
        name,
        stem,
        extension,
        size: metadata.len(),
        created: metadata.created().ok(),
        modified: metadata.modified().ok(),
    })
}

fn classify_duplicates(files: &[ScannedFile]) -> Vec<DuplicateFile> {
    if files.len() < 2 {
        return Vec::new();
    }

    let mut union = DisjointSet::new(files.len());
    let mut exact = HashMap::<(&str, &str, u64), usize>::new();
    for (index, file) in files.iter().enumerate() {
        if let Some(previous) = exact.insert((&file.name, &file.extension, file.size), index) {
            union.join(previous, index);
        }
    }

    let mut by_extension = HashMap::<&str, Vec<usize>>::new();
    for (index, file) in files.iter().enumerate() {
        by_extension.entry(&file.extension).or_default().push(index);
    }
    for indexes in by_extension.values_mut() {
        // Explorer and browsers commonly preserve both files by adding
        // localized copy markers or numeric counters. Group those names by
        // their stable base even when the unsuffixed original is absent.
        let mut copy_families = HashMap::<&str, usize>::new();
        for &index in indexes.iter() {
            let family = copy_family_stem(&files[index].stem);
            if family.chars().count() < MINIMUM_PREFIX_LENGTH {
                continue;
            }
            if let Some(previous) = copy_families.insert(family, index) {
                union.join(previous, index);
            }
        }

        indexes.sort_unstable_by(|left, right| files[*left].stem.cmp(&files[*right].stem));
        let mut base: Option<usize> = None;
        for &index in indexes.iter() {
            let stem = &files[index].stem;
            if stem.chars().count() < MINIMUM_PREFIX_LENGTH {
                base = Some(index);
                continue;
            }
            if let Some(base_index) = base {
                let base_stem = &files[base_index].stem;
                if names_share_candidate_prefix(base_stem, stem)
                    && base_stem.chars().count() >= MINIMUM_PREFIX_LENGTH
                {
                    union.join(base_index, index);
                    continue;
                }
            }
            base = Some(index);
        }
    }

    let mut groups = HashMap::<usize, Vec<usize>>::new();
    for index in 0..files.len() {
        let root = union.root(index);
        groups.entry(root).or_default().push(index);
    }

    let mut result = Vec::new();
    for mut indexes in groups.into_values().filter(|group| group.len() > 1) {
        indexes.sort_unstable_by(|left, right| compare_file_age(&files[*left], &files[*right]));
        let original = indexes[0];
        for index in indexes {
            let file = &files[index];
            let kind = if index == original {
                DuplicateKind::Original
            } else if files.iter().enumerate().any(|(other_index, other)| {
                other_index != index
                    && other.name == file.name
                    && other.extension == file.extension
                    && other.size == file.size
            }) {
                DuplicateKind::Exact
            } else {
                DuplicateKind::Possible
            };
            result.push(DuplicateFile {
                path: file.path.clone(),
                name: file.name.clone(),
                extension: file.extension.clone(),
                size: file.size,
                created: file.created,
                modified: file.modified,
                kind,
            });
        }
    }
    result.sort_by(|left, right| {
        left.extension
            .cmp(&right.extension)
            .then_with(|| compare_duplicate_age(left, right))
    });
    result
}

fn names_share_candidate_prefix(shorter: &str, longer: &str) -> bool {
    let Some(remainder) = longer.strip_prefix(shorter) else {
        return false;
    };
    // A new dot-separated suffix denotes another extension-like component,
    // not a renamed copy. `libffmpeg.so`, for example, must not be compared
    // with `libffmpeg.zip.so`.
    !remainder.starts_with('.')
}

fn copy_family_stem(stem: &str) -> &str {
    let original = stem.trim();
    let (without_counter, had_counter) = strip_copy_counter(original)
        .map(|base| (base, true))
        .unwrap_or((original, false));
    let without_copy_marker = [" - copia", "-copia", " copia", " - copy", "-copy", " copy"]
        .into_iter()
        .find_map(|suffix| without_counter.strip_suffix(suffix))
        .map(str::trim_end);
    let family = without_copy_marker.unwrap_or({
        if had_counter {
            without_counter
        } else {
            original
        }
    });

    if family.chars().count() >= MINIMUM_PREFIX_LENGTH {
        family
    } else {
        original
    }
}

fn strip_copy_counter(value: &str) -> Option<&str> {
    let value = value.trim_end();
    let body = value.strip_suffix(')')?;
    let open = body.rfind('(')?;
    let number = &body[open + 1..];
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = number.parse::<u64>().ok()?;
    (number > 0).then(|| body[..open].trim_end())
}

fn compare_file_age(left: &ScannedFile, right: &ScannedFile) -> Ordering {
    compare_optional_time(
        left.created.or(left.modified),
        right.created.or(right.modified),
    )
    .then_with(|| left.path.cmp(&right.path))
}

fn compare_duplicate_age(left: &DuplicateFile, right: &DuplicateFile) -> Ordering {
    compare_optional_time(
        left.created.or(left.modified),
        right.created.or(right.modified),
    )
    .then_with(|| left.path.cmp(&right.path))
}

fn compare_optional_time(left: Option<SystemTime>, right: Option<SystemTime>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(length: usize) -> Self {
        Self {
            parent: (0..length).collect(),
        }
    }

    fn root(&mut self, index: usize) -> usize {
        let parent = self.parent[index];
        if parent == index {
            index
        } else {
            let root = self.root(parent);
            self.parent[index] = root;
            root
        }
    }

    fn join(&mut self, left: usize, right: usize) {
        let left = self.root(left);
        let right = self.root(right);
        if left != right {
            self.parent[right] = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Duration, UNIX_EPOCH};

    fn file(name: &str, size: u64, created_seconds: u64) -> ScannedFile {
        let path = PathBuf::from(name);
        ScannedFile {
            path: path.clone(),
            name: name.into(),
            stem: path.file_stem().unwrap().to_string_lossy().to_lowercase(),
            extension: path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase(),
            size,
            created: Some(UNIX_EPOCH + Duration::from_secs(created_seconds)),
            modified: None,
        }
    }

    fn temporary_folder() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bexplorer-duplicate-test-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn exact_duplicates_keep_the_oldest_as_original() {
        let entries =
            classify_duplicates(&[file("photo.jpg", 120, 20), file("photo.jpg", 120, 10)]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, DuplicateKind::Original);
        assert_eq!(entries[1].kind, DuplicateKind::Exact);
    }

    #[test]
    fn matching_name_prefixes_are_possible_duplicates() {
        let entries =
            classify_duplicates(&[file("report.txt", 10, 10), file("report copy.txt", 20, 20)]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, DuplicateKind::Original);
        assert_eq!(entries[1].kind, DuplicateKind::Possible);
    }

    #[test]
    fn windows_copy_suffixes_share_a_family_without_the_original() {
        let entries = classify_duplicates(&[
            file("Informe - Copia.txt", 10, 10),
            file("Informe - Copia (2).txt", 20, 20),
            file("Informe copia(3).txt", 30, 30),
        ]);

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, DuplicateKind::Original);
        assert!(
            entries[1..]
                .iter()
                .all(|entry| entry.kind == DuplicateKind::Possible)
        );
    }

    #[test]
    fn english_windows_copy_suffixes_are_supported() {
        let entries = classify_duplicates(&[
            file("Report - Copy.txt", 10, 10),
            file("Report - Copy (2).txt", 20, 20),
        ]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, DuplicateKind::Original);
        assert_eq!(entries[1].kind, DuplicateKind::Possible);
    }

    #[test]
    fn numeric_copy_suffixes_share_a_family_without_the_original() {
        let entries =
            classify_duplicates(&[file("photo (1).jpg", 10, 10), file("photo(2).jpg", 20, 20)]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, DuplicateKind::Original);
        assert_eq!(entries[1].kind, DuplicateKind::Possible);
    }

    #[test]
    fn copy_words_inside_a_name_are_not_removed() {
        assert_eq!(copy_family_stem("fotocopia"), "fotocopia");
        assert_eq!(copy_family_stem("copywriter"), "copywriter");
        assert_eq!(copy_family_stem("informe (final)"), "informe (final)");
    }

    #[test]
    fn candidate_snapshots_are_throttled_during_large_scans() {
        assert!(!should_refresh_candidate_snapshot(
            SNAPSHOT_FILE_INTERVAL,
            SNAPSHOT_TIME_INTERVAL,
        ));
        assert!(should_refresh_candidate_snapshot(
            CANDIDATE_SNAPSHOT_FILE_INTERVAL,
            Duration::ZERO,
        ));
        assert!(should_refresh_candidate_snapshot(
            1,
            CANDIDATE_SNAPSHOT_TIME_INTERVAL,
        ));
    }

    #[test]
    fn unrelated_names_and_different_extensions_stay_out() {
        let entries = classify_duplicates(&[
            file("report.txt", 10, 10),
            file("holiday.jpg", 10, 20),
            file("report.pdf", 10, 30),
        ]);

        assert!(entries.is_empty());
    }

    #[test]
    fn dot_separated_suffixes_are_not_name_variants() {
        let entries = classify_duplicates(&[
            file("libffmpeg.so", 10, 10),
            file("libffmpeg.zip.so", 20, 20),
        ]);

        assert!(entries.is_empty());
    }

    #[test]
    fn equal_names_with_different_sizes_are_possible_matches() {
        let entries = classify_duplicates(&[
            file("bstream_icon.png", 10, 10),
            file("bstream_icon.png", 20, 20),
        ]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, DuplicateKind::Original);
        assert_eq!(entries[1].kind, DuplicateKind::Possible);
    }

    #[test]
    fn results_are_grouped_by_extension_then_creation_date() {
        let entries = classify_duplicates(&[
            file("video.mp4", 10, 30),
            file("video copy.mp4", 20, 40),
            file("photo.png", 10, 10),
            file("photo copy.png", 20, 20),
        ]);

        let extensions = entries
            .iter()
            .map(|entry| entry.extension.as_str())
            .collect::<Vec<_>>();
        assert_eq!(extensions, ["mp4", "mp4", "png", "png"]);
        for group in entries.chunk_by(|left, right| left.extension == right.extension) {
            let dates = group
                .iter()
                .map(|entry| entry.created.unwrap())
                .collect::<Vec<_>>();
            assert!(dates.windows(2).all(|pair| pair[0] <= pair[1]));
        }
    }

    #[test]
    fn results_are_sorted_oldest_first_across_groups() {
        let entries = classify_duplicates(&[
            file("second.txt", 10, 40),
            file("second copy.txt", 11, 50),
            file("first.jpg", 10, 10),
            file("first edited.jpg", 12, 20),
        ]);

        let dates = entries
            .iter()
            .map(|entry| entry.created.unwrap())
            .collect::<Vec<_>>();
        assert!(dates.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn recursive_scan_publishes_nested_exact_duplicates() {
        let root = temporary_folder();
        fs::create_dir_all(root.join("first")).unwrap();
        fs::create_dir_all(root.join("second")).unwrap();
        fs::write(root.join("first").join("document.txt"), b"same").unwrap();
        fs::write(root.join("second").join("document.txt"), b"same").unwrap();
        let (sender, receiver) = mpsc::channel();

        scan_folder(root.clone(), sender, Arc::new(AtomicBool::new(false)));

        let finished = receiver
            .into_iter()
            .find_map(|event| match event {
                DuplicateScanEvent::Finished { duplicates, .. } => Some(duplicates),
                _ => None,
            })
            .unwrap();
        assert_eq!(finished.len(), 2);
        assert_eq!(
            finished
                .iter()
                .filter(|entry| entry.kind == DuplicateKind::Original)
                .count(),
            1
        );
        assert_eq!(
            finished
                .iter()
                .filter(|entry| entry.kind == DuplicateKind::Exact)
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pre_cancelled_scan_never_walks_the_folder() {
        let root = temporary_folder();
        fs::create_dir_all(&root).unwrap();
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(true));

        scan_folder(root.clone(), sender, cancel);

        assert!(matches!(
            receiver.recv().unwrap(),
            DuplicateScanEvent::Cancelled
        ));
        fs::remove_dir_all(root).unwrap();
    }
}

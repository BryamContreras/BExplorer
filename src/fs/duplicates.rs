use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
mod content;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

use walkdir::WalkDir;

const SNAPSHOT_FILE_INTERVAL: usize = 512;
const SNAPSHOT_TIME_INTERVAL: Duration = Duration::from_millis(250);
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
    scan_folder_impl(root, sender, cancel, true, None);
}

/// Scan a directory for a final duplicate-space estimate without publishing
/// heavyweight candidate snapshots while the scan is still in progress.
pub fn scan_folder_for_estimate(
    root: PathBuf,
    known_total: usize,
    sender: Sender<DuplicateScanEvent>,
    cancel: Arc<AtomicBool>,
) {
    scan_folder_impl(root, sender, cancel, false, Some(known_total));
}

fn scan_folder_impl(
    root: PathBuf,
    sender: Sender<DuplicateScanEvent>,
    cancel: Arc<AtomicBool>,
    _publish_candidate_snapshots: bool,
    known_total: Option<usize>,
) {
    if !root.is_dir() {
        let _ = sender.send(DuplicateScanEvent::Failed(format!(
            "{} is not a directory",
            root.display()
        )));
        return;
    }

    if cancel.load(AtomicOrdering::Relaxed) {
        let _ = sender.send(DuplicateScanEvent::Cancelled);
        return;
    }
    let _ = sender.send(DuplicateScanEvent::Counting {
        files_found: known_total.unwrap_or(0),
    });
    let mut files = Vec::new();
    let mut skipped = 0_usize;
    let mut last_snapshot = Instant::now();
    // Gather names and metadata in one traversal. Content is verified once,
    // after collection, instead of rehashing every growing preview snapshot.
    for entry in WalkDir::new(&root)
        .follow_links(false)
        .follow_root_links(true)
        .same_file_system(true)
    {
        if cancel.load(AtomicOrdering::Relaxed) {
            let _ = sender.send(DuplicateScanEvent::Cancelled);
            return;
        }
        let entry = match entry {
            Ok(entry) if entry.file_type().is_file() => entry,
            Ok(_) => continue,
            Err(error) if error.depth() == 0 => {
                let _ = sender.send(DuplicateScanEvent::Failed(error.to_string()));
                return;
            }
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        match scanned_file(entry.path()) {
            Ok(file) => files.push(file),
            Err(_) => skipped += 1,
        }
        if files.len().is_multiple_of(SNAPSHOT_FILE_INTERVAL)
            || last_snapshot.elapsed() >= SNAPSHOT_TIME_INTERVAL
        {
            if sender
                .send(DuplicateScanEvent::Counting {
                    files_found: files.len(),
                })
                .is_err()
            {
                return;
            }
            last_snapshot = Instant::now();
        }
    }
    let total = files.len();
    let mut last_progress = Instant::now() - SNAPSHOT_TIME_INTERVAL;
    let result = classify_verified_duplicates(&files, &cancel, |processed, path| {
        if processed.is_multiple_of(SNAPSHOT_FILE_INTERVAL)
            || last_progress.elapsed() >= SNAPSHOT_TIME_INTERVAL
        {
            last_progress = Instant::now();
            sender
                .send(DuplicateScanEvent::Progress {
                    scanned: processed,
                    total,
                    current_path: Some(path.to_path_buf()),
                    duplicates: None,
                    skipped,
                })
                .is_ok()
        } else {
            true
        }
    });
    let Some(duplicates) = result else {
        let _ = sender.send(DuplicateScanEvent::Cancelled);
        return;
    };
    let _ = sender.send(DuplicateScanEvent::Finished {
        scanned: total,
        total,
        duplicates,
        skipped,
    });
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

#[cfg(test)]
fn classify_duplicates(files: &[ScannedFile]) -> Vec<DuplicateFile> {
    classify_duplicates_cancellable(files, None).expect("uncancelled duplicate classification")
}

#[cfg(test)]
fn classify_duplicates_cancellable(
    files: &[ScannedFile],
    cancel: Option<&AtomicBool>,
) -> Option<Vec<DuplicateFile>> {
    let never_cancel = AtomicBool::new(false);
    classify_verified_duplicates(files, cancel.unwrap_or(&never_cancel), |_, _| true)
}

fn classify_verified_duplicates(
    files: &[ScannedFile],
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, &Path) -> bool,
) -> Option<Vec<DuplicateFile>> {
    let mut result = classify_name_candidates_cancellable(files, Some(cancel))?;
    let mut by_size = HashMap::<u64, usize>::new();
    for file in files {
        *by_size.entry(file.size).or_default() += 1;
    }
    let mut by_content = HashMap::<(u64, [u8; 16]), Vec<(usize, content::ContentStamp)>>::new();
    for (index, file) in files.iter().enumerate() {
        if cancel.load(AtomicOrdering::Relaxed) || !progress(index, &file.path) {
            return None;
        }
        if by_size[&file.size] < 2 {
            continue;
        }
        if let Ok(stamp) = content::fingerprint(&file.path, cancel)
            && stamp.size == file.size
            && stamp.modified == file.modified
        {
            by_content
                .entry((stamp.size, stamp.digest))
                .or_default()
                .push((index, stamp));
        }
    }
    let mut positions = result
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.clone(), index))
        .collect::<HashMap<_, _>>();
    for mut candidates in by_content.into_values() {
        candidates.sort_by(|(left, _), (right, _)| compare_file_age(&files[*left], &files[*right]));
        // Multiple names for the same inode/file ID do not occupy duplicate
        // storage and must not be offered as verified removable copies.
        let mut seen_identities = HashSet::new();
        candidates.retain(|(_, stamp)| seen_identities.insert(stamp.identity));
        let mut classes = Vec::<Vec<(usize, content::ContentStamp)>>::new();
        for candidate in candidates {
            if cancel.load(AtomicOrdering::Relaxed) {
                return None;
            }
            let matching = classes.iter().position(|class| {
                let representative = &class[0];
                candidate.1.identity != representative.1.identity
                    && content::equal(
                        &files[candidate.0].path,
                        &files[representative.0].path,
                        cancel,
                    )
                    .unwrap_or(false)
            });
            if let Some(class) = matching {
                classes[class].push(candidate);
            } else {
                classes.push(vec![candidate]);
            }
        }
        for class in classes.into_iter().filter(|class| class.len() > 1) {
            for (position, (index, _)) in class.into_iter().enumerate() {
                let file = &files[index];
                let kind = if position == 0 {
                    DuplicateKind::Original
                } else {
                    DuplicateKind::Exact
                };
                if let Some(&position) = positions.get(&file.path) {
                    result[position].kind = kind;
                } else {
                    positions.insert(file.path.clone(), result.len());
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
        }
    }
    if cancel.load(AtomicOrdering::Relaxed) {
        return None;
    }
    result.sort_by(|left, right| {
        left.extension
            .cmp(&right.extension)
            .then_with(|| compare_duplicate_age(left, right))
    });
    Some(result)
}

/// Validate the complete selection before any trash mutation. Exact matches
/// must still have an equal, stable copy outside the selection.
pub fn verify_delete_selection(
    entries: &[DuplicateFile],
    selected: &[PathBuf],
) -> crate::utils::errors::Result<()> {
    use crate::utils::errors::BExplorerError;
    let selected_set = selected.iter().collect::<HashSet<_>>();
    let cancel = AtomicBool::new(false);
    let mut stamps = HashMap::new();
    for path in selected {
        let entry = entries
            .iter()
            .find(|entry| &entry.path == path)
            .ok_or_else(|| {
                BExplorerError::Operation(
                    "The duplicate selection is no longer available; scan again".into(),
                )
            })?;
        if entry.kind != DuplicateKind::Exact {
            continue;
        }
        let stamp = content::fingerprint(path, &cancel)?;
        let mut retained = false;
        for keeper in entries
            .iter()
            .filter(|entry| !selected_set.contains(&entry.path) && entry.size == stamp.size)
        {
            let keeper_stamp = stamps
                .entry(keeper.path.clone())
                .or_insert_with(|| content::fingerprint(&keeper.path, &cancel).ok());
            if keeper_stamp.as_ref().is_some_and(|keeper| {
                keeper.digest == stamp.digest && keeper.identity != stamp.identity
            }) && content::equal(path, &keeper.path, &cancel).unwrap_or(false)
            {
                retained = true;
                break;
            }
        }
        if !retained {
            return Err(BExplorerError::Operation(format!(
                "{} no longer has a verified copy outside the selection; scan again before deleting",
                path.display()
            )));
        }
    }
    Ok(())
}

fn classify_name_candidates_cancellable(
    files: &[ScannedFile],
    cancel: Option<&AtomicBool>,
) -> Option<Vec<DuplicateFile>> {
    if files.len() < 2 {
        return Some(Vec::new());
    }

    let mut union = DisjointSet::new(files.len());
    let mut exact = HashMap::<(&str, &str, u64), (usize, usize)>::new();
    for (index, file) in files.iter().enumerate() {
        if duplicate_classification_cancelled(cancel, index) {
            return None;
        }
        let key = (&*file.name, &*file.extension, file.size);
        if let Some((previous, count)) = exact.get_mut(&key) {
            union.join(*previous, index);
            *count = count.saturating_add(1);
        } else {
            exact.insert(key, (index, 1));
        }
    }

    let mut by_extension = HashMap::<&str, Vec<usize>>::new();
    for (index, file) in files.iter().enumerate() {
        by_extension.entry(&file.extension).or_default().push(index);
    }
    for indexes in by_extension.values_mut() {
        if cancel.is_some_and(|cancel| cancel.load(AtomicOrdering::Relaxed)) {
            return None;
        }
        // Explorer and browsers commonly preserve both files by adding
        // localized copy markers or numeric counters. Group those names by
        // their stable base even when the unsuffixed original is absent.
        let mut copy_families = HashMap::<&str, usize>::new();
        for (position, &index) in indexes.iter().enumerate() {
            if duplicate_classification_cancelled(cancel, position) {
                return None;
            }
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
        for (position, &index) in indexes.iter().enumerate() {
            if duplicate_classification_cancelled(cancel, position) {
                return None;
            }
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
        if duplicate_classification_cancelled(cancel, index) {
            return None;
        }
        let root = union.root(index);
        groups.entry(root).or_default().push(index);
    }

    let mut result = Vec::new();
    for (group_index, mut indexes) in groups
        .into_values()
        .filter(|group| group.len() > 1)
        .enumerate()
    {
        if duplicate_classification_cancelled(cancel, group_index) {
            return None;
        }
        indexes.sort_unstable_by(|left, right| compare_file_age(&files[*left], &files[*right]));
        let original = indexes[0];
        for index in indexes {
            let file = &files[index];
            let kind = if index == original {
                DuplicateKind::Original
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
    Some(result)
}

fn duplicate_classification_cancelled(cancel: Option<&AtomicBool>, index: usize) -> bool {
    index.is_multiple_of(SNAPSHOT_FILE_INTERVAL)
        && cancel.is_some_and(|cancel| cancel.load(AtomicOrdering::Relaxed))
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
    use flate2::{Compression, GzBuilder};
    use std::fs;
    use std::io::Write;
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
        let root = temporary_folder();
        fs::create_dir_all(&root).unwrap();
        let first = root.join("photo.jpg");
        let second = root.join("renamed.jpg");
        fs::write(&first, b"same content").unwrap();
        fs::write(&second, b"same content").unwrap();
        let mut first = scanned_file(&first).unwrap();
        first.created = Some(UNIX_EPOCH + Duration::from_secs(20));
        let mut second = scanned_file(&second).unwrap();
        second.created = Some(UNIX_EPOCH + Duration::from_secs(10));
        let entries = classify_duplicates(&[first, second]);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, DuplicateKind::Original);
        assert_eq!(entries[1].kind, DuplicateKind::Exact);
        verify_delete_selection(&entries, &[entries[1].path.clone()]).unwrap();
        assert!(
            verify_delete_selection(
                &entries,
                &entries
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
        fs::write(&entries[1].path, b"new contents").unwrap();
        assert!(verify_delete_selection(&entries, &[entries[1].path.clone()]).is_err());
        assert_eq!(fs::read(&entries[0].path).unwrap(), b"same content");
        assert_eq!(fs::read(&entries[1].path).unwrap(), b"new contents");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn equal_name_and_size_with_different_bytes_are_only_possible() {
        let root = temporary_folder();
        fs::create_dir_all(root.join("one")).unwrap();
        fs::create_dir_all(root.join("two")).unwrap();
        let first = root.join("one/photo.jpg");
        let second = root.join("two/photo.jpg");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"other").unwrap();
        let entries = classify_duplicates(&[
            scanned_file(&first).unwrap(),
            scanned_file(&second).unwrap(),
        ]);
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| entry.kind != DuplicateKind::Exact)
        );
        let possible = entries
            .iter()
            .find(|entry| entry.kind == DuplicateKind::Possible)
            .unwrap();
        verify_delete_selection(&entries, std::slice::from_ref(&possible.path)).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hardlinked_names_are_not_verified_duplicate_storage() {
        let root = temporary_folder();
        fs::create_dir_all(&root).unwrap();
        let first = root.join("photo.jpg");
        let second = root.join("photo copy.jpg");
        fs::write(&first, b"same").unwrap();
        fs::hard_link(&first, &second).unwrap();
        let entries = classify_duplicates(&[
            scanned_file(&first).unwrap(),
            scanned_file(&second).unwrap(),
        ]);
        assert!(
            entries
                .iter()
                .all(|entry| entry.kind != DuplicateKind::Exact)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn final_classification_honors_cancellation() {
        let files = [file("photo.jpg", 120, 20), file("photo.jpg", 120, 10)];
        let cancel = AtomicBool::new(true);

        assert!(classify_duplicates_cancellable(&files, Some(&cancel)).is_none());
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
    fn name_candidates_never_claim_verified_content() {
        let files = [file("photo.jpg", 120, 10), file("photo.jpg", 120, 20)];
        let candidates = classify_name_candidates_cancellable(&files, None).unwrap();
        assert!(
            candidates
                .iter()
                .all(|entry| entry.kind != DuplicateKind::Exact)
        );
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
    fn estimate_scan_uses_the_known_total_and_reaches_completion() {
        let root = temporary_folder();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("first.txt"), b"first").unwrap();
        fs::write(root.join("second.txt"), b"second").unwrap();
        let (sender, receiver) = mpsc::channel();

        scan_folder_for_estimate(root.clone(), 2, sender, Arc::new(AtomicBool::new(false)));

        let events = receiver.into_iter().collect::<Vec<_>>();
        assert!(matches!(
            events.first(),
            Some(DuplicateScanEvent::Counting { files_found: 2 })
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            DuplicateScanEvent::Finished {
                scanned: 2,
                total: 2,
                ..
            }
        )));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compressed_archive_is_scanned_as_one_file_without_visiting_internal_names() {
        let root = temporary_folder();
        fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("archive.gz");
        let archive_file = fs::File::create(&archive_path).unwrap();
        let mut archive = GzBuilder::new()
            .filename("inside.txt")
            .write(archive_file, Compression::default());
        archive.write_all(b"same").unwrap();
        archive.finish().unwrap();
        fs::write(root.join("inside.txt"), b"same").unwrap();
        let (sender, receiver) = mpsc::channel();

        scan_folder(root.clone(), sender, Arc::new(AtomicBool::new(false)));

        let (scanned, total, duplicates) = receiver
            .into_iter()
            .find_map(|event| match event {
                DuplicateScanEvent::Finished {
                    scanned,
                    total,
                    duplicates,
                    ..
                } => Some((scanned, total, duplicates)),
                _ => None,
            })
            .unwrap();
        assert_eq!(scanned, 2);
        assert_eq!(total, 2);
        assert!(
            duplicates.is_empty(),
            "the internal inside.txt must not be compared with the real file"
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

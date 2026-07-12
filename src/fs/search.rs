use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::fs::archive::{ArchiveListEntry, list_7z_entries, list_zip_entries};
use crate::fs::explorer::{self, EntryKind, FileEntry};

const MAX_SEARCH_RESULTS: usize = 20_000;
const SEARCH_BATCH_SIZE: usize = 32;
const SEARCH_BATCH_INTERVAL: Duration = Duration::from_millis(40);
const SEARCH_YIELD_INTERVAL: usize = 128;
const SEARCH_COOPERATIVE_PAUSE: Duration = Duration::from_millis(1);

#[derive(Clone, Debug)]
pub struct SearchOptions {
    pub root: PathBuf,
    pub query: String,
    pub show_hidden: bool,
    pub include_archives: bool,
}

#[derive(Clone, Debug)]
pub struct SearchOutput {
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub enum SearchEvent {
    Batch(Vec<FileEntry>),
    Finished { truncated: bool },
}

pub fn search_files_streaming<F>(
    options: SearchOptions,
    cancelled: &AtomicBool,
    mut on_batch: F,
) -> SearchOutput
where
    F: FnMut(Vec<FileEntry>) -> bool,
{
    let matcher = NameMatcher::new(&options.query);
    if matcher.is_empty() {
        return SearchOutput { truncated: false };
    }

    if explorer::is_portable_path(&options.root) {
        return search_portable_files(options, &matcher, cancelled, on_batch);
    }

    let mut batch = Vec::new();
    let mut pending = VecDeque::from([options.root]);
    let mut matched_count = 0;
    let mut truncated = false;
    let mut last_batch = Instant::now();
    let mut scanned_items = 0usize;

    while let Some(folder) = pending.pop_front() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }

        let Ok(read_dir) = fs::read_dir(&folder) else {
            continue;
        };

        for item in read_dir {
            scanned_items = scanned_items.saturating_add(1);
            if scanned_items.is_multiple_of(SEARCH_YIELD_INTERVAL) {
                std::thread::sleep(SEARCH_COOPERATIVE_PAUSE);
            }
            if cancelled.load(Ordering::Relaxed) {
                break;
            }

            let Ok(item) = item else {
                continue;
            };

            let path = item.path();
            let name = item.file_name().to_string_lossy().to_string();
            let Ok(file_type) = item.file_type() else {
                continue;
            };
            let kind = if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Folder
            } else if file_type.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            let matches = matcher.matches(&name);
            let needs_metadata = matches || (file_type.is_dir() && !options.show_hidden);
            let metadata = if needs_metadata {
                fs::symlink_metadata(&path).ok()
            } else {
                None
            };
            let hidden = metadata
                .as_ref()
                .is_some_and(|metadata| is_hidden_entry(metadata, &name))
                || name.starts_with('.');
            if hidden && !options.show_hidden {
                continue;
            }

            if file_type.is_dir() && !file_type.is_symlink() {
                pending.push_back(path.clone());
            }

            if matches {
                let Some(metadata) = metadata.or_else(|| fs::symlink_metadata(&path).ok()) else {
                    continue;
                };
                let category = crate::fs::explorer::classify_file_category(&path);
                let entry = FileEntry {
                    name,
                    path: path.clone(),
                    kind,
                    category,
                    drive_kind: None,
                    file_system: String::new(),
                    free_space: None,
                    size: if metadata.is_file() {
                        Some(metadata.len())
                    } else {
                        None
                    },
                    percent_full: None,
                    modified: metadata.modified().ok().map(format_system_time),
                    created: metadata.created().ok().map(format_system_time),
                    is_hidden: hidden,
                };
                if !push_search_entry(
                    entry,
                    &mut batch,
                    &mut matched_count,
                    &mut truncated,
                    &mut last_batch,
                    &mut on_batch,
                ) {
                    pending.clear();
                    break;
                }
            }

            if options.include_archives
                && file_type.is_file()
                && crate::fs::archive_listing::has_browsable_archive_extension(&path)
                && !search_archive_entries(
                    &path,
                    &matcher,
                    cancelled,
                    &mut batch,
                    &mut matched_count,
                    &mut truncated,
                    &mut last_batch,
                    &mut on_batch,
                )
            {
                pending.clear();
                break;
            }
        }
    }

    flush_search_batch(&mut batch, &mut last_batch, &mut on_batch);

    SearchOutput { truncated }
}

fn search_archive_entries<F>(
    archive_path: &Path,
    matcher: &NameMatcher,
    cancelled: &AtomicBool,
    batch: &mut Vec<FileEntry>,
    matched_count: &mut usize,
    truncated: &mut bool,
    last_batch: &mut Instant,
    on_batch: &mut F,
) -> bool
where
    F: FnMut(Vec<FileEntry>) -> bool,
{
    let entries = match list_archive_entries(archive_path) {
        Ok(entries) => entries,
        Err(error) => {
            crate::utils::log::error(format!(
                "Archive search listing failed for {}: {error}",
                archive_path.display()
            ));
            return true;
        }
    };

    for (index, entry) in entries.into_iter().enumerate() {
        if index > 0 && index.is_multiple_of(SEARCH_YIELD_INTERVAL) {
            std::thread::sleep(SEARCH_COOPERATIVE_PAUSE);
        }
        if cancelled.load(Ordering::Relaxed) {
            return false;
        }

        let internal_name = entry.name.replace('\\', "/");
        let Some(name) = archive_entry_display_name(&internal_name) else {
            continue;
        };

        if !matcher.matches(&name) && !matcher.matches(&internal_name) {
            continue;
        }

        let virtual_path = archive_virtual_path(archive_path, &internal_name);
        let kind = if entry.is_dir {
            EntryKind::Folder
        } else {
            EntryKind::File
        };
        let category = if entry.is_dir {
            explorer::FileCategory::Other
        } else {
            explorer::classify_file_category(Path::new(&name))
        };
        let modified = entry.modified.map(format_system_time);
        let result = FileEntry {
            name,
            path: virtual_path,
            kind,
            category,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: if entry.is_dir { None } else { entry.size },
            percent_full: None,
            modified,
            created: None,
            is_hidden: false,
        };

        if !push_search_entry(
            result,
            batch,
            matched_count,
            truncated,
            last_batch,
            on_batch,
        ) {
            return false;
        }
        if *truncated {
            return false;
        }
    }

    true
}

fn list_archive_entries(path: &Path) -> crate::utils::errors::Result<Vec<ArchiveListEntry>> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        list_zip_entries(path)
    } else {
        list_7z_entries(path)
    }
}

fn archive_entry_display_name(name: &str) -> Option<String> {
    name.trim_matches('/')
        .rsplit('/')
        .find(|part| !part.trim().is_empty())
        .map(|part| part.to_string())
}

fn archive_virtual_path(archive_path: &Path, internal_name: &str) -> PathBuf {
    let mut path = archive_path.to_path_buf();
    for segment in internal_name
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        path.push(segment);
    }
    path
}

fn push_search_entry<F>(
    entry: FileEntry,
    batch: &mut Vec<FileEntry>,
    matched_count: &mut usize,
    truncated: &mut bool,
    last_batch: &mut Instant,
    on_batch: &mut F,
) -> bool
where
    F: FnMut(Vec<FileEntry>) -> bool,
{
    *matched_count += 1;
    batch.push(entry);

    if *matched_count >= MAX_SEARCH_RESULTS {
        *truncated = true;
        return flush_search_batch(batch, last_batch, on_batch);
    }
    if batch.len() >= SEARCH_BATCH_SIZE || last_batch.elapsed() >= SEARCH_BATCH_INTERVAL {
        return flush_search_batch(batch, last_batch, on_batch);
    }
    true
}

fn search_portable_files<F>(
    options: SearchOptions,
    matcher: &NameMatcher,
    cancelled: &AtomicBool,
    mut on_batch: F,
) -> SearchOutput
where
    F: FnMut(Vec<FileEntry>) -> bool,
{
    let mut batch = Vec::new();
    let mut pending = VecDeque::from([options.root]);
    let mut matched_count = 0;
    let mut truncated = false;
    let mut last_batch = Instant::now();
    let mut scanned_items = 0usize;

    while let Some(folder) = pending.pop_front() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }

        let Ok(children) = explorer::list_entries(Some(&folder), options.show_hidden) else {
            continue;
        };

        for entry in children {
            scanned_items = scanned_items.saturating_add(1);
            if scanned_items.is_multiple_of(SEARCH_YIELD_INTERVAL) {
                std::thread::sleep(SEARCH_COOPERATIVE_PAUSE);
            }
            if cancelled.load(Ordering::Relaxed) {
                break;
            }

            if entry.is_hidden && !options.show_hidden {
                continue;
            }

            if entry.kind == EntryKind::Folder {
                pending.push_back(entry.path.clone());
            }

            if matcher.matches(&entry.name) {
                matched_count += 1;
                batch.push(entry);
                if matched_count >= MAX_SEARCH_RESULTS {
                    truncated = true;
                    flush_search_batch(&mut batch, &mut last_batch, &mut on_batch);
                    pending.clear();
                    break;
                }
                if batch.len() >= SEARCH_BATCH_SIZE || last_batch.elapsed() >= SEARCH_BATCH_INTERVAL
                {
                    if !flush_search_batch(&mut batch, &mut last_batch, &mut on_batch) {
                        pending.clear();
                        break;
                    }
                }
            }
        }
    }

    flush_search_batch(&mut batch, &mut last_batch, &mut on_batch);

    SearchOutput { truncated }
}

fn flush_search_batch<F>(
    batch: &mut Vec<FileEntry>,
    last_batch: &mut Instant,
    on_batch: &mut F,
) -> bool
where
    F: FnMut(Vec<FileEntry>) -> bool,
{
    if batch.is_empty() {
        return true;
    }
    *last_batch = Instant::now();
    on_batch(std::mem::take(batch))
}

#[derive(Clone, Debug)]
pub(crate) struct NameMatcher {
    pattern: MatchPattern,
}

#[derive(Clone, Debug)]
enum MatchPattern {
    Empty,
    Contains(String),
    Extension(String),
    Wildcard(Vec<char>),
}

impl NameMatcher {
    pub(crate) fn new(query: &str) -> Self {
        let query = query.trim().to_lowercase();
        let pattern = if query.is_empty() {
            MatchPattern::Empty
        } else if let Some(extension) = extension_wildcard(&query) {
            MatchPattern::Extension(extension)
        } else if query.contains('*') || query.contains('?') {
            MatchPattern::Wildcard(query.chars().collect())
        } else {
            MatchPattern::Contains(query)
        };
        Self { pattern }
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self.pattern, MatchPattern::Empty)
    }

    pub(crate) fn matches(&self, name: &str) -> bool {
        match &self.pattern {
            MatchPattern::Empty => true,
            MatchPattern::Contains(query) => name.to_lowercase().contains(query),
            MatchPattern::Extension(extension) => file_extension_matches(name, extension),
            MatchPattern::Wildcard(pattern) => wildcard_match(pattern, &name.to_lowercase()),
        }
    }
}

fn extension_wildcard(query: &str) -> Option<String> {
    let extension = query
        .strip_prefix("*.*")
        .or_else(|| query.strip_prefix("*."))?;
    if extension.is_empty()
        || extension
            .chars()
            .any(|ch| matches!(ch, '*' | '?' | '.' | '/' | '\\'))
    {
        return None;
    }
    Some(extension.to_string())
}

fn file_extension_matches(name: &str, expected: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn wildcard_match(pattern: &[char], text: &str) -> bool {
    let text = text.chars().collect::<Vec<_>>();
    let (mut pattern_index, mut text_index) = (0, 0);
    let mut star_index = None;
    let mut star_text_index = 0;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            star_text_index = text_index;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_text_index += 1;
            text_index = star_text_index;
        } else {
            return false;
        }
    }

    pattern[pattern_index..].iter().all(|ch| *ch == '*')
}

fn format_system_time(time: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

#[cfg(target_os = "windows")]
fn is_hidden_entry(metadata: &fs::Metadata, name: &str) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    name.starts_with('.') || metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(target_os = "windows"))]
fn is_hidden_entry(_metadata: &fs::Metadata, name: &str) -> bool {
    name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{NameMatcher, SearchOptions, search_files_streaming};

    fn name_matches_query(name: &str, query: &str) -> bool {
        NameMatcher::new(query).matches(name)
    }

    fn temp_test_dir(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bexplorer-search-{name}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn matches_extension_wildcards() {
        assert!(name_matches_query("setup.exe", "*.exe"));
        assert!(name_matches_query("setup.exe", "*.*exe"));
        assert!(name_matches_query("SETUP.EXE", "*.exe"));
        assert!(!name_matches_query("setup.exe.bak", "*.exe"));
        assert!(name_matches_query("photo.JPG", "*.jpg"));
        assert!(name_matches_query("photo.JPG", "*.*jpg"));
        assert!(name_matches_query("my.photo.jpeg", "*.jpeg"));
        assert!(!name_matches_query("photo.jpg.bak", "*.jpg"));
    }

    #[test]
    fn complete_search_finds_files_inside_zip_archives() {
        let root = temp_test_dir("archive");
        let source = root.join("inside_target.txt");
        let archive = root.join("packed.zip");
        std::fs::write(&source, b"inside").expect("write source file");
        crate::fs::archive::compress(
            std::slice::from_ref(&source),
            &archive,
            crate::fs::archive::ArchiveFormat::Zip,
        )
        .expect("create zip archive");
        std::fs::remove_file(&source).expect("remove source file");

        let cancelled = AtomicBool::new(false);
        let mut quick_results = Vec::new();
        search_files_streaming(
            SearchOptions {
                root: root.clone(),
                query: "inside_target.txt".into(),
                show_hidden: true,
                include_archives: false,
            },
            &cancelled,
            |entries| {
                quick_results.extend(entries);
                true
            },
        );
        assert!(quick_results.is_empty());

        let mut complete_results = Vec::new();
        search_files_streaming(
            SearchOptions {
                root: root.clone(),
                query: "inside_target.txt".into(),
                show_hidden: true,
                include_archives: true,
            },
            &cancelled,
            |entries| {
                complete_results.extend(entries);
                true
            },
        );

        assert!(complete_results.iter().any(|entry| {
            entry.name == "inside_target.txt" && entry.path.starts_with(&archive)
        }));

        let _ = std::fs::remove_dir_all(root);
    }
}

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::fs::archive::list_archive_entries_cancellable;
use crate::fs::explorer::{self, EntryKind, FileEntry};

const MAX_SEARCH_RESULTS: usize = 20_000;
const SEARCH_BATCH_SIZE: usize = 256;
const SEARCH_BATCH_INTERVAL: Duration = Duration::from_millis(120);
const SEARCH_YIELD_INTERVAL: usize = 128;
const SEARCH_COOPERATIVE_PAUSE: Duration = Duration::from_millis(1);
const MAX_TEXT_SEARCH_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
    Names,
    NamesAndArchives,
    TextContents,
}

#[derive(Clone, Debug)]
pub struct SearchOptions {
    pub root: PathBuf,
    pub query: String,
    pub show_hidden: bool,
    pub scope: SearchScope,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOutput {
    pub truncated: bool,
    pub unavailable: usize,
    pub oversized_files: usize,
}

#[derive(Clone, Debug)]
pub enum SearchEvent {
    Batch(Vec<FileEntry>),
    Finished(SearchOutput),
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
        return SearchOutput::default();
    }

    let mut output = SearchOutput::default();
    if explorer::is_portable_path(&options.root) {
        if options.scope == SearchScope::TextContents {
            output.unavailable = 1;
            return output;
        }
        return search_portable_files(options, &matcher, cancelled, on_batch);
    }

    let mut content =
        (options.scope == SearchScope::TextContents).then(|| TextSearch::new(&options.query));
    let mut batch = Vec::new();
    let mut pending = VecDeque::from([options.root]);
    let mut matched_count = 0;
    let mut last_batch = Instant::now();
    let mut scanned_items = 0usize;
    let mut visited = HashSet::new();

    while let Some(folder) = pending.pop_front() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        // Junctions can appear as directories on Windows. Track physical
        // directory identity so they cannot create a recursive search cycle.
        let Ok(identity) = crate::fs::transfer_queue::path_identity(&folder) else {
            output.unavailable += 1;
            continue;
        };
        if !visited.insert(identity) {
            continue;
        }

        let Ok(read_dir) = fs::read_dir(&folder) else {
            output.unavailable += 1;
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
                output.unavailable += 1;
                continue;
            };

            let path = item.path();
            let name = item.file_name().to_string_lossy().to_string();
            let Ok(file_type) = item.file_type() else {
                output.unavailable += 1;
                continue;
            };
            let kind = if file_type.is_symlink() {
                crate::fs::explorer::symlink_target_kind(&path)
            } else if file_type.is_dir() {
                EntryKind::Folder
            } else if file_type.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            let name_matches = content.is_none() && matcher.matches(&name);
            let search_content = content.is_some() && file_type.is_file();
            let needs_metadata =
                name_matches || search_content || (file_type.is_dir() && !options.show_hidden);
            let metadata = match needs_metadata.then(|| item.metadata()) {
                Some(Ok(metadata)) => Some(metadata),
                Some(Err(_)) => {
                    output.unavailable += 1;
                    continue;
                }
                None => None,
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

            let mut matches = name_matches;
            if let (Some(content), Some(metadata)) = (&mut content, &metadata)
                && search_content
            {
                match content.check(&path, metadata) {
                    Ok(TextMatch::Matched) => matches = true,
                    Ok(TextMatch::Oversized) => output.oversized_files += 1,
                    Ok(TextMatch::NoMatch) => {}
                    Err(_) => output.unavailable += 1,
                }
            }

            if matches && let Some(metadata) = metadata {
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
                    modified_time: metadata.modified().ok(),
                    modified: metadata.modified().ok().map(format_system_time),
                    created: metadata.created().ok().map(format_system_time),
                    is_hidden: hidden,
                };
                if !push_search_entry(
                    entry,
                    &mut batch,
                    &mut matched_count,
                    &mut output.truncated,
                    &mut last_batch,
                    &mut on_batch,
                ) {
                    pending.clear();
                    break;
                }
            }

            if options.scope == SearchScope::NamesAndArchives
                && file_type.is_file()
                && crate::fs::archive_listing::has_browsable_archive_extension(&path)
                && !search_archive_entries(
                    &path,
                    &mut ArchiveSearchContext {
                        matcher: &matcher,
                        cancelled,
                        batch: &mut batch,
                        matched_count: &mut matched_count,
                        output: &mut output,
                        last_batch: &mut last_batch,
                        on_batch: &mut on_batch,
                    },
                )
            {
                pending.clear();
                break;
            }
        }
    }

    flush_search_batch(&mut batch, &mut last_batch, &mut on_batch);

    output
}

struct ArchiveSearchContext<'a, F> {
    matcher: &'a NameMatcher,
    cancelled: &'a AtomicBool,
    batch: &'a mut Vec<FileEntry>,
    matched_count: &'a mut usize,
    output: &'a mut SearchOutput,
    last_batch: &'a mut Instant,
    on_batch: &'a mut F,
}

fn search_archive_entries<F>(archive_path: &Path, context: &mut ArchiveSearchContext<'_, F>) -> bool
where
    F: FnMut(Vec<FileEntry>) -> bool,
{
    if context.cancelled.load(Ordering::Relaxed) {
        return false;
    }
    let entries = match list_archive_entries_cancellable(archive_path, context.cancelled) {
        Ok(entries) => entries,
        Err(error) => {
            if context.cancelled.load(Ordering::Relaxed) {
                return false;
            }
            context.output.unavailable += 1;
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
        if context.cancelled.load(Ordering::Relaxed) {
            return false;
        }

        let internal_name = crate::fs::archive::normalize_archive_item_name(&entry.name);
        if internal_name.is_empty() {
            continue;
        }
        let Some(name) = archive_entry_display_name(&internal_name) else {
            continue;
        };

        if !context.matcher.matches(&name) && !context.matcher.matches(&internal_name) {
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
            modified_time: entry.modified,
            modified,
            created: None,
            is_hidden: false,
        };

        if !push_search_entry(
            result,
            context.batch,
            context.matched_count,
            &mut context.output.truncated,
            context.last_batch,
            context.on_batch,
        ) {
            return false;
        }
    }

    true
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
    if *matched_count == MAX_SEARCH_RESULTS {
        *truncated = true;
        flush_search_batch(batch, last_batch, on_batch);
        return false;
    }
    *matched_count += 1;
    batch.push(entry);
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
    let mut output = SearchOutput::default();
    let mut last_batch = Instant::now();
    let mut scanned_items = 0usize;
    let mut visited = HashSet::new();

    while let Some(folder) = pending.pop_front() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }

        if !visited.insert(folder.clone()) {
            continue;
        }
        let Ok(children) = explorer::list_entries(Some(&folder), options.show_hidden) else {
            output.unavailable += 1;
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

            if matcher.matches(&entry.name)
                && !push_search_entry(
                    entry,
                    &mut batch,
                    &mut matched_count,
                    &mut output.truncated,
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

    output
}

struct TextSearch {
    query: String,
    buffer: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextMatch {
    Matched,
    NoMatch,
    Oversized,
}

impl TextSearch {
    fn new(query: &str) -> Self {
        Self {
            query: query.trim().to_lowercase(),
            buffer: Vec::new(),
        }
    }

    fn check(&mut self, path: &Path, metadata: &fs::Metadata) -> io::Result<TextMatch> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            const OFFLINE: u32 = 0x1000;
            const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
            if metadata.file_attributes() & (OFFLINE | RECALL_ON_DATA_ACCESS) != 0 {
                return Err(io::Error::other(
                    "Cloud file content is not available locally",
                ));
            }
        }
        if metadata.len() > MAX_TEXT_SEARCH_BYTES {
            return Ok(TextMatch::Oversized);
        }
        self.buffer.clear();
        fs::File::open(path)?
            .take(MAX_TEXT_SEARCH_BYTES + 1)
            .read_to_end(&mut self.buffer)?;
        if self.buffer.len() as u64 > MAX_TEXT_SEARCH_BYTES {
            return Ok(TextMatch::Oversized);
        }

        let matches = if let Some(bytes) = self.buffer.strip_prefix(&[0xff, 0xfe]) {
            self.matches_utf16(bytes, u16::from_le_bytes)
        } else if let Some(bytes) = self.buffer.strip_prefix(&[0xfe, 0xff]) {
            self.matches_utf16(bytes, u16::from_be_bytes)
        } else {
            let bytes = self
                .buffer
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(&self.buffer);
            std::str::from_utf8(bytes).is_ok_and(|text| self.matches_text(text))
        };
        Ok(if matches {
            TextMatch::Matched
        } else {
            TextMatch::NoMatch
        })
    }

    fn matches_utf16(&self, bytes: &[u8], decode: fn([u8; 2]) -> u16) -> bool {
        let (units, remainder) = bytes.as_chunks::<2>();
        remainder.is_empty()
            && String::from_utf16(&units.iter().copied().map(decode).collect::<Vec<_>>())
                .is_ok_and(|text| self.matches_text(&text))
    }

    fn matches_text(&self, text: &str) -> bool {
        !text.contains('\0') && text.to_lowercase().contains(&self.query)
    }
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

    use super::{NameMatcher, SearchOptions, SearchScope, search_files_streaming};

    #[test]
    fn local_result_limit_stops_production_at_the_last_batch() {
        use crate::fs::explorer::{EntryKind, FileCategory, FileEntry};
        let mut batch = Vec::new();
        let mut matched = 0;
        let mut truncated = false;
        let mut last_batch = std::time::Instant::now();
        let mut received = 0;
        let mut accept = |entries: Vec<FileEntry>| {
            assert!(entries.len() <= super::SEARCH_BATCH_SIZE);
            received += entries.len();
            true
        };
        for index in 0..super::MAX_SEARCH_RESULTS + 10 {
            let entry = FileEntry {
                name: format!("match-{index}"),
                path: std::path::PathBuf::from(format!("match-{index}")),
                kind: EntryKind::File,
                category: FileCategory::Other,
                drive_kind: None,
                file_system: String::new(),
                free_space: None,
                size: Some(1),
                percent_full: None,
                modified_time: None,
                modified: None,
                created: None,
                is_hidden: false,
            };
            let more = super::push_search_entry(
                entry,
                &mut batch,
                &mut matched,
                &mut truncated,
                &mut last_batch,
                &mut accept,
            );
            if index + 1 == super::MAX_SEARCH_RESULTS {
                assert!(
                    !truncated,
                    "an exact result count is not a truncated search"
                );
            }
            if !more {
                break;
            }
        }
        assert!(truncated);
        assert_eq!(matched, super::MAX_SEARCH_RESULTS);
        assert_eq!(received, super::MAX_SEARCH_RESULTS);
        assert!(batch.is_empty());
    }

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
                scope: SearchScope::Names,
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
                scope: SearchScope::NamesAndArchives,
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

    #[test]
    fn content_search_matches_text_but_not_filenames_or_binary_data() {
        let root = temp_test_dir("text");
        std::fs::write(root.join("notes"), "La versión SIGUIENTE").unwrap();
        let mut utf16 = vec![0xff, 0xfe];
        utf16.extend(
            "La versión siguiente"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        std::fs::write(root.join("wide.log"), utf16).unwrap();
        std::fs::write(root.join("versión siguiente.txt"), "different content").unwrap();
        std::fs::write(root.join("binary"), "\0versión siguiente").unwrap();
        std::fs::write(root.join(".hidden"), "versión siguiente").unwrap();
        std::fs::File::create(root.join("oversized.txt"))
            .unwrap()
            .set_len(super::MAX_TEXT_SEARCH_BYTES + 1)
            .unwrap();

        let mut names = Vec::new();
        let output = search_files_streaming(
            SearchOptions {
                root: root.clone(),
                query: "VERSIÓN siguiente".into(),
                show_hidden: false,
                scope: SearchScope::TextContents,
            },
            &AtomicBool::new(false),
            |entries| {
                names.extend(entries.into_iter().map(|entry| entry.name));
                true
            },
        );
        std::fs::remove_dir_all(root).unwrap();
        names.sort();
        assert_eq!(names, ["notes", "wide.log"]);
        assert_eq!(output.oversized_files, 1);
        assert_eq!(output.unavailable, 0);
        assert!(!output.truncated);
    }

    #[test]
    fn a_missing_search_root_is_reported_as_unavailable() {
        let root = temp_test_dir("missing");
        let output = search_files_streaming(
            SearchOptions {
                root: root.join("missing"),
                query: "match".into(),
                show_hidden: false,
                scope: SearchScope::Names,
            },
            &AtomicBool::new(false),
            |_| panic!("a missing folder cannot produce a search result"),
        );
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(output.unavailable, 1);
    }

    #[test]
    #[ignore = "creates 10,000 and 100,000 local fixtures, or reads BEXPLORER_BENCH_DIRECTORY"]
    fn benchmark_listing_and_search() {
        use std::path::{Path, PathBuf};
        use std::time::Instant;

        fn measure(root: &Path, query: Option<&str>, expected_entries: Option<usize>) {
            for run in 1..=3 {
                let started = Instant::now();
                let entries = crate::fs::explorer::list_entries(Some(root), false).unwrap();
                let listing_ms = started.elapsed().as_millis();
                let count = entries.len();
                if let Some(expected_entries) = expected_entries {
                    assert_eq!(count, expected_entries);
                }
                drop(entries);

                let Some(query) = query else {
                    println!("{root:?}\trun={run}\tentries={count}\tlisting_ms={listing_ms}");
                    continue;
                };

                let started = Instant::now();
                let mut first_batch_ms = None;
                let mut matches = 0;
                let output = search_files_streaming(
                    SearchOptions {
                        root: root.to_path_buf(),
                        query: query.into(),
                        show_hidden: false,
                        scope: SearchScope::Names,
                    },
                    &AtomicBool::new(false),
                    |entries| {
                        first_batch_ms.get_or_insert_with(|| started.elapsed().as_millis());
                        matches += entries.len();
                        true
                    },
                );
                let search_ms = started.elapsed().as_millis();
                if let Some(expected_entries) = expected_entries {
                    assert_eq!(matches, expected_entries / 100);
                    assert_eq!(output, super::SearchOutput::default());
                }
                println!(
                    "{root:?}\trun={run}\tentries={count}\tlisting_ms={listing_ms}\tfirst_batch_ms={}\tsearch_ms={search_ms}\tmatches={matches}\tunavailable={}",
                    first_batch_ms.map_or_else(|| "-".into(), |ms| ms.to_string()),
                    output.unavailable,
                );
            }
        }

        if let Some(root) = std::env::var_os("BEXPLORER_BENCH_DIRECTORY") {
            let query = std::env::var("BEXPLORER_BENCH_QUERY").unwrap_or_else(|_| "*".into());
            let list_only = std::env::var_os("BEXPLORER_BENCH_LIST_ONLY").is_some();
            measure(
                Path::new(&root),
                (!list_only).then_some(query.as_str()),
                None,
            );
            return;
        }

        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(temp_test_dir("benchmark"));
        for count in [10_000, 100_000] {
            let folder = fixture.0.join(count.to_string());
            std::fs::create_dir(&folder).unwrap();
            for index in 0..count {
                let label = if index % 100 == 0 { "match" } else { "entry" };
                std::fs::File::create(folder.join(format!("{label}-{index:06}.txt"))).unwrap();
            }
            measure(&folder, Some("match-"), Some(count));
        }
    }
}

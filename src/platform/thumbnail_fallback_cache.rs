#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

use image::{ImageEncoder, ImageFormat};
use walkdir::WalkDir;

use crate::platform::NativeIconImage;

const CACHE_VERSION: &str = "v2";
const LEGACY_CACHE_VERSION: &str = "v1";
const CACHE_BUCKETS: [u32; 4] = [128, 256, 512, 1024];
const MAX_CACHED_PNG_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODE_ALLOC_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
// Tiny thumbnails also need an entry-count bound so cleanup bookkeeping does
// not grow with an unbounded number of small cache files.
const MAX_CACHE_FILES: usize = 32_768;
const MAX_CACHE_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const MAX_TEMP_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const CLEANUP_INTERVAL_SAVES: u64 = 64;
const TEMP_FILE_MARKER: &str = ".bexplorer-cache-";

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceIdentity {
    path_hash: String,
    file_name: String,
    size: u64,
    modified: std::time::SystemTime,
}

static ACTIVE_CACHE_FILES: OnceLock<Mutex<HashMap<PathBuf, usize>>> = OnceLock::new();

struct ActiveCacheFile(PathBuf);

impl ActiveCacheFile {
    fn acquire(path: &Path) -> Self {
        let mut active = ACTIVE_CACHE_FILES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active.entry(path.to_path_buf()).or_default() += 1;
        Self(path.to_path_buf())
    }
}

impl Drop for ActiveCacheFile {
    fn drop(&mut self) {
        let mut active = ACTIVE_CACHE_FILES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = active.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                active.remove(&self.0);
            }
        }
    }
}

fn remove_unused_cache_file(path: &Path) -> bool {
    // Hold the small registry lock through removal so a reader cannot start
    // between the activity check and deleting its cached PNG.
    let active = ACTIVE_CACHE_FILES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.contains_key(path) {
        return false;
    }
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

pub(crate) fn load(path: &Path, size: u32) -> Option<NativeIconImage> {
    let root = cache_root()?;
    load_in(&root, path, size)
}

pub(crate) fn save_checked(
    path: &Path,
    size: u32,
    image: &NativeIconImage,
    expected: (u64, std::time::SystemTime),
) -> bool {
    let Some(root) = cache_root() else {
        return false;
    };
    let Some(identity) = source_identity(path) else {
        return false;
    };
    if (identity.size, identity.modified) != expected {
        return false;
    }
    let saved = save_identity_in(&root, path, size, image, &identity);
    if saved {
        schedule_cleanup(root);
    }
    saved
}

fn cache_root() -> Option<PathBuf> {
    crate::utils::paths::cache_dir()
        .ok()
        .map(|directory| directory.join("thumbnails"))
}

fn load_in(root: &Path, path: &Path, size: u32) -> Option<NativeIconImage> {
    let identity = source_identity(path)?;
    for bucket in preferred_buckets(size) {
        for cached in [
            cache_destination(root, *bucket, &identity),
            root.join(LEGACY_CACHE_VERSION)
                .join(bucket.to_string())
                .join(&identity.file_name),
        ] {
            let _active = ActiveCacheFile::acquire(&cached);
            let Ok(metadata) = fs::metadata(&cached) else {
                continue;
            };
            if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CACHED_PNG_BYTES {
                continue;
            }
            if let Some(image) = decode_png(&cached, size) {
                return Some(image);
            }
        }
    }
    None
}

#[cfg(test)]
fn save_in(root: &Path, path: &Path, size: u32, image: &NativeIconImage) -> bool {
    let Some(identity) = source_identity(path) else {
        return false;
    };
    save_identity_in(root, path, size, image, &identity)
}

fn save_identity_in(
    root: &Path,
    path: &Path,
    size: u32,
    image: &NativeIconImage,
    identity: &SourceIdentity,
) -> bool {
    let bucket = cache_bucket(size);
    let Some(png) = encode_png(image, bucket) else {
        return false;
    };
    if png.len() as u64 > MAX_CACHED_PNG_BYTES {
        return false;
    }
    if source_identity(path).as_ref() != Some(identity) {
        return false;
    }

    // Use the captured identity even if the source changes during the atomic
    // write: an older decode must never be tagged as a newer source version.
    let destination = cache_destination(root, bucket, identity);
    let _active = ActiveCacheFile::acquire(&destination);
    let Some(directory) = destination.parent() else {
        return false;
    };
    if fs::create_dir_all(directory).is_err() {
        return false;
    }
    if !write_cache_file(&destination, &png) {
        return false;
    }
    // Never scan a bucket for each save. Superseded source versions are
    // collected by the one asynchronous cleanup worker instead.
    true
}

fn cache_destination(root: &Path, bucket: u32, identity: &SourceIdentity) -> PathBuf {
    root.join(CACHE_VERSION)
        .join(bucket.to_string())
        .join(&identity.path_hash[..2])
        .join(&identity.file_name)
}

fn write_cache_file(destination: &Path, bytes: &[u8]) -> bool {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

    let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("thumbnail.png");
    let temporary = destination.with_file_name(format!(
        ".{name}{TEMP_FILE_MARKER}{}-{sequence}",
        std::process::id()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        drop(file);
        replace_cache_file(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.is_ok()
}

#[cfg(target_os = "windows")]
fn replace_cache_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
    use windows::core::PCWSTR;

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING,
        )
        .map_err(std::io::Error::from)
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_cache_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

fn source_identity(path: &Path) -> Option<SourceIdentity> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified_time = metadata.modified().ok()?;
    let modified = modified_time.duration_since(UNIX_EPOCH).ok()?;
    let resolved = fs::canonicalize(path)
        .ok()
        .or_else(|| absolute_path(path))?;
    let path_hash = format!("{:x}", md5::compute(path_identity_bytes(&resolved)));
    let file_name = format!(
        "{path_hash}-{:016x}-{:016x}-{:08x}.png",
        metadata.len(),
        modified.as_secs(),
        modified.subsec_nanos()
    );
    Some(SourceIdentity {
        path_hash,
        file_name,
        size: metadata.len(),
        modified: modified_time,
    })
}

fn absolute_path(path: &Path) -> Option<PathBuf> {
    path.is_absolute().then(|| path.to_path_buf()).or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|directory| directory.join(path))
    })
}

#[cfg(target_os = "windows")]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(unix)]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(any(target_os = "windows", unix)))]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

fn cache_bucket(size: u32) -> u32 {
    CACHE_BUCKETS
        .into_iter()
        .find(|bucket| size <= *bucket)
        .unwrap_or(1024)
}

fn preferred_buckets(size: u32) -> &'static [u32] {
    match cache_bucket(size) {
        128 => &[128, 256, 512, 1024],
        256 => &[256, 512, 1024],
        512 => &[512, 1024],
        _ => &[1024],
    }
}

fn encode_png(image: &NativeIconImage, max_edge: u32) -> Option<Vec<u8>> {
    let width = u32::try_from(image.width).ok()?;
    let height = u32::try_from(image.height).ok()?;
    let expected = image.width.checked_mul(image.height)?.checked_mul(4)?;
    if width == 0 || height == 0 || image.rgba.len() != expected {
        return None;
    }

    let resized;
    let (rgba, width, height) = if width.max(height) > max_edge {
        let source = image::RgbaImage::from_raw(width, height, image.rgba.clone())?;
        resized = image::DynamicImage::ImageRgba8(source)
            .resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        (
            resized.as_raw().as_slice(),
            resized.width(),
            resized.height(),
        )
    } else {
        (image.rgba.as_slice(), width, height)
    };

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(png)
}

fn decode_png(path: &Path, max_edge: u32) -> Option<NativeIconImage> {
    let file = fs::File::open(path).ok()?;
    let mut reader = image::ImageReader::with_format(BufReader::new(file), ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let max_edge = max_edge.clamp(1, 1024);
    let image = if image.width().max(image.height()) > max_edge {
        image.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
    } else {
        image
    }
    .to_rgba8();
    Some(NativeIconImage {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.into_raw(),
    })
}

fn schedule_cleanup(root: PathBuf) {
    static SAVE_COUNT: AtomicU64 = AtomicU64::new(0);
    static CLEANUP_RUNNING: AtomicBool = AtomicBool::new(false);

    let count = SAVE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if count != 1 && !count.is_multiple_of(CLEANUP_INTERVAL_SAVES) {
        return;
    }
    if CLEANUP_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let result = std::thread::Builder::new()
        .name("bexplorer-thumbnail-cache-cleanup".into())
        .spawn(move || {
            prune_cache(&root, MAX_CACHE_BYTES, MAX_CACHE_AGE);
            CLEANUP_RUNNING.store(false, Ordering::Release);
        });
    if result.is_err() {
        CLEANUP_RUNNING.store(false, Ordering::Release);
    }
}

fn prune_cache(root: &Path, max_bytes: u64, max_age: Duration) {
    prune_cache_with_limit(root, max_bytes, max_age, MAX_CACHE_FILES);
}

fn cache_source_key(root: &Path, path: &Path) -> Option<(u32, String)> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = relative.components();
    let version = parts.next()?.as_os_str();
    if version != CACHE_VERSION && version != LEGACY_CACHE_VERSION {
        return None;
    }
    let bucket = parts.next()?.as_os_str().to_str()?.parse().ok()?;
    if !CACHE_BUCKETS.contains(&bucket) {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    let (hash, _) = name.split_once('-')?;
    (hash.len() == 32 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| (bucket, hash.to_owned()))
}

fn prune_cache_with_limit(root: &Path, max_bytes: u64, max_age: Duration, max_files: usize) {
    let now = std::time::SystemTime::now();
    // Retain only the newest entries that fit the shared v1/v2 budget. These
    // structures stay bounded even if a previous version left a huge cache.
    let mut entries = BTreeSet::<(std::time::SystemTime, PathBuf, u64)>::new();
    let mut sources = HashMap::<(u32, String), (std::time::SystemTime, PathBuf, u64)>::new();
    let mut total = 0_u64;
    for entry in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let age = now.duration_since(modified).ok();
        if entry
            .file_name()
            .to_string_lossy()
            .contains(TEMP_FILE_MARKER)
        {
            if age.is_some_and(|age| age > MAX_TEMP_FILE_AGE) {
                remove_unused_cache_file(entry.path());
            }
            continue;
        }
        if entry.path().extension().is_none_or(|value| value != "png") {
            continue;
        }
        if age.is_some_and(|age| age > max_age) {
            remove_unused_cache_file(entry.path());
            continue;
        }
        let candidate = (modified, entry.path().to_path_buf(), metadata.len());
        if let Some(key) = cache_source_key(root, entry.path()) {
            let mut newest = true;
            if let Some(previous) = sources.get(&key) {
                if (candidate.0, &candidate.1) <= (previous.0, &previous.1) {
                    if remove_unused_cache_file(entry.path()) {
                        continue;
                    }
                    newest = false;
                } else if remove_unused_cache_file(&previous.1) {
                    total = total.saturating_sub(previous.2);
                    entries.remove(previous);
                }
            }
            if newest {
                sources.insert(key, candidate.clone());
            }
        }
        total = total.saturating_add(candidate.2);
        entries.insert(candidate);
        while total > max_bytes || entries.len() > max_files {
            let Some(oldest) = entries.pop_first() else {
                break;
            };
            if let Some(key) = cache_source_key(root, &oldest.1)
                && sources.get(&key).is_some_and(|entry| entry.1 == oldest.1)
            {
                sources.remove(&key);
            }
            if remove_unused_cache_file(&oldest.1) {
                total = total.saturating_sub(oldest.2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_root(name: &str) -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "bexplorer-thumbnail-fallback-{name}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn solid_image(width: usize, height: usize, rgba: [u8; 4]) -> NativeIconImage {
        NativeIconImage {
            rgba: rgba.repeat(width * height),
            width,
            height,
        }
    }

    #[test]
    fn fallback_thumbnail_round_trips_and_invalidates_with_the_source() {
        let root = test_root("roundtrip");
        fs::create_dir_all(&root).expect("create cache test root");
        let source = root.join("photo.custom");
        fs::write(&source, b"original").expect("write source");
        let image = solid_image(160, 90, [25, 90, 180, 255]);

        assert!(save_in(&root, &source, 256, &image));
        let loaded = load_in(&root, &source, 48).expect("load cached thumbnail");
        assert_eq!((loaded.width, loaded.height), (48, 27));
        assert_eq!(&loaded.rgba[..4], &[25, 90, 180, 255]);

        fs::write(&source, b"modified source").expect("modify source");
        assert!(load_in(&root, &source, 48).is_none());

        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn fallback_cache_prefers_the_closest_standard_size() {
        let root = test_root("sizes");
        fs::create_dir_all(&root).expect("create cache test root");
        let source = root.join("photo.custom");
        fs::write(&source, b"source").expect("write source");
        assert!(save_in(
            &root,
            &source,
            128,
            &solid_image(128, 72, [210, 35, 20, 255])
        ));
        assert!(save_in(
            &root,
            &source,
            256,
            &solid_image(256, 144, [20, 80, 220, 255])
        ));

        let small = load_in(&root, &source, 48).expect("load normal cache size");
        let standard = load_in(&root, &source, 256).expect("load large cache size");
        assert_eq!(&small.rgba[..4], &[210, 35, 20, 255]);
        assert_eq!(&standard.rgba[..4], &[20, 80, 220, 255]);

        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn fallback_cache_does_not_use_a_smaller_thumbnail_for_a_large_preview() {
        let root = test_root("no-upscale");
        fs::create_dir_all(&root).expect("create cache test root");
        let source = root.join("photo.custom");
        fs::write(&source, b"source").expect("write source");
        assert!(save_in(
            &root,
            &source,
            256,
            &solid_image(256, 144, [20, 80, 220, 255])
        ));

        assert!(load_in(&root, &source, 1024).is_none());

        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn fallback_cache_reads_legacy_files_and_prefers_the_new_sharded_copy() {
        let root = test_root("legacy-layout");
        fs::create_dir_all(&root).expect("create cache test root");
        let source = root.join("photo.custom");
        fs::write(&source, b"source").expect("write source");
        let identity = source_identity(&source).expect("source identity");
        let legacy = root
            .join(LEGACY_CACHE_VERSION)
            .join("128")
            .join(&identity.file_name);
        fs::create_dir_all(legacy.parent().unwrap()).expect("create legacy bucket");
        fs::write(
            &legacy,
            encode_png(&solid_image(32, 32, [200, 20, 10, 255]), 128).unwrap(),
        )
        .expect("write legacy thumbnail");
        let loaded = load_in(&root, &source, 32).expect("read existing v1 thumbnail");
        assert_eq!(&loaded.rgba[..4], &[200, 20, 10, 255]);

        assert!(save_in(
            &root,
            &source,
            128,
            &solid_image(32, 32, [10, 30, 220, 255])
        ));
        let destination = cache_destination(&root, 128, &identity);
        assert!(destination.is_file());
        assert_eq!(
            destination.parent().unwrap().file_name().unwrap(),
            &identity.path_hash[..2]
        );
        let loaded = load_in(&root, &source, 32).expect("prefer v2 thumbnail");
        assert_eq!(&loaded.rgba[..4], &[10, 30, 220, 255]);
        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn superseded_versions_are_collected_in_cleanup_instead_of_during_each_save() {
        let root = test_root("superseded");
        fs::create_dir_all(&root).expect("create cache test root");
        let source = root.join("photo.custom");
        fs::write(&source, b"first").expect("write first source");
        let first_identity = source_identity(&source).unwrap();
        assert!(save_in(
            &root,
            &source,
            128,
            &solid_image(8, 8, [200, 20, 10, 255])
        ));
        let first = cache_destination(&root, 128, &first_identity);
        fs::write(&source, b"second version").expect("replace source");
        let second_identity = source_identity(&source).unwrap();
        assert!(save_in(
            &root,
            &source,
            128,
            &solid_image(8, 8, [10, 30, 220, 255])
        ));
        let second = cache_destination(&root, 128, &second_identity);
        assert_ne!(first, second);
        assert!(
            first.is_file(),
            "saving does not enumerate and delete older files"
        );
        fs::OpenOptions::new()
            .write(true)
            .open(&first)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&second)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(2))
            .unwrap();

        prune_cache(&root, MAX_CACHE_BYTES, Duration::MAX);
        assert!(!first.exists());
        assert!(second.is_file());
        assert!(load_in(&root, &source, 8).is_some());
        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn legacy_and_sharded_files_share_one_size_budget_and_a_file_count_limit() {
        let root = test_root("shared-budget");
        let legacy = root.join(LEGACY_CACHE_VERSION).join("128");
        let current = root.join(CACHE_VERSION).join("128").join("ab");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&current).unwrap();
        fs::write(legacy.join("legacy.png"), [1_u8; 4]).unwrap();
        fs::write(current.join("current.png"), [2_u8; 4]).unwrap();
        prune_cache(&root, 5, Duration::MAX);
        let total = WalkDir::new(&root)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.metadata().unwrap().len())
            .sum::<u64>();
        assert!(total <= 5);

        for index in 0..4 {
            fs::write(current.join(format!("tiny-{index}.png")), [index as u8]).unwrap();
        }
        prune_cache_with_limit(&root, 1024, Duration::MAX, 2);
        let count = WalkDir::new(&root)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .count();
        assert!(count <= 2);
        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn cleanup_keeps_files_while_a_decoder_is_reading_them() {
        let root = test_root("active-reader");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("reading.png");
        fs::write(&path, [1_u8; 4]).unwrap();
        let active = ActiveCacheFile::acquire(&path);
        prune_cache(&root, 0, Duration::MAX);
        assert!(path.is_file());
        drop(active);
        prune_cache(&root, 0, Duration::MAX);
        assert!(!path.exists());
        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn a_decode_of_an_older_source_is_not_saved_under_the_replacement_identity() {
        let root = test_root("changed-while-decoding");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.custom");
        fs::write(&source, b"before").unwrap();
        let captured = source_identity(&source).unwrap();
        fs::write(&source, b"replacement contents").unwrap();
        let replacement = source_identity(&source).unwrap();
        assert!(!save_identity_in(
            &root,
            &source,
            128,
            &solid_image(8, 8, [1, 2, 3, 255]),
            &captured
        ));
        assert!(!cache_destination(&root, 128, &captured).exists());
        assert!(!cache_destination(&root, 128, &replacement).exists());
        fs::remove_dir_all(root).expect("cleanup cache test root");
    }

    #[test]
    fn fallback_cache_prunes_to_its_size_budget() {
        let root = test_root("prune-size");
        fs::create_dir_all(&root).expect("create cache test root");
        for index in 0..3 {
            fs::write(root.join(format!("{index}.png")), [index as u8; 4])
                .expect("write cached thumbnail");
        }

        prune_cache(&root, 5, Duration::MAX);

        let remaining_bytes = fs::read_dir(&root)
            .expect("read cache test root")
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| entry.metadata().ok())
            .map(|metadata| metadata.len())
            .sum::<u64>();
        assert!(remaining_bytes <= 5);

        fs::remove_dir_all(root).expect("cleanup cache test root");
    }
}

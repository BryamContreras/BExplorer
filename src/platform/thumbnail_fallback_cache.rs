#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use image::{ImageEncoder, ImageFormat};
use walkdir::WalkDir;

use crate::platform::NativeIconImage;

const CACHE_VERSION: &str = "v1";
const CACHE_BUCKETS: [u32; 4] = [128, 256, 512, 1024];
const MAX_CACHED_PNG_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODE_ALLOC_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CACHE_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const MAX_TEMP_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const CLEANUP_INTERVAL_SAVES: u64 = 64;
const TEMP_FILE_MARKER: &str = ".bexplorer-cache-";

#[derive(Clone, Debug)]
struct SourceIdentity {
    path_hash: String,
    file_name: String,
}

pub(crate) fn load(path: &Path, size: u32) -> Option<NativeIconImage> {
    let root = cache_root()?;
    load_in(&root, path, size)
}

pub(crate) fn save(path: &Path, size: u32, image: &NativeIconImage) -> bool {
    let Some(root) = cache_root() else {
        return false;
    };
    let saved = save_in(&root, path, size, image);
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
        let cached = root
            .join(CACHE_VERSION)
            .join(bucket.to_string())
            .join(&identity.file_name);
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
    None
}

fn save_in(root: &Path, path: &Path, size: u32, image: &NativeIconImage) -> bool {
    let Some(identity) = source_identity(path) else {
        return false;
    };
    let bucket = cache_bucket(size);
    let Some(png) = encode_png(image, bucket) else {
        return false;
    };
    if png.len() as u64 > MAX_CACHED_PNG_BYTES {
        return false;
    }

    let directory = root.join(CACHE_VERSION).join(bucket.to_string());
    if fs::create_dir_all(&directory).is_err() {
        return false;
    }
    let destination = directory.join(&identity.file_name);
    if !write_cache_file(&destination, &png) {
        return false;
    }
    remove_superseded_entries(&directory, &identity.path_hash, &destination);
    true
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
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
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

fn remove_superseded_entries(directory: &Path, path_hash: &str, keep: &Path) {
    let prefix = format!("{path_hash}-");
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let candidate = entry.path();
        if candidate != keep && entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = fs::remove_file(candidate);
        }
    }
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
    let now = std::time::SystemTime::now();
    let mut entries = Vec::new();
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
                let _ = fs::remove_file(entry.path());
            }
            continue;
        }
        if entry.path().extension().is_none_or(|value| value != "png") {
            continue;
        }
        if age.is_some_and(|age| age > max_age) {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        total = total.saturating_add(metadata.len());
        entries.push((modified, metadata.len(), entry.path().to_path_buf()));
    }

    if total <= max_bytes {
        return;
    }
    entries.sort_by_key(|(modified, _, _)| *modified);
    for (_, size, path) in entries {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(path).is_ok() {
            total = total.saturating_sub(size);
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

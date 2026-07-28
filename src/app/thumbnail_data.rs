use std::io::{BufRead, BufReader, Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

#[cfg(not(target_os = "windows"))]
use directories::UserDirs;
use image::ImageDecoder;

#[cfg(not(target_os = "windows"))]
use crate::fs::explorer::DriveKind;
use crate::fs::explorer::{self, EntryKind, FileCategory, FileEntry};
use crate::platform::NativeIconImage;

pub const NATIVE_ICON_SIZE: u32 = 256;
pub const SMALL_ENTRY_IMAGE_SIZE: u32 = 48;
const PREVIEW_MAX_EDGE: u32 = 1200;
const MAX_PDF_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SVG_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CONCURRENT_IMAGE_DECODERS: usize = 2;
#[cfg(target_os = "windows")]
const WINDOWS_PREVIEW_CACHE_EDGE: u32 = 1024;
const PDF_PREVIEW_SCALE: f32 = 1.15;

pub fn is_thumbnail_candidate(entry: &FileEntry) -> bool {
    if !entry.kind.is_file() {
        return false;
    }

    matches!(entry.category, FileCategory::Image | FileCategory::Video)
        || is_pdf_preview_candidate(entry)
}

pub fn is_visual_preview_candidate(entry: &FileEntry) -> bool {
    entry.kind.is_file()
        && (matches!(entry.category, FileCategory::Image) || is_pdf_preview_candidate(entry))
}

pub fn is_pdf_preview_candidate(entry: &FileEntry) -> bool {
    entry.kind.is_file()
        && entry
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

pub fn is_text_preview_candidate(entry: &FileEntry) -> bool {
    if !entry.kind.is_file() {
        return false;
    }
    if entry.category == FileCategory::Code {
        return true;
    }
    entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "txt"
                    | "md"
                    | "markdown"
                    | "rst"
                    | "log"
                    | "csv"
                    | "tsv"
                    | "json"
                    | "xml"
                    | "yaml"
                    | "yml"
                    | "toml"
                    | "ini"
                    | "cfg"
                    | "conf"
                    | "sql"
                    | "bat"
                    | "cmd"
                    | "ps1"
                    | "sh"
                    | "html"
                    | "css"
                    | "scss"
                    | "sass"
                    | "less"
            )
        })
}

pub fn hides_preview_metadata(entry: &FileEntry) -> bool {
    is_pdf_preview_candidate(entry) || is_text_preview_candidate(entry)
}

pub fn read_text_preview(path: &Path, max_bytes: usize) -> Option<String> {
    if explorer::is_portable_path(path) {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(max_bytes.min(96 * 1024) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes).replace('\r', "");
    let truncated = bytes.len() >= max_bytes.min(96 * 1024);
    let mut preview = text.chars().take(12_000).collect::<String>();
    if truncated || text.chars().count() > 12_000 {
        preview.push_str("\n…");
    }
    (!preview.trim().is_empty()).then_some(preview)
}

pub fn virtual_native_icon_request(
    entry: &FileEntry,
    size: u32,
) -> Option<(PathBuf, PathBuf, bool)> {
    let virtual_kind = if explorer::is_portable_path(&entry.path) {
        "portable"
    } else if explorer::is_trash_item_path(&entry.path) {
        "trash"
    } else {
        return None;
    };

    match entry.kind {
        EntryKind::Folder | EntryKind::SymlinkFolder => Some((
            PathBuf::from(format!(
                "__bexplorer_{virtual_kind}_folder_icon_size_{size}"
            )),
            PathBuf::from("bexplorer-folder"),
            true,
        )),
        EntryKind::File | EntryKind::SymlinkFile | EntryKind::Symlink | EntryKind::Other => {
            let extension = entry
                .path
                .extension()
                .and_then(|value| value.to_str())
                .or_else(|| entry.name.rsplit_once('.').map(|(_, extension)| extension))
                .map(|extension| {
                    extension
                        .trim()
                        .trim_start_matches('.')
                        .to_ascii_lowercase()
                })
                .filter(|extension| !extension.is_empty())
                .unwrap_or_else(|| "file".into());
            Some((
                PathBuf::from(format!(
                    "__bexplorer_{virtual_kind}_ext_{extension}_size_{size}"
                )),
                PathBuf::from(format!("bexplorer.{extension}")),
                false,
            ))
        }
        EntryKind::Drive if virtual_kind == "portable" => {
            Some(portable_device_native_icon_request(entry, size))
        }
        EntryKind::Drive => None,
    }
}

pub fn portable_device_native_icon_request(
    entry: &FileEntry,
    size: u32,
) -> (PathBuf, PathBuf, bool) {
    #[cfg(target_os = "linux")]
    {
        let device_id =
            explorer::portable_object_from_path(&entry.path).map(|(device_id, _)| device_id);
        let lookup_path =
            crate::platform::portable_device_icon_lookup_path(device_id.as_deref(), &entry.path);
        let cache_key = PathBuf::from(format!("__{}_size_{size}", lookup_path.to_string_lossy()));
        (cache_key, lookup_path, true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = entry;
        (
            PathBuf::from(format!("__bexplorer_portable_device_icon_size_{size}")),
            PathBuf::from("bexplorer-portable-device"),
            true,
        )
    }
}

#[cfg(target_os = "windows")]
pub fn native_entry_icon_cache_key_at_size(entry: &FileEntry, _size: u32) -> PathBuf {
    match entry.kind {
        EntryKind::Drive => PathBuf::from(format!(
            "__bexplorer_drive_{:?}_{}",
            entry.drive_kind,
            entry.path.display().to_string().replace(['\\', ':'], "_")
        )),
        EntryKind::Folder
        | EntryKind::File
        | EntryKind::SymlinkFolder
        | EntryKind::SymlinkFile
        | EntryKind::Symlink
        | EntryKind::Other => entry.path.clone(),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn native_entry_icon_cache_key_at_size(entry: &FileEntry, size: u32) -> PathBuf {
    match entry.kind {
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::Portable) => {
            portable_device_native_icon_request(entry, size).0
        }
        EntryKind::Drive => PathBuf::from(format!(
            "__bexplorer_drive_{:?}_{}_size_{size}",
            entry.drive_kind,
            native_directory_icon_class(&entry.path)
        )),
        EntryKind::Folder | EntryKind::SymlinkFolder => {
            native_path_icon_cache_key(&entry.path, true, size)
        }
        EntryKind::File | EntryKind::SymlinkFile | EntryKind::Symlink | EntryKind::Other => {
            native_file_icon_cache_key(&entry.path, Some(&entry.name), size)
        }
    }
}

#[cfg(target_os = "windows")]
pub fn native_path_icon_cache_key(path: &Path, _is_directory: bool, _size: u32) -> PathBuf {
    path.to_path_buf()
}

#[cfg(not(target_os = "windows"))]
pub fn native_path_icon_cache_key(path: &Path, is_directory: bool, size: u32) -> PathBuf {
    if is_directory {
        PathBuf::from(format!(
            "__bexplorer_native_folder_{}_size_{size}",
            native_directory_icon_class(path)
        ))
    } else {
        native_file_icon_cache_key(path, None, size)
    }
}

#[cfg(not(target_os = "windows"))]
fn native_directory_icon_class(path: &Path) -> &'static str {
    if path == Path::new("/") {
        "root"
    } else if standard_removable_mount_root(path) {
        "removable"
    } else if path.starts_with("/mnt") {
        "mnt"
    } else if let Some(class) = native_user_directory_icon_class(path) {
        class
    } else {
        "folder"
    }
}

#[cfg(not(target_os = "windows"))]
fn standard_removable_mount_root(path: &Path) -> bool {
    path.strip_prefix("/run/media")
        .is_ok_and(|relative| relative.components().count() == 2)
        || path
            .strip_prefix("/media")
            .is_ok_and(|relative| matches!(relative.components().count(), 1 | 2))
}

#[cfg(not(target_os = "windows"))]
fn native_user_directory_icon_class(path: &Path) -> Option<&'static str> {
    let directories = UserDirs::new()?;
    let candidates = [
        (Some(directories.home_dir()), "home"),
        (directories.desktop_dir(), "desktop"),
        (directories.document_dir(), "documents"),
        (directories.download_dir(), "downloads"),
        (directories.audio_dir(), "music"),
        (directories.picture_dir(), "pictures"),
        (directories.public_dir(), "public"),
        (directories.template_dir(), "templates"),
        (directories.video_dir(), "videos"),
    ];
    candidates.into_iter().find_map(|(candidate, class)| {
        candidate
            .filter(|candidate| *candidate == path)
            .map(|_| class)
    })
}

#[cfg(not(target_os = "windows"))]
fn native_file_icon_cache_key(path: &Path, fallback_name: Option<&str>, size: u32) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .or_else(|| {
            fallback_name.and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        })
        .map(|extension| {
            extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase()
        })
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "none".into());
    PathBuf::from(format!(
        "__bexplorer_native_file_ext_{extension}_size_{size}"
    ))
}

pub fn load_thumbnail_image(path: &Path, max_edge: u32) -> Option<NativeIconImage> {
    let image = std::fs::File::open(path)
        .ok()
        .and_then(|file| load_image_from_reader(BufReader::new(file), max_edge));
    image.or_else(|| render_svg_image(path, max_edge))
}

pub fn load_desktop_thumbnail_image(path: &Path, max_edge: u32) -> Option<NativeIconImage> {
    crate::platform::cached_desktop_thumbnail(path, max_edge)
}

pub fn load_thumbnail_image_with_fallback(path: &Path, max_edge: u32) -> Option<NativeIconImage> {
    let category = explorer::classify_file_category(path);
    if category == FileCategory::Video {
        return load_desktop_thumbnail_image(path, max_edge)
            .and_then(|image| resize_native_image(image, max_edge))
            .or_else(|| {
                crate::platform::video_thumbnail(path, NATIVE_ICON_SIZE)
                    .and_then(|image| resize_native_image(image, max_edge))
            });
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        if let Some(image) = load_desktop_thumbnail_image(path, max_edge) {
            return resize_native_image(image, max_edge);
        }
        return render_pdf_first_page(path)
            .and_then(|image| resize_native_image(image, NATIVE_ICON_SIZE))
            .and_then(|image| {
                let _ = crate::platform::cache_desktop_thumbnail(path, NATIVE_ICON_SIZE, &image);
                resize_native_image(image, max_edge)
            });
    }
    if category == FileCategory::Image {
        if let Some(image) = load_desktop_thumbnail_image(path, max_edge) {
            return resize_native_image(image, max_edge);
        }

        // Windows owns a global thumbnail cache and invokes installed Shell
        // providers for images and videos. Let it extract and persist a native
        // thumbnail before using BExplorer's internal image decoder.
        #[cfg(target_os = "windows")]
        if let Some(image) = crate::platform::image_thumbnail(path, NATIVE_ICON_SIZE) {
            return resize_native_image(image, max_edge);
        }

        if let Some(image) = load_thumbnail_image(path, NATIVE_ICON_SIZE) {
            let _ = crate::platform::cache_desktop_thumbnail(path, NATIVE_ICON_SIZE, &image);
            return resize_native_image(image, max_edge);
        }

        #[cfg(not(target_os = "windows"))]
        return crate::platform::image_thumbnail(path, NATIVE_ICON_SIZE)
            .and_then(|image| resize_native_image(image, max_edge));
    }
    None
}

/// Rendered only for the selected item in the preview panel. Keeping this separate
/// from the grid thumbnail loader avoids retaining large images for every entry.
pub fn load_preview_image(path: &Path) -> Option<NativeIconImage> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return render_pdf_first_page(path)
            .or_else(|| load_desktop_thumbnail_image(path, PREVIEW_MAX_EDGE));
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(image) = load_desktop_thumbnail_image(path, WINDOWS_PREVIEW_CACHE_EDGE) {
            return Some(image);
        }
        if let Some(image) = crate::platform::image_thumbnail(path, WINDOWS_PREVIEW_CACHE_EDGE) {
            return Some(image);
        }
        load_thumbnail_image(path, WINDOWS_PREVIEW_CACHE_EDGE).inspect(|image| {
            let _ =
                crate::platform::cache_desktop_thumbnail(path, WINDOWS_PREVIEW_CACHE_EDGE, image);
        })
    }

    #[cfg(not(target_os = "windows"))]
    if std::fs::metadata(path).ok()?.len() > MAX_PDF_PREVIEW_BYTES {
        return load_desktop_thumbnail_image(path, PREVIEW_MAX_EDGE);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let bytes = std::fs::read(path).ok()?;
        load_image_from_bytes(&bytes, PREVIEW_MAX_EDGE)
            .or_else(|| render_svg_image(path, PREVIEW_MAX_EDGE))
            .or_else(|| load_desktop_thumbnail_image(path, PREVIEW_MAX_EDGE))
    }
}

pub fn render_pdf_preview_page(path: &Path, page_index: usize) -> Option<(usize, NativeIconImage)> {
    if std::fs::metadata(path).ok()?.len() > MAX_PDF_PREVIEW_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).ok()?;
    let pages = pdf.pages();
    let page_count = pages.len();
    let page = pages.get(page_index)?;
    let cache = hayro::RenderCache::new();
    let interpreter_settings = hayro::hayro_interpret::InterpreterSettings::default();
    let render_settings = hayro::RenderSettings {
        x_scale: PDF_PREVIEW_SCALE,
        y_scale: PDF_PREVIEW_SCALE,
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        ..Default::default()
    };
    let pixmap = hayro::render(page, &cache, &interpreter_settings, &render_settings);
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    if width == 0 || height == 0 {
        return None;
    }
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in pixmap.take_unpremultiplied() {
        rgba.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
    }
    Some((
        page_count,
        NativeIconImage {
            width,
            height,
            rgba,
        },
    ))
}

fn render_pdf_first_page(path: &Path) -> Option<NativeIconImage> {
    render_pdf_preview_page(path, 0).map(|(_, image)| image)
}

pub fn load_native_icon_image(
    path: &Path,
    is_directory: bool,
    size: u32,
) -> Option<NativeIconImage> {
    if size >= 128 {
        crate::platform::native_file_icon_highres(path, is_directory)
            .or_else(|| crate::platform::native_file_icon(path, is_directory, size))
    } else {
        crate::platform::native_file_icon(path, is_directory, size)
            .or_else(|| crate::platform::native_file_icon_highres(path, is_directory))
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn load_thumbnail_image_from_bytes(bytes: &[u8], max_edge: u32) -> Option<NativeIconImage> {
    load_image_from_bytes(bytes, max_edge)
}

fn load_image_from_bytes(bytes: &[u8], max_edge: u32) -> Option<NativeIconImage> {
    load_image_from_reader(Cursor::new(bytes), max_edge)
}

fn load_image_from_reader<R>(reader: R, max_edge: u32) -> Option<NativeIconImage>
where
    R: BufRead + Seek,
{
    let _permit = acquire_image_decode_permit();
    let mut reader = image::ImageReader::new(reader).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_IMAGE_DECODE_ALLOC_BYTES);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    if decoder.total_bytes() > MAX_IMAGE_DECODE_ALLOC_BYTES {
        return None;
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let image = image::DynamicImage::from_decoder(decoder).ok()?;
    let max_edge = max_edge.max(1);
    let mut thumbnail = if image.width().max(image.height()) > max_edge {
        image.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    thumbnail.apply_orientation(orientation);
    let thumbnail = thumbnail.to_rgba8();
    Some(NativeIconImage {
        width: thumbnail.width() as usize,
        height: thumbnail.height() as usize,
        rgba: thumbnail.into_raw(),
    })
}

struct ImageDecodePermit {
    state: &'static (Mutex<usize>, Condvar),
}

impl Drop for ImageDecodePermit {
    fn drop(&mut self) {
        let (active, available) = self.state;
        let mut active = active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = active.saturating_sub(1);
        available.notify_one();
    }
}

fn acquire_image_decode_permit() -> ImageDecodePermit {
    static STATE: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    let state = STATE.get_or_init(|| (Mutex::new(0), Condvar::new()));
    let (active, available) = state;
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while *active >= MAX_CONCURRENT_IMAGE_DECODERS {
        active = available
            .wait(active)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    *active += 1;
    drop(active);
    ImageDecodePermit { state }
}

fn resize_native_image(image: NativeIconImage, max_edge: u32) -> Option<NativeIconImage> {
    if image.width <= max_edge as usize && image.height <= max_edge as usize {
        return Some(image);
    }

    let width = u32::try_from(image.width).ok()?;
    let height = u32::try_from(image.height).ok()?;
    let source = image::RgbaImage::from_raw(width, height, image.rgba)?;
    let resized = image::DynamicImage::ImageRgba8(source)
        .resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    Some(NativeIconImage {
        width: resized.width() as usize,
        height: resized.height() as usize,
        rgba: resized.into_raw(),
    })
}

fn render_svg_image(path: &Path, max_edge: u32) -> Option<NativeIconImage> {
    if std::fs::metadata(path).ok()?.len() > MAX_SVG_SOURCE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&bytes, &options).ok()?;
    let base_size = tree.size().to_int_size();
    let scale =
        (max_edge as f32 / base_size.width().max(base_size.height()) as f32).clamp(0.01, 1.0);
    let width = ((base_size.width() as f32 * scale).round() as u32).max(1);
    let height = ((base_size.height() as f32 * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    unpremultiply_rgba(&mut rgba);
    Some(NativeIconImage {
        width: width as usize,
        height: height as usize,
        rgba,
    })
}

fn unpremultiply_rgba(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        pixel[0] = ((pixel[0] as u32 * 255) / alpha).min(255) as u8;
        pixel[1] = ((pixel[1] as u32 * 255) / alpha).min(255) as u8;
        pixel[2] = ((pixel[2] as u32 * 255) / alpha).min(255) as u8;
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn load_portable_thumbnail_image(
    path: &Path,
    max_bytes: usize,
    allow_default_resource: bool,
    max_edge: u32,
) -> Option<NativeIconImage> {
    let (device_id, object_id) = explorer::portable_object_from_path(path)?;
    let bytes = crate::platform::portable_device_thumbnail(
        &device_id,
        &object_id,
        max_bytes,
        allow_default_resource,
    )?;
    load_thumbnail_image_from_bytes(&bytes, max_edge)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn load_portable_thumbnail_image(
    _path: &Path,
    _max_bytes: usize,
    _allow_default_resource: bool,
    _max_edge: u32,
) -> Option<NativeIconImage> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_video_files_are_thumbnail_candidates() {
        let entry = FileEntry {
            name: "movie.mp4".into(),
            path: PathBuf::from("/tmp/movie.mp4"),
            kind: EntryKind::File,
            category: FileCategory::Video,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: Some(128 * 1024 * 1024),
            percent_full: None,
            modified: None,
            created: None,
            is_hidden: false,
        };

        assert!(is_thumbnail_candidate(&entry));
        assert!(
            !is_visual_preview_candidate(&entry),
            "video thumbnails should not implicitly enable the full preview panel"
        );
    }

    #[test]
    fn trashed_items_use_virtual_type_icons_without_decoding_the_virtual_path() {
        let entry = FileEntry {
            name: "vacation.mp4".into(),
            path: explorer::trash_item_path(std::ffi::OsStr::new("native-id")),
            kind: EntryKind::File,
            category: FileCategory::Video,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: Some(42),
            percent_full: None,
            modified: Some("/home/example/Videos".into()),
            created: Some("2026-07-26 12:00".into()),
            is_hidden: false,
        };

        let (cache_key, lookup_path, is_directory) =
            virtual_native_icon_request(&entry, SMALL_ENTRY_IMAGE_SIZE)
                .expect("trash item icon request");
        assert!(cache_key.to_string_lossy().contains("trash_ext_mp4"));
        assert_eq!(lookup_path, PathBuf::from("bexplorer.mp4"));
        assert!(!is_directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn portable_devices_request_the_icon_profile_of_their_linux_backend() {
        let mut entry = FileEntry {
            name: "Phone".into(),
            path: explorer::portable_device_path("linux-kio-mtp:/org/kde/kmtp/device_1", "Phone"),
            kind: EntryKind::Drive,
            category: FileCategory::Other,
            drive_kind: Some(explorer::DriveKind::Portable),
            file_system: "MTP".into(),
            free_space: None,
            size: None,
            percent_full: None,
            modified: None,
            created: None,
            is_hidden: false,
        };

        let (cache_key, lookup_path, is_directory) =
            virtual_native_icon_request(&entry, SMALL_ENTRY_IMAGE_SIZE)
                .expect("portable device icon request");
        assert!(cache_key.to_string_lossy().contains("portable-device"));
        assert!(
            lookup_path
                .to_string_lossy()
                .starts_with("bexplorer-portable-device-icons@multimedia-player")
        );
        assert!(is_directory);

        entry.path =
            explorer::portable_device_path("linux-gvfs-mtp:mtp://Google_Pixel_ABC123/", "Phone");
        let (gvfs_key, gvfs_lookup, _) =
            virtual_native_icon_request(&entry, SMALL_ENTRY_IMAGE_SIZE)
                .expect("GVfs portable device icon request");
        assert_ne!(cache_key, gvfs_key);
        assert!(
            gvfs_lookup
                .to_string_lossy()
                .starts_with("bexplorer-portable-device-icons@phone")
        );
    }

    #[test]
    fn large_local_images_are_still_thumbnail_candidates() {
        let entry = FileEntry {
            name: "camera-photo.jpg".into(),
            path: PathBuf::from("/tmp/camera-photo.jpg"),
            kind: EntryKind::File,
            category: FileCategory::Image,
            drive_kind: None,
            file_system: String::new(),
            free_space: None,
            size: Some(80 * 1024 * 1024),
            percent_full: None,
            modified: None,
            created: None,
            is_hidden: false,
        };

        assert!(is_thumbnail_candidate(&entry));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn user_home_and_generic_folders_have_distinct_native_icon_keys() {
        let Some(directories) = UserDirs::new() else {
            return;
        };
        let home = native_path_icon_cache_key(directories.home_dir(), true, NATIVE_ICON_SIZE);
        let generic = native_path_icon_cache_key(
            &directories.home_dir().join("bexplorer-generic-folder"),
            true,
            NATIVE_ICON_SIZE,
        );
        assert_ne!(home, generic);
        assert!(home.to_string_lossy().contains("home"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn removable_root_and_its_folders_have_distinct_native_icon_keys() {
        let root = native_path_icon_cache_key(
            Path::new("/run/media/example/USB"),
            true,
            SMALL_ENTRY_IMAGE_SIZE,
        );
        let child = native_path_icon_cache_key(
            Path::new("/run/media/example/USB/Documents"),
            true,
            SMALL_ENTRY_IMAGE_SIZE,
        );

        assert!(root.to_string_lossy().contains("removable"));
        assert!(child.to_string_lossy().contains("folder"));
        assert_ne!(root, child);
    }

    #[test]
    fn small_entry_image_is_resampled_to_its_own_pixel_size() {
        let source = NativeIconImage {
            width: 120,
            height: 60,
            rgba: vec![255; 120 * 60 * 4],
        };

        let resized =
            resize_native_image(source, SMALL_ENTRY_IMAGE_SIZE).expect("resized thumbnail");

        assert_eq!(resized.width, SMALL_ENTRY_IMAGE_SIZE as usize);
        assert_eq!(resized.height, (SMALL_ENTRY_IMAGE_SIZE / 2) as usize);
        assert_eq!(resized.rgba.len(), resized.width * resized.height * 4);
    }

    #[test]
    fn thumbnail_loader_keeps_separate_small_and_standard_sources() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons/appicon.png");
        let small = load_thumbnail_image_with_fallback(&path, SMALL_ENTRY_IMAGE_SIZE)
            .expect("small thumbnail");
        let standard = load_thumbnail_image_with_fallback(&path, NATIVE_ICON_SIZE)
            .expect("standard thumbnail");

        assert_eq!(
            small.width.max(small.height),
            SMALL_ENTRY_IMAGE_SIZE as usize
        );
        assert_eq!(
            standard.width.max(standard.height),
            NATIVE_ICON_SIZE as usize
        );
        assert!(small.rgba.len() < standard.rgba.len());
    }

    #[test]
    fn internal_decoder_applies_exif_orientation_before_display() {
        let source = image::RgbImage::from_pixel(2, 3, image::Rgb([40, 120, 220]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&source)
            .expect("encode JPEG");

        // Big-endian TIFF metadata with one Orientation=6 (rotate 90°)
        // entry, wrapped in a JPEG APP1 Exif segment.
        let exif = [
            0xFF, 0xE1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0x00, 0x00, b'M', b'M', 0x00, 0x2A,
            0x00, 0x00, 0x00, 0x08, 0x00, 0x01, 0x01, 0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01,
            0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut oriented = jpeg[..2].to_vec();
        oriented.extend_from_slice(&exif);
        oriented.extend_from_slice(&jpeg[2..]);

        let decoded = load_thumbnail_image_from_bytes(&oriented, 3).expect("decode oriented JPEG");

        assert_eq!((decoded.width, decoded.height), (3, 2));
    }
}

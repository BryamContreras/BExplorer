use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "windows"))]
use directories::UserDirs;

use crate::fs::explorer::{self, EntryKind, FileCategory, FileEntry};
use crate::platform::NativeIconImage;

pub const NATIVE_ICON_SIZE: u32 = 256;
pub const SMALL_ENTRY_IMAGE_SIZE: u32 = 48;
const PREVIEW_MAX_EDGE: u32 = 1200;
const MAX_PDF_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
const PDF_PREVIEW_SCALE: f32 = 1.15;

pub fn is_thumbnail_candidate(entry: &FileEntry) -> bool {
    if !entry.kind.is_file() {
        return false;
    }

    matches!(entry.category, FileCategory::Image)
        || is_pdf_preview_candidate(entry)
        || (explorer::is_portable_path(&entry.path) && entry.category == FileCategory::Video)
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
    if !explorer::is_portable_path(&entry.path) {
        return None;
    }

    match entry.kind {
        EntryKind::Folder | EntryKind::SymlinkFolder => Some((
            PathBuf::from(format!("__bexplorer_portable_folder_icon_size_{size}")),
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
                PathBuf::from(format!("__bexplorer_portable_ext_{extension}_size_{size}")),
                PathBuf::from(format!("bexplorer.{extension}")),
                false,
            ))
        }
        EntryKind::Drive => None,
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
    } else if path.starts_with("/media") || path.starts_with("/run/media") {
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
    let bytes = std::fs::read(path).ok()?;
    load_thumbnail_image_from_bytes(&bytes, max_edge).or_else(|| render_svg_image(path, max_edge))
}

pub fn load_desktop_thumbnail_image(path: &Path, max_edge: u32) -> Option<NativeIconImage> {
    crate::platform::cached_desktop_thumbnail(path, max_edge)
}

pub fn load_thumbnail_image_with_fallback(path: &Path, max_edge: u32) -> Option<NativeIconImage> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return load_desktop_thumbnail_image(path, max_edge)
            .or_else(|| render_pdf_first_page(path))
            .and_then(|image| resize_native_image(image, max_edge));
    }
    load_desktop_thumbnail_image(path, max_edge)
        .and_then(|image| resize_native_image(image, max_edge))
        .or_else(|| load_thumbnail_image(path, max_edge))
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

    if std::fs::metadata(path).ok()?.len() > MAX_PDF_PREVIEW_BYTES {
        return load_desktop_thumbnail_image(path, PREVIEW_MAX_EDGE);
    }

    let bytes = std::fs::read(path).ok()?;
    load_image_from_bytes(&bytes, PREVIEW_MAX_EDGE)
        .or_else(|| render_svg_image(path, PREVIEW_MAX_EDGE))
        .or_else(|| load_desktop_thumbnail_image(path, PREVIEW_MAX_EDGE))
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

pub fn load_thumbnail_image_from_bytes(bytes: &[u8], max_edge: u32) -> Option<NativeIconImage> {
    load_image_from_bytes(bytes, max_edge)
}

fn load_image_from_bytes(bytes: &[u8], max_edge: u32) -> Option<NativeIconImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let thumbnail = image
        .resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    Some(NativeIconImage {
        width: thumbnail.width() as usize,
        height: thumbnail.height() as usize,
        rgba: thumbnail.into_raw(),
    })
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

#[cfg(target_os = "windows")]
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

#[cfg(not(target_os = "windows"))]
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
}

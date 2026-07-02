use std::path::Path;

use eframe::egui::ColorImage;

use crate::fs::explorer::{EntryKind, FileCategory, FileEntry};

const MAX_TEXT_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_BINARY_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
const IMAGE_PREVIEW_MAX_EDGE: u32 = 1200;
const MAX_PDF_PREVIEW_PAGES: usize = 6;
const PDF_PREVIEW_SCALE: f32 = 0.9;

pub enum PreviewContent {
    Images {
        images: Vec<ColorImage>,
        append: bool,
        finished: bool,
        page_count: Option<usize>,
    },
    Text(String),
    Unsupported,
}

pub fn render_entry_streaming<F>(entry: &FileEntry, max_bytes: usize, mut emit: F)
where
    F: FnMut(PreviewContent) -> bool,
{
    if entry.kind != EntryKind::File {
        emit(PreviewContent::Unsupported);
        return;
    }
    if entry
        .size
        .is_some_and(|size| size > MAX_BINARY_PREVIEW_BYTES)
    {
        emit(PreviewContent::Unsupported);
        return;
    }

    if is_pdf_path(&entry.path) {
        if !render_pdf_pages(&entry.path, &mut emit) {
            emit(PreviewContent::Unsupported);
        }
        return;
    }

    if is_svg_path(&entry.path) {
        emit(image_preview_update(render_svg(&entry.path)));
        return;
    }

    if entry.category == FileCategory::Image {
        emit(image_preview_update(render_raster_image(&entry.path)));
        return;
    }

    if is_text_preview_candidate(entry) {
        emit(
            read_text_preview(&entry.path, max_bytes)
                .map(PreviewContent::Text)
                .unwrap_or(PreviewContent::Unsupported),
        );
        return;
    }

    emit(PreviewContent::Unsupported);
}

fn image_preview_update(image: Option<ColorImage>) -> PreviewContent {
    image
        .map(|image| PreviewContent::Images {
            images: vec![image],
            append: false,
            finished: true,
            page_count: None,
        })
        .unwrap_or(PreviewContent::Unsupported)
}

fn render_pdf_pages<F>(path: &Path, emit: &mut F) -> bool
where
    F: FnMut(PreviewContent) -> bool,
{
    let Some(bytes) = std::fs::read(path).ok() else {
        return false;
    };
    let Some(pdf) = hayro::hayro_syntax::Pdf::new(bytes).ok() else {
        return false;
    };
    let pages = pdf.pages();
    let cache = hayro::RenderCache::new();
    let interpreter_settings = hayro::hayro_interpret::InterpreterSettings::default();
    let render_settings = hayro::RenderSettings {
        x_scale: PDF_PREVIEW_SCALE,
        y_scale: PDF_PREVIEW_SCALE,
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        ..Default::default()
    };
    let page_count = pages.len().min(MAX_PDF_PREVIEW_PAGES);
    let mut rendered = 0usize;
    for (index, page) in pages.iter().take(page_count).enumerate() {
        let pixmap = hayro::render(page, &cache, &interpreter_settings, &render_settings);
        let Some(image) = color_image_from_pixmap(pixmap) else {
            continue;
        };
        rendered += 1;
        if !emit(PreviewContent::Images {
            images: vec![image],
            append: index > 0,
            finished: index + 1 >= page_count,
            page_count: Some(page_count),
        }) {
            return rendered > 0;
        }
    }
    rendered > 0
}

fn color_image_from_pixmap(pixmap: hayro::vello_cpu::Pixmap) -> Option<ColorImage> {
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    let mut rgba = Vec::with_capacity(size[0] * size[1] * 4);
    for pixel in pixmap.take_unpremultiplied() {
        rgba.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
    }
    Some(ColorImage::from_rgba_unmultiplied(size, &rgba))
}

fn render_raster_image(path: &Path) -> Option<ColorImage> {
    let bytes = std::fs::read(path).ok()?;
    color_image_from_bytes(&bytes)
}

fn color_image_from_bytes(bytes: &[u8]) -> Option<ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let thumbnail = image
        .thumbnail(IMAGE_PREVIEW_MAX_EDGE, IMAGE_PREVIEW_MAX_EDGE)
        .to_rgba8();
    let size = [thumbnail.width() as usize, thumbnail.height() as usize];
    let pixels = thumbnail.into_raw();
    Some(ColorImage::from_rgba_unmultiplied(size, &pixels))
}

fn render_svg(path: &Path) -> Option<ColorImage> {
    let bytes = std::fs::read(path).ok()?;
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&bytes, &options).ok()?;
    let base_size = tree.size().to_int_size();
    let scale = (IMAGE_PREVIEW_MAX_EDGE as f32 / base_size.width().max(base_size.height()) as f32)
        .min(1.0)
        .max(0.01);
    let width = ((base_size.width() as f32 * scale).round() as u32).max(1);
    let height = ((base_size.height() as f32 * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    unpremultiply_rgba(&mut rgba);
    Some(ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        &rgba,
    ))
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

fn read_text_preview(path: &Path, max_bytes: usize) -> Option<String> {
    let limit = max_bytes.min(MAX_TEXT_PREVIEW_BYTES);
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > limit {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some(text.replace('\0', ""))
}

fn is_text_preview_candidate(entry: &FileEntry) -> bool {
    if matches!(entry.category, FileCategory::Code) {
        return true;
    }
    let Some(extension) = entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
    else {
        return false;
    };
    matches!(
        extension.as_str(),
        "txt"
            | "md"
            | "markdown"
            | "log"
            | "ini"
            | "cfg"
            | "conf"
            | "json"
            | "xml"
            | "yml"
            | "yaml"
            | "toml"
            | "csv"
            | "tsv"
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
}

fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

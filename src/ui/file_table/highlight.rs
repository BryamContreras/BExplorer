use std::ops::Range;

use eframe::egui::{self, Color32, FontId};

use crate::app::config::{AppConfig, ThemePreference};
use crate::ui::theme;

pub(super) fn highlighted_text_layout_job(
    text: &str,
    font: FontId,
    color: Color32,
    config: &AppConfig,
    selected: bool,
    ranges: &[Range<usize>],
) -> egui::text::LayoutJob {
    let normal = egui::TextFormat::simple(font.clone(), color);
    let mut highlighted = egui::TextFormat::simple(font, search_highlight_text(config, selected));
    highlighted.background = search_highlight_fill(config, selected);

    let mut job = egui::text::LayoutJob {
        break_on_newline: false,
        ..Default::default()
    };
    let mut cursor = 0;
    for range in ranges {
        let start = range.start.min(text.chars().count());
        let end = range.end.min(text.chars().count());
        if start < cursor || start >= end {
            continue;
        }
        let normal_start = char_to_byte_index(text, cursor);
        let normal_end = char_to_byte_index(text, start);
        if normal_start < normal_end {
            job.append(&text[normal_start..normal_end], 0.0, normal.clone());
        }
        let highlight_start = char_to_byte_index(text, start);
        let highlight_end = char_to_byte_index(text, end);
        if highlight_start < highlight_end {
            job.append(
                &text[highlight_start..highlight_end],
                0.0,
                highlighted.clone(),
            );
        }
        cursor = end;
    }
    let tail_start = char_to_byte_index(text, cursor);
    if tail_start < text.len() {
        job.append(&text[tail_start..], 0.0, normal);
    }
    job
}

fn search_highlight_fill(config: &AppConfig, selected: bool) -> Color32 {
    if selected {
        return Color32::from_white_alpha(72);
    }
    let accent = theme::accent(config);
    let alpha = match config.theme {
        ThemePreference::Dark => 118,
        ThemePreference::Light | ThemePreference::Gray => 92,
    };
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha)
}

fn search_highlight_text(config: &AppConfig, selected: bool) -> Color32 {
    match (selected, config.theme) {
        (true, _) => Color32::WHITE,
        (false, ThemePreference::Dark) => Color32::WHITE,
        (false, ThemePreference::Light | ThemePreference::Gray) => theme::text(config),
    }
}

pub(super) fn search_highlight_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let query = query.trim();
    if text.is_empty() || query.is_empty() {
        return Vec::new();
    }

    if let Some(extension) = extension_highlight_query(query) {
        return extension_highlight_range(text, extension)
            .into_iter()
            .collect();
    }

    if query.contains('*') || query.contains('?') {
        return Vec::new();
    }

    contains_highlight_ranges(text, query)
}

fn contains_highlight_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    if lower_query.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut search_start = 0;
    while search_start <= lower_text.len() {
        let Some(offset) = lower_text[search_start..].find(&lower_query) else {
            break;
        };
        let start_byte = search_start + offset;
        let end_byte = start_byte + lower_query.len();
        let start = lower_text[..start_byte].chars().count();
        let end = lower_text[..end_byte].chars().count();
        if start < end {
            ranges.push(start..end);
        }
        search_start = end_byte;
    }
    ranges
}

fn extension_highlight_query(query: &str) -> Option<&str> {
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
    Some(extension)
}

fn extension_highlight_range(text: &str, extension: &str) -> Option<Range<usize>> {
    let extension_len = extension.chars().count();
    if extension_len == 0 {
        return None;
    }

    let lower_text = text.to_lowercase();
    let lower_extension = extension.to_lowercase();
    let suffix = format!(".{lower_extension}");
    if lower_text.ends_with(&suffix) {
        let total_chars = text.chars().count();
        let end = total_chars;
        let start = end.saturating_sub(extension_len);
        return Some(start..end);
    }

    if lower_text.ends_with(&lower_extension) {
        let total_chars = text.chars().count();
        let end = total_chars;
        let start = end.saturating_sub(extension_len);
        return Some(start..end);
    }

    None
}

fn char_to_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

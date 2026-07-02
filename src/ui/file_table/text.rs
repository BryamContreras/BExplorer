use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect};

pub(super) fn draw_text_clipped(
    ui: &mut egui::Ui,
    clip_rect: Rect,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
) {
    ui.painter()
        .with_clip_rect(clip_rect)
        .text(snap_pos(pos), align, text, font, color);
}

pub(super) fn draw_text_elided(
    ui: &mut egui::Ui,
    clip_rect: Rect,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
) {
    let max_width = clip_rect.width().max(0.0);
    let display = elide_middle_to_width(ui, text, &font, color, max_width);
    ui.painter()
        .with_clip_rect(clip_rect)
        .text(snap_pos(pos), align, display, font, color);
}

fn elide_middle_to_width(
    ui: &egui::Ui,
    text: &str,
    font: &FontId,
    color: Color32,
    max_width: f32,
) -> String {
    if text_width(ui, text, font, color) <= max_width {
        return text.to_string();
    }

    const ELLIPSIS: &str = "...";
    if text_width(ui, ELLIPSIS, font, color) > max_width {
        return String::new();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut low = 0_usize;
    let mut high = chars.len();

    while low < high {
        let keep = (low + high + 1) / 2;
        let candidate = middle_elided_candidate(&chars, keep);
        if text_width(ui, &candidate, font, color) <= max_width {
            low = keep;
        } else {
            high = keep - 1;
        }
    }

    middle_elided_candidate(&chars, low)
}

fn middle_elided_candidate(chars: &[char], keep: usize) -> String {
    const ELLIPSIS: &str = "...";
    if keep == 0 {
        return ELLIPSIS.to_string();
    }

    let prefix_len = keep.div_ceil(2).min(chars.len());
    let suffix_len = keep / 2;
    let suffix_start = chars.len().saturating_sub(suffix_len);

    let mut text = String::with_capacity(keep + ELLIPSIS.len());
    text.extend(chars.iter().take(prefix_len));
    text.push_str(ELLIPSIS);
    text.extend(chars.iter().skip(suffix_start));
    text
}

fn text_width(ui: &egui::Ui, text: &str, font: &FontId, color: Color32) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font.clone(), color)
        .size()
        .x
}

pub(super) fn draw_text(
    ui: &mut egui::Ui,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
) {
    ui.painter().text(snap_pos(pos), align, text, font, color);
}

pub(super) fn snap_pos(pos: Pos2) -> Pos2 {
    Pos2::new(pos.x.round(), pos.y.round())
}

pub(super) fn snap_rect(rect: Rect) -> Rect {
    Rect::from_min_max(snap_pos(rect.min), snap_pos(rect.max))
}

pub(super) fn format_bytes_opt(value: Option<u64>) -> String {
    value.map(format_bytes).unwrap_or_default()
}

pub fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{value} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

use std::path::Path;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::config::AppConfig;
use crate::app::state::{BExplorerApp, PreviewContentRef};
use crate::ui::{i18n, theme};

const PREVIEW_PANEL_MIN_WIDTH: f32 = 220.0;
const PREVIEW_PANEL_MAX_WIDTH: f32 = 560.0;
const PREVIEW_PANEL_MIN_MAIN_WIDTH: f32 = 420.0;
const PREVIEW_PANEL_DIVIDER_WIDTH: f32 = 6.0;

pub(super) fn show_with_panel<F>(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    mut show_content_view: F,
) where
    F: FnMut(&mut BExplorerApp, &mut egui::Ui),
{
    let rect = ui.available_rect_before_wrap();
    let max_preview_width = (rect.width() - PREVIEW_PANEL_MIN_MAIN_WIDTH)
        .clamp(PREVIEW_PANEL_MIN_WIDTH, PREVIEW_PANEL_MAX_WIDTH);
    let preview_width = app
        .config
        .preview_panel_width
        .clamp(PREVIEW_PANEL_MIN_WIDTH, max_preview_width);
    if rect.width() < PREVIEW_PANEL_MIN_MAIN_WIDTH + PREVIEW_PANEL_MIN_WIDTH
        || rect.height() < 140.0
    {
        show_content_view(app, ui);
        return;
    }

    let content_rect = Rect::from_min_max(
        rect.min,
        Pos2::new(
            rect.right() - preview_width - PREVIEW_PANEL_DIVIDER_WIDTH,
            rect.bottom(),
        ),
    );
    let divider_rect = Rect::from_min_size(
        Pos2::new(content_rect.right(), rect.top()),
        Vec2::new(PREVIEW_PANEL_DIVIDER_WIDTH, rect.height()),
    );
    let preview_rect = Rect::from_min_max(
        Pos2::new(divider_rect.right(), rect.top()),
        rect.right_bottom(),
    );

    let divider_response = ui.allocate_rect(divider_rect, Sense::drag());
    if divider_response.hovered() || divider_response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if divider_response.dragged() {
        app.config.preview_panel_width = (preview_width - divider_response.drag_delta().x)
            .clamp(PREVIEW_PANEL_MIN_WIDTH, max_preview_width);
        ui.ctx().request_repaint();
    }
    if divider_response.drag_stopped() {
        app.save_config();
    }

    ui.painter()
        .rect_filled(divider_rect, 0.0, theme::subtle_stroke(&app.config));
    if divider_response.hovered() || divider_response.dragged() {
        let line =
            Rect::from_center_size(divider_rect.center(), Vec2::new(2.0, divider_rect.height()));
        theme::paint_selection_gradient(ui.painter(), line, &app.config);
    }

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.set_clip_rect(content_rect);
        show_content_view(app, ui);
    });
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(preview_rect), |ui| {
        ui.set_clip_rect(preview_rect);
        paint_preview_panel(app, ui, preview_rect);
    });
}

fn paint_preview_panel(app: &mut BExplorerApp, ui: &mut egui::Ui, rect: Rect) {
    ui.painter()
        .rect_filled(rect, 0.0, theme::surface(&app.config));
    ui.painter().line_segment(
        [rect.left_top(), rect.left_bottom()],
        Stroke::new(1.0, theme::stroke(&app.config)),
    );

    let inner = rect.shrink2(Vec2::new(10.0, 10.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_width(inner.width());
        ui.spacing_mut().item_spacing = Vec2::ZERO;

        let selected_count = app.selected.len();
        if selected_count == 0 {
            ui.add_sized(
                Vec2::new(inner.width(), 0.0),
                egui::Label::new(
                    egui::RichText::new(i18n::tr(&app.config, "no_preview_selection"))
                        .size(app.config.font_size)
                        .color(theme::muted(&app.config)),
                )
                .wrap(),
            );
            return;
        }

        if selected_count > 1 {
            ui.add_sized(
                Vec2::new(inner.width(), 0.0),
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{} {}",
                        selected_count,
                        i18n::tr(&app.config, "items")
                    ))
                    .size(app.config.font_size)
                    .color(theme::muted(&app.config)),
                )
                .wrap(),
            );
            return;
        }

        let Some(entry) = app.selected_visible_entry() else {
            return;
        };

        match app.preview_content(ui.ctx(), &entry) {
            Some(PreviewContentRef::Images {
                images,
                loading,
                page_count,
            }) => {
                paint_preview_images(ui, &app.config, inner, &images, loading, page_count);
            }
            Some(PreviewContentRef::Text(text)) => {
                paint_preview_text(app, ui, inner, &entry.path, &text);
            }
            Some(PreviewContentRef::Loading) => {
                paint_preview_loading(ui, &app.config, inner);
            }
            None => {
                paint_preview_message(
                    ui,
                    &app.config,
                    inner,
                    i18n::tr(&app.config, "no_preview_available"),
                );
            }
        }
    });
}

fn paint_preview_images(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: Rect,
    images: &[(egui::TextureId, Vec2)],
    loading: bool,
    page_count: Option<usize>,
) {
    if let Some((texture_id, texture_size)) = images.first().filter(|_| images.len() == 1) {
        paint_preview_image(ui, config, rect, *texture_id, *texture_size);
        if loading {
            paint_preview_loading_border(ui, config, rect);
        }
        if let Some(page_count) = page_count {
            paint_pdf_page_indicator(ui, config, rect, 1, page_count);
        }
        return;
    }

    ui.painter()
        .rect_filled(rect, 6.0, theme::surface_elevated(config));
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(1.0, theme::stroke(config)));

    let inner = rect.shrink(10.0);
    let mut page_heights = Vec::with_capacity(images.len());
    let mut page_spacing = 0.0;
    let mut current_pdf_page = None;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_clip_rect(inner);
        let output = egui::ScrollArea::vertical()
            .id_salt("preview_images_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(inner.width());
                for (index, (texture_id, texture_size)) in images.iter().enumerate() {
                    let source = Vec2::new(texture_size.x.max(1.0), texture_size.y.max(1.0));
                    let page_width = ui.available_width().max(1.0);
                    let page_height = (source.y * (page_width / source.x)).max(1.0);
                    page_heights.push(page_height);
                    let (page_rect, _) =
                        ui.allocate_exact_size(Vec2::new(page_width, page_height), Sense::hover());
                    ui.painter().rect_filled(page_rect, 4.0, Color32::WHITE);
                    ui.painter().image(
                        *texture_id,
                        page_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if index + 1 < images.len() {
                        page_spacing = 10.0;
                        ui.add_space(page_spacing);
                    }
                }
            });
        if let Some(page_count) = page_count {
            let viewport_center = output.state.offset.y + output.inner_rect.height() * 0.5;
            let current_page = current_preview_page(viewport_center, &page_heights, page_spacing);
            current_pdf_page = Some((current_page, page_count));
        }
    });
    if loading {
        paint_preview_loading_border(ui, config, rect);
    }
    if let Some((current_page, page_count)) = current_pdf_page {
        paint_pdf_page_indicator(ui, config, rect, current_page, page_count);
    }
}

fn current_preview_page(viewport_center_y: f32, page_heights: &[f32], page_spacing: f32) -> usize {
    let mut top = 0.0;
    let mut best_page = 1usize;
    let mut best_distance = f32::INFINITY;
    for (index, height) in page_heights.iter().enumerate() {
        let center = top + height * 0.5;
        let distance = (center - viewport_center_y).abs();
        if distance < best_distance {
            best_distance = distance;
            best_page = index + 1;
        }
        top += height + page_spacing;
    }
    best_page
}

fn paint_preview_image(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: Rect,
    texture_id: egui::TextureId,
    texture_size: Vec2,
) {
    ui.painter()
        .rect_filled(rect, 6.0, theme::surface_elevated(config));
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(1.0, theme::stroke(config)));

    let available = rect.shrink(10.0);
    let source = Vec2::new(texture_size.x.max(1.0), texture_size.y.max(1.0));
    let scale = (available.width() / source.x).min(available.height() / source.y);
    let image_size = source * scale;
    let image_rect = Rect::from_center_size(available.center(), image_size);
    ui.painter().image(
        texture_id,
        image_rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn paint_preview_message(ui: &mut egui::Ui, config: &AppConfig, rect: Rect, text: &str) {
    ui.painter()
        .rect_filled(rect, 6.0, theme::surface_elevated(config));
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(1.0, theme::stroke(config)));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(config.font_size),
        theme::muted(config),
    );
}

fn paint_pdf_page_indicator(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: Rect,
    current_page: usize,
    page_count: usize,
) {
    let page_count = page_count.max(1);
    let current_page = current_page.clamp(1, page_count);
    let text = format!("{current_page}-{page_count}");
    let width = (text.chars().count() as f32 * 8.0 + 18.0).max(46.0);
    let indicator_rect = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.bottom() - 19.0),
        Vec2::new(width, 24.0),
    );
    let fill = theme::popup_surface(config);
    ui.painter().rect_filled(
        indicator_rect,
        6.0,
        Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 225),
    );
    ui.painter().rect_stroke(
        indicator_rect,
        6.0,
        Stroke::new(1.0, theme::popup_stroke(config)),
    );
    ui.painter().text(
        indicator_rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(config.font_size * 0.9),
        theme::text(config),
    );
}

fn paint_preview_loading(ui: &mut egui::Ui, config: &AppConfig, rect: Rect) {
    ui.painter()
        .rect_filled(rect, 6.0, theme::surface_elevated(config));
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(1.0, theme::stroke(config)));
    paint_preview_loading_border(ui, config, rect);
}

fn paint_preview_loading_border(ui: &mut egui::Ui, config: &AppConfig, rect: Rect) {
    let time = ui.ctx().input(|input| input.time) as f32;
    let progress = (time * 0.85).fract();
    let perimeter = (rect.width() + rect.height()) * 2.0;
    let segment = (perimeter * 0.22).clamp(56.0, 180.0);
    let start = progress * perimeter;
    let stroke = Stroke::new(2.0, theme::accent(config));
    paint_preview_loading_segment(ui, rect.shrink(1.0), start, segment, stroke);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(16));
}

fn paint_preview_loading_segment(
    ui: &mut egui::Ui,
    rect: Rect,
    start: f32,
    length: f32,
    stroke: Stroke,
) {
    let perimeter = (rect.width() + rect.height()) * 2.0;
    if perimeter <= 0.0 {
        return;
    }

    let mut remaining = length;
    let mut cursor = start.rem_euclid(perimeter);
    while remaining > 0.0 {
        let next_edge_end = edge_end_distance(rect, cursor);
        let step = remaining.min(next_edge_end - cursor);
        let a = point_on_rect_perimeter(rect, cursor);
        let b = point_on_rect_perimeter(rect, cursor + step);
        ui.painter().line_segment([a, b], stroke);
        remaining -= step;
        cursor = (cursor + step).rem_euclid(perimeter);
        if step <= 0.1 {
            break;
        }
    }
}

fn edge_end_distance(rect: Rect, distance: f32) -> f32 {
    let w = rect.width();
    let h = rect.height();
    let perimeter = (w + h) * 2.0;
    let distance = distance.rem_euclid(perimeter);
    if distance < w {
        w
    } else if distance < w + h {
        w + h
    } else if distance < w * 2.0 + h {
        w * 2.0 + h
    } else {
        perimeter
    }
}

fn point_on_rect_perimeter(rect: Rect, distance: f32) -> Pos2 {
    let w = rect.width();
    let h = rect.height();
    let perimeter = (w + h) * 2.0;
    let distance = distance.rem_euclid(perimeter);
    if distance < w {
        Pos2::new(rect.left() + distance, rect.top())
    } else if distance < w + h {
        Pos2::new(rect.right(), rect.top() + distance - w)
    } else if distance < w * 2.0 + h {
        Pos2::new(rect.right() - (distance - w - h), rect.bottom())
    } else {
        Pos2::new(rect.left(), rect.bottom() - (distance - w * 2.0 - h))
    }
}

struct ReadOnlyTextBuffer<'a> {
    text: &'a str,
}

impl egui::TextBuffer for ReadOnlyTextBuffer<'_> {
    fn is_mutable(&self) -> bool {
        false
    }

    fn as_str(&self) -> &str {
        self.text
    }

    fn insert_text(&mut self, _text: &str, _char_index: usize) -> usize {
        0
    }

    fn delete_char_range(&mut self, _char_range: std::ops::Range<usize>) {}
}

fn paint_preview_text(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    rect: Rect,
    path: &Path,
    text: &str,
) {
    let config = app.config.clone();
    ui.painter()
        .rect_filled(rect, 6.0, theme::surface_elevated(&config));
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(1.0, theme::stroke(&config)));
    let inner = rect.shrink(10.0);
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_clip_rect(inner);
        egui::ScrollArea::both()
            .id_salt("preview_text_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut buffer = ReadOnlyTextBuffer { text };
                let line_count = text.lines().count().clamp(1, 20_000);
                let longest_line = text
                    .lines()
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap_or(1);
                let text_width = (longest_line as f32 * config.font_size * 0.62 + 16.0)
                    .clamp(inner.width().max(240.0), 6000.0);

                let output = egui::TextEdit::multiline(&mut buffer)
                    .id_salt(("preview_text_edit", path))
                    .font(egui::FontId::monospace(config.font_size))
                    .text_color(theme::text(&config))
                    .desired_width(text_width)
                    .desired_rows(line_count)
                    .frame(false)
                    .margin(egui::Margin::same(0.0))
                    .cursor_at_end(false)
                    .show(ui);

                let selected_range = output
                    .cursor_range
                    .as_ref()
                    .filter(|cursor_range| !cursor_range.is_empty())
                    .map(|cursor_range| cursor_range.as_ccursor_range());
                let selected_text = output
                    .cursor_range
                    .as_ref()
                    .filter(|cursor_range| !cursor_range.is_empty())
                    .map(|cursor_range| cursor_range.slice_str(text).to_string());
                if let (Some(selected_text), Some(selected_range)) =
                    (selected_text.as_ref(), selected_range)
                {
                    app.set_preview_text_selection(path, selected_text.clone(), selected_range);
                } else if output.response.clicked_by(egui::PointerButton::Primary) {
                    app.clear_preview_text_selection(path);
                }

                let context_text = selected_text
                    .as_deref()
                    .or_else(|| app.preview_text_selection(path))
                    .unwrap_or(text)
                    .to_string();
                output.response.context_menu(|ui| {
                    if ui.button(i18n::tr(&config, "copy")).clicked() {
                        ui.ctx().copy_text(context_text.clone());
                        ui.close_menu();
                    }
                });
                if output.response.clicked_by(egui::PointerButton::Secondary) {
                    if let Some(selection_range) = app.preview_text_selection_range(path) {
                        let mut state = output.state.clone();
                        state.cursor.set_char_range(Some(selection_range));
                        state.store(ui.ctx(), output.response.id);
                        ui.ctx().request_repaint();
                        app.mark_text_input_active();
                    }
                }

                if output.response.dragged()
                    || output.response.has_focus()
                    || selected_text.is_some()
                    || app.preview_text_selection(path).is_some()
                {
                    app.mark_text_input_active();
                }
            });
    });
}

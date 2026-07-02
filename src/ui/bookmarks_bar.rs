use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::state::BExplorerApp;
use crate::fs::explorer::{self, FileEntry};
use crate::ui::i18n;
use crate::ui::icons::{self, SidebarIcon};
use crate::ui::theme;

const BOOKMARK_BAR_HEIGHT: f32 = 42.0;
const BOOKMARK_ROW_HEIGHT: f32 = 31.0;
const BOOKMARK_ROW_VERTICAL_NUDGE: f32 = 0.0;

#[derive(Clone)]
struct BookmarkBarItem {
    path: PathBuf,
    label: String,
    entry: Option<FileEntry>,
}

pub fn show(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    let force_visible = !app.sidebar_visible;
    if !app.config.show_bookmark_bar && !force_visible {
        return;
    }

    let items = bookmark_bar_items(app, force_visible);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BOOKMARK_BAR_HEIGHT), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme::bookmark_bar(&app.config));
    paint_bookmark_bar_border(ui, &app.config, rect);

    if items.is_empty() {
        ui.painter().text(
            Pos2::new(rect.left() + 14.0, rect.center().y),
            Align2::LEFT_CENTER,
            i18n::tr(&app.config, "no_favorites"),
            theme::font(&app.config, 12.0),
            theme::muted(&app.config),
        );
        return;
    }

    let mut open_path = None;
    let content_center = Pos2::new(
        rect.center().x,
        rect.center().y + BOOKMARK_ROW_VERTICAL_NUDGE,
    );
    let content_rect =
        Rect::from_center_size(content_center, Vec2::new(rect.width(), BOOKMARK_ROW_HEIGHT));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        egui::ScrollArea::horizontal()
            .id_salt("bookmarks_bar_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_height(BOOKMARK_ROW_HEIGHT);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    for item in items {
                        if paint_bookmark_button(app, ui, &item).clicked() {
                            open_path = Some(item.path);
                        }
                    }
                });
            });
    });

    if let Some(path) = open_path {
        app.navigate_to(Some(path));
    }
}

fn bookmark_bar_items(app: &BExplorerApp, include_this_pc: bool) -> Vec<BookmarkBarItem> {
    let mut items: Vec<BookmarkBarItem> = app
        .config
        .favorites
        .iter()
        .map(|path| BookmarkBarItem {
            path: path.clone(),
            label: bookmark_path_label(path),
            entry: None,
        })
        .collect();

    if include_this_pc {
        for entry in explorer::combine_storage_and_portable_entries(
            &app.storage_entries,
            &app.portable_devices,
        ) {
            if items.iter().any(|item| item.path == entry.path) {
                continue;
            }
            items.push(BookmarkBarItem {
                path: entry.path.clone(),
                label: entry.name.clone(),
                entry: Some(entry),
            });
        }
    }

    items
}

fn bookmark_path_label(path: &Path) -> String {
    explorer::virtual_display_name(path).unwrap_or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| path.to_str().unwrap_or(""))
            .to_string()
    })
}

fn paint_bookmark_bar_border(ui: &egui::Ui, config: &crate::app::config::AppConfig, rect: Rect) {
    let border = theme::toolbar_hairline(config);
    let border_rect = rect.shrink(0.5);
    ui.painter()
        .rect_stroke(border_rect, 0.0, Stroke::new(0.45, border));
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(0.5, border),
    );
}

fn paint_bookmark_button(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    item: &BookmarkBarItem,
) -> egui::Response {
    let label = item.label.as_str();
    let font = theme::font(&app.config, 12.1);
    let text_color = theme::text(&app.config);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), text_color)
        .size()
        .x;
    let width = (text_width + 46.0).ceil().clamp(72.0, 240.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, BOOKMARK_ROW_HEIGHT), Sense::click());
    if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 5.0, &app.config);
    }

    let group_width = (16.0 + 8.0 + text_width).min(rect.width() - 16.0);
    let group_left = (rect.center().x - group_width * 0.5).max(rect.left() + 8.0);
    let icon_rect = Rect::from_center_size(
        Pos2::new(group_left + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    if let Some(texture_id) = native_bookmark_icon(app, ui.ctx(), &item.path, item.entry.as_ref()) {
        ui.painter().image(
            texture_id,
            icon_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            eframe::egui::Color32::WHITE,
        );
    } else if let Some(entry) = item.entry.as_ref() {
        icons::draw_entry_icon(ui.painter(), icon_rect, entry);
    } else {
        icons::draw_sidebar_icon(ui.painter(), icon_rect, SidebarIcon::Folder);
    }

    let text_left = group_left + 24.0;
    let text_rect = Rect::from_min_max(
        Pos2::new(text_left, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_rect).text(
        Pos2::new(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        text_color,
    );

    response.on_hover_text(path_display(&item.path))
}

fn native_bookmark_icon(
    app: &mut BExplorerApp,
    ctx: &egui::Context,
    path: &Path,
    entry: Option<&FileEntry>,
) -> Option<egui::TextureId> {
    if explorer::is_virtual_path(path) {
        return None;
    }
    let is_directory = entry
        .map(|entry| entry.kind.is_container())
        .unwrap_or_else(|| path.is_dir());
    app.native_path_icon_texture_id(ctx, path, is_directory)
}

fn path_display(path: &Path) -> String {
    PathBuf::from(path).display().to_string()
}

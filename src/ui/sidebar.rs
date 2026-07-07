use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::config::{AppConfig, SidebarSection};
use crate::app::state::BExplorerApp;
use crate::fs::explorer::{self, DriveKind, FileEntry};
use crate::ui::i18n;
use crate::ui::icons::{self, SidebarIcon};
use crate::ui::theme;

const DEFAULT_WIDTH: f32 = 220.0;
const MIN_WIDTH: f32 = 168.0;
const MAX_WIDTH: f32 = 360.0;
const SECTION_HEIGHT: f32 = 28.0;
const ROW_HEIGHT: f32 = 27.0;
const SIDEBAR_DRAG_OFFSET: Vec2 = Vec2::new(16.0, 14.0);
const SIDEBAR_DRAG_PREVIEW_SIZE: Vec2 = Vec2::new(176.0, 34.0);

pub fn visibility_t(ctx: &egui::Context, visible: bool) -> f32 {
    ctx.animate_bool_with_time(Id::new("sidebar_visibility_slide"), visible, 0.18)
}

pub fn show(app: &mut BExplorerApp, ctx: &egui::Context, visibility_t: f32) {
    let sidebar_fill = theme::sidebar(&app.config);
    if !app.config.sidebar_width.is_finite() || app.config.sidebar_width <= MIN_WIDTH + 1.0 {
        app.config.sidebar_width = DEFAULT_WIDTH;
    }
    let expanded_width = app.config.sidebar_width.clamp(MIN_WIDTH, MAX_WIDTH);
    let animating = visibility_t < 0.999;
    let animated_width = (expanded_width * visibility_t.clamp(0.0, 1.0)).max(1.0);
    let panel_id = if animating {
        Id::new(("sidebar_animating", expanded_width.round() as i32))
    } else {
        Id::new(("sidebar", expanded_width.round() as i32))
    };
    let mut panel = egui::SidePanel::left(panel_id).frame(egui::Frame::none().fill(sidebar_fill));
    if animating {
        panel = panel.resizable(false).exact_width(animated_width);
    } else {
        panel = panel
            .resizable(true)
            .default_width(expanded_width)
            .min_width(MIN_WIDTH)
            .max_width(MAX_WIDTH);
    }

    let panel = panel.show(ctx, |ui| {
        paint_sidebar_contents(app, ui, "global");
    });
    let next_width = panel.response.rect.width().clamp(MIN_WIDTH, MAX_WIDTH);
    let pending_width_id = Id::new("sidebar_pending_width");
    if animating {
        if !app.config.sidebar_width.is_finite() {
            app.config.sidebar_width = DEFAULT_WIDTH;
        }
    } else if !app.config.sidebar_width.is_finite() {
        app.config.sidebar_width = DEFAULT_WIDTH;
    } else if ctx.input(|input| input.pointer.primary_down())
        && (next_width - app.config.sidebar_width).abs() > 0.5
    {
        ctx.data_mut(|data| data.insert_temp(pending_width_id, next_width));
    }
    if !animating && ctx.input(|input| input.pointer.primary_released()) {
        let pending_width = ctx
            .data(|data| data.get_temp::<f32>(pending_width_id))
            .unwrap_or(next_width);
        if (pending_width - app.config.sidebar_width).abs() > 0.5 {
            app.config.sidebar_width = pending_width.clamp(MIN_WIDTH, MAX_WIDTH);
            app.save_config();
        }
        ctx.data_mut(|data| data.remove::<f32>(pending_width_id));
    }

    finish_drag_or_preview(app, ctx);
}

pub fn inline_width(config: &AppConfig, available_width: f32) -> f32 {
    let max_width = MAX_WIDTH.min((available_width * 0.42).max(MIN_WIDTH));
    config.sidebar_width.clamp(MIN_WIDTH, max_width)
}

pub fn global_width(config: &AppConfig, visibility_t: f32) -> f32 {
    if visibility_t <= 0.01 {
        return 0.0;
    }

    let expanded_width = config.sidebar_width.clamp(MIN_WIDTH, MAX_WIDTH);
    if visibility_t < 0.999 {
        (expanded_width * visibility_t.clamp(0.0, 1.0)).max(1.0)
    } else {
        expanded_width
    }
}

pub fn show_inline(app: &mut BExplorerApp, ui: &mut egui::Ui, rect: Rect, pane_id: usize) {
    let salt = if pane_id == 0 {
        "split-sidebar-left"
    } else {
        "split-sidebar-right"
    };
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect);
        paint_sidebar_contents(app, ui, salt);
    });
}

pub fn finish_drag_or_preview(app: &mut BExplorerApp, ctx: &egui::Context) {
    if ctx.input(|input| input.pointer.primary_released()) {
        finish_sidebar_drag(app);
    } else {
        paint_sidebar_drag_preview(app, ctx);
    }
}

fn paint_sidebar_contents(app: &mut BExplorerApp, ui: &mut egui::Ui, id_salt: &'static str) {
    let rect = ui.max_rect();
    theme::paint_sidebar_gradient(ui.painter(), rect, &app.config);
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, theme::stroke(&app.config)),
    );

    ui.add_space(6.0);

    ui.push_id(id_salt, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("sidebar_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                app.config.normalize_sidebar_order();
                let render_order = sidebar_preview_order(app);
                for section_id in render_order {
                    render_section(app, ui, section_id);
                }
            });
    });
}

fn render_section(app: &mut BExplorerApp, ui: &mut egui::Ui, section_id: SidebarSection) {
    let top = ui.cursor().min.y;
    let left = ui.min_rect().left();
    let width = ui.available_width();
    match section_id {
        SidebarSection::Recents => render_recents(app, ui),
        SidebarSection::Favorites => render_favorites(app, ui),
        SidebarSection::Storage => render_storage(app, ui),
        SidebarSection::Network => render_network(app, ui),
        SidebarSection::Places => render_places(app, ui),
    }
    let height = (ui.cursor().min.y - top).max(SECTION_HEIGHT);
    let rect = Rect::from_min_size(Pos2::new(left, top), egui::vec2(width, height));
    update_sidebar_drop_target(app, ui, section_id, rect);
}

fn section_header(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    section_id: SidebarSection,
    label: &str,
    icon: SidebarIcon,
    open: &mut bool,
    config: &AppConfig,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SECTION_HEIGHT),
        Sense::click_and_drag(),
    );
    if response.hovered() {
        theme::paint_row_hover_gradient(ui.painter(), rect, 0.0, config);
    }
    if app.sidebar_drag == Some(section_id) {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(6.0, 2.0)),
            4.0,
            egui::Color32::from_rgba_unmultiplied(
                theme::accent(config).r(),
                theme::accent(config).g(),
                theme::accent(config).b(),
                36,
            ),
        );
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Vec2::splat(16.0),
    );
    icons::draw_sidebar_icon(ui.painter(), icon_rect, icon);

    ui.painter().text(
        Pos2::new(rect.left() + 33.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(config, 12.4),
        theme::sidebar_muted(config),
    );

    draw_section_chevron(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.right() - 18.0, rect.center().y),
            Vec2::splat(12.0),
        ),
        *open,
        theme::sidebar_muted(config),
    );

    if response.clicked() {
        *open = !*open;
    }

    if response.drag_started() {
        app.sidebar_drag = Some(section_id);
        app.sidebar_drop_target = None;
    }

    response
}

fn render_recents(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    let config = app.config.clone();
    let mut open = app.sidebar_open.recents;
    let response = section_header(
        app,
        ui,
        SidebarSection::Recents,
        i18n::tr(&config, "recents"),
        SidebarIcon::Recent,
        &mut open,
        &config,
    );
    app.sidebar_open.recents = open;
    if section_content_hidden(app, &response, SidebarSection::Recents, open) {
        return;
    }
    let recents: Vec<_> = app.config.recent_paths.iter().take(5).cloned().collect();
    for path in recents {
        let label = display_label(&path);
        if sidebar_path_row(app, ui, SidebarIcon::Folder, &label, Some(&path), false) {
            app.navigate_to(Some(path));
        }
    }
}

fn render_favorites(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    let config = app.config.clone();
    let mut open = app.sidebar_open.favorites;
    let response = section_header(
        app,
        ui,
        SidebarSection::Favorites,
        i18n::tr(&config, "bookmarks"),
        SidebarIcon::Bookmark,
        &mut open,
        &config,
    );
    app.sidebar_open.favorites = open;
    if section_content_hidden(app, &response, SidebarSection::Favorites, open) {
        return;
    }
    if app.config.favorites.is_empty() {
        muted_row(ui, &config, i18n::tr(&config, "no_favorites"));
    }

    let favorites = app.config.favorites.clone();
    for path in favorites {
        let label = display_label(&path);
        match favorite_row(app, ui, &label, &path) {
            FavoriteAction::Open => app.navigate_to(Some(path.clone())),
            FavoriteAction::Remove => app.remove_favorite(&path),
            FavoriteAction::None => {}
        }
    }
}

fn render_storage(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    let config = app.config.clone();
    let mut open = app.sidebar_open.storage;
    let response = section_header(
        app,
        ui,
        SidebarSection::Storage,
        i18n::tr(&config, "storage"),
        SidebarIcon::Storage,
        &mut open,
        &config,
    );
    app.sidebar_open.storage = open;
    if section_content_hidden(app, &response, SidebarSection::Storage, open) {
        return;
    }
    let storage = app.storage_entries.clone();
    for entry in storage {
        if storage_row(app, ui, &entry) {
            app.navigate_to(Some(entry.path.clone()));
        }
    }

    let portable_devices = app.portable_devices.clone();
    if !portable_devices.is_empty() {
        muted_row(ui, &config, i18n::tr(&config, "portable_devices"));
        for device in portable_devices {
            if portable_device_row(app, ui, &device.name, &device.description) {
                app.navigate_to(Some(explorer::portable_device_path(
                    &device.id,
                    &device.name,
                )));
            }
        }
    }
}

fn render_network(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    let config = app.config.clone();
    let mut open = app.sidebar_open.network;
    let response = section_header(
        app,
        ui,
        SidebarSection::Network,
        i18n::tr(&config, "network"),
        SidebarIcon::Network,
        &mut open,
        &config,
    );
    app.sidebar_open.network = open;
    if section_content_hidden(app, &response, SidebarSection::Network, open) {
        return;
    }

    if sidebar_path_row(
        app,
        ui,
        SidebarIcon::Network,
        i18n::tr(&config, "open_network"),
        None,
        false,
    ) {
        app.navigate_to(Some(explorer::network_root_path()));
    }

    let network_drives: Vec<_> = app
        .storage_entries
        .iter()
        .filter(|entry| entry.drive_kind == Some(DriveKind::Network))
        .cloned()
        .collect();
    for entry in network_drives {
        if storage_row(app, ui, &entry) {
            app.navigate_to(Some(entry.path.clone()));
        }
    }
}

fn render_places(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    let config = app.config.clone();
    let mut open = app.sidebar_open.places;
    let response = section_header(
        app,
        ui,
        SidebarSection::Places,
        i18n::tr(&config, "places"),
        SidebarIcon::Places,
        &mut open,
        &config,
    );
    app.sidebar_open.places = open;
    if section_content_hidden(app, &response, SidebarSection::Places, open) {
        return;
    }
    if sidebar_path_row(
        app,
        ui,
        SidebarIcon::Computer,
        i18n::tr(&config, "this_pc"),
        None,
        app.active_path().is_none(),
    ) {
        app.navigate_to(None);
    }

    for place in crate::utils::paths::common_places() {
        let selected = app
            .active_path()
            .map(|path| path == place.path)
            .unwrap_or(false);
        if sidebar_path_row(
            app,
            ui,
            icons::sidebar_icon_for_label(&place.label),
            &i18n::place_label(&config, &place.label),
            Some(&place.path),
            selected,
        ) {
            app.navigate_to(Some(place.path));
        }
    }
}

fn reorder_sidebar_sections(
    app: &mut BExplorerApp,
    dragged: SidebarSection,
    target: SidebarSection,
    after: bool,
) -> bool {
    let order = sidebar_order_with_reorder(
        app.config.normalized_sidebar_order(),
        dragged,
        target,
        after,
    );
    if order == app.config.sidebar_order {
        return false;
    }
    app.config.sidebar_order = order;
    app.save_config();
    true
}

fn sidebar_preview_order(app: &BExplorerApp) -> Vec<SidebarSection> {
    let order = app.config.normalized_sidebar_order();
    let Some(dragged) = app.sidebar_drag else {
        return order;
    };
    let Some((target, after)) = app.sidebar_drop_target else {
        return order;
    };
    sidebar_order_with_reorder(order, dragged, target, after)
}

fn sidebar_order_with_reorder(
    mut order: Vec<SidebarSection>,
    dragged: SidebarSection,
    target: SidebarSection,
    after: bool,
) -> Vec<SidebarSection> {
    if dragged == target {
        return order;
    }
    let Some(from) = order.iter().position(|section| *section == dragged) else {
        return order;
    };
    order.remove(from);
    let Some(target_index) = order.iter().position(|section| *section == target) else {
        return order;
    };
    let insert_at = if after {
        target_index + 1
    } else {
        target_index
    };
    order.insert(insert_at.min(order.len()), dragged);
    order
}

fn finish_sidebar_drag(app: &mut BExplorerApp) {
    let dragged = app.sidebar_drag.take();
    let target = app.sidebar_drop_target.take();
    if let (Some(dragged), Some((target, after))) = (dragged, target) {
        reorder_sidebar_sections(app, dragged, target, after);
    }
}

fn section_content_hidden(
    app: &BExplorerApp,
    response: &egui::Response,
    section: SidebarSection,
    open: bool,
) -> bool {
    !open || response.dragged() || app.sidebar_drag == Some(section)
}

fn update_sidebar_drop_target(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    section: SidebarSection,
    rect: Rect,
) {
    let Some(dragged) = app.sidebar_drag else {
        return;
    };
    if dragged == section {
        return;
    }
    let Some(pointer) = ui.ctx().input(|input| input.pointer.hover_pos()) else {
        return;
    };
    if !rect.contains(pointer) {
        return;
    }
    let after = pointer.y > rect.center().y;
    app.sidebar_drop_target = Some((section, after));
    paint_sidebar_drop_line(ui, rect, after, &app.config);
    ui.ctx().request_repaint();
}

fn paint_sidebar_drop_line(ui: &mut egui::Ui, rect: Rect, after: bool, config: &AppConfig) {
    let y = if after { rect.bottom() } else { rect.top() };
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 10.0, y),
            Pos2::new(rect.right() - 10.0, y),
        ],
        Stroke::new(2.0, theme::accent(config)),
    );
}

fn paint_sidebar_drag_preview(app: &BExplorerApp, ctx: &egui::Context) {
    let Some(section) = app.sidebar_drag else {
        return;
    };
    let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let (label, icon) = section_label_and_icon(&app.config, section);
    let pos = sidebar_drag_preview_pos(ctx, pointer, SIDEBAR_DRAG_PREVIEW_SIZE);

    egui::Area::new(Id::new("sidebar_drag_preview"))
        .order(Order::Tooltip)
        .fixed_pos(pos)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(
                    theme::popup_surface(&app.config).r(),
                    theme::popup_surface(&app.config).g(),
                    theme::popup_surface(&app.config).b(),
                    224,
                ))
                .stroke(Stroke::new(1.0, theme::accent(&app.config)))
                .shadow(theme::popup_shadow(&app.config))
                .rounding(7.0)
                .inner_margin(egui::Margin::symmetric(9.0, 7.0))
                .show(ui, |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(SIDEBAR_DRAG_PREVIEW_SIZE, Sense::hover());
                    let icon_rect = Rect::from_center_size(
                        Pos2::new(rect.left() + 17.0, rect.center().y),
                        Vec2::splat(16.0),
                    );
                    icons::draw_sidebar_icon(ui.painter(), icon_rect, icon);
                    let text_rect = Rect::from_min_max(
                        Pos2::new(rect.left() + 36.0, rect.top()),
                        Pos2::new(rect.right() - 8.0, rect.bottom()),
                    );
                    ui.painter().with_clip_rect(text_rect).text(
                        Pos2::new(text_rect.left(), rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        theme::font(&app.config, 12.4),
                        theme::sidebar_text(&app.config),
                    );
                });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
}

fn sidebar_drag_preview_pos(ctx: &egui::Context, pointer: Pos2, size: Vec2) -> Pos2 {
    let screen = ctx.screen_rect().shrink(8.0);
    let max_x = (screen.right() - size.x).max(screen.left());
    let max_y = (screen.bottom() - size.y).max(screen.top());
    Pos2::new(
        (pointer.x + SIDEBAR_DRAG_OFFSET.x).clamp(screen.left(), max_x),
        (pointer.y + SIDEBAR_DRAG_OFFSET.y).clamp(screen.top(), max_y),
    )
}

fn section_label_and_icon(config: &AppConfig, section: SidebarSection) -> (&str, SidebarIcon) {
    match section {
        SidebarSection::Recents => (i18n::tr(config, "recents"), SidebarIcon::Recent),
        SidebarSection::Favorites => (i18n::tr(config, "bookmarks"), SidebarIcon::Bookmark),
        SidebarSection::Storage => (i18n::tr(config, "storage"), SidebarIcon::Storage),
        SidebarSection::Network => (i18n::tr(config, "network"), SidebarIcon::Network),
        SidebarSection::Places => (i18n::tr(config, "places"), SidebarIcon::Places),
    }
}

fn draw_section_chevron(painter: &egui::Painter, rect: Rect, open: bool, color: egui::Color32) {
    if open {
        painter.line_segment(
            [
                Pos2::new(rect.left() + 2.0, rect.top() + 4.0),
                Pos2::new(rect.center().x, rect.bottom() - 3.0),
            ],
            Stroke::new(1.25, color),
        );
        painter.line_segment(
            [
                Pos2::new(rect.center().x, rect.bottom() - 3.0),
                Pos2::new(rect.right() - 2.0, rect.top() + 4.0),
            ],
            Stroke::new(1.25, color),
        );
    } else {
        painter.line_segment(
            [
                Pos2::new(rect.left() + 4.0, rect.top() + 2.0),
                Pos2::new(rect.right() - 3.0, rect.center().y),
            ],
            Stroke::new(1.25, color),
        );
        painter.line_segment(
            [
                Pos2::new(rect.right() - 3.0, rect.center().y),
                Pos2::new(rect.left() + 4.0, rect.bottom() - 2.0),
            ],
            Stroke::new(1.25, color),
        );
    }
}

fn sidebar_path_row(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    icon: SidebarIcon,
    label: &str,
    path: Option<&Path>,
    selected: bool,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let row_rect = rect.shrink2(egui::vec2(8.0, 0.0));
    if selected {
        theme::paint_sidebar_row_gradient(ui.painter(), row_rect, 4.0, &app.config);
    } else if response.hovered() {
        theme::paint_row_hover_gradient(ui.painter(), row_rect, 4.0, &app.config);
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 43.0, rect.center().y),
        Vec2::splat(16.0),
    );
    if let Some(path) = path {
        if let Some(texture_id) = app.native_path_icon_texture_id(ui.ctx(), path, path.is_dir()) {
            ui.painter().image(
                texture_id,
                icon_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                eframe::egui::Color32::WHITE,
            );
        } else {
            icons::draw_sidebar_icon(ui.painter(), icon_rect, icon);
        }
    } else {
        icons::draw_sidebar_icon(ui.painter(), icon_rect, icon);
    }

    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 62.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_rect).text(
        Pos2::new(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(&app.config, 12.3),
        theme::sidebar_text(&app.config),
    );

    if let Some(path) = path {
        response.on_hover_text(path.display().to_string()).clicked()
    } else {
        response.clicked()
    }
}

enum FavoriteAction {
    None,
    Open,
    Remove,
}

fn favorite_row(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    label: &str,
    path: &Path,
) -> FavoriteAction {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let row_rect = rect.shrink2(egui::vec2(8.0, 0.0));
    if response.hovered() {
        theme::paint_row_hover_gradient(ui.painter(), row_rect, 4.0, &app.config);
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 43.0, rect.center().y),
        Vec2::splat(16.0),
    );
    if let Some(texture_id) = app.native_path_icon_texture_id(ui.ctx(), path, path.is_dir()) {
        ui.painter().image(
            texture_id,
            icon_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            eframe::egui::Color32::WHITE,
        );
    } else {
        icons::draw_sidebar_icon(ui.painter(), icon_rect, SidebarIcon::Folder);
    }

    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 62.0, rect.top()),
        Pos2::new(rect.right() - 28.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_rect).text(
        Pos2::new(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(&app.config, 12.3),
        theme::sidebar_text(&app.config),
    );

    let remove_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 18.0, rect.center().y),
        Vec2::splat(16.0),
    );
    let remove_response = ui
        .allocate_rect(remove_rect, Sense::click())
        .on_hover_text(i18n::tr(&app.config, "remove_favorite"));
    if remove_response.hovered() {
        theme::paint_hover_gradient(ui.painter(), remove_rect, 3.0, &app.config);
    }
    ui.painter().text(
        remove_rect.center(),
        Align2::CENTER_CENTER,
        "x",
        theme::font(&app.config, 11.0),
        theme::sidebar_muted(&app.config),
    );

    if remove_response.clicked() {
        FavoriteAction::Remove
    } else if response.on_hover_text(path.display().to_string()).clicked() {
        FavoriteAction::Open
    } else {
        FavoriteAction::None
    }
}

fn storage_row(app: &mut BExplorerApp, ui: &mut egui::Ui, entry: &FileEntry) -> bool {
    let selected = app
        .active_path()
        .map(|path| path == entry.path)
        .unwrap_or(false);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let row_rect = rect.shrink2(egui::vec2(8.0, 0.0));
    if selected {
        theme::paint_sidebar_row_gradient(ui.painter(), row_rect, 4.0, &app.config);
    } else if response.hovered() {
        theme::paint_row_hover_gradient(ui.painter(), row_rect, 4.0, &app.config);
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 43.0, rect.center().y),
        Vec2::splat(16.0),
    );
    if let Some(texture_id) = app.native_icon_texture_id(ui.ctx(), entry) {
        ui.painter().image(
            texture_id,
            icon_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            eframe::egui::Color32::WHITE,
        );
    } else {
        icons::draw_sidebar_icon(ui.painter(), icon_rect, SidebarIcon::Storage);
    }

    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 62.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_rect).text(
        Pos2::new(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        &entry.name,
        theme::font(&app.config, 12.3),
        theme::sidebar_text(&app.config),
    );

    let response = response.on_hover_text(entry.path.display().to_string());
    let mut eject = false;
    if crate::ui::file_table::is_ejectable_drive(entry) {
        let config = app.config.clone();
        response.context_menu(|ui| {
            begin_sidebar_context_menu(ui, &config);
            if sidebar_context_row(ui, &config, i18n::tr(&config, "eject")).clicked() {
                eject = true;
                ui.close_menu();
            }
        });
    }

    if eject {
        app.eject_drive(entry.path.clone());
        false
    } else {
        response.clicked()
    }
}

fn begin_sidebar_context_menu(ui: &mut egui::Ui, config: &AppConfig) {
    let fill = theme::popup_surface(config);
    ui.visuals_mut().window_fill = fill;
    ui.visuals_mut().panel_fill = fill;
    ui.visuals_mut().widgets.noninteractive.bg_fill = fill;
    ui.visuals_mut().window_stroke = Stroke::new(1.0, theme::popup_stroke(config));
    ui.visuals_mut().popup_shadow = theme::popup_shadow(config);
    ui.set_min_width(168.0);
    ui.set_max_width(168.0);
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
}

fn sidebar_context_row(ui: &mut egui::Ui, config: &AppConfig, label: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
    if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
    draw_sidebar_eject_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 18.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        theme::sidebar_muted(config),
    );
    ui.painter().text(
        Pos2::new(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(config, 12.3),
        theme::sidebar_text(config),
    );
    response
}

fn draw_sidebar_eject_icon(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let triangle = vec![
        Pos2::new(rect.center().x, rect.top() + 4.0),
        Pos2::new(rect.left() + 4.0, rect.center().y + 2.0),
        Pos2::new(rect.right() - 4.0, rect.center().y + 2.0),
    ];
    painter.add(egui::Shape::closed_line(triangle, Stroke::new(1.2, color)));
    painter.line_segment(
        [
            Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0),
            Pos2::new(rect.right() - 4.0, rect.bottom() - 4.0),
        ],
        Stroke::new(1.3, color),
    );
}

fn portable_device_row(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    label: &str,
    description: &str,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let row_rect = rect.shrink2(egui::vec2(8.0, 0.0));
    if response.hovered() {
        theme::paint_row_hover_gradient(ui.painter(), row_rect, 4.0, &app.config);
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 43.0, rect.center().y),
        Vec2::splat(16.0),
    );
    icons::draw_sidebar_icon(ui.painter(), icon_rect, SidebarIcon::Device);

    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 62.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_rect).text(
        Pos2::new(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(&app.config, 12.3),
        theme::sidebar_text(&app.config),
    );

    let response = if description.trim().is_empty() {
        response
    } else {
        response.on_hover_text(description)
    };
    response.clicked()
}

fn muted_row(ui: &mut egui::Ui, config: &AppConfig, text: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    ui.painter().text(
        Pos2::new(rect.left() + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::font(config, 12.0),
        theme::sidebar_faint(config),
    );
}

fn display_label(path: &PathBuf) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

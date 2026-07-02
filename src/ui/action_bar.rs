use eframe::egui::{self, Align2, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::config::AppConfig;
use crate::app::state::{BExplorerApp, FileGroup};
use crate::ui::i18n;
use crate::ui::theme;

const ACTION_BAR_HEIGHT: f32 = 42.0;
const ACTION_ROW_HEIGHT: f32 = 31.0;
const ACTION_ROW_VERTICAL_NUDGE: f32 = 0.0;

#[derive(Clone, Copy)]
enum ActionBarCommand {
    Paste,
    Copy,
    Cut,
    Rename,
    Delete,
    Compress,
    Preview,
}

#[derive(Clone, Copy)]
enum ActionBarIcon {
    New,
    Paste,
    Copy,
    Cut,
    Rename,
    Delete,
    Compress,
    Group,
    Preview,
}

pub fn show(app: &mut BExplorerApp, ui: &mut egui::Ui, pane_id: usize) {
    if !app.config.show_action_bar {
        app.action_bar_new_menu_open = false;
        return;
    }

    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ACTION_BAR_HEIGHT), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme::action_bar(&app.config));
    paint_action_bar_border(ui, &app.config, rect);

    let has_selection = !app.selected.is_empty();
    let single_selection = app.selected.len() == 1;
    let can_paste = app.can_paste();
    let mut command = None;
    let content_center = Pos2::new(rect.center().x, rect.center().y + ACTION_ROW_VERTICAL_NUDGE);
    let content_rect =
        Rect::from_center_size(content_center, Vec2::new(rect.width(), ACTION_ROW_HEIGHT));
    let labels = ActionLabels::new(&app.config);

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.set_clip_rect(content_rect);
        ui.set_height(ACTION_ROW_HEIGHT);
        ui.spacing_mut().item_spacing.x = 2.0;
        let group_button_width = action_button_width(ui, &app.config, &labels.group, true);
        let preview_button_width = action_button_width(ui, &app.config, &labels.preview, false);
        let right_controls_width =
            (group_button_width + preview_button_width + 2.0).min(content_rect.width().max(0.0));
        let right_controls_rect = Rect::from_min_size(
            Pos2::new(
                content_rect.right() - right_controls_width - 8.0,
                content_rect.top(),
            ),
            Vec2::new(right_controls_width, ACTION_ROW_HEIGHT),
        );
        let left_rect = Rect::from_min_max(
            content_rect.left_top(),
            Pos2::new(
                (right_controls_rect.left() - 6.0).max(content_rect.left()),
                content_rect.bottom(),
            ),
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left_rect), |ui| {
            ui.set_clip_rect(left_rect);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let new_response = paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::New,
                    &labels.new,
                    true,
                    true,
                );
                crate::ui::file_table::show_main_new_submenu(app, ui.ctx(), &new_response, pane_id);
                if paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Paste,
                    &labels.paste,
                    can_paste,
                    false,
                )
                .clicked()
                {
                    command = Some(ActionBarCommand::Paste);
                }
                if paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Copy,
                    &labels.copy,
                    has_selection,
                    false,
                )
                .clicked()
                {
                    command = Some(ActionBarCommand::Copy);
                }
                if paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Cut,
                    &labels.cut,
                    has_selection,
                    false,
                )
                .clicked()
                {
                    command = Some(ActionBarCommand::Cut);
                }
                if paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Rename,
                    &labels.rename,
                    single_selection,
                    false,
                )
                .clicked()
                {
                    command = Some(ActionBarCommand::Rename);
                }
                if paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Delete,
                    &labels.delete,
                    has_selection,
                    false,
                )
                .clicked()
                {
                    command = Some(ActionBarCommand::Delete);
                }
                if paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Compress,
                    &labels.compress,
                    has_selection,
                    false,
                )
                .clicked()
                {
                    command = Some(ActionBarCommand::Compress);
                }
            });
        });

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right_controls_rect), |ui| {
            ui.set_clip_rect(right_controls_rect);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let group_response = paint_action_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Group,
                    &labels.group,
                    true,
                    true,
                );
                show_group_popup(app, ui, pane_id, &group_response);

                if paint_action_toggle_button(
                    ui,
                    &app.config,
                    ActionBarIcon::Preview,
                    &labels.preview,
                    true,
                    false,
                    app.preview_panel_visible,
                )
                .clicked()
                {
                    command = Some(ActionBarCommand::Preview);
                }
            });
        });
    });

    if let Some(command) = command {
        match command {
            ActionBarCommand::Paste => app.paste_into_active(),
            ActionBarCommand::Copy => app.copy_selection(false),
            ActionBarCommand::Cut => app.copy_selection(true),
            ActionBarCommand::Rename => app.rename_selected(),
            ActionBarCommand::Delete => app.request_delete_selected(false),
            ActionBarCommand::Compress => app.open_compress_dialog(),
            ActionBarCommand::Preview => {
                app.preview_panel_visible = !app.preview_panel_visible;
                app.config.show_preview_panel = app.preview_panel_visible;
                app.save_config();
            }
        }
    }
}

struct ActionLabels {
    new: String,
    paste: String,
    copy: String,
    cut: String,
    rename: String,
    delete: String,
    compress: String,
    group: String,
    preview: String,
}

impl ActionLabels {
    fn new(config: &AppConfig) -> Self {
        Self {
            new: i18n::tr(config, "new").to_owned(),
            paste: i18n::tr(config, "paste").to_owned(),
            copy: i18n::tr(config, "copy").to_owned(),
            cut: i18n::tr(config, "cut").to_owned(),
            rename: i18n::tr(config, "rename").to_owned(),
            delete: i18n::tr(config, "delete").to_owned(),
            compress: i18n::tr(config, "compress").to_owned(),
            group: i18n::tr(config, "group").to_owned(),
            preview: i18n::tr(config, "preview").to_owned(),
        }
    }
}

fn paint_action_bar_border(ui: &egui::Ui, config: &AppConfig, rect: Rect) {
    let border = theme::toolbar_hairline(config);
    let border_rect = rect.shrink(0.5);
    ui.painter()
        .rect_stroke(border_rect, 0.0, Stroke::new(0.45, border));
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(0.5, border),
    );
}

fn show_group_popup(
    app: &mut BExplorerApp,
    ui: &mut egui::Ui,
    pane_id: usize,
    response: &egui::Response,
) {
    let popup_id = ui.make_persistent_id(("action_group_menu", pane_id));
    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }

    let (current_group, ascending) = app.group_state();
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        response,
        egui::AboveOrBelow::Below,
        egui::popup::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(190.0);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
            for (group, label) in [
                (FileGroup::Name, i18n::tr(&app.config, "name")),
                (FileGroup::Type, i18n::tr(&app.config, "type")),
                (FileGroup::TotalSize, i18n::tr(&app.config, "total_size")),
                (
                    FileGroup::FreeSpace,
                    i18n::tr(&app.config, "available_space"),
                ),
                (FileGroup::None, i18n::tr(&app.config, "none_group")),
            ] {
                if paint_group_menu_row(ui, &app.config, current_group == group, label).clicked() {
                    app.set_group(group);
                    ui.memory_mut(|memory| memory.close_popup());
                }
            }
            ui.separator();
            if paint_group_menu_row(
                ui,
                &app.config,
                ascending,
                i18n::tr(&app.config, "ascending"),
            )
            .clicked()
            {
                app.set_group_ascending(true);
                ui.memory_mut(|memory| memory.close_popup());
            }
            if paint_group_menu_row(
                ui,
                &app.config,
                !ascending,
                i18n::tr(&app.config, "descending"),
            )
            .clicked()
            {
                app.set_group_ascending(false);
                ui.memory_mut(|memory| memory.close_popup());
            }
        },
    );
}

fn paint_group_menu_row(
    ui: &mut egui::Ui,
    config: &AppConfig,
    selected: bool,
    label: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(190.0, 27.0), Sense::click());
    if selected {
        theme::paint_sidebar_row_gradient(ui.painter(), rect, 4.0, config);
    } else if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 4.0, config);
    }
    if selected {
        ui.painter().circle_filled(
            Pos2::new(rect.left() + 15.0, rect.center().y),
            2.0,
            theme::text(config),
        );
    }
    ui.painter().text(
        Pos2::new(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(config, 12.5),
        if selected {
            theme::text(config)
        } else {
            theme::muted(config)
        },
    );
    response
}

fn paint_action_button(
    ui: &mut egui::Ui,
    config: &AppConfig,
    icon: ActionBarIcon,
    label: &str,
    enabled: bool,
    submenu: bool,
) -> egui::Response {
    paint_action_button_inner(ui, config, icon, label, enabled, submenu, false)
}

fn paint_action_toggle_button(
    ui: &mut egui::Ui,
    config: &AppConfig,
    icon: ActionBarIcon,
    label: &str,
    enabled: bool,
    submenu: bool,
    active: bool,
) -> egui::Response {
    paint_action_button_inner(ui, config, icon, label, enabled, submenu, active)
}

fn paint_action_button_inner(
    ui: &mut egui::Ui,
    config: &AppConfig,
    icon: ActionBarIcon,
    label: &str,
    enabled: bool,
    submenu: bool,
    active: bool,
) -> egui::Response {
    let font = theme::font(config, 12.1);
    let text_color = if enabled {
        theme::text(config)
    } else {
        theme::muted(config).gamma_multiply(0.62)
    };
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), text_color)
        .size()
        .x;
    let width = action_button_width_for_text(text_width, submenu);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ACTION_ROW_HEIGHT), sense);

    if enabled && active {
        theme::paint_selection_gradient(ui.painter(), rect, config);
    } else if enabled && response.hovered() {
        theme::paint_hover_gradient(ui.painter(), rect, 5.0, config);
    }

    let group_width =
        (16.0 + 8.0 + text_width + if submenu { 16.0 } else { 0.0 }).min(rect.width() - 14.0);
    let group_left = (rect.center().x - group_width * 0.5).max(rect.left() + 7.0);
    let icon_rect = Rect::from_center_size(
        Pos2::new(group_left + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    paint_action_icon(ui, icon_rect, config, icon, enabled);

    let text_rect = Rect::from_min_max(
        Pos2::new(group_left + 24.0, rect.top()),
        Pos2::new(
            rect.right() - if submenu { 23.0 } else { 7.0 },
            rect.bottom(),
        ),
    );
    ui.painter().with_clip_rect(text_rect).text(
        Pos2::new(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        text_color,
    );

    if submenu {
        paint_action_chevron(
            ui,
            Rect::from_center_size(
                Pos2::new(rect.right() - 10.0, rect.center().y),
                Vec2::splat(12.0),
            ),
            text_color,
        );
    }

    response.on_hover_text(label)
}

fn action_button_width(ui: &egui::Ui, config: &AppConfig, label: &str, submenu: bool) -> f32 {
    let text_width = ui
        .painter()
        .layout_no_wrap(
            label.to_string(),
            theme::font(config, 12.1),
            theme::text(config),
        )
        .size()
        .x;
    action_button_width_for_text(text_width, submenu)
}

fn action_button_width_for_text(text_width: f32, submenu: bool) -> f32 {
    (text_width + if submenu { 58.0 } else { 43.0 })
        .ceil()
        .clamp(74.0, 154.0)
}

fn paint_action_icon(
    ui: &egui::Ui,
    rect: Rect,
    config: &AppConfig,
    icon: ActionBarIcon,
    enabled: bool,
) {
    let color = if enabled {
        theme::text(config)
    } else {
        theme::muted(config).gamma_multiply(0.62)
    };
    let accent = if enabled {
        theme::accent(config)
    } else {
        theme::muted(config).gamma_multiply(0.48)
    };
    let stroke = Stroke::new(1.25, color);
    let painter = ui.painter();

    match icon {
        ActionBarIcon::New => {
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 3.5, rect.center().y),
                    Pos2::new(rect.right() - 3.5, rect.center().y),
                ],
                Stroke::new(1.45, color),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.center().x, rect.top() + 3.5),
                    Pos2::new(rect.center().x, rect.bottom() - 3.5),
                ],
                Stroke::new(1.45, color),
            );
        }
        ActionBarIcon::Paste => {
            let body = Rect::from_min_max(
                Pos2::new(rect.left() + 3.0, rect.top() + 4.0),
                Pos2::new(rect.right() - 3.0, rect.bottom() - 2.0),
            );
            painter.rect_stroke(body, 2.0, stroke);
            let clip = Rect::from_min_max(
                Pos2::new(rect.left() + 5.0, rect.top() + 1.5),
                Pos2::new(rect.right() - 5.0, rect.top() + 6.0),
            );
            painter.rect_filled(clip, 1.5, accent);
        }
        ActionBarIcon::Copy => {
            let back = Rect::from_min_max(
                Pos2::new(rect.left() + 3.0, rect.top() + 4.0),
                Pos2::new(rect.right() - 6.0, rect.bottom() - 3.0),
            );
            let front = back.translate(Vec2::new(3.0, -2.0));
            painter.rect_stroke(back, 2.0, Stroke::new(1.1, color.gamma_multiply(0.72)));
            painter.rect_stroke(front, 2.0, stroke);
        }
        ActionBarIcon::Cut => {
            let c1 = Pos2::new(rect.left() + 4.0, rect.top() + 4.0);
            let c2 = Pos2::new(rect.right() - 4.0, rect.bottom() - 4.0);
            let c3 = Pos2::new(rect.right() - 4.0, rect.top() + 4.0);
            let c4 = Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0);
            painter.line_segment([c1, c2], stroke);
            painter.line_segment([c3, c4], stroke);
            painter.circle_stroke(Pos2::new(rect.left() + 4.0, rect.top() + 4.0), 2.2, stroke);
            painter.circle_stroke(
                Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0),
                2.2,
                stroke,
            );
        }
        ActionBarIcon::Rename => {
            let line_y = rect.bottom() - 3.0;
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 3.0, line_y),
                    Pos2::new(rect.right() - 3.0, line_y),
                ],
                Stroke::new(1.0, color.gamma_multiply(0.72)),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 6.0, rect.bottom() - 5.0),
                    Pos2::new(rect.right() - 4.0, rect.top() + 5.0),
                ],
                Stroke::new(1.6, accent),
            );
            painter.circle_filled(Pos2::new(rect.right() - 4.0, rect.top() + 5.0), 1.7, color);
        }
        ActionBarIcon::Delete => {
            let lid_y = rect.top() + 4.0;
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 3.5, lid_y),
                    Pos2::new(rect.right() - 3.5, lid_y),
                ],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.center().x - 3.0, lid_y - 2.0),
                    Pos2::new(rect.center().x + 3.0, lid_y - 2.0),
                ],
                Stroke::new(1.2, color),
            );
            let body = Rect::from_min_max(
                Pos2::new(rect.left() + 5.0, rect.top() + 6.0),
                Pos2::new(rect.right() - 5.0, rect.bottom() - 3.0),
            );
            painter.line_segment([body.left_top(), body.left_bottom()], stroke);
            painter.line_segment([body.right_top(), body.right_bottom()], stroke);
            painter.line_segment([body.left_bottom(), body.right_bottom()], stroke);
            for x in [body.left() + 3.0, body.center().x, body.right() - 3.0] {
                painter.line_segment(
                    [
                        Pos2::new(x, body.top() + 2.0),
                        Pos2::new(x, body.bottom() - 2.0),
                    ],
                    Stroke::new(0.95, color.gamma_multiply(0.82)),
                );
            }
        }
        ActionBarIcon::Compress => {
            let box_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 2.5, rect.top() + 4.0),
                Pos2::new(rect.right() - 2.5, rect.bottom() - 3.0),
            );
            painter.rect_stroke(box_rect, 2.0, stroke);
            let x = box_rect.center().x;
            painter.line_segment(
                [
                    Pos2::new(x, box_rect.top()),
                    Pos2::new(x, box_rect.bottom()),
                ],
                Stroke::new(1.0, accent),
            );
            for y in [
                box_rect.top() + 2.0,
                box_rect.top() + 5.0,
                box_rect.top() + 8.0,
            ] {
                painter.line_segment(
                    [Pos2::new(x - 1.8, y), Pos2::new(x + 1.8, y)],
                    Stroke::new(1.0, accent),
                );
            }
        }
        ActionBarIcon::Group => {
            let center_y = rect.center().y - 1.0;
            for index in 0..3 {
                let y = center_y - 6.0 + index as f32 * 5.0;
                ui.painter()
                    .circle_filled(Pos2::new(rect.left() + 3.0, y + 1.0), 1.2, accent);
                ui.painter().line_segment(
                    [
                        Pos2::new(rect.left() + 7.0, y + 1.0),
                        Pos2::new(rect.right() - 2.0, y + 1.0),
                    ],
                    Stroke::new(1.3, color),
                );
            }
        }
        ActionBarIcon::Preview => {
            let outer = Rect::from_min_size(
                Pos2::new(rect.left() + 2.5, rect.top() + 3.5),
                Vec2::new(12.0, 9.5),
            );
            painter.rect_stroke(outer, 1.5, stroke);
            painter.line_segment(
                [
                    Pos2::new(outer.right() - 4.0, outer.top()),
                    Pos2::new(outer.right() - 4.0, outer.bottom()),
                ],
                Stroke::new(1.2, accent),
            );
        }
    }
}

fn paint_action_chevron(ui: &egui::Ui, rect: Rect, color: eframe::egui::Color32) {
    let center = rect.center();
    let half_width = 3.3;
    let top_y = center.y - 1.2;
    let bottom = Pos2::new(center.x, center.y + 2.0);
    let stroke = Stroke::new(1.45, color);
    ui.painter()
        .line_segment([Pos2::new(center.x - half_width, top_y), bottom], stroke);
    ui.painter()
        .line_segment([bottom, Pos2::new(center.x + half_width, top_y)], stroke);
}

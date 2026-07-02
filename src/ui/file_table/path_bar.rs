use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::config::AppConfig;
use crate::app::state::BExplorerApp;
use crate::fs::explorer;
use crate::ui::{i18n, theme};

use super::text::{draw_text, draw_text_elided};
use super::{
    CONTEXT_MENU_MAX_WIDTH, CONTEXT_MENU_MIN_WIDTH, MenuIcon, MenuRowMeasure, TOOLBAR_HEIGHT,
    apply_context_menu_width, begin_context_menu_animation, context_menu_row,
    context_menu_row_width, context_menu_width, menu_text_width,
};

pub(super) fn show_navigation_bar(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, TOOLBAR_HEIGHT), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme::canvas(&app.config));
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );

    let mut x = rect.left() + 12.0;
    let y = rect.center().y;

    let buttons = [
        ("back", app.can_go_back(), -1.0),
        ("forward", app.can_go_forward(), 1.0),
    ];

    for (tooltip_key, enabled, direction) in buttons {
        let button_rect = Rect::from_center_size(Pos2::new(x + 10.0, y), Vec2::splat(24.0));
        let response = ui
            .allocate_rect(button_rect, Sense::click())
            .on_hover_text(i18n::tr(&app.config, tooltip_key));
        paint_nav_arrow(
            ui,
            &app.config,
            button_rect,
            direction,
            enabled,
            response.hovered(),
        );
        if response.clicked() && enabled {
            match tooltip_key {
                "back" => app.go_back(),
                "forward" => app.go_forward(),
                _ => {}
            }
        }
        x += 28.0;
    }

    let up_enabled = app.can_go_up();
    let button_rect = Rect::from_center_size(Pos2::new(x + 10.0, y), Vec2::splat(24.0));
    let response = ui
        .allocate_rect(button_rect, Sense::click())
        .on_hover_text(i18n::tr(&app.config, "up"));
    paint_nav_up_arrow(ui, &app.config, button_rect, up_enabled, response.hovered());
    if response.clicked() && up_enabled {
        app.go_up();
    }
    x += 28.0;

    x += 6.0;
    ui.painter().line_segment(
        [
            Pos2::new(x, rect.top() + 9.0),
            Pos2::new(x, rect.bottom() - 9.0),
        ],
        Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );
    x += 14.0;

    let fav_path = app.active_path();
    let is_fav = fav_path
        .as_deref()
        .map(|p| app.is_favorite(p))
        .unwrap_or(false);
    let button_rect = Rect::from_center_size(Pos2::new(x + 10.0, y), Vec2::splat(24.0));
    let response = ui
        .allocate_rect(button_rect, Sense::click())
        .on_hover_text(if is_fav {
            i18n::tr(&app.config, "remove_favorite")
        } else {
            i18n::tr(&app.config, "add_favorite")
        });
    if response.hovered() {
        theme::paint_hover_gradient(ui.painter(), button_rect.shrink(1.0), 5.0, &app.config);
    }
    draw_favorite_toggle(ui.painter(), button_rect, is_fav, &app.config);
    if response.clicked()
        && let Some(path) = fav_path
    {
        app.toggle_favorite(path);
    }
    x += 28.0;

    x += 6.0;
    ui.painter().line_segment(
        [
            Pos2::new(x, rect.top() + 9.0),
            Pos2::new(x, rect.bottom() - 9.0),
        ],
        Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );
    x += 6.0;

    let path_bar_rect = Rect::from_min_max(
        Pos2::new(x - 4.0, rect.top() + 6.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 6.0),
    );
    ui.painter()
        .rect_filled(path_bar_rect, 6.0, theme::surface_elevated(&app.config));

    if app.path_bar_text_visible {
        show_path_bar_editor(app, ui, path_bar_rect);
    } else {
        let crumbs = breadcrumbs(app.active_path().as_deref());
        for (index, crumb) in crumbs.iter().enumerate() {
            if index > 0 {
                let chevron_rect =
                    Rect::from_center_size(Pos2::new(x + 5.0, y), Vec2::new(10.0, 12.0));
                draw_breadcrumb_chevron(ui.painter(), chevron_rect, theme::faint(&app.config));
                x += 16.0;
            }
            let crumb_text = if crumb.label == "This PC" {
                i18n::tr(&app.config, "this_pc")
            } else {
                &crumb.label
            };
            let color = if index + 1 == crumbs.len() {
                theme::text(&app.config)
            } else {
                theme::muted(&app.config)
            };
            let crumb_font = theme::font(&app.config, 12.5);
            let crumb_width = (menu_text_width(ui, crumb_text, crumb_font.clone(), color) + 16.0)
                .ceil()
                .clamp(36.0, 230.0);
            let crumb_rect = Rect::from_min_size(
                Pos2::new(x - 6.0, rect.top() + 7.0),
                Vec2::new(crumb_width, 22.0),
            );
            let response = ui
                .allocate_rect(crumb_rect, Sense::click())
                .on_hover_text(crumb_text.to_string());
            if index + 1 == crumbs.len() {
                theme::paint_row_hover_gradient(ui.painter(), crumb_rect, 4.0, &app.config);
            } else if response.hovered() {
                theme::paint_hover_gradient(ui.painter(), crumb_rect, 4.0, &app.config);
            }
            if response.clicked() && index + 1 == crumbs.len() {
                activate_path_bar_editor(app, ui);
            } else if response.clicked() {
                app.navigate_to(crumb.path.clone());
            }
            draw_text(
                ui,
                Pos2::new(x, y),
                crumb_text,
                crumb_font,
                color,
                Align2::LEFT_CENTER,
            );
            x += crumb_width;
        }

        let empty_rect = Rect::from_min_max(
            Pos2::new(x.max(path_bar_rect.left() + 8.0), path_bar_rect.top()),
            path_bar_rect.right_bottom(),
        );
        if empty_rect.width() > 6.0 {
            let path_text = path_bar_display_text(app);
            let empty_response = ui
                .allocate_rect(empty_rect, Sense::click())
                .on_hover_text(i18n::tr(&app.config, "copy_path"));
            if empty_response.clicked() || empty_response.secondary_clicked() {
                activate_path_bar_editor(app, ui);
            }
            path_bar_context_menu(app, &empty_response, &path_text);
        }
    }

    ui.painter().rect_stroke(
        path_bar_rect,
        6.0,
        Stroke::new(1.0, theme::stroke(&app.config)),
    );
    ui.painter().line_segment(
        [
            Pos2::new(path_bar_rect.left() + 6.0, path_bar_rect.bottom() - 0.5),
            Pos2::new(path_bar_rect.right() - 6.0, path_bar_rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::subtle_stroke(&app.config)),
    );

    ui.advance_cursor_after_rect(rect);
}

fn activate_path_bar_editor(app: &mut BExplorerApp, ui: &mut egui::Ui) {
    app.path_bar_edit_text = path_bar_editor_text(app);
    app.path_bar_text_visible = true;
    app.path_bar_focus_pending = true;
    ui.ctx().request_repaint();
}

fn show_path_bar_editor(app: &mut BExplorerApp, ui: &mut egui::Ui, rect: Rect) {
    let config = app.config.clone();
    let editor_id = ui.make_persistent_id("path-bar-editor");
    let edit_rect = rect.shrink2(egui::vec2(9.0, 2.0));
    let mut commit = false;
    let mut cancel = false;
    let mut lost_focus = false;

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
        ui.visuals_mut().override_text_color = Some(theme::text(&config));
        let mut output = egui::TextEdit::singleline(&mut app.path_bar_edit_text)
            .id_salt(editor_id)
            .frame(false)
            .desired_width(edit_rect.width())
            .show(ui);

        if app.path_bar_focus_pending {
            output.response.request_focus();
            let end = app.path_bar_edit_text.chars().count();
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(end),
                )));
            output.state.clone().store(ui.ctx(), output.response.id);
            app.path_bar_focus_pending = false;
            app.path_bar_selection_range = Some((0, end));
        }

        if output.response.has_focus() {
            app.mark_text_input_active();
        }

        let response = output.response.clone();
        let response_id = response.id;
        let editor_state = output.state.clone();
        let current_range = editor_state.cursor.char_range().map(|range| {
            let [start, end] = range.sorted();
            start.index..end.index
        });
        let restore_range = if response.secondary_clicked() {
            app.path_bar_selection_range
                .map(|(start, end)| start..end)
                .filter(|range| range.start != range.end)
        } else {
            None
        };
        let selected_range = restore_range.clone().or_else(|| current_range.clone());
        if let Some(range) = &restore_range {
            let mut next_state = editor_state.clone();
            next_state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(range.start),
                    egui::text::CCursor::new(range.end),
                )));
            next_state.store(ui.ctx(), response_id);
            ui.ctx().request_repaint();
        } else if let Some(range) = &current_range {
            app.path_bar_selection_range = if range.start == range.end {
                None
            } else {
                Some((range.start, range.end))
            };
        }
        let selected_text = selected_range
            .as_ref()
            .map(|range| path_bar_text_char_range(&app.path_bar_edit_text, range.clone()))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| app.path_bar_edit_text.clone());
        let copy_enabled = !selected_text.is_empty();

        response.context_menu(|ui| {
            begin_context_menu_animation(ui, &config, "path-bar-editor-context-menu");
            let rows = [
                MenuRowMeasure {
                    label: i18n::tr(&config, "copy"),
                    shortcut: Some("Ctrl+C"),
                    submenu: false,
                },
                MenuRowMeasure {
                    label: i18n::tr(&config, "paste"),
                    shortcut: Some("Ctrl+V"),
                    submenu: false,
                },
            ];
            let menu_width = context_menu_width(
                ui,
                &config,
                &rows,
                CONTEXT_MENU_MIN_WIDTH,
                CONTEXT_MENU_MAX_WIDTH,
            );
            apply_context_menu_width(ui, menu_width);

            if context_menu_row(
                ui,
                &config,
                MenuIcon::Copy,
                i18n::tr(&config, "copy"),
                Some("Ctrl+C"),
                copy_enabled,
                false,
            )
            .clicked()
                && copy_enabled
            {
                match crate::platform::shell::copy_text(&selected_text) {
                    Ok(()) => app.status_message = "Path copied".into(),
                    Err(error) => app.status_message = error.to_string(),
                }
                ui.close_menu();
            }

            if context_menu_row(
                ui,
                &config,
                MenuIcon::Paste,
                i18n::tr(&config, "paste"),
                Some("Ctrl+V"),
                true,
                false,
            )
            .clicked()
            {
                match crate::platform::shell::read_text() {
                    Ok(text) => {
                        let insertion_range = selected_range.clone().unwrap_or_else(|| {
                            editor_state
                                .cursor
                                .char_range()
                                .map(|range| range.primary.index..range.primary.index)
                                .unwrap_or_else(|| {
                                    let end = app.path_bar_edit_text.chars().count();
                                    end..end
                                })
                        });
                        let cursor = replace_path_bar_text_range(
                            &mut app.path_bar_edit_text,
                            insertion_range,
                            &text,
                        );
                        let mut next_state = editor_state.clone();
                        next_state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(
                                egui::text::CCursor::new(cursor),
                            )));
                        next_state.store(ui.ctx(), response_id);
                    }
                    Err(error) => app.status_message = error.to_string(),
                }
                ui.close_menu();
            }
        });

        commit = ui.input(|input| input.key_pressed(egui::Key::Enter));
        cancel = ui.input(|input| input.key_pressed(egui::Key::Escape));
        lost_focus = output.response.lost_focus() && !output.response.context_menu_opened();
    });

    if cancel {
        app.path_bar_text_visible = false;
        app.path_bar_focus_pending = false;
        app.path_bar_selection_range = None;
        return;
    }

    if commit {
        let value = app.path_bar_edit_text.clone();
        if commit_path_bar_editor(app, &value) {
            app.path_bar_text_visible = false;
            app.path_bar_focus_pending = false;
            app.path_bar_selection_range = None;
        } else {
            app.path_bar_focus_pending = true;
        }
        return;
    }

    if lost_focus {
        app.path_bar_text_visible = false;
        app.path_bar_focus_pending = false;
        app.path_bar_selection_range = None;
    }
}

fn path_bar_text_char_range(text: &str, range: std::ops::Range<usize>) -> String {
    let start = char_to_byte_index(text, range.start);
    let end = char_to_byte_index(text, range.end);
    text.get(start..end).unwrap_or("").to_string()
}

fn replace_path_bar_text_range(
    text: &mut String,
    range: std::ops::Range<usize>,
    replacement: &str,
) -> usize {
    let start = char_to_byte_index(text, range.start);
    let end = char_to_byte_index(text, range.end);
    text.replace_range(start..end, replacement);
    range.start + replacement.chars().count()
}

pub(super) fn char_to_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn path_bar_display_text(app: &BExplorerApp) -> String {
    let Some(path) = app.active_path() else {
        return i18n::tr(&app.config, "this_pc").to_string();
    };

    if explorer::is_virtual_path(&path) || explorer::unc_breadcrumbs(&path).is_some() {
        return breadcrumbs(Some(&path))
            .into_iter()
            .map(|crumb| {
                if crumb.label == "This PC" {
                    i18n::tr(&app.config, "this_pc").to_string()
                } else {
                    crumb.label
                }
            })
            .collect::<Vec<_>>()
            .join(" > ");
    }

    path.display().to_string()
}

fn path_bar_editor_text(app: &BExplorerApp) -> String {
    let Some(path) = app.active_path() else {
        return i18n::tr(&app.config, "this_pc").to_string();
    };

    if explorer::is_virtual_path(&path) {
        path_bar_display_text(app)
    } else {
        path.display().to_string()
    }
}

fn commit_path_bar_editor(app: &mut BExplorerApp, value: &str) -> bool {
    let Some(target) = path_from_path_bar_input(app, value) else {
        return false;
    };

    let Some(path) = target else {
        app.navigate_to(None);
        return true;
    };

    if explorer::is_virtual_path(&path)
        || crate::fs::archive_listing::is_archive_navigation_path(&path)
        || path.is_dir()
    {
        app.navigate_to(Some(path));
        return true;
    }

    if path.is_file() {
        app.open_location_for(&path);
        return true;
    }

    app.status_message = format!("Path does not exist: {}", path.display());
    false
}

fn path_from_path_bar_input(app: &mut BExplorerApp, value: &str) -> Option<Option<PathBuf>> {
    let text = cleaned_path_bar_input(value);
    if text.is_empty() {
        app.status_message = "Enter a path".into();
        return None;
    }

    if text.eq_ignore_ascii_case("this pc")
        || text.eq_ignore_ascii_case("este equipo")
        || text.eq_ignore_ascii_case(i18n::tr(&app.config, "this_pc"))
    {
        return Some(None);
    }

    let text = expand_leading_env_var(&text);
    let text = drive_root_input(&text).unwrap_or(text);
    let mut path = PathBuf::from(&text);

    if path.is_relative()
        && !text.starts_with("__bexplorer_virtual__")
        && !text.starts_with("\\\\")
        && let Some(base) = app.active_path()
        && base.is_dir()
    {
        path = base.join(path);
    }

    Some(Some(path))
}

fn cleaned_path_bar_input(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return trimmed[1..trimmed.len() - 1].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn drive_root_input(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        Some(format!("{}:\\", value))
    } else {
        None
    }
}

fn expand_leading_env_var(value: &str) -> String {
    let Some(rest) = value.strip_prefix('%') else {
        return value.to_string();
    };
    let Some(end) = rest.find('%') else {
        return value.to_string();
    };
    let var = &rest[..end];
    let suffix = &rest[end + 1..];
    match std::env::var(var) {
        Ok(prefix) => format!("{prefix}{suffix}"),
        Err(_) => value.to_string(),
    }
}

fn path_bar_context_menu(app: &mut BExplorerApp, response: &egui::Response, path_text: &str) {
    let config = app.config.clone();
    let path_text = path_text.to_string();
    response.context_menu(|ui| {
        begin_context_menu_animation(ui, &config, "path-bar-context-menu");
        let rows = [
            MenuRowMeasure {
                label: i18n::tr(&config, "copy_path"),
                shortcut: None,
                submenu: false,
            },
            MenuRowMeasure {
                label: i18n::tr(&config, "paste"),
                shortcut: Some("Ctrl+V"),
                submenu: false,
            },
        ];
        let label_width = menu_text_width(
            ui,
            &path_text,
            theme::font(&config, 11.6),
            theme::muted(&config),
        ) + 34.0;
        let menu_width = context_menu_width(
            ui,
            &config,
            &rows,
            CONTEXT_MENU_MIN_WIDTH.max(label_width.min(CONTEXT_MENU_MAX_WIDTH)),
            CONTEXT_MENU_MAX_WIDTH,
        );
        apply_context_menu_width(ui, menu_width);
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);

        let row_width = context_menu_row_width(ui);
        let (path_rect, _) = ui.allocate_exact_size(egui::vec2(row_width, 28.0), Sense::hover());
        draw_text_elided(
            ui,
            path_rect.shrink2(egui::vec2(10.0, 0.0)),
            Pos2::new(path_rect.left() + 10.0, path_rect.center().y),
            &path_text,
            theme::font(&config, 11.6),
            theme::muted(&config),
            Align2::LEFT_CENTER,
        );
        ui.separator();

        if context_menu_row(
            ui,
            &config,
            MenuIcon::Copy,
            i18n::tr(&config, "copy_path"),
            None,
            true,
            false,
        )
        .clicked()
        {
            app.copy_current_path_to_clipboard();
            ui.close_menu();
        }

        if context_menu_row(
            ui,
            &config,
            MenuIcon::Paste,
            i18n::tr(&config, "paste"),
            Some("Ctrl+V"),
            true,
            false,
        )
        .clicked()
        {
            match crate::platform::shell::read_text() {
                Ok(text) => {
                    if commit_path_bar_editor(app, &text) {
                        app.path_bar_text_visible = false;
                        app.path_bar_focus_pending = false;
                    } else {
                        app.path_bar_edit_text = text;
                        app.path_bar_text_visible = true;
                        app.path_bar_focus_pending = true;
                    }
                }
                Err(error) => app.status_message = error.to_string(),
            }
            ui.close_menu();
        }
    });
}

fn paint_nav_arrow(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: Rect,
    direction: f32,
    enabled: bool,
    hovered: bool,
) {
    if hovered && enabled {
        theme::paint_hover_gradient(ui.painter(), rect.shrink(1.0), 5.0, config);
    }
    let color = if enabled {
        theme::muted(config)
    } else {
        theme::faint(config)
    };
    let center = rect.center();
    let tip = Pos2::new(center.x + direction * 5.8, center.y);
    let head_x = tip.x - direction * 6.0;
    let shaft_start = Pos2::new(center.x - direction * 5.2, center.y);
    let shaft_end = Pos2::new(tip.x - direction * 1.2, center.y);
    ui.painter().line_segment(
        [Pos2::new(head_x, center.y - 5.0), tip],
        Stroke::new(1.45, color),
    );
    ui.painter().line_segment(
        [tip, Pos2::new(head_x, center.y + 5.0)],
        Stroke::new(1.45, color),
    );
    ui.painter()
        .line_segment([shaft_start, shaft_end], Stroke::new(1.35, color));
}

fn paint_nav_up_arrow(
    ui: &mut egui::Ui,
    config: &AppConfig,
    rect: Rect,
    enabled: bool,
    hovered: bool,
) {
    if hovered && enabled {
        theme::paint_hover_gradient(ui.painter(), rect.shrink(1.0), 5.0, config);
    }
    let color = if enabled {
        theme::muted(config)
    } else {
        theme::faint(config)
    };
    let center = rect.center();
    let tip = Pos2::new(center.x, center.y - 6.0);
    let head_y = tip.y + 6.0;
    let shaft_start = Pos2::new(center.x, center.y + 5.2);
    let shaft_end = Pos2::new(center.x, tip.y + 1.2);
    ui.painter().line_segment(
        [Pos2::new(center.x - 5.0, head_y), tip],
        Stroke::new(1.45, color),
    );
    ui.painter().line_segment(
        [tip, Pos2::new(center.x + 5.0, head_y)],
        Stroke::new(1.45, color),
    );
    ui.painter()
        .line_segment([shaft_start, shaft_end], Stroke::new(1.35, color));
}

fn draw_favorite_toggle(painter: &egui::Painter, rect: Rect, active: bool, config: &AppConfig) {
    let points = vec![
        Pos2::new(rect.left() + 4.0, rect.top() + 3.0),
        Pos2::new(rect.right() - 4.0, rect.top() + 3.0),
        Pos2::new(rect.right() - 4.0, rect.bottom() - 3.0),
        Pos2::new(rect.center().x, rect.bottom() - 6.0),
        Pos2::new(rect.left() + 4.0, rect.bottom() - 3.0),
    ];
    if active {
        painter.add(egui::Shape::convex_polygon(
            points.clone(),
            theme::accent(config),
            Stroke::NONE,
        ));
    }
    let color = if active {
        theme::accent(config)
    } else {
        theme::muted(config)
    };
    painter.add(egui::Shape::closed_line(points, Stroke::new(1.4, color)));
}

fn breadcrumbs(path: Option<&Path>) -> Vec<Crumb> {
    let Some(path) = path else {
        return vec![Crumb {
            label: "This PC".into(),
            path: None,
        }];
    };

    if let Some(crumbs) = explorer::virtual_breadcrumbs(path) {
        return crumbs
            .into_iter()
            .map(|(label, path)| Crumb { label, path })
            .collect();
    }

    if let Some(crumbs) = explorer::unc_breadcrumbs(path) {
        return crumbs
            .into_iter()
            .map(|(label, path)| Crumb { label, path })
            .collect();
    }

    let mut crumbs = Vec::new();
    crumbs.push(Crumb {
        label: "This PC".into(),
        path: None,
    });

    let mut ancestors: Vec<_> = path.ancestors().collect();
    ancestors.reverse();
    for ancestor in ancestors {
        let label = ancestor
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| ancestor.display().to_string());
        if !label.is_empty() {
            crumbs.push(Crumb {
                label,
                path: Some(ancestor.to_path_buf()),
            });
        }
    }

    crumbs
}

struct Crumb {
    label: String,
    path: Option<std::path::PathBuf>,
}

fn draw_breadcrumb_chevron(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.line_segment(
        [
            Pos2::new(rect.left() + 3.0, rect.top() + 2.0),
            rect.center(),
        ],
        Stroke::new(1.2, color),
    );
    painter.line_segment(
        [
            rect.center(),
            Pos2::new(rect.left() + 3.0, rect.bottom() - 2.0),
        ],
        Stroke::new(1.2, color),
    );
}

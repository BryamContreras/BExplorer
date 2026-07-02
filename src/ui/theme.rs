use eframe::egui::{self, Color32, FontFamily, FontId, Stroke, TextStyle};

use crate::app::config::{AppConfig, ThemePreference, VibrancyMode};

pub fn apply(ctx: &egui::Context, config: &AppConfig) {
    match config.theme {
        ThemePreference::Dark => apply_dark(ctx, config),
        ThemePreference::Light | ThemePreference::Gray => apply_light(ctx, config),
    }
}

fn apply_dark(ctx: &egui::Context, config: &AppConfig) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = surface(config);
    visuals.window_fill = popup_surface(config);
    visuals.window_stroke = Stroke::new(1.0, popup_stroke(config));
    visuals.extreme_bg_color = canvas(config);
    visuals.faint_bg_color = row_alt(config);
    visuals.widgets.noninteractive.bg_fill = surface(config);
    visuals.widgets.inactive.bg_fill = control(config);
    visuals.widgets.hovered.bg_fill = hover(config);
    visuals.widgets.active.bg_fill = accent_dim(config);
    visuals.selection.bg_fill = accent_dim(config);
    visuals.selection.stroke = Stroke::new(1.0, accent(config));
    visuals.hyperlink_color = accent_text(config);
    visuals.window_rounding = egui::Rounding::same(6.0);
    visuals.menu_rounding = egui::Rounding::same(5.0);
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = popup_shadow(config);
    ctx.set_visuals(visuals);

    apply_text_style(ctx, config);
}

fn apply_light(ctx: &egui::Context, config: &AppConfig) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = surface(config);
    visuals.window_fill = popup_surface(config);
    visuals.window_stroke = Stroke::new(1.0, popup_stroke(config));
    visuals.extreme_bg_color = canvas(config);
    visuals.faint_bg_color = row_alt(config);
    visuals.widgets.noninteractive.bg_fill = surface(config);
    visuals.widgets.inactive.bg_fill = control(config);
    visuals.widgets.hovered.bg_fill = hover(config);
    visuals.widgets.active.bg_fill = accent_dim(config);
    visuals.selection.bg_fill = accent_dim(config);
    visuals.selection.stroke = Stroke::new(1.0, accent(config));
    visuals.hyperlink_color = accent(config);
    visuals.window_rounding = egui::Rounding::same(6.0);
    visuals.menu_rounding = egui::Rounding::same(5.0);
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = popup_shadow(config);
    ctx.set_visuals(visuals);

    apply_text_style(ctx, config);
}

fn apply_text_style(ctx: &egui::Context, config: &AppConfig) {
    let font_size = config.font_size.clamp(10.0, 18.0);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(4.0, 3.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    style.spacing.window_margin = egui::Margin::same(6.0);
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(font_size, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new((font_size - 0.5).max(10.0), FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new((font_size - 1.5).max(9.0), FontFamily::Proportional),
    );
    ctx.set_style(style);
}

pub fn accent(config: &AppConfig) -> Color32 {
    rgb(config.accent_color)
}

pub fn accent_dim(config: &AppConfig) -> Color32 {
    scale(accent(config), 0.55)
}

pub fn accent_text(config: &AppConfig) -> Color32 {
    blend(accent(config), Color32::WHITE, 0.55)
}

pub fn selection_fill(config: &AppConfig) -> Color32 {
    accent(config)
}

pub fn selection_rect_fill(config: &AppConfig) -> Color32 {
    let color = accent(config);
    let alpha = match config.theme {
        ThemePreference::Dark => 7,
        ThemePreference::Light | ThemePreference::Gray => 42,
    };
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn selection_rect_stroke(config: &AppConfig) -> Color32 {
    blend(accent(config), Color32::WHITE, 0.50)
}

pub fn percent_fill(config: &AppConfig) -> Color32 {
    accent(config)
}

fn vibrancy_alpha(config: &AppConfig) -> u8 {
    match config.vibrancy {
        VibrancyMode::None | VibrancyMode::Blur => 255,
        VibrancyMode::Mica | VibrancyMode::Acrylic => {
            let max_reduction: u32 = match (config.vibrancy, config.theme) {
                (VibrancyMode::Mica, ThemePreference::Light) => 250,
                (VibrancyMode::Mica, _) => 215,
                (VibrancyMode::Acrylic, ThemePreference::Light) => 280,
                (VibrancyMode::Acrylic, _) => 280,
                _ => 195,
            };
            let reduction = (max_reduction * config.vibrancy_intensity as u32 / 200).min(255);
            255 - reduction as u8
        }
    }
}

fn darken_for_vibrancy(_config: &AppConfig, base: Color32) -> Color32 {
    base
}

pub fn titlebar(config: &AppConfig) -> Color32 {
    let base = darken_for_vibrancy(config, pick(config, TITLEBAR, LIGHT_TITLEBAR));
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), vibrancy_alpha(config))
}

pub fn action_bar(config: &AppConfig) -> Color32 {
    let base = pick(config, TITLEBAR, LIGHT_TITLEBAR);
    let color = match config.theme {
        ThemePreference::Dark => blend(base, Color32::BLACK, 0.035),
        ThemePreference::Light | ThemePreference::Gray => blend(base, Color32::BLACK, 0.018),
    };
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), vibrancy_alpha(config))
}

pub fn bookmark_bar(config: &AppConfig) -> Color32 {
    let base = pick(config, TITLEBAR, LIGHT_TITLEBAR);
    let color = match config.theme {
        ThemePreference::Dark => blend(base, Color32::WHITE, 0.018),
        ThemePreference::Light | ThemePreference::Gray => blend(base, Color32::WHITE, 0.055),
    };
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), vibrancy_alpha(config))
}

pub fn toolbar_hairline(config: &AppConfig) -> Color32 {
    let base = blend(subtle_stroke(config), accent(config), 0.10);
    let alpha = match config.theme {
        ThemePreference::Dark => 58,
        ThemePreference::Light | ThemePreference::Gray => 72,
    };
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha)
}

pub fn tab_active(config: &AppConfig) -> Color32 {
    pick(config, TAB_ACTIVE, LIGHT_TAB_ACTIVE)
}

pub fn tab_inactive(config: &AppConfig) -> Color32 {
    pick(config, TAB_INACTIVE, LIGHT_TAB_INACTIVE)
}

pub fn tab_hover(config: &AppConfig) -> Color32 {
    let amount = match config.theme {
        ThemePreference::Dark => 0.22,
        ThemePreference::Light | ThemePreference::Gray => 0.14,
    };
    blend(
        pick(config, TAB_HOVER, LIGHT_TAB_HOVER),
        accent(config),
        amount,
    )
}

pub fn sidebar(config: &AppConfig) -> Color32 {
    let base = darken_for_vibrancy(config, pick(config, SIDEBAR, LIGHT_SIDEBAR));
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), vibrancy_alpha(config))
}

pub fn sidebar_row(config: &AppConfig) -> Color32 {
    let amount = match config.theme {
        ThemePreference::Dark => 0.54,
        ThemePreference::Light | ThemePreference::Gray => 0.34,
    };
    blend(
        pick(config, SIDEBAR_ROW, LIGHT_SIDEBAR_ROW),
        accent(config),
        amount,
    )
}

pub fn surface_elevated(config: &AppConfig) -> Color32 {
    pick(config, SURFACE_ELEVATED, LIGHT_SURFACE_ELEVATED)
}

pub fn popup_surface(config: &AppConfig) -> Color32 {
    let darker = match config.theme {
        ThemePreference::Dark => blend(surface_elevated(config), Color32::BLACK, 0.15),
        ThemePreference::Light | ThemePreference::Gray => {
            blend(surface_elevated(config), Color32::BLACK, 0.08)
        }
    };
    darker
}

pub fn popup_stroke(config: &AppConfig) -> Color32 {
    let base = blend(stroke(config), accent(config), 0.12);
    let alpha = match config.theme {
        ThemePreference::Dark => 150,
        ThemePreference::Light | ThemePreference::Gray => 165,
    };
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha)
}

pub fn popup_shadow(config: &AppConfig) -> egui::epaint::Shadow {
    let alpha = match config.theme {
        ThemePreference::Dark => 96,
        ThemePreference::Light | ThemePreference::Gray => 48,
    };
    egui::epaint::Shadow {
        offset: egui::vec2(0.0, 10.0),
        blur: 18.0,
        spread: 1.0,
        color: Color32::from_black_alpha(alpha),
    }
}

pub fn control(config: &AppConfig) -> Color32 {
    pick(config, CONTROL, LIGHT_CONTROL)
}

pub fn hover(config: &AppConfig) -> Color32 {
    let amount = match config.theme {
        ThemePreference::Dark => 0.46,
        ThemePreference::Light | ThemePreference::Gray => 0.30,
    };
    blend(pick(config, HOVER, LIGHT_HOVER), accent(config), amount)
}

pub fn canvas(config: &AppConfig) -> Color32 {
    let base = darken_for_vibrancy(config, pick(config, CANVAS, LIGHT_CANVAS));
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), vibrancy_alpha(config))
}

pub fn surface(config: &AppConfig) -> Color32 {
    let base = darken_for_vibrancy(config, pick(config, SURFACE, LIGHT_SURFACE));
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), vibrancy_alpha(config))
}

pub fn row_alt(config: &AppConfig) -> Color32 {
    let base = darken_for_vibrancy(config, pick(config, ROW_ALT, LIGHT_ROW_ALT));
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), vibrancy_alpha(config))
}

pub fn row_hover(config: &AppConfig) -> Color32 {
    let amount = match config.theme {
        ThemePreference::Dark => 0.44,
        ThemePreference::Light | ThemePreference::Gray => 0.28,
    };
    blend(
        pick(config, ROW_HOVER, LIGHT_ROW_HOVER),
        accent(config),
        amount,
    )
}

pub fn stroke(config: &AppConfig) -> Color32 {
    pick(config, STROKE, LIGHT_STROKE)
}

pub fn subtle_stroke(config: &AppConfig) -> Color32 {
    pick(config, SUBTLE_STROKE, LIGHT_SUBTLE_STROKE)
}

pub fn text(config: &AppConfig) -> Color32 {
    pick(config, TEXT, LIGHT_TEXT)
}

pub fn muted(config: &AppConfig) -> Color32 {
    pick(config, MUTED, LIGHT_MUTED)
}

pub fn faint(config: &AppConfig) -> Color32 {
    pick(config, FAINT, LIGHT_FAINT)
}

pub fn hidden_tint(config: &AppConfig, color: Color32) -> Color32 {
    let alpha = match config.theme {
        ThemePreference::Dark => 92,
        ThemePreference::Light | ThemePreference::Gray => 200,
    };
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn hidden_icon_tint(config: &AppConfig, color: Color32) -> Color32 {
    let alpha = match config.theme {
        ThemePreference::Dark => 200,
        ThemePreference::Light | ThemePreference::Gray => 240,
    };
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn cut_tint(config: &AppConfig, color: Color32) -> Color32 {
    let alpha = match config.theme {
        ThemePreference::Dark => 145,
        ThemePreference::Light | ThemePreference::Gray => 150,
    };
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn font(config: &AppConfig, base_size: f32) -> FontId {
    let delta = config.font_size.clamp(10.0, 18.0) - 12.5;
    FontId::new((base_size + delta).max(8.0), FontFamily::Proportional)
}

pub fn paint_canvas_gradient(painter: &egui::Painter, rect: egui::Rect, config: &AppConfig) {
    if config.vibrancy != VibrancyMode::None {
        return;
    }
    let base = canvas(config);
    let accent = accent(config);
    let alpha = vibrancy_alpha(config);
    let apply_alpha = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            apply_alpha(blend(base, Color32::BLACK, 0.010)),
            apply_alpha(blend(base, accent, 0.010)),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            apply_alpha(blend(base, accent, 0.005)),
            apply_alpha(blend(base, Color32::WHITE, 0.020)),
        ),
    };

    paint_horizontal_gradient_rect(painter, rect, left, right);
}

pub fn paint_titlebar_gradient(painter: &egui::Painter, rect: egui::Rect, config: &AppConfig) {
    let base = titlebar(config);
    let accent = accent(config);
    let alpha = vibrancy_alpha(config);
    let apply_alpha = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            apply_alpha(blend(base, Color32::BLACK, 0.008)),
            apply_alpha(blend(base, accent, 0.008)),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            apply_alpha(blend(base, accent, 0.005)),
            apply_alpha(blend(base, Color32::WHITE, 0.025)),
        ),
    };

    paint_horizontal_gradient_rect(painter, rect, left, right);
}

pub fn paint_sidebar_gradient(painter: &egui::Painter, rect: egui::Rect, config: &AppConfig) {
    let base = sidebar(config);
    let accent = accent(config);
    let alpha = vibrancy_alpha(config);
    let apply_alpha = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            apply_alpha(blend(base, canvas(config), 0.030)),
            apply_alpha(blend(base, accent, 0.010)),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            apply_alpha(blend(base, accent, 0.006)),
            apply_alpha(blend(base, canvas(config), 0.040)),
        ),
    };

    paint_horizontal_gradient_rect(painter, rect, left, right);
}

pub fn paint_status_gradient(painter: &egui::Painter, rect: egui::Rect, config: &AppConfig) {
    let base = canvas(config);
    let accent = accent(config);
    let alpha = vibrancy_alpha(config);
    let apply_alpha = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            apply_alpha(blend(base, Color32::BLACK, 0.008)),
            apply_alpha(blend(base, accent, 0.008)),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            apply_alpha(blend(base, accent, 0.005)),
            apply_alpha(blend(base, Color32::WHITE, 0.020)),
        ),
    };

    paint_horizontal_gradient_rect(painter, rect, left, right);
}

pub fn paint_selection_gradient(painter: &egui::Painter, rect: egui::Rect, config: &AppConfig) {
    let base = selection_fill(config);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            blend(base, Color32::BLACK, 0.120),
            blend(base, Color32::WHITE, 0.180),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            blend(base, Color32::BLACK, 0.090),
            blend(base, Color32::WHITE, 0.240),
        ),
    };

    paint_horizontal_gradient_rect(painter, rect, left, right);
}

pub fn paint_selection_gradient_rounded(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: f32,
    config: &AppConfig,
) {
    let base = selection_fill(config);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            blend(base, Color32::BLACK, 0.120),
            blend(base, Color32::WHITE, 0.180),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            blend(base, Color32::BLACK, 0.090),
            blend(base, Color32::WHITE, 0.240),
        ),
    };

    paint_rounded_horizontal_gradient_rect(painter, rect, rounding, left, right);
}

pub fn paint_hover_gradient(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: f32,
    config: &AppConfig,
) {
    let base = hover(config);
    let accent = accent(config);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            blend(base, Color32::BLACK, 0.060),
            blend(base, accent, 0.420),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            blend(base, Color32::WHITE, 0.080),
            blend(base, accent, 0.310),
        ),
    };

    paint_rounded_horizontal_gradient_rect(painter, rect, rounding, left, right);
}

pub fn paint_row_hover_gradient(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: f32,
    config: &AppConfig,
) {
    let base = row_hover(config);
    let accent = accent(config);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            blend(base, Color32::BLACK, 0.050),
            blend(base, accent, 0.380),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            blend(base, Color32::WHITE, 0.070),
            blend(base, accent, 0.290),
        ),
    };

    paint_rounded_horizontal_gradient_rect(painter, rect, rounding, left, right);
}

pub fn paint_sidebar_row_gradient(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: f32,
    config: &AppConfig,
) {
    let base = sidebar_row(config);
    let accent = accent(config);
    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            blend(base, Color32::BLACK, 0.050),
            blend(base, accent, 0.560),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            blend(base, Color32::WHITE, 0.080),
            blend(base, accent, 0.380),
        ),
    };

    paint_rounded_horizontal_gradient_rect(painter, rect, rounding, left, right);
}

pub fn paint_percent_gradient(painter: &egui::Painter, rect: egui::Rect, config: &AppConfig) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    let base = percent_fill(config);
    painter.rect_filled(rect, 3.0, base);

    let inner = rect.shrink(1.0);
    if inner.width() <= 0.0 || inner.height() <= 0.0 {
        return;
    }

    let (left, right) = match config.theme {
        ThemePreference::Dark => (
            blend(base, Color32::BLACK, 0.180),
            blend(base, Color32::WHITE, 0.280),
        ),
        ThemePreference::Light | ThemePreference::Gray => (
            blend(base, Color32::BLACK, 0.130),
            blend(base, Color32::WHITE, 0.350),
        ),
    };

    paint_horizontal_gradient_rect(painter, inner, left, right);
}

fn rgb(value: [u8; 3]) -> Color32 {
    Color32::from_rgb(value[0], value[1], value[2])
}

fn scale(color: Color32, factor: f32) -> Color32 {
    Color32::from_rgb(
        (color.r() as f32 * factor).round().clamp(0.0, 255.0) as u8,
        (color.g() as f32 * factor).round().clamp(0.0, 255.0) as u8,
        (color.b() as f32 * factor).round().clamp(0.0, 255.0) as u8,
    )
}

fn pick(config: &AppConfig, dark: Color32, light: Color32) -> Color32 {
    match config.theme {
        ThemePreference::Dark => dark,
        ThemePreference::Light | ThemePreference::Gray => light,
    }
}

fn blend(base: Color32, tint: Color32, amount: f32) -> Color32 {
    let mix = |left: u8, right: u8| -> u8 {
        ((left as f32 * (1.0 - amount)) + (right as f32 * amount))
            .round()
            .clamp(0.0, 255.0) as u8
    };

    Color32::from_rgb(
        mix(base.r(), tint.r()),
        mix(base.g(), tint.g()),
        mix(base.b(), tint.b()),
    )
}

fn paint_horizontal_gradient_rect(
    painter: &egui::Painter,
    rect: egui::Rect,
    left: Color32,
    right: Color32,
) {
    paint_gradient_rect(painter, rect, left, right, right, left);
}

fn paint_rounded_horizontal_gradient_rect(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: f32,
    left: Color32,
    right: Color32,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    painter.rect_filled(rect, rounding, left);

    let steps = ((rect.width() / 5.0).round() as usize).clamp(8, 48);
    for step in 0..steps {
        let t0 = step as f32 / steps as f32;
        let t1 = (step + 1) as f32 / steps as f32;
        let strip = egui::Rect::from_min_max(
            egui::Pos2::new(rect.left() + rect.width() * t0, rect.top()),
            egui::Pos2::new(rect.left() + rect.width() * t1 + 0.75, rect.bottom()),
        );
        let color = blend(left, right, (t0 + t1) * 0.5);
        let strip_rounding = if step == 0 || step + 1 == steps {
            egui::Rounding::same(rounding)
        } else {
            egui::Rounding::ZERO
        };
        painter.rect_filled(strip, strip_rounding, color);
    }
}

fn paint_gradient_rect(
    painter: &egui::Painter,
    rect: egui::Rect,
    top_left: Color32,
    top_right: Color32,
    bottom_right: Color32,
    bottom_left: Color32,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    let mut mesh = egui::epaint::Mesh::default();
    let first = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_top(), top_left);
    mesh.colored_vertex(rect.right_top(), top_right);
    mesh.colored_vertex(rect.right_bottom(), bottom_right);
    mesh.colored_vertex(rect.left_bottom(), bottom_left);
    mesh.add_triangle(first, first + 1, first + 2);
    mesh.add_triangle(first, first + 2, first + 3);
    painter.add(egui::Shape::mesh(mesh));
}

pub const TITLEBAR: Color32 = Color32::from_rgb(29, 32, 32);
pub const TAB_ACTIVE: Color32 = Color32::from_rgb(24, 28, 28);
pub const TAB_INACTIVE: Color32 = Color32::from_rgb(22, 25, 25);
pub const TAB_HOVER: Color32 = Color32::from_rgb(34, 39, 39);
pub const SIDEBAR: Color32 = Color32::from_rgb(27, 31, 31);
pub const SIDEBAR_ROW: Color32 = Color32::from_rgb(34, 39, 39);
pub const CANVAS: Color32 = Color32::from_rgb(18, 21, 21);
pub const SURFACE: Color32 = Color32::from_rgb(20, 23, 23);
pub const SURFACE_ELEVATED: Color32 = Color32::from_rgb(25, 29, 29);
pub const ROW_ALT: Color32 = Color32::from_rgb(21, 24, 24);
pub const ROW_HOVER: Color32 = Color32::from_rgb(28, 34, 34);
pub const CONTROL: Color32 = Color32::from_rgb(33, 37, 37);
pub const HOVER: Color32 = Color32::from_rgb(42, 48, 48);
pub const STROKE: Color32 = Color32::from_rgb(48, 55, 55);
pub const SUBTLE_STROKE: Color32 = Color32::from_rgb(34, 40, 40);
pub const TEXT: Color32 = Color32::from_rgb(218, 224, 224);
pub const MUTED: Color32 = Color32::from_rgb(132, 142, 142);
pub const FAINT: Color32 = Color32::from_rgb(92, 101, 101);
pub const FOLDER: Color32 = Color32::from_rgb(252, 196, 65);
pub const FILE: Color32 = Color32::from_rgb(221, 228, 230);
pub const IMAGE: Color32 = Color32::from_rgb(77, 170, 222);
pub const MUSIC: Color32 = Color32::from_rgb(222, 102, 132);
pub const VIDEO: Color32 = Color32::from_rgb(151, 101, 226);
pub const DRIVE: Color32 = Color32::from_rgb(138, 158, 161);

const LIGHT_TITLEBAR: Color32 = Color32::from_rgb(236, 240, 240);
const LIGHT_TAB_ACTIVE: Color32 = Color32::from_rgb(250, 252, 252);
const LIGHT_TAB_INACTIVE: Color32 = Color32::from_rgb(226, 232, 232);
const LIGHT_TAB_HOVER: Color32 = Color32::from_rgb(238, 244, 244);
const LIGHT_SIDEBAR: Color32 = Color32::from_rgb(238, 243, 243);
const LIGHT_SIDEBAR_ROW: Color32 = Color32::from_rgb(225, 234, 234);
const LIGHT_CANVAS: Color32 = Color32::from_rgb(248, 250, 250);
const LIGHT_SURFACE: Color32 = Color32::from_rgb(244, 247, 247);
const LIGHT_SURFACE_ELEVATED: Color32 = Color32::from_rgb(255, 255, 255);
const LIGHT_ROW_ALT: Color32 = Color32::from_rgb(243, 247, 247);
const LIGHT_ROW_HOVER: Color32 = Color32::from_rgb(229, 238, 238);
const LIGHT_CONTROL: Color32 = Color32::from_rgb(229, 236, 236);
const LIGHT_HOVER: Color32 = Color32::from_rgb(218, 229, 229);
const LIGHT_STROKE: Color32 = Color32::from_rgb(196, 210, 210);
const LIGHT_SUBTLE_STROKE: Color32 = Color32::from_rgb(224, 232, 232);
const LIGHT_TEXT: Color32 = Color32::from_rgb(32, 42, 42);
const LIGHT_MUTED: Color32 = Color32::from_rgb(87, 101, 101);
const LIGHT_FAINT: Color32 = Color32::from_rgb(132, 148, 148);

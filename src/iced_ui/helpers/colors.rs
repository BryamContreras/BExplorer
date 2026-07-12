use super::*;
pub(in crate::iced_ui) fn normalized_hue(hue: f32) -> f32 {
    if hue.is_finite() {
        hue.rem_euclid(360.0)
    } else {
        140.0
    }
}

pub(in crate::iced_ui) fn mix_color(color: Color, target: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color {
        r: color.r + (target.r - color.r) * amount,
        g: color.g + (target.g - color.g) * amount,
        b: color.b + (target.b - color.b) * amount,
        a: color.a + (target.a - color.a) * amount,
    }
}

pub(in crate::iced_ui) fn hover_tint(palette: Palette) -> Color {
    mix_color(palette.hover, palette.accent, 0.32)
}

/// Keeps chrome such as the address field and detail headers visibly glassy
/// when native vibrancy is active. Inputs deliberately use a denser surface
/// elsewhere for readability, but these broad navigation surfaces should not
/// hide KWin's blur behind an almost opaque fill.
pub(in crate::iced_ui) fn chrome_glass_background(
    palette: Palette,
    mut background: Color,
) -> Color {
    if palette.table_bg.a < 0.99 {
        background.a = (palette.table_bg.a + 0.14).min(0.74);
    }
    background
}

/// The shared selected-color surface used throughout the explorer.
///
/// Keeping its stops here ensures files, controls, transfer progress, and
/// drive capacity use the same understated interpretation of the user accent.
pub(in crate::iced_ui) fn accent_gradient(palette: Palette) -> gradient::Linear {
    let (start, middle, end) = accent_gradient_colors(palette);
    gradient::Linear::new(-std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, start)
        .add_stop(0.52, middle)
        .add_stop(1.0, end)
}

pub(in crate::iced_ui) fn translucent_accent_gradient(
    palette: Palette,
    alpha: f32,
) -> gradient::Linear {
    let (start, middle, end) = accent_gradient_colors(palette);
    gradient::Linear::new(-std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, translucent_color(start, alpha))
        .add_stop(0.52, translucent_color(middle, alpha))
        .add_stop(1.0, translucent_color(end, alpha))
}

fn accent_gradient_colors(palette: Palette) -> (Color, Color, Color) {
    (
        mix_color(palette.accent, Color::WHITE, 0.07),
        palette.accent,
        mix_color(palette.accent, Color::BLACK, 0.10),
    )
}

pub(in crate::iced_ui) fn translucent_color(mut color: Color, alpha: f32) -> Color {
    color.a = alpha.clamp(0.0, 1.0);
    color
}

pub(in crate::iced_ui) fn window_border_color(palette: Palette) -> Color {
    let mut color = mix_color(palette.border, softened_accent(palette.accent), 0.38);
    color.a = 0.48;
    color
}

pub(in crate::iced_ui) fn bottom_radius(bottom_left: bool, bottom_right: bool) -> border::Radius {
    let radius = WINDOW_RADIUS - WINDOW_BORDER_WIDTH;
    border::Radius::default()
        .bottom_left(if bottom_left { radius } else { 0.0 })
        .bottom_right(if bottom_right { radius } else { 0.0 })
}

pub(in crate::iced_ui) fn softened_accent(color: Color) -> Color {
    let luma = color.r * 0.299 + color.g * 0.587 + color.b * 0.114;
    let gray = Color {
        r: luma,
        g: luma,
        b: luma,
        a: color.a,
    };
    mix_color(color, gray, 0.24)
}

pub(in crate::iced_ui) fn accent_rgb_strings(color: [u8; 3]) -> [String; 3] {
    [
        color[0].to_string(),
        color[1].to_string(),
        color[2].to_string(),
    ]
}

#[cfg(test)]
pub(in crate::iced_ui) fn accent_color_from_hue(hue: f32) -> [u8; 3] {
    accent_color_from_hsv(hue, 0.62, 0.82)
}

pub(in crate::iced_ui) fn accent_color_from_hsv(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let hue = normalized_hue(hue) / 60.0;
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);
    let chroma = value * saturation;
    let secondary = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let base = value - chroma;
    let (r, g, b) = match hue.floor() as i32 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    [
        ((r + base) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + base) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + base) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

pub(in crate::iced_ui) fn accent_hsv_from_color(color: [u8; 3]) -> (f32, f32, f32) {
    let r = color[0] as f32 / 255.0;
    let g = color[1] as f32 / 255.0;
    let b = color[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta <= f32::EPSILON {
        return (140.0, 0.0, max);
    }

    let hue = if (max - r).abs() <= f32::EPSILON {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() <= f32::EPSILON {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let saturation = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    (normalized_hue(hue), saturation, max)
}

#[cfg(test)]
pub(in crate::iced_ui) fn accent_hue_from_color(color: [u8; 3]) -> f32 {
    accent_hsv_from_color(color).0
}

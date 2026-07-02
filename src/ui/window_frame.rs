use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Stroke};

pub const RESIZE_EDGE: f32 = 6.0;
const CORNER: f32 = 18.0;
const WINDOW_ROUNDING: f32 = 7.0;
const TITLEBAR_HEIGHT: f32 = 40.0;
const WINDOW_CONTROL_ZONE: f32 = 145.0;

pub fn show_resize_handles(ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    paint_border(ctx, screen, maximized);

    if maximized {
        return;
    }

    let Some(pos) = ctx.input(|input| input.pointer.hover_pos()) else {
        return;
    };

    let Some((direction, cursor)) = resize_hit(screen, pos) else {
        return;
    };

    ctx.output_mut(|output| output.cursor_icon = cursor);
    if ctx.input(|input| input.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

/// Returns `true` when the pointer is currently in a window resize edge/corner
/// and the window is not maximized. Used to suppress `StartDrag` commands that
/// would otherwise race with `BeginResize` (both start an OS modal loop; the
/// first one wins), which made the top edge un-resizable when hovering the
/// titlebar drag rects.
pub fn pointer_in_resize_edge(ctx: &egui::Context) -> bool {
    if ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
        return false;
    }
    let screen = ctx.screen_rect();
    let Some(pos) = ctx.input(|input| input.pointer.hover_pos()) else {
        return false;
    };
    resize_hit(screen, pos).is_some()
}

fn paint_border(ctx: &egui::Context, screen: Rect, maximized: bool) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("window_visible_border"),
    ));
    let rounding = if maximized { 0.0 } else { WINDOW_ROUNDING };
    painter.rect_stroke(
        screen.shrink(0.5),
        rounding,
        Stroke::new(1.0, Color32::from_rgb(49, 58, 60)),
    );
}

fn resize_hit(screen: Rect, pos: Pos2) -> Option<(egui::viewport::ResizeDirection, CursorIcon)> {
    use egui::viewport::ResizeDirection;

    let left = screen.left();
    let right = screen.right();
    let top = screen.top();
    let bottom = screen.bottom();

    if pos.y <= top + TITLEBAR_HEIGHT && pos.x >= right - WINDOW_CONTROL_ZONE {
        return None;
    }

    let near_left = pos.x <= left + RESIZE_EDGE;
    let near_right = pos.x >= right - RESIZE_EDGE;
    let near_top = pos.y <= top + RESIZE_EDGE;
    let near_bottom = pos.y >= bottom - RESIZE_EDGE;
    let corner_left = pos.x <= left + CORNER;
    let corner_right = pos.x >= right - CORNER;
    let corner_top = pos.y <= top + CORNER;
    let corner_bottom = pos.y >= bottom - CORNER;

    if corner_left && corner_top {
        return Some((ResizeDirection::NorthWest, CursorIcon::ResizeNwSe));
    }
    if corner_right && corner_top {
        return Some((ResizeDirection::NorthEast, CursorIcon::ResizeNeSw));
    }
    if corner_left && corner_bottom {
        return Some((ResizeDirection::SouthWest, CursorIcon::ResizeNeSw));
    }
    if corner_right && corner_bottom {
        return Some((ResizeDirection::SouthEast, CursorIcon::ResizeNwSe));
    }
    if near_top && pos.x > left + CORNER && pos.x < right - CORNER {
        return Some((ResizeDirection::North, CursorIcon::ResizeVertical));
    }
    if near_bottom && pos.x > left + CORNER && pos.x < right - CORNER {
        return Some((ResizeDirection::South, CursorIcon::ResizeVertical));
    }
    if near_left && pos.y > top + CORNER && pos.y < bottom - CORNER {
        return Some((ResizeDirection::West, CursorIcon::ResizeHorizontal));
    }
    if near_right && pos.y > top + CORNER && pos.y < bottom - CORNER {
        return Some((ResizeDirection::East, CursorIcon::ResizeHorizontal));
    }

    None
}

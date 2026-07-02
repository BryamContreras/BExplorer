use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

use crate::fs::explorer::{DriveKind, EntryKind, FileEntry};
use crate::ui::theme;

pub fn draw_entry_icon(painter: &egui::Painter, rect: Rect, entry: &FileEntry) {
    match entry.kind {
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::Portable) => {
            draw_portable_device_icon(painter, rect)
        }
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::NetworkComputer) => {
            draw_network_computer_icon(painter, rect)
        }
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::NetworkPrinter) => {
            draw_network_printer_icon(painter, rect)
        }
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::NetworkScanner) => {
            draw_network_scanner_icon(painter, rect)
        }
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::NetworkMultifunction) => {
            draw_network_multifunction_icon(painter, rect)
        }
        EntryKind::Drive if entry.drive_kind == Some(DriveKind::NetworkDevice) => {
            draw_network_device_icon(painter, rect)
        }
        EntryKind::Drive => draw_drive_icon(painter, rect),
        EntryKind::Folder => draw_folder_icon(painter, rect, theme::FOLDER),
        EntryKind::Symlink => draw_file_icon(painter, rect, Color32::from_rgb(145, 197, 228)),
        EntryKind::File => draw_file_by_name(painter, rect, &entry.name),
        EntryKind::Other => draw_file_icon(painter, rect, theme::MUTED),
    }
}

pub fn draw_sidebar_icon(painter: &egui::Painter, rect: Rect, kind: SidebarIcon) {
    match kind {
        SidebarIcon::Recent => {
            painter.circle_stroke(
                rect.center(),
                rect.width() * 0.36,
                Stroke::new(1.5, theme::MUTED),
            );
            painter.line_segment(
                [rect.center(), Pos2::new(rect.center().x, rect.top() + 4.0)],
                Stroke::new(1.2, theme::MUTED),
            );
            painter.line_segment(
                [
                    rect.center(),
                    Pos2::new(rect.right() - 4.0, rect.center().y),
                ],
                Stroke::new(1.2, theme::MUTED),
            );
        }
        SidebarIcon::Bookmark => {
            let points = vec![
                Pos2::new(rect.left() + 4.0, rect.top() + 3.0),
                Pos2::new(rect.right() - 4.0, rect.top() + 3.0),
                Pos2::new(rect.right() - 4.0, rect.bottom() - 3.0),
                Pos2::new(rect.center().x, rect.bottom() - 6.0),
                Pos2::new(rect.left() + 4.0, rect.bottom() - 3.0),
            ];
            painter.add(egui::Shape::closed_line(
                points,
                Stroke::new(1.4, theme::MUTED),
            ));
        }
        SidebarIcon::Storage => draw_drive_icon(painter, rect),
        SidebarIcon::Network => {
            let top = Pos2::new(rect.center().x, rect.top() + 4.0);
            let left = Pos2::new(rect.left() + 4.0, rect.bottom() - 5.0);
            let right = Pos2::new(rect.right() - 4.0, rect.bottom() - 5.0);
            painter.line_segment([top, left], Stroke::new(1.2, theme::MUTED));
            painter.line_segment([top, right], Stroke::new(1.2, theme::MUTED));
            painter.circle_filled(top, 2.8, Color32::from_rgb(38, 164, 208));
            painter.circle_filled(left, 2.8, Color32::from_rgb(20, 206, 182));
            painter.circle_filled(right, 2.8, Color32::from_rgb(20, 206, 182));
        }
        SidebarIcon::Device => draw_sidebar_device_icon(painter, rect),
        SidebarIcon::Places => {
            painter.rect_stroke(rect.shrink(3.0), 2.0, Stroke::new(1.4, theme::MUTED));
        }
        SidebarIcon::Computer => {
            let screen = Rect::from_min_max(
                Pos2::new(rect.left() + 2.0, rect.top() + 4.0),
                Pos2::new(rect.right() - 2.0, rect.bottom() - 6.0),
            );
            painter.rect_filled(screen, 1.5, Color32::from_rgb(38, 164, 208));
            painter.line_segment(
                [
                    Pos2::new(rect.center().x, screen.bottom()),
                    Pos2::new(rect.center().x, rect.bottom() - 2.0),
                ],
                Stroke::new(1.4, theme::MUTED),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 5.0, rect.bottom() - 2.0),
                    Pos2::new(rect.right() - 5.0, rect.bottom() - 2.0),
                ],
                Stroke::new(1.4, theme::MUTED),
            );
        }
        SidebarIcon::Folder => draw_folder_icon(painter, rect, theme::FOLDER),
        SidebarIcon::Download => {
            painter.line_segment(
                [
                    Pos2::new(rect.center().x, rect.top() + 2.0),
                    Pos2::new(rect.center().x, rect.bottom() - 6.0),
                ],
                Stroke::new(1.7, Color32::from_rgb(20, 206, 182)),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.center().x - 5.0, rect.bottom() - 10.0),
                    Pos2::new(rect.center().x, rect.bottom() - 5.0),
                ],
                Stroke::new(1.7, Color32::from_rgb(20, 206, 182)),
            );
            painter.line_segment(
                [
                    Pos2::new(rect.center().x + 5.0, rect.bottom() - 10.0),
                    Pos2::new(rect.center().x, rect.bottom() - 5.0),
                ],
                Stroke::new(1.7, Color32::from_rgb(20, 206, 182)),
            );
        }
        SidebarIcon::Image => draw_file_icon(painter, rect, theme::IMAGE),
        SidebarIcon::Music => draw_file_icon(painter, rect, theme::MUSIC),
        SidebarIcon::Video => draw_file_icon(painter, rect, theme::VIDEO),
    }
}

#[derive(Clone, Copy)]
pub enum SidebarIcon {
    Recent,
    Bookmark,
    Storage,
    Network,
    Device,
    Places,
    Computer,
    Folder,
    Download,
    Image,
    Music,
    Video,
}

pub fn sidebar_icon_for_label(label: &str) -> SidebarIcon {
    match label.to_ascii_lowercase().as_str() {
        "desktop" | "documents" | "home" => SidebarIcon::Folder,
        "downloads" => SidebarIcon::Download,
        "pictures" => SidebarIcon::Image,
        "music" => SidebarIcon::Music,
        "videos" => SidebarIcon::Video,
        _ => SidebarIcon::Folder,
    }
}

fn draw_file_by_name(painter: &egui::Painter, rect: Rect, name: &str) {
    let color = match name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "webp" | "bmp") => theme::IMAGE,
        Some("mp3" | "wav" | "flac" | "ogg") => theme::MUSIC,
        Some("mp4" | "mkv" | "mov" | "avi") => theme::VIDEO,
        Some("json" | "xml" | "csv" | "md" | "txt" | "log") => Color32::from_rgb(236, 230, 170),
        _ => theme::FILE,
    };
    draw_file_icon(painter, rect, color);
}

fn draw_portable_device_icon(painter: &egui::Painter, rect: Rect) {
    let body = rect.shrink2(egui::vec2(rect.width() * 0.24, rect.height() * 0.08));
    let radius = (body.width() * 0.16).clamp(2.0, 7.0);
    painter.rect_filled(body, radius, Color32::from_rgb(20, 166, 197));

    let shine = Rect::from_min_max(
        body.left_top() + egui::vec2(body.width() * 0.16, body.height() * 0.10),
        Pos2::new(
            body.left() + body.width() * 0.44,
            body.bottom() - body.height() * 0.10,
        ),
    );
    painter.rect_filled(
        shine,
        radius * 0.6,
        Color32::from_rgba_unmultiplied(112, 231, 242, 92),
    );
    painter.rect_stroke(
        body,
        radius,
        Stroke::new(1.1, Color32::from_rgb(84, 218, 232)),
    );
    painter.circle_filled(
        Pos2::new(body.center().x, body.bottom() - body.height() * 0.08),
        (body.width() * 0.055).clamp(0.8, 2.5),
        Color32::from_rgb(220, 252, 255),
    );
}

fn draw_sidebar_device_icon(painter: &egui::Painter, rect: Rect) {
    let body = rect.shrink2(egui::vec2(4.5, 2.0));
    painter.rect_stroke(body, 2.0, Stroke::new(1.4, Color32::from_rgb(38, 164, 208)));
    painter.circle_filled(
        Pos2::new(body.center().x, body.bottom() - 3.0),
        1.2,
        theme::MUTED,
    );
}

fn draw_network_computer_icon(painter: &egui::Painter, rect: Rect) {
    let screen = Rect::from_min_max(
        Pos2::new(
            rect.left() + rect.width() * 0.13,
            rect.top() + rect.height() * 0.18,
        ),
        Pos2::new(
            rect.right() - rect.width() * 0.13,
            rect.top() + rect.height() * 0.68,
        ),
    );
    let stand_top = Pos2::new(screen.center().x, screen.bottom());
    let stand_bottom = Pos2::new(screen.center().x, rect.bottom() - rect.height() * 0.13);
    let base_left = Pos2::new(rect.left() + rect.width() * 0.28, stand_bottom.y);
    let base_right = Pos2::new(rect.right() - rect.width() * 0.28, stand_bottom.y);

    painter.rect_filled(screen, 2.0, Color32::from_rgb(32, 168, 207));
    painter.rect_stroke(
        screen,
        2.0,
        Stroke::new(1.0, Color32::from_rgb(110, 222, 238)),
    );
    painter.line_segment([stand_top, stand_bottom], Stroke::new(1.4, theme::MUTED));
    painter.line_segment([base_left, base_right], Stroke::new(1.4, theme::MUTED));
}

fn draw_network_printer_icon(painter: &egui::Painter, rect: Rect) {
    let paper = Rect::from_min_max(
        Pos2::new(
            rect.left() + rect.width() * 0.24,
            rect.top() + rect.height() * 0.10,
        ),
        Pos2::new(
            rect.right() - rect.width() * 0.24,
            rect.top() + rect.height() * 0.42,
        ),
    );
    let body = Rect::from_min_max(
        Pos2::new(
            rect.left() + rect.width() * 0.12,
            rect.top() + rect.height() * 0.34,
        ),
        Pos2::new(
            rect.right() - rect.width() * 0.12,
            rect.bottom() - rect.height() * 0.18,
        ),
    );
    let tray = Rect::from_min_max(
        Pos2::new(
            rect.left() + rect.width() * 0.24,
            rect.bottom() - rect.height() * 0.26,
        ),
        Pos2::new(
            rect.right() - rect.width() * 0.24,
            rect.bottom() - rect.height() * 0.08,
        ),
    );

    painter.rect_filled(paper, 1.5, Color32::from_rgb(232, 239, 238));
    painter.rect_stroke(
        paper,
        1.5,
        Stroke::new(0.9, Color32::from_rgb(157, 174, 176)),
    );
    painter.rect_filled(body, 2.0, Color32::from_rgb(80, 100, 104));
    painter.rect_stroke(
        body,
        2.0,
        Stroke::new(1.0, Color32::from_rgb(172, 188, 188)),
    );
    painter.rect_filled(tray, 1.5, Color32::from_rgb(206, 219, 219));
    painter.circle_filled(
        Pos2::new(body.left() + body.width() * 0.18, body.center().y),
        (rect.width() * 0.045).max(1.2),
        Color32::from_rgb(51, 219, 86),
    );
}

fn draw_network_scanner_icon(painter: &egui::Painter, rect: Rect) {
    let body = Rect::from_min_max(
        Pos2::new(
            rect.left() + rect.width() * 0.12,
            rect.top() + rect.height() * 0.32,
        ),
        Pos2::new(
            rect.right() - rect.width() * 0.12,
            rect.bottom() - rect.height() * 0.18,
        ),
    );
    let glass = Rect::from_min_max(
        Pos2::new(
            body.left() + body.width() * 0.13,
            body.top() + body.height() * 0.18,
        ),
        Pos2::new(
            body.right() - body.width() * 0.13,
            body.bottom() - body.height() * 0.26,
        ),
    );
    painter.rect_filled(body, 3.0, Color32::from_rgb(70, 91, 96));
    painter.rect_stroke(
        body,
        3.0,
        Stroke::new(1.0, Color32::from_rgb(162, 181, 181)),
    );
    painter.rect_filled(glass, 2.0, Color32::from_rgb(54, 180, 210));
    painter.line_segment(
        [
            Pos2::new(glass.left() + glass.width() * 0.15, glass.center().y),
            Pos2::new(glass.right() - glass.width() * 0.15, glass.center().y),
        ],
        Stroke::new(1.2, Color32::from_rgb(136, 238, 244)),
    );
}

fn draw_network_multifunction_icon(painter: &egui::Painter, rect: Rect) {
    draw_network_printer_icon(painter, rect);
    let lid = Rect::from_min_max(
        Pos2::new(
            rect.left() + rect.width() * 0.20,
            rect.top() + rect.height() * 0.08,
        ),
        Pos2::new(
            rect.right() - rect.width() * 0.20,
            rect.top() + rect.height() * 0.24,
        ),
    );
    painter.rect_filled(lid, 1.5, Color32::from_rgb(45, 172, 204));
    painter.rect_stroke(lid, 1.5, Stroke::new(0.9, Color32::from_rgb(132, 229, 238)));
}

fn draw_network_device_icon(painter: &egui::Painter, rect: Rect) {
    let body = Rect::from_min_max(
        Pos2::new(
            rect.left() + rect.width() * 0.16,
            rect.top() + rect.height() * 0.18,
        ),
        Pos2::new(
            rect.right() - rect.width() * 0.16,
            rect.bottom() - rect.height() * 0.15,
        ),
    );
    painter.rect_filled(body, 3.0, Color32::from_rgb(84, 104, 108));
    painter.rect_stroke(
        body,
        3.0,
        Stroke::new(1.0, Color32::from_rgb(165, 185, 186)),
    );
    painter.circle_filled(
        Pos2::new(body.left() + body.width() * 0.18, body.center().y),
        (rect.width() * 0.04).max(1.1),
        Color32::from_rgb(41, 214, 101),
    );
}

fn draw_folder_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let tab = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, rect.top() + 4.0),
        Pos2::new(rect.left() + rect.width() * 0.52, rect.top() + 8.0),
    );
    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, rect.top() + 7.0),
        Pos2::new(rect.right() - 1.0, rect.bottom() - 2.0),
    );
    painter.rect_filled(tab, 1.5, Color32::from_rgb(255, 216, 86));
    painter.rect_filled(body, 2.0, color);
    painter.line_segment(
        [body.left_bottom(), body.right_bottom()],
        Stroke::new(1.0, Color32::from_rgb(205, 150, 38)),
    );
}

fn draw_drive_icon(painter: &egui::Painter, rect: Rect) {
    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 2.0, rect.top() + 7.0),
        Pos2::new(rect.right() - 2.0, rect.bottom() - 4.0),
    );
    painter.rect_filled(body, 2.0, theme::DRIVE);
    painter.rect_stroke(
        body,
        2.0,
        Stroke::new(1.0, Color32::from_rgb(195, 205, 205)),
    );
    painter.circle_filled(
        Pos2::new(body.left() + 4.0, body.center().y),
        1.3,
        theme::CANVAS,
    );
}

fn draw_file_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 3.0, rect.top() + 2.0),
        Pos2::new(rect.right() - 3.0, rect.bottom() - 2.0),
    );
    painter.rect_filled(body, 1.5, color);
    let fold = vec![
        Pos2::new(body.right() - 5.0, body.top()),
        Pos2::new(body.right(), body.top() + 5.0),
        Pos2::new(body.right() - 5.0, body.top() + 5.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        fold,
        Color32::from_rgba_premultiplied(255, 255, 255, 90),
        Stroke::NONE,
    ));
}

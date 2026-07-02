use eframe::egui::{self, FontId};

use crate::app::config::AppConfig;
use crate::app::state::{BExplorerApp, FileSort};
use crate::fs::explorer::FileEntry;
use crate::ui::{i18n, theme};

use super::text::format_bytes_opt;
use super::{entry_display_name, entry_location_text};

pub(super) const NAME_MIN_WIDTH: f32 = 240.0;
pub(super) const NAME_MAX_WIDTH: f32 = 700.0;
pub(super) const COLUMN_MIN_WIDTH: f32 = 92.0;
pub(super) const COLUMN_MAX_WIDTH: f32 = 500.0;
pub(super) const LOCATION_MIN_WIDTH: f32 = 180.0;
pub(super) const LOCATION_MAX_WIDTH: f32 = 900.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ColumnKind {
    Name,
    Type,
    FileSystem,
    FreeSpace,
    Size,
    PercentFull,
    Modified,
    Location,
}

#[derive(Clone, Copy)]
pub(super) struct ColumnSpec {
    pub(super) title: &'static str,
    pub(super) width: f32,
    pub(super) min_width: f32,
    pub(super) max_width: f32,
    pub(super) sort: Option<FileSort>,
    pub(super) right_align: bool,
    pub(super) kind: ColumnKind,
}

pub(super) fn columns_for_view(app: &BExplorerApp, width: f32) -> Vec<ColumnSpec> {
    let all = vec![
        ColumnSpec {
            title: "Name",
            width: NAME_MIN_WIDTH,
            min_width: NAME_MIN_WIDTH,
            max_width: NAME_MAX_WIDTH,
            sort: Some(FileSort::Name),
            right_align: false,
            kind: ColumnKind::Name,
        },
        ColumnSpec {
            title: "Type",
            width: COLUMN_MIN_WIDTH,
            min_width: COLUMN_MIN_WIDTH,
            max_width: COLUMN_MAX_WIDTH,
            sort: Some(FileSort::Type),
            right_align: false,
            kind: ColumnKind::Type,
        },
        ColumnSpec {
            title: "File System",
            width: COLUMN_MIN_WIDTH,
            min_width: COLUMN_MIN_WIDTH,
            max_width: COLUMN_MAX_WIDTH,
            sort: None,
            right_align: false,
            kind: ColumnKind::FileSystem,
        },
        ColumnSpec {
            title: "Free Space",
            width: COLUMN_MIN_WIDTH,
            min_width: COLUMN_MIN_WIDTH,
            max_width: COLUMN_MAX_WIDTH,
            sort: None,
            right_align: false,
            kind: ColumnKind::FreeSpace,
        },
        ColumnSpec {
            title: "Size",
            width: COLUMN_MIN_WIDTH,
            min_width: COLUMN_MIN_WIDTH,
            max_width: COLUMN_MAX_WIDTH,
            sort: Some(FileSort::Size),
            right_align: true,
            kind: ColumnKind::Size,
        },
        ColumnSpec {
            title: "Percent Full",
            width: COLUMN_MIN_WIDTH,
            min_width: COLUMN_MIN_WIDTH,
            max_width: COLUMN_MAX_WIDTH,
            sort: None,
            right_align: false,
            kind: ColumnKind::PercentFull,
        },
        ColumnSpec {
            title: "Modified Date",
            width: COLUMN_MIN_WIDTH,
            min_width: COLUMN_MIN_WIDTH,
            max_width: COLUMN_MAX_WIDTH,
            sort: Some(FileSort::Modified),
            right_align: false,
            kind: ColumnKind::Modified,
        },
        ColumnSpec {
            title: "Location",
            width: LOCATION_MIN_WIDTH,
            min_width: LOCATION_MIN_WIDTH,
            max_width: LOCATION_MAX_WIDTH,
            sort: None,
            right_align: false,
            kind: ColumnKind::Location,
        },
    ];

    // Apply saved widths (clamped to [min, max])
    let mut all_mut = all;
    for (i, col) in all_mut.iter_mut().enumerate() {
        let saved = app.column_widths[i].max(col.min_width).min(col.max_width);
        col.width = saved;
    }

    // Filter out drive-only columns in regular folder view
    let mut columns: Vec<ColumnSpec> = if app.is_storage_view() {
        all_mut
            .into_iter()
            .filter(|c| c.kind != ColumnKind::Location)
            .collect()
    } else {
        all_mut
            .into_iter()
            .filter(|c| {
                !matches!(
                    c.kind,
                    ColumnKind::FileSystem
                        | ColumnKind::FreeSpace
                        | ColumnKind::PercentFull
                        | ColumnKind::Location
                )
            })
            .collect()
    };
    if app.showing_complete_search_results() && !app.is_storage_view() {
        let location = ColumnSpec {
            title: "Location",
            width: app.column_widths[column_index(ColumnKind::Location)]
                .max(LOCATION_MIN_WIDTH)
                .min(LOCATION_MAX_WIDTH),
            min_width: LOCATION_MIN_WIDTH,
            max_width: LOCATION_MAX_WIDTH,
            sort: None,
            right_align: false,
            kind: ColumnKind::Location,
        };
        columns.push(location);
    }

    let base: f32 = columns.iter().map(|column| column.width).sum();
    if width > base {
        let extra = width - base;
        if let Some(last) = columns.last_mut() {
            last.width += extra;
        }
    } else if width < base {
        let mut deficit = base - width;
        for column in columns.iter_mut().rev() {
            let available = (column.width - column.min_width).max(0.0);
            let take = available.min(deficit);
            column.width -= take;
            deficit -= take;
            if deficit <= 0.0 {
                break;
            }
        }
    }

    columns
}

pub(super) fn compute_auto_fit_widths(
    entries: &[FileEntry],
    visible_kinds: &[ColumnKind],
    available_width: f32,
    ctx: &egui::Context,
    config: &AppConfig,
) -> [f32; 8] {
    if entries.is_empty() {
        return [
            NAME_MIN_WIDTH,
            COLUMN_MIN_WIDTH,
            COLUMN_MIN_WIDTH,
            COLUMN_MIN_WIDTH,
            COLUMN_MIN_WIDTH,
            COLUMN_MIN_WIDTH,
            COLUMN_MIN_WIDTH,
            LOCATION_MIN_WIDTH,
        ];
    }

    let font_12 = theme::font(config, 12.0);
    let font_name = theme::font(config, 12.6);
    let color = egui::Color32::WHITE; // any color, just for measurement

    let mut widths = [0.0_f32; 8];

    for kind in visible_kinds {
        let max_width = entries
            .iter()
            .map(|entry| {
                let (text, font): (String, FontId) = match kind {
                    ColumnKind::Name => {
                        let display = entry_display_name(config, entry);
                        (display, font_name.clone())
                    }
                    ColumnKind::Type => (
                        localized_type_label(config, &entry.type_label()),
                        font_12.clone(),
                    ),
                    ColumnKind::FileSystem => (entry.file_system.clone(), font_12.clone()),
                    ColumnKind::FreeSpace => (format_bytes_opt(entry.free_space), font_12.clone()),
                    ColumnKind::Size => (format_bytes_opt(entry.size), font_12.clone()),
                    ColumnKind::Modified => (
                        entry.modified.as_deref().unwrap_or("").to_string(),
                        font_12.clone(),
                    ),
                    ColumnKind::PercentFull => ("100%".to_string(), font_12.clone()),
                    ColumnKind::Location => (entry_location_text(entry), font_12.clone()),
                };
                let galley = ctx.fonts(|f| f.layout_no_wrap(text, font.clone(), color));
                let w = galley.size().x;
                match kind {
                    ColumnKind::Name => w + 17.0 + 6.0 + 20.0, // icon + gap + padding
                    ColumnKind::PercentFull => w + 20.0,       // padding
                    _ => w + 20.0,
                }
            })
            .fold(f32::MIN, f32::max);

        let (min_w, max_w) = match kind {
            ColumnKind::Name => (NAME_MIN_WIDTH, NAME_MAX_WIDTH),
            ColumnKind::Location => (LOCATION_MIN_WIDTH, LOCATION_MAX_WIDTH),
            _ => (COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH),
        };
        let width = max_width.max(min_w).min(max_w);

        let index = match kind {
            ColumnKind::Name => 0,
            ColumnKind::Type => 1,
            ColumnKind::FileSystem => 2,
            ColumnKind::FreeSpace => 3,
            ColumnKind::Size => 4,
            ColumnKind::PercentFull => 5,
            ColumnKind::Modified => 6,
            ColumnKind::Location => 7,
        };
        widths[index] = width;
    }

    // Defaults for non-visible columns
    for w in widths.iter_mut() {
        if *w == 0.0 {
            *w = COLUMN_MIN_WIDTH;
        }
    }

    compact_widths_to_available(&mut widths, visible_kinds, available_width);

    widths
}

pub(super) fn compact_column_widths(visible_kinds: &[ColumnKind]) -> [f32; 8] {
    let mut widths = [
        NAME_MIN_WIDTH,
        COLUMN_MIN_WIDTH,
        COLUMN_MIN_WIDTH,
        COLUMN_MIN_WIDTH,
        COLUMN_MIN_WIDTH,
        COLUMN_MIN_WIDTH,
        COLUMN_MIN_WIDTH,
        LOCATION_MIN_WIDTH,
    ];

    for kind in visible_kinds {
        widths[column_index(*kind)] = column_min_width(*kind);
    }

    widths
}

pub(super) fn compact_widths_to_available(
    widths: &mut [f32; 8],
    visible_kinds: &[ColumnKind],
    available_width: f32,
) {
    let visible_total: f32 = visible_kinds
        .iter()
        .map(|kind| widths[column_index(*kind)])
        .sum();
    let mut deficit = visible_total - available_width.max(0.0);
    if deficit <= 0.0 {
        return;
    }

    for kind in visible_kinds.iter().rev() {
        let index = column_index(*kind);
        let min_width = column_min_width(*kind);
        let available = (widths[index] - min_width).max(0.0);
        let take = available.min(deficit);
        widths[index] -= take;
        deficit -= take;
        if deficit <= 0.0 {
            break;
        }
    }
}

pub(super) fn column_index(kind: ColumnKind) -> usize {
    match kind {
        ColumnKind::Name => 0,
        ColumnKind::Type => 1,
        ColumnKind::FileSystem => 2,
        ColumnKind::FreeSpace => 3,
        ColumnKind::Size => 4,
        ColumnKind::PercentFull => 5,
        ColumnKind::Modified => 6,
        ColumnKind::Location => 7,
    }
}

pub(super) fn column_min_width(kind: ColumnKind) -> f32 {
    match kind {
        ColumnKind::Name => NAME_MIN_WIDTH,
        ColumnKind::Location => LOCATION_MIN_WIDTH,
        _ => COLUMN_MIN_WIDTH,
    }
}

pub(super) fn localized_column_title<'a>(app: &'a BExplorerApp, title: &'static str) -> &'a str {
    match title {
        "Name" => i18n::tr(&app.config, "name"),
        "Type" => i18n::tr(&app.config, "type"),
        "File System" => i18n::tr(&app.config, "file_system"),
        "Free Space" => i18n::tr(&app.config, "free_space"),
        "Size" => i18n::tr(&app.config, "size"),
        "Percent Full" => i18n::tr(&app.config, "percent_full"),
        "Modified Date" => i18n::tr(&app.config, "modified_date"),
        "Location" => i18n::tr(&app.config, "location"),
        _ => title,
    }
}

pub(super) fn localized_type_label(config: &AppConfig, label: &str) -> String {
    if config.language != "es" {
        return label.to_string();
    }

    match label {
        "Drive" => return "Unidad".into(),
        "Folder" => return "Carpeta".into(),
        "File" => return "Archivo".into(),
        "Symlink" => return "Enlace".into(),
        "Other" => return "Otro".into(),
        "Local Disk" => return "Disco local".into(),
        "USB Drive" => return "Unidad USB".into(),
        "Network Drive" => return "Unidad de red".into(),
        "Network Computer" => return "Equipo de red".into(),
        "Network Printer" => return "Impresora de red".into(),
        "Network Scanner" => return "Escaner de red".into(),
        "Network Multifunction Device" => return "Dispositivo multifuncion".into(),
        "Network Device" => return "Dispositivo de red".into(),
        "Optical Drive" => return "Unidad optica".into(),
        "RAM Disk" => return "Disco RAM".into(),
        _ => {}
    }

    if let Some((base, ext)) = label.rsplit_once(' ') {
        let translated_base = match base {
            "Application" => "Aplicacion",
            "Image" => "Imagen",
            "Audio" => "Audio",
            "Video" => "Video",
            "Archive" => "Archivo comprimido",
            "Document" => "Documento",
            "Spreadsheet" => "Hoja de calculo",
            "Presentation" => "Presentacion",
            "Source code" => "Codigo fuente",
            "Font" => "Fuente",
            "System file" => "Archivo del sistema",
            "Disk image" => "Imagen de disco",
            "File" => "Archivo",
            _ => base,
        };
        return format!("{} {}", translated_base, ext);
    }

    label.to_string()
}

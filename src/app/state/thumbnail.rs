use std::path::{Path, PathBuf};

use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Duration;

use eframe::egui::{self, ColorImage};

use crate::fs::explorer::{self, EntryKind, FileCategory, FileEntry};

use super::types::{
    NativeIconJob, NativeIconState, PortableThumbnailJob, PreviewCacheState, PreviewJob,
    ThumbnailJob, ThumbnailState,
};
use super::{BExplorerApp, PreviewContentRef};

const NATIVE_ICON_SIZE: u32 = 256;

impl BExplorerApp {
    pub fn thumbnail_texture_id(
        &mut self,
        ctx: &egui::Context,
        entry: &FileEntry,
    ) -> Option<egui::TextureId> {
        if !is_thumbnail_candidate(entry) {
            return None;
        }

        if let Some(state) = self.thumbnail_cache.get(&entry.path) {
            return match state {
                ThumbnailState::Ready(texture) => Some(texture.id()),
                ThumbnailState::Loading => None,
                ThumbnailState::Missing => None,
            };
        }

        if explorer::is_portable_path(&entry.path) {
            self.thumbnail_cache
                .insert(entry.path.clone(), ThumbnailState::Loading);
            let max_bytes = self.config.preview_limit_bytes.max(512 * 1024);
            let allow_default_resource = entry
                .size
                .is_some_and(|size| size <= self.config.preview_limit_bytes as u64);
            let _ = self.portable_thumbnail_tx.send(PortableThumbnailJob {
                path: entry.path.clone(),
                max_bytes,
                allow_default_resource,
            });
            ctx.request_repaint_after(Duration::from_millis(80));
            return None;
        }

        let Some(size) = entry.size else {
            self.thumbnail_cache
                .insert(entry.path.clone(), ThumbnailState::Missing);
            return None;
        };

        if size > self.config.preview_limit_bytes as u64 {
            self.thumbnail_cache
                .insert(entry.path.clone(), ThumbnailState::Missing);
            return None;
        }

        self.thumbnail_cache
            .insert(entry.path.clone(), ThumbnailState::Loading);
        if self
            .thumbnail_job_tx
            .send(ThumbnailJob {
                path: entry.path.clone(),
            })
            .is_err()
        {
            self.thumbnail_cache
                .insert(entry.path.clone(), ThumbnailState::Missing);
            return None;
        }
        ctx.request_repaint_after(Duration::from_millis(80));
        None
    }

    pub fn native_icon_texture_id(
        &mut self,
        ctx: &egui::Context,
        entry: &FileEntry,
    ) -> Option<egui::TextureId> {
        if explorer::is_virtual_path(&entry.path) {
            let (key, icon_path, is_directory) = virtual_native_icon_request(entry)?;
            return self.native_icon_texture_for_key(ctx, &key, &icon_path, is_directory);
        }
        let key = native_entry_icon_cache_key(entry);
        self.native_icon_texture_for_key(
            ctx,
            &key,
            &entry.path,
            entry.kind == EntryKind::Folder || entry.kind == EntryKind::Drive,
        )
    }

    pub fn native_path_icon_texture_id(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        is_directory: bool,
    ) -> Option<egui::TextureId> {
        if explorer::is_virtual_path(path) {
            if is_directory {
                return self.native_icon_texture_for_key(
                    ctx,
                    Path::new("__bexplorer_virtual_folder_icon"),
                    Path::new("bexplorer-folder"),
                    true,
                );
            }
            return None;
        }
        let key = native_path_icon_cache_key(path, is_directory, NATIVE_ICON_SIZE);
        self.native_icon_texture_for_key(ctx, &key, path, is_directory)
    }

    pub fn preview_content(
        &mut self,
        ctx: &egui::Context,
        entry: &FileEntry,
    ) -> Option<PreviewContentRef> {
        let active_path_changed = self.preview_active_path.as_ref() != Some(&entry.path);
        if active_path_changed {
            self.preview_active_path = Some(entry.path.clone());
        }

        if let Some(state) = self.preview_cache.get(&entry.path) {
            match state {
                PreviewCacheState::Images {
                    textures,
                    generation: _,
                    loading,
                    page_count,
                } => {
                    if *loading {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                    return Some(PreviewContentRef::Images {
                        images: textures
                            .iter()
                            .map(|texture| (texture.id(), texture.size_vec2()))
                            .collect(),
                        loading: *loading,
                        page_count: *page_count,
                    });
                }
                PreviewCacheState::Text(text) => {
                    return Some(PreviewContentRef::Text(text.clone()));
                }
                PreviewCacheState::Loading(_) => {
                    ctx.request_repaint_after(Duration::from_millis(16));
                    return Some(PreviewContentRef::Loading);
                }
                PreviewCacheState::Missing => return None,
            }
        }

        let current_generation = self
            .preview_generation
            .fetch_add(1, AtomicOrdering::Relaxed)
            .saturating_add(1);
        self.preview_cache.insert(
            entry.path.clone(),
            PreviewCacheState::Loading(current_generation),
        );
        if self
            .preview_tx
            .send(PreviewJob {
                entry: entry.clone(),
                max_bytes: self.config.preview_limit_bytes,
                generation: current_generation,
            })
            .is_err()
        {
            self.preview_cache
                .insert(entry.path.clone(), PreviewCacheState::Missing);
            return None;
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        Some(PreviewContentRef::Loading)
    }

    fn native_icon_texture_for_key(
        &mut self,
        ctx: &egui::Context,
        cache_key: &Path,
        path: &Path,
        is_directory: bool,
    ) -> Option<egui::TextureId> {
        if let Some(state) = self.native_icon_cache.get(cache_key) {
            return match state {
                NativeIconState::Ready(texture) => Some(texture.id()),
                NativeIconState::Loading => None,
                NativeIconState::Missing => None,
            };
        }

        self.native_icon_cache
            .insert(cache_key.to_path_buf(), NativeIconState::Loading);
        if self
            .native_icon_job_tx
            .send(NativeIconJob {
                cache_key: cache_key.to_path_buf(),
                path: path.to_path_buf(),
                is_directory,
                size: NATIVE_ICON_SIZE,
            })
            .is_err()
        {
            self.native_icon_cache
                .insert(cache_key.to_path_buf(), NativeIconState::Missing);
            return None;
        }
        ctx.request_repaint_after(Duration::from_millis(80));
        None
    }
}

pub(super) fn is_thumbnail_candidate(entry: &FileEntry) -> bool {
    if entry.kind != EntryKind::File {
        return false;
    }

    matches!(entry.category, FileCategory::Image)
        || (explorer::is_portable_path(&entry.path) && entry.category == FileCategory::Video)
}

pub(super) fn is_iso_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("iso"))
}

pub(super) fn virtual_native_icon_request(entry: &FileEntry) -> Option<(PathBuf, PathBuf, bool)> {
    if !explorer::is_portable_path(&entry.path) {
        return None;
    }

    match entry.kind {
        EntryKind::Folder => Some((
            PathBuf::from(format!(
                "__bexplorer_portable_folder_icon_size_{NATIVE_ICON_SIZE}"
            )),
            PathBuf::from("bexplorer-folder"),
            true,
        )),
        EntryKind::File | EntryKind::Symlink | EntryKind::Other => {
            let extension = entry
                .path
                .extension()
                .and_then(|value| value.to_str())
                .or_else(|| entry.name.rsplit_once('.').map(|(_, extension)| extension))
                .map(|extension| {
                    extension
                        .trim()
                        .trim_start_matches('.')
                        .to_ascii_lowercase()
                })
                .filter(|extension| !extension.is_empty())
                .unwrap_or_else(|| "file".into());
            Some((
                PathBuf::from(format!(
                    "__bexplorer_portable_ext_{extension}_size_{NATIVE_ICON_SIZE}"
                )),
                PathBuf::from(format!("bexplorer.{extension}")),
                false,
            ))
        }
        EntryKind::Drive => None,
    }
}

#[cfg(target_os = "windows")]
pub(super) fn native_entry_icon_cache_key(entry: &FileEntry) -> PathBuf {
    match entry.kind {
        EntryKind::Drive => PathBuf::from(format!(
            "__bexplorer_drive_{:?}_{}",
            entry.drive_kind,
            entry.path.display().to_string().replace(['\\', ':'], "_")
        )),
        EntryKind::Folder | EntryKind::File | EntryKind::Symlink | EntryKind::Other => {
            entry.path.clone()
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn native_entry_icon_cache_key(entry: &FileEntry) -> PathBuf {
    match entry.kind {
        EntryKind::Drive => PathBuf::from(format!(
            "__bexplorer_drive_{:?}_{}_size_{NATIVE_ICON_SIZE}",
            entry.drive_kind,
            native_directory_icon_class(&entry.path)
        )),
        EntryKind::Folder => native_path_icon_cache_key(&entry.path, true, NATIVE_ICON_SIZE),
        EntryKind::File | EntryKind::Symlink | EntryKind::Other => {
            native_file_icon_cache_key(&entry.path, Some(&entry.name), NATIVE_ICON_SIZE)
        }
    }
}

#[cfg(target_os = "windows")]
fn native_path_icon_cache_key(path: &Path, _is_directory: bool, _size: u32) -> PathBuf {
    path.to_path_buf()
}

#[cfg(not(target_os = "windows"))]
fn native_path_icon_cache_key(path: &Path, is_directory: bool, size: u32) -> PathBuf {
    if is_directory {
        PathBuf::from(format!(
            "__bexplorer_native_folder_{}_size_{size}",
            native_directory_icon_class(path)
        ))
    } else {
        native_file_icon_cache_key(path, None, size)
    }
}

fn native_directory_icon_class(path: &Path) -> &'static str {
    if path == Path::new("/") {
        "root"
    } else if path.starts_with("/media") || path.starts_with("/run/media") {
        "removable"
    } else if path.starts_with("/mnt") {
        "mnt"
    } else {
        "folder"
    }
}

fn native_file_icon_cache_key(path: &Path, fallback_name: Option<&str>, size: u32) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .or_else(|| {
            fallback_name.and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        })
        .map(|extension| {
            extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase()
        })
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "none".into());
    PathBuf::from(format!(
        "__bexplorer_native_file_ext_{extension}_size_{size}"
    ))
}

pub(super) fn load_thumbnail_image(path: &Path) -> Option<ColorImage> {
    let bytes = std::fs::read(path).ok()?;
    load_thumbnail_image_from_bytes(&bytes)
}

pub(super) fn load_desktop_thumbnail_image(path: &Path) -> Option<ColorImage> {
    let image = crate::platform::cached_desktop_thumbnail(path)?;
    Some(ColorImage::from_rgba_unmultiplied(
        [image.width, image.height],
        &image.rgba,
    ))
}

pub(super) fn load_native_icon_image(
    path: &Path,
    is_directory: bool,
    size: u32,
) -> Option<ColorImage> {
    let icon = if size >= 128 {
        crate::platform::native_file_icon_highres(path, is_directory)
            .or_else(|| crate::platform::native_file_icon(path, is_directory, size))
    } else {
        crate::platform::native_file_icon(path, is_directory, size)
            .or_else(|| crate::platform::native_file_icon_highres(path, is_directory))
    }?;
    Some(ColorImage::from_rgba_unmultiplied(
        [icon.width, icon.height],
        &icon.rgba,
    ))
}

pub(super) fn load_thumbnail_image_from_bytes(bytes: &[u8]) -> Option<ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let thumbnail = image.thumbnail(256, 256).to_rgba8();
    let size = [thumbnail.width() as usize, thumbnail.height() as usize];
    let pixels = thumbnail.into_raw();
    Some(ColorImage::from_rgba_unmultiplied(size, &pixels))
}

#[cfg(target_os = "windows")]
pub(super) fn load_portable_thumbnail_image(
    path: &Path,
    max_bytes: usize,
    allow_default_resource: bool,
) -> Option<ColorImage> {
    let (device_id, object_id) = explorer::portable_object_from_path(path)?;
    let bytes = crate::platform::portable_device_thumbnail(
        &device_id,
        &object_id,
        max_bytes,
        allow_default_resource,
    )?;
    load_thumbnail_image_from_bytes(&bytes)
}

#[cfg(not(target_os = "windows"))]
pub(super) fn load_portable_thumbnail_image(
    _path: &Path,
    _max_bytes: usize,
    _allow_default_resource: bool,
) -> Option<ColorImage> {
    None
}

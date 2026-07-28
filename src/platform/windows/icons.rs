use std::sync::{Condvar, Mutex, OnceLock};

use crate::platform::NativeIconImage;

const MAX_WINDOWS_THUMBNAIL_EDGE: u32 = 1024;
const MAX_WINDOWS_THUMBNAIL_BYTES: usize = 64 * 1024 * 1024;
const MAX_CONCURRENT_WINDOWS_THUMBNAILERS: usize = 2;

#[derive(Clone, Copy)]
enum ShellThumbnailMode {
    CacheOnly,
    Extract,
}

struct ShellThumbnailResult {
    image: NativeIconImage,
    low_quality: bool,
}

struct WindowsThumbnailPermit {
    state: &'static (Mutex<usize>, Condvar),
}

impl Drop for WindowsThumbnailPermit {
    fn drop(&mut self) {
        let (active, available) = self.state;
        let mut active = active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = active.saturating_sub(1);
        available.notify_one();
    }
}

#[cfg(target_os = "windows")]
pub fn native_file_icon(
    path: &std::path::Path,
    is_directory: bool,
    size: u32,
) -> Option<NativeIconImage> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
    };
    use windows::Win32::UI::Shell::{
        SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES,
        SHGetFileInfoW,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
    use windows::core::PCWSTR;

    if path == std::path::Path::new(crate::app::thumbnail_data::TRASH_EMPTY_ICON_LOOKUP_PATH) {
        return native_recycle_bin_icon(false, size);
    }
    if path == std::path::Path::new(crate::app::thumbnail_data::TRASH_FULL_ICON_LOOKUP_PATH) {
        return native_recycle_bin_icon(true, size);
    }

    // SHGetFileInfo only exposes the legacy 16 px and 32 px image lists.
    // Ask the Shell item factory for compact 48 px sources so Details and
    // Small Icons do not upscale a 32 px icon or downscale the 256 px one.
    if size > 32
        && size < 128
        && let Some(image) = shell_item_icon(path, size)
    {
        return Some(image);
    }

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let attributes: FILE_FLAGS_AND_ATTRIBUTES = if is_directory {
        FILE_ATTRIBUTE_DIRECTORY
    } else {
        FILE_ATTRIBUTE_NORMAL
    };

    let mut info = SHFILEINFOW::default();
    let icon_size_flag = if size <= 16 {
        SHGFI_SMALLICON
    } else {
        SHGFI_LARGEICON
    };
    let flags = SHGFI_ICON | icon_size_flag;

    let mut result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };

    if result == 0 || info.hIcon.0.is_null() {
        result = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                attributes,
                Some(&mut info),
                size_of::<SHFILEINFOW>() as u32,
                flags | SHGFI_USEFILEATTRIBUTES,
            )
        };
    }

    if result == 0 || info.hIcon.0.is_null() {
        return None;
    }

    let icon = info.hIcon;
    let image = hicon_to_rgba(icon, size);
    let _ = unsafe { DestroyIcon(icon) };
    image
}

fn native_recycle_bin_icon(full: bool, size: u32) -> Option<NativeIconImage> {
    use std::mem::size_of;

    use windows::Win32::UI::Shell::{
        SHGSI_ICON, SHGSI_LARGEICON, SHGSI_SMALLICON, SHGetStockIconInfo, SHSTOCKICONINFO,
        SIID_RECYCLER, SIID_RECYCLERFULL,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

    let mut info = SHSTOCKICONINFO {
        cbSize: size_of::<SHSTOCKICONINFO>() as u32,
        ..Default::default()
    };
    let stock_id = if full {
        SIID_RECYCLERFULL
    } else {
        SIID_RECYCLER
    };
    let size_flag = if size <= 16 {
        SHGSI_SMALLICON
    } else {
        SHGSI_LARGEICON
    };
    unsafe {
        SHGetStockIconInfo(stock_id, SHGSI_ICON | size_flag, &mut info).ok()?;
    }
    if info.hIcon.0.is_null() {
        return None;
    }

    let image = hicon_to_rgba(info.hIcon, size.clamp(16, 256));
    let _ = unsafe { DestroyIcon(info.hIcon) };
    image
}

#[cfg(target_os = "windows")]
pub fn native_file_icon_highres(
    path: &std::path::Path,
    is_directory: bool,
) -> Option<NativeIconImage> {
    shell_item_icon(path, 256)
        .or_else(|| native_file_icon_highres_from_system_list(path, is_directory))
}

pub fn cached_desktop_thumbnail(path: &std::path::Path, size: u32) -> Option<NativeIconImage> {
    shell_thumbnail(path, size, ShellThumbnailMode::CacheOnly)
        .filter(|thumbnail| !thumbnail.low_quality)
        .map(|thumbnail| thumbnail.image)
        .or_else(|| crate::platform::thumbnail_fallback_cache::load(path, size))
}

pub fn cache_desktop_thumbnail(path: &std::path::Path, size: u32, image: &NativeIconImage) -> bool {
    crate::platform::thumbnail_fallback_cache::save(path, size, image)
}

pub fn image_thumbnail(path: &std::path::Path, size: u32) -> Option<NativeIconImage> {
    shell_thumbnail(path, size, ShellThumbnailMode::Extract).map(|thumbnail| thumbnail.image)
}

pub fn video_thumbnail(path: &std::path::Path, size: u32) -> Option<NativeIconImage> {
    shell_thumbnail(path, size, ShellThumbnailMode::Extract).map(|thumbnail| thumbnail.image)
}

fn shell_thumbnail(
    path: &std::path::Path,
    size: u32,
    mode: ShellThumbnailMode,
) -> Option<ShellThumbnailResult> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        ISharedBitmap, IShellItem, IThumbnailCache, LocalThumbnailCache,
        SHCreateItemFromParsingName, WTS_CACHEFLAGS, WTS_EXTRACT, WTS_INCACHEONLY, WTS_LOWQUALITY,
    };
    use windows::core::PCWSTR;

    if !path.is_file() {
        return None;
    }
    let _permit =
        matches!(mode, ShellThumbnailMode::Extract).then(acquire_windows_thumbnail_permit);
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let edge = size.clamp(32, MAX_WINDOWS_THUMBNAIL_EDGE);

    let thumbnail = (|| {
        let item: IShellItem =
            unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
        let cache: IThumbnailCache =
            unsafe { CoCreateInstance(&LocalThumbnailCache, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let mut shared: Option<ISharedBitmap> = None;
        let mut cache_flags = WTS_CACHEFLAGS::default();
        let request_flags = match mode {
            ShellThumbnailMode::CacheOnly => WTS_INCACHEONLY,
            ShellThumbnailMode::Extract => WTS_EXTRACT,
        };
        unsafe {
            cache
                .GetThumbnail(
                    &item,
                    edge,
                    request_flags,
                    Some(&mut shared),
                    Some(&mut cache_flags),
                    None,
                )
                .ok()?;
        }
        Some(ShellThumbnailResult {
            image: shared_bitmap_to_rgba(&shared?)?,
            low_quality: cache_flags.contains(WTS_LOWQUALITY),
        })
    })();

    if initialized {
        unsafe { CoUninitialize() };
    }
    thumbnail
}

fn shared_bitmap_to_rgba(
    shared: &windows::Win32::UI::Shell::ISharedBitmap,
) -> Option<NativeIconImage> {
    let size = unsafe { shared.GetSize() }.ok()?;
    let width = u32::try_from(size.cx).ok()?;
    let height = u32::try_from(size.cy).ok()?;
    if width == 0
        || height == 0
        || width > MAX_WINDOWS_THUMBNAIL_EDGE
        || height > MAX_WINDOWS_THUMBNAIL_EDGE
        || (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?
            > MAX_WINDOWS_THUMBNAIL_BYTES
    {
        return None;
    }
    let bitmap = unsafe { shared.GetSharedBitmap() }.ok()?;
    let alpha = unsafe { shared.GetFormat() }.unwrap_or(windows::Win32::UI::Shell::WTSAT_UNKNOWN);
    hbitmap_thumbnail_to_rgba(bitmap, width, height, alpha)
}

fn hbitmap_thumbnail_to_rgba(
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    width: u32,
    height: u32,
    alpha_type: windows::Win32::UI::Shell::WTS_ALPHATYPE,
) -> Option<NativeIconImage> {
    use std::mem::size_of;

    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        GetDIBits,
    };
    use windows::Win32::UI::Shell::{WTSAT_ARGB, WTSAT_RGB};

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let byte_len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if byte_len > MAX_WINDOWS_THUMBNAIL_BYTES {
        return None;
    }
    let mut bgra = vec![0_u8; byte_len];
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return None;
    }
    let rows = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    let _ = unsafe { DeleteDC(dc) };
    if rows != height as i32 {
        return None;
    }

    let preserve_alpha = alpha_type == WTSAT_ARGB
        || (alpha_type != WTSAT_RGB && bgra.chunks_exact(4).any(|pixel| pixel[3] != 0));
    let mut rgba = Vec::with_capacity(byte_len);
    for pixel in bgra.chunks_exact(4) {
        let alpha = if preserve_alpha { pixel[3] } else { 255 };
        let (red, green, blue) = if alpha_type == WTSAT_ARGB && alpha > 0 && alpha < 255 {
            (
                unpremultiply_channel(pixel[2], alpha),
                unpremultiply_channel(pixel[1], alpha),
                unpremultiply_channel(pixel[0], alpha),
            )
        } else {
            (pixel[2], pixel[1], pixel[0])
        };
        rgba.extend_from_slice(&[red, green, blue, alpha]);
    }
    Some(NativeIconImage {
        rgba,
        width: width as usize,
        height: height as usize,
    })
}

fn unpremultiply_channel(channel: u8, alpha: u8) -> u8 {
    ((channel as u32 * 255 + alpha as u32 / 2) / alpha as u32).min(255) as u8
}

fn acquire_windows_thumbnail_permit() -> WindowsThumbnailPermit {
    static STATE: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    let state = STATE.get_or_init(|| (Mutex::new(0), Condvar::new()));
    let (active, available) = state;
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while *active >= MAX_CONCURRENT_WINDOWS_THUMBNAILERS {
        active = available
            .wait(active)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    *active += 1;
    drop(active);
    WindowsThumbnailPermit { state }
}

#[cfg(target_os = "windows")]
fn shell_item_icon(path: &std::path::Path, size: u32) -> Option<NativeIconImage> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::DeleteObject;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
        SIIGBF_SCALEUP,
    };
    use windows::core::PCWSTR;

    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let image = (|| {
        let factory: IShellItemImageFactory =
            unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
        let bitmap = unsafe {
            factory.GetImage(
                SIZE {
                    cx: size as i32,
                    cy: size as i32,
                },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK | SIIGBF_SCALEUP,
            )
        }
        .ok()?;
        let image = hbitmap_to_rgba(bitmap, size);
        let _ = unsafe { DeleteObject(bitmap) };
        image
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    image
}

#[cfg(target_os = "windows")]
fn native_file_icon_highres_from_system_list(
    path: &std::path::Path,
    is_directory: bool,
) -> Option<NativeIconImage> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
    };
    use windows::Win32::UI::Controls::IImageList;
    use windows::Win32::UI::Shell::{
        SHFILEINFOW, SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
        SHGetImageList, SHIL_JUMBO,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
    use windows::core::PCWSTR;

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let attributes: FILE_FLAGS_AND_ATTRIBUTES = if is_directory {
        FILE_ATTRIBUTE_DIRECTORY
    } else {
        FILE_ATTRIBUTE_NORMAL
    };

    let mut info = SHFILEINFOW::default();
    let mut result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX | SHGFI_LARGEICON,
        )
    };
    if result == 0 {
        result = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                attributes,
                Some(&mut info),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
            )
        };
    }
    if result == 0 {
        return None;
    }

    let image_list: IImageList = unsafe { SHGetImageList(SHIL_JUMBO as i32) }.ok()?;

    let hicon = unsafe { image_list.GetIcon(info.iIcon, 0) }.ok()?;
    if hicon.0.is_null() {
        return None;
    }

    let image = hicon_to_rgba(hicon, 256);
    let _ = unsafe { DestroyIcon(hicon) };
    image
}

#[cfg(target_os = "windows")]
fn hbitmap_to_rgba(
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    size: u32,
) -> Option<NativeIconImage> {
    use std::mem::size_of;

    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        GetDIBits,
    };

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0_u8; (size * size * 4) as usize];
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return None;
    }
    let rows = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            size,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    let _ = unsafe { DeleteDC(dc) };
    if rows != size as i32 {
        return None;
    }

    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    Some(normalize_native_icon_canvas(rgba, size as usize))
}

fn hicon_to_rgba(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    size: u32,
) -> Option<NativeIconImage> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null_mut;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, HBRUSH, HGDIOBJ, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx};

    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.0.is_null() {
            return None;
        }

        let mut bits: *mut c_void = null_mut();
        let bitmap = match CreateDIBSection(
            hdc,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        ) {
            Ok(bitmap) => bitmap,
            Err(_) => {
                let _ = DeleteDC(hdc);
                return None;
            }
        };

        if bitmap.0.is_null() || bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(hdc);
            return None;
        }

        let previous = SelectObject(hdc, HGDIOBJ(bitmap.0));
        let draw_result = DrawIconEx(
            hdc,
            0,
            0,
            icon,
            size as i32,
            size as i32,
            0,
            HBRUSH::default(),
            DI_NORMAL,
        );

        if !previous.0.is_null() {
            let _ = SelectObject(hdc, previous);
        }

        if draw_result.is_err() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(hdc);
            return None;
        }

        let pixel_count = (size * size) as usize;
        let raw = std::slice::from_raw_parts(bits as *const u8, pixel_count * 4);
        let mut rgba = Vec::with_capacity(pixel_count * 4);
        let mut any_alpha = false;

        for bgra in raw.chunks_exact(4) {
            let blue = bgra[0];
            let green = bgra[1];
            let red = bgra[2];
            let alpha = bgra[3];
            any_alpha |= alpha != 0;
            rgba.extend_from_slice(&[red, green, blue, alpha]);
        }

        if !any_alpha {
            for pixel in rgba.chunks_exact_mut(4) {
                if pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0 {
                    pixel[3] = 255;
                }
            }
        }

        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(hdc);

        Some(normalize_native_icon_canvas(rgba, size as usize))
    }
}

fn normalize_native_icon_canvas(rgba: Vec<u8>, size: usize) -> NativeIconImage {
    let Some((left, top, right, bottom)) = visible_icon_bounds(&rgba, size, size) else {
        return NativeIconImage {
            rgba,
            width: size,
            height: size,
        };
    };

    let content_width = right - left + 1;
    let content_height = bottom - top + 1;
    if content_width < 2 || content_height < 2 {
        return NativeIconImage {
            rgba,
            width: size,
            height: size,
        };
    }

    let content_ratio = content_width.max(content_height) as f32 / size as f32;
    let center_x = (left + right) as f32 * 0.5;
    let center_y = (top + bottom) as f32 * 0.5;
    let canvas_center = (size.saturating_sub(1)) as f32 * 0.5;
    let off_center = (center_x - canvas_center).abs() > size as f32 * 0.08
        || (center_y - canvas_center).abs() > size as f32 * 0.08;

    if content_ratio >= 0.72 && !off_center {
        return NativeIconImage {
            rgba,
            width: size,
            height: size,
        };
    }

    if rgba.len() != size * size * 4 {
        return NativeIconImage {
            rgba,
            width: size,
            height: size,
        };
    }

    let image = image::RgbaImage::from_raw(size as u32, size as u32, rgba)
        .expect("native icon buffer length was validated");

    let crop = image::imageops::crop_imm(
        &image,
        left as u32,
        top as u32,
        content_width as u32,
        content_height as u32,
    )
    .to_image();
    let target_max = (size as f32 * 0.84).round().clamp(1.0, size as f32) as u32;
    let scale = target_max as f32 / content_width.max(content_height) as f32;
    let target_width = ((content_width as f32 * scale).round() as u32).max(1);
    let target_height = ((content_height as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(
        &crop,
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    );

    let mut output = vec![0_u8; size * size * 4];
    let x_offset = (size as u32).saturating_sub(target_width) / 2;
    let y_offset = (size as u32).saturating_sub(target_height) / 2;

    for y in 0..target_height {
        for x in 0..target_width {
            let pixel = resized.get_pixel(x, y).0;
            let dst = (((y + y_offset) as usize * size) + (x + x_offset) as usize) * 4;
            output[dst..dst + 4].copy_from_slice(&pixel);
        }
    }

    NativeIconImage {
        rgba: output,
        width: size,
        height: size,
    }
}

fn visible_icon_bounds(
    rgba: &[u8],
    width: usize,
    height: usize,
) -> Option<(usize, usize, usize, usize)> {
    let mut left = width;
    let mut top = height;
    let mut right = 0;
    let mut bottom = 0;

    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            let Some(pixel) = rgba.get(index..index + 4) else {
                continue;
            };
            // Shell icon providers can leave a very wide, faint alpha halo
            // across the 256 px canvas. It is not part of the icon's visible
            // artwork; using a stronger alpha cutoff lets small legacy icons
            // be cropped and enlarged for large-icon views.
            if pixel[3] <= 96 {
                continue;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }

    (left <= right && top <= bottom).then_some((left, top, right, bottom))
}

#[cfg(target_os = "windows")]
pub struct NativeIconImage {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
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

#[cfg(target_os = "windows")]
pub fn native_file_icon_highres(
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
            if pixel[3] <= 8 {
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

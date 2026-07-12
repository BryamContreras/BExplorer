#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn normalize_long_path(path: &std::path::Path) -> std::path::PathBuf {
    path.to_path_buf()
}

#[cfg(target_os = "windows")]
pub fn apply_small_window_corners(
    handle: &raw_window_handle::WindowHandle<'_>,
) -> crate::utils::errors::Result<()> {
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL, DwmSetWindowAttribute,
    };

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Ok(());
    };

    let preference = DWMWCP_ROUNDSMALL;
    unsafe {
        DwmSetWindowAttribute(
            HWND(handle.hwnd.get() as *mut _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            std::mem::size_of_val(&preference) as u32,
        )?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn install_autoplay_cancel(
    handle: &raw_window_handle::WindowHandle<'_>,
) -> crate::utils::errors::Result<()> {
    use std::sync::atomic::Ordering;

    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWLP_WNDPROC, RegisterWindowMessageW, SetWindowLongPtrW,
    };
    use windows::core::w;

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Ok(());
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let hwnd_value = hwnd.0 as isize;

    if AUTOPLAY_CANCEL_HWND.load(Ordering::Acquire) == hwnd_value {
        return Ok(());
    }

    let message = AUTOPLAY_CANCEL_MESSAGE.load(Ordering::Acquire);
    if message == 0 {
        let registered = unsafe { RegisterWindowMessageW(w!("QueryCancelAutoPlay")) };
        if registered == 0 {
            return Ok(());
        }
        AUTOPLAY_CANCEL_MESSAGE.store(registered, Ordering::Release);
    }

    if AUTOPLAY_CANCEL_HWND.load(Ordering::Acquire) != 0 {
        return Ok(());
    }

    let previous = unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            autoplay_cancel_wndproc as *const () as isize,
        )
    };
    if previous == 0 {
        return Ok(());
    }

    AUTOPLAY_CANCEL_PREV_WNDPROC.store(previous, Ordering::Release);
    AUTOPLAY_CANCEL_HWND.store(hwnd_value, Ordering::Release);
    Ok(())
}

#[cfg(target_os = "windows")]
static AUTOPLAY_CANCEL_MESSAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "windows")]
static AUTOPLAY_CANCEL_HWND: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

#[cfg(target_os = "windows")]
static AUTOPLAY_CANCEL_PREV_WNDPROC: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

#[cfg(target_os = "windows")]
unsafe extern "system" fn autoplay_cancel_wndproc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVNODES_CHANGED,
        DefWindowProcW, WM_DEVICECHANGE, WNDPROC,
    };

    let autoplay_message = AUTOPLAY_CANCEL_MESSAGE.load(Ordering::Acquire);
    if autoplay_message != 0 && msg == autoplay_message {
        return LRESULT(1);
    }
    if msg == WM_DEVICECHANGE
        && matches!(
            wparam.0 as u32,
            DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE | DBT_DEVNODES_CHANGED
        )
    {
        super::storage_watch::notify_storage_change();
    }

    let previous = AUTOPLAY_CANCEL_PREV_WNDPROC.load(Ordering::Acquire);
    if previous != 0 {
        let previous: WNDPROC = unsafe { std::mem::transmute(previous) };
        return unsafe { CallWindowProcW(previous, hwnd, msg, wparam, lparam) };
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

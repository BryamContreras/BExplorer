#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn normalize_long_path(path: &std::path::Path) -> std::path::PathBuf {
    path.to_path_buf()
}

#[cfg(target_os = "windows")]
pub fn apply_small_window_corners(
    handle: &raw_window_handle::WindowHandle<'_>,
    radius: u32,
) -> crate::utils::errors::Result<()> {
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, HRGN, SetWindowRgn};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Ok(());
    };

    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let preference = if radius <= 1 {
        DWMWCP_DONOTROUND
    } else {
        DWMWCP_ROUND
    };
    unsafe {
        // Winit extends the client area over native non-client styles. DWM can
        // still compose that latent frame during Shell transitions, so disable
        // its rendering here. The main-window hook below removes WS_CAPTION
        // while retaining the resize and window-management styles used by Snap.
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &DWMNCRP_DISABLED as *const _ as *const _,
            std::mem::size_of_val(&DWMNCRP_DISABLED) as u32,
        )?;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            std::mem::size_of_val(&preference) as u32,
        )?;
    }

    // DWM's corner preference affects the native frame, but a transparent
    // borderless Iced surface can still paint its client background into the
    // square corner pixels. Apply a real window region as well so the desktop
    // is visible outside the rounded edge on every window we create.
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect)? };
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let region = if radius <= 1 {
        HRGN::default()
    } else {
        let diameter = (radius.saturating_mul(2)).max(2) as i32;
        unsafe { CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter) }
    };
    if region.is_invalid() && radius > 1 {
        return Err(windows::core::Error::from_win32().into());
    }
    if unsafe { SetWindowRgn(hwnd, region, true) } == 0 {
        if !region.is_invalid() {
            let _ = unsafe { DeleteObject(region) };
        }
        return Err(windows::core::Error::from_win32().into());
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn install_main_window_hooks(
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

    if MAIN_WINDOW_HWND.load(Ordering::Acquire) == hwnd_value {
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

    if MAIN_WINDOW_HWND.load(Ordering::Acquire) != 0 {
        return Ok(());
    }

    let previous = unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            main_window_wndproc as *const () as isize,
        )
    };
    if previous == 0 {
        return Ok(());
    }

    MAIN_WINDOW_PREV_WNDPROC.store(previous, Ordering::Release);
    MAIN_WINDOW_HWND.store(hwnd_value, Ordering::Release);
    remove_native_frame_styles(hwnd)
}

#[cfg(target_os = "windows")]
fn remove_native_frame_styles(
    hwnd: windows::Win32::Foundation::HWND,
) -> crate::utils::errors::Result<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos,
    };

    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let style = custom_frame_style(style);
    if unsafe { SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize) } == 0 {
        return Err(windows::core::Error::from_win32().into());
    }

    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let ex_style = custom_frame_ex_style(ex_style);
    if unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize) } == 0 {
        return Err(windows::core::Error::from_win32().into());
    }

    unsafe {
        SetWindowPos(
            hwnd,
            HWND::default(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        )?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
static AUTOPLAY_CANCEL_MESSAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_PREV_WNDPROC: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

#[cfg(target_os = "windows")]
unsafe extern "system" fn main_window_wndproc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVNODES_CHANGED,
        DefWindowProcW, IsIconic, WM_ACTIVATE, WM_DEVICECHANGE, WM_GETMINMAXINFO, WM_NCACTIVATE,
        WM_NCPAINT, WM_STYLECHANGING, WM_SYNCPAINT, WNDPROC,
    };

    let is_minimized = if matches!(msg, WM_ACTIVATE | WM_NCACTIVATE | WM_NCPAINT | WM_SYNCPAINT) {
        unsafe { IsIconic(hwnd).as_bool() }
    } else {
        false
    };

    // DWM non-client rendering is disabled for this custom-framed window, but
    // taskbar activation can still request a classic GDI frame paint. Iced
    // owns every visible pixel, so acknowledge that paint without forwarding
    // it to DefWindowProc, which would draw the native caption over our client.
    if should_suppress_native_frame_paint(msg, is_minimized) {
        return LRESULT(0);
    }

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

    let previous = MAIN_WINDOW_PREV_WNDPROC.load(Ordering::Acquire);
    if previous != 0 {
        let previous: WNDPROC = unsafe { std::mem::transmute(previous) };
        let lparam = if msg == WM_NCACTIVATE {
            non_client_activation_lparam(is_minimized, lparam)
        } else {
            lparam
        };
        let result = unsafe { CallWindowProcW(previous, hwnd, msg, wparam, lparam) };
        if msg == WM_STYLECHANGING {
            enforce_pending_custom_frame_style(wparam, lparam);
        } else if msg == WM_GETMINMAXINFO {
            constrain_maximized_window_to_work_area(hwnd, lparam);
        }
        if should_request_client_redraw(msg, wparam.0, is_minimized) {
            use windows::Win32::Graphics::Gdi::{HRGN, RDW_INTERNALPAINT, RedrawWindow};

            // Let the original procedure finish activation/synchronized paint,
            // then queue the client redraw so Iced presents a fresh frame
            // immediately instead of waiting for the next pointer event.
            let _ = unsafe { RedrawWindow(hwnd, None, HRGN::default(), RDW_INTERNALPAINT) };
        }
        return result;
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(target_os = "windows")]
fn enforce_pending_custom_frame_style(
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) {
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GWL_STYLE, STYLESTRUCT};

    if lparam.0 == 0 {
        return;
    }
    let styles = unsafe { &mut *(lparam.0 as *mut STYLESTRUCT) };
    match wparam.0 as i32 {
        index if index == GWL_STYLE.0 => styles.styleNew = custom_frame_style(styles.styleNew),
        index if index == GWL_EXSTYLE.0 => styles.styleNew = custom_frame_ex_style(styles.styleNew),
        _ => {}
    }
}

#[cfg(target_os = "windows")]
fn constrain_maximized_window_to_work_area(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::MINMAXINFO;

    if lparam.0 == 0 {
        return;
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return;
    }

    let (position, size) = work_area_maximize_metrics(info.rcMonitor, info.rcWork);
    let minmax = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
    minmax.ptMaxPosition.x = position[0];
    minmax.ptMaxPosition.y = position[1];
    minmax.ptMaxSize.x = size[0];
    minmax.ptMaxSize.y = size[1];
}

#[cfg(target_os = "windows")]
fn work_area_maximize_metrics(
    monitor: windows::Win32::Foundation::RECT,
    work: windows::Win32::Foundation::RECT,
) -> ([i32; 2], [i32; 2]) {
    (
        [work.left - monitor.left, work.top - monitor.top],
        [work.right - work.left, work.bottom - work.top],
    )
}

#[cfg(target_os = "windows")]
fn custom_frame_style(style: u32) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::WS_CAPTION;

    style & !WS_CAPTION.0
}

#[cfg(target_os = "windows")]
fn custom_frame_ex_style(style: u32) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::WS_EX_WINDOWEDGE;

    style & !WS_EX_WINDOWEDGE.0
}

#[cfg(target_os = "windows")]
fn non_client_activation_lparam(
    is_minimized: bool,
    original: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LPARAM {
    if is_minimized {
        original
    } else {
        // Winit must still receive WM_NCACTIVATE so its focus state remains
        // correct. LPARAM(-1) asks DefWindowProc to complete the activation
        // without repainting the latent native frame beneath our custom one.
        windows::Win32::Foundation::LPARAM(-1)
    }
}

#[cfg(target_os = "windows")]
fn should_suppress_native_frame_paint(message: u32, is_minimized: bool) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::WM_NCPAINT;

    message == WM_NCPAINT && !is_minimized
}

#[cfg(target_os = "windows")]
fn should_request_client_redraw(message: u32, wparam: usize, is_minimized: bool) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{WA_INACTIVE, WM_ACTIVATE, WM_SYNCPAINT};

    !is_minimized
        && (message == WM_SYNCPAINT
            || (message == WM_ACTIVATE && (wparam & 0xffff) != WA_INACTIVE as usize))
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_NCPAINT, WM_PAINT, WM_SYNCPAINT, WS_CAPTION, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    use super::{
        custom_frame_ex_style, custom_frame_style, non_client_activation_lparam,
        should_request_client_redraw, should_suppress_native_frame_paint,
        work_area_maximize_metrics,
    };

    #[test]
    fn custom_frame_removes_caption_but_keeps_window_management_styles() {
        let original = WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
        let style = custom_frame_style(original.0);

        assert_eq!(style & WS_CAPTION.0, 0);
        for preserved in [WS_THICKFRAME, WS_SYSMENU, WS_MINIMIZEBOX, WS_MAXIMIZEBOX] {
            assert_ne!(style & preserved.0, 0);
        }
    }

    #[test]
    fn custom_frame_removes_only_the_extended_window_edge() {
        use windows::Win32::UI::WindowsAndMessaging::{WS_EX_ACCEPTFILES, WS_EX_APPWINDOW};

        let original = WS_EX_WINDOWEDGE.0 | WS_EX_ACCEPTFILES.0 | WS_EX_APPWINDOW.0;
        let style = custom_frame_ex_style(original);

        assert_eq!(style & WS_EX_WINDOWEDGE.0, 0);
        assert_ne!(style & WS_EX_ACCEPTFILES.0, 0);
        assert_ne!(style & WS_EX_APPWINDOW.0, 0);
    }

    #[test]
    fn maximized_custom_frame_uses_monitor_work_area() {
        use windows::Win32::Foundation::RECT;

        let monitor = RECT {
            left: 1920,
            top: 0,
            right: 3360,
            bottom: 1080,
        };
        let work = RECT {
            left: 1968,
            top: 0,
            right: 3360,
            bottom: 1040,
        };

        assert_eq!(
            work_area_maximize_metrics(monitor, work),
            ([48, 0], [1392, 1040])
        );
    }

    #[test]
    fn active_custom_window_suppresses_native_frame_repaint() {
        assert_eq!(non_client_activation_lparam(false, LPARAM(42)).0, -1);
    }

    #[test]
    fn minimized_window_keeps_original_activation_context() {
        assert_eq!(non_client_activation_lparam(true, LPARAM(42)).0, 42);
    }

    #[test]
    fn visible_custom_window_suppresses_native_frame_paint() {
        assert!(should_suppress_native_frame_paint(WM_NCPAINT, false));
        assert!(!should_suppress_native_frame_paint(WM_PAINT, false));
    }

    #[test]
    fn minimized_window_keeps_native_frame_paint() {
        assert!(!should_suppress_native_frame_paint(WM_NCPAINT, true));
    }

    #[test]
    fn synchronized_paint_requests_an_immediate_client_redraw() {
        assert!(should_request_client_redraw(WM_SYNCPAINT, 0, false));
        assert!(!should_request_client_redraw(WM_PAINT, 0, false));
        assert!(!should_request_client_redraw(WM_SYNCPAINT, 0, true));
    }

    #[test]
    fn activation_requests_a_redraw_only_when_becoming_visible() {
        use windows::Win32::UI::WindowsAndMessaging::{WA_ACTIVE, WA_INACTIVE, WM_ACTIVATE};

        assert!(should_request_client_redraw(
            WM_ACTIVATE,
            WA_ACTIVE as usize,
            false
        ));
        assert!(!should_request_client_redraw(
            WM_ACTIVATE,
            WA_INACTIVE as usize,
            false
        ));
        assert!(!should_request_client_redraw(
            WM_ACTIVATE,
            WA_ACTIVE as usize,
            true
        ));
    }
}

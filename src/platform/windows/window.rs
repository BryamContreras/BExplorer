#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn normalize_long_path(path: &std::path::Path) -> std::path::PathBuf {
    path.to_path_buf()
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MainWindowAppearanceEvent {
    pub generation: u64,
    pub revision: u64,
    pub maximized: bool,
    pub refresh_backdrop: bool,
}

/// Returns the coalesced wake-up receiver for native main-window appearance
/// transitions. After every wake-up, call
/// [`take_main_window_appearance_event`] to read the newest accumulated state.
#[cfg(target_os = "windows")]
pub fn main_window_appearance_receiver() -> std::sync::mpsc::Receiver<()> {
    use std::sync::atomic::Ordering;
    use std::sync::{Mutex, mpsc};

    let (sender, receiver) = mpsc::sync_channel(1);
    if let Ok(mut current) = MAIN_WINDOW_APPEARANCE_SENDER
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *current = Some(sender);
        // A native settle may have been published before Iced finished
        // creating its subscription (or while the subscription was being
        // recreated). Wake the new receiver so it can recover the latest
        // coalesced snapshot instead of waiting for another window change.
        if MAIN_WINDOW_APPEARANCE_GENERATION.load(Ordering::Acquire) != 0
            && let Some(sender) = current.as_ref()
        {
            let _ = sender.try_send(());
        }
    }
    receiver
}

/// Reads the latest native appearance state and consumes the cumulative
/// backdrop-refresh request.
#[cfg(target_os = "windows")]
pub fn take_main_window_appearance_event() -> MainWindowAppearanceEvent {
    use std::sync::atomic::Ordering;

    loop {
        let generation_before = MAIN_WINDOW_APPEARANCE_GENERATION.load(Ordering::Acquire);
        let revision = MAIN_WINDOW_APPEARANCE_SETTLED_REVISION.load(Ordering::Acquire);
        let maximized = MAIN_WINDOW_APPEARANCE_SETTLED_MAXIMIZED.load(Ordering::Acquire);
        let refresh_backdrop = MAIN_WINDOW_APPEARANCE_REFRESH.swap(false, Ordering::AcqRel);
        let generation_after = MAIN_WINDOW_APPEARANCE_GENERATION.load(Ordering::Acquire);

        if generation_before == generation_after {
            return MainWindowAppearanceEvent {
                generation: generation_after,
                revision,
                maximized,
                refresh_backdrop,
            };
        }

        // A settle raced this snapshot. Return its cumulative refresh bit to
        // the shared state and retry against the newest generation.
        if refresh_backdrop {
            MAIN_WINDOW_APPEARANCE_REFRESH.store(true, Ordering::Release);
        }
    }
}

/// Returns the last native settle generation without consuming its cumulative
/// backdrop-refresh request. Iced uses this as a lifecycle baseline when the
/// main window is recreated.
#[cfg(target_os = "windows")]
pub fn main_window_appearance_generation() -> u64 {
    use std::sync::atomic::Ordering;

    MAIN_WINDOW_APPEARANCE_GENERATION.load(Ordering::Acquire)
}

/// Returns the current native transition revision. A revision changes before
/// Windows starts resizing the surface, so asynchronous work captured from an
/// older settle can safely decline to mutate the new transition.
#[cfg(target_os = "windows")]
pub fn main_window_appearance_revision() -> u64 {
    use std::sync::atomic::Ordering;

    MAIN_WINDOW_APPEARANCE_REVISION.load(Ordering::Acquire)
}

/// Whether a delayed main-window region update still belongs to the latest
/// settled native transition.
#[cfg(target_os = "windows")]
pub fn main_window_region_update_is_current(revision: u64) -> bool {
    use std::sync::atomic::Ordering;

    revision != 0
        && MAIN_WINDOW_APPEARANCE_REVISION.load(Ordering::Acquire) == revision
        && !MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire)
}

/// Whether Acrylic can be reasserted for the supplied native revision.
///
/// The final HRGN must already be restored; otherwise DWM and GDI would rebuild
/// two different visual bounds concurrently.
#[cfg(target_os = "windows")]
pub fn main_window_backdrop_update_is_current(revision: u64) -> bool {
    use std::sync::atomic::Ordering;

    main_window_region_update_is_current(revision)
        && !MAIN_WINDOW_REGION_SUSPENDED.load(Ordering::Acquire)
}

#[cfg(target_os = "windows")]
pub fn apply_small_window_corners(
    handle: &raw_window_handle::WindowHandle<'_>,
    radius: u32,
) -> crate::utils::errors::Result<()> {
    use std::sync::atomic::Ordering;

    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
    };

    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return Ok(());
    };

    let hwnd = HWND(win32.hwnd.get() as *mut _);
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
    if MAIN_WINDOW_HWND.load(Ordering::Acquire) == hwnd.0 as isize {
        apply_main_window_region(handle, radius)
    } else {
        set_window_region(hwnd, radius, false)
    }
}

/// Applies only the cached HRGN for BExplorer's main window.
///
/// Unlike [`apply_small_window_corners`], this function deliberately leaves
/// every DWM attribute untouched. It is used after a native resize/Snap
/// transaction has settled so the compositor backdrop and the client region
/// are not rebuilt in parallel.
#[cfg(target_os = "windows")]
pub fn apply_main_window_region(
    handle: &raw_window_handle::WindowHandle<'_>,
    radius: u32,
) -> crate::utils::errors::Result<()> {
    use std::sync::atomic::Ordering;

    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Ok(());
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let hwnd_value = hwnd.0 as isize;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect)? };
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let radius = radius.max(1);

    if MAIN_WINDOW_REGION_CACHE_HWND.load(Ordering::Acquire) == hwnd_value
        && MAIN_WINDOW_REGION_CACHE_WIDTH.load(Ordering::Acquire) == width
        && MAIN_WINDOW_REGION_CACHE_HEIGHT.load(Ordering::Acquire) == height
        && MAIN_WINDOW_REGION_CACHE_RADIUS.load(Ordering::Acquire) == radius
        && !MAIN_WINDOW_REGION_SUSPENDED.load(Ordering::Acquire)
    {
        return Ok(());
    }

    // Publish the intended cache entry before SetWindowRgn. The call sends
    // WINDOWPOS messages synchronously, which can re-enter this WndProc and
    // ultimately produce WM_SIZE. The mutation guard makes those messages
    // observational only, while the early cache update prevents recursion.
    MAIN_WINDOW_REGION_CACHE_HWND.store(hwnd_value, Ordering::Release);
    MAIN_WINDOW_REGION_CACHE_WIDTH.store(width, Ordering::Release);
    MAIN_WINDOW_REGION_CACHE_HEIGHT.store(height, Ordering::Release);
    MAIN_WINDOW_REGION_CACHE_RADIUS.store(radius, Ordering::Release);
    MAIN_WINDOW_REGION_MUTATING.store(true, Ordering::Release);
    let result = set_window_region(hwnd, radius, false);
    MAIN_WINDOW_REGION_MUTATING.store(false, Ordering::Release);

    match result {
        Ok(()) => {
            MAIN_WINDOW_REGION_SUSPENDED.store(false, Ordering::Release);
            Ok(())
        }
        Err(error) => {
            invalidate_main_window_region_cache();
            Err(error)
        }
    }
}

#[cfg(target_os = "windows")]
fn set_window_region(
    hwnd: windows::Win32::Foundation::HWND,
    radius: u32,
    redraw: bool,
) -> crate::utils::errors::Result<()> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, HRGN, SetWindowRgn};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

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
    if unsafe { SetWindowRgn(hwnd, region, redraw) } == 0 {
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
        GWLP_WNDPROC, GetForegroundWindow, RegisterWindowMessageW, SetWindowLongPtrW,
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
    let appearance_message = DEFERRED_APPEARANCE_SETTLE_MESSAGE.load(Ordering::Acquire);
    if appearance_message == 0 {
        let registered = unsafe { RegisterWindowMessageW(w!("BExplorer.WindowAppearanceSettled")) };
        if registered != 0 {
            DEFERRED_APPEARANCE_SETTLE_MESSAGE.store(registered, Ordering::Release);
        }
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
    MAIN_WINDOW_ACTIVE.store(unsafe { GetForegroundWindow() == hwnd }, Ordering::Release);
    reset_main_window_transition_state(hwnd);
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
static DEFERRED_CLIENT_REDRAW_TIMER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
const DEFERRED_CLIENT_REDRAW_DELAY_MS: u32 = 20;

#[cfg(target_os = "windows")]
static DEFERRED_APPEARANCE_SETTLE_MESSAGE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "windows")]
static DEFERRED_APPEARANCE_SETTLE_PENDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static DEFERRED_APPEARANCE_REFRESH_PENDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_APPEARANCE_SENDER: std::sync::OnceLock<
    std::sync::Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
> = std::sync::OnceLock::new();

#[cfg(target_os = "windows")]
static MAIN_WINDOW_APPEARANCE_GENERATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_APPEARANCE_REVISION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_APPEARANCE_SETTLED_REVISION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_APPEARANCE_SETTLED_MAXIMIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_APPEARANCE_REFRESH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_NATIVE_MAXIMIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_IN_SIZE_MOVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_SIZE_MOVE_SAW_SIZING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_SIZE_MOVE_DPI_CHANGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_SIZE_MOVE_STARTED_MAXIMIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_SIZE_MOVE_START_WIDTH: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_SIZE_MOVE_START_HEIGHT: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_REGION_MUTATING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_REGION_SUSPENDED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_REGION_CACHE_HWND: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_REGION_CACHE_WIDTH: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_REGION_CACHE_HEIGHT: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_REGION_CACHE_RADIUS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_PREV_WNDPROC: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

#[cfg(target_os = "windows")]
fn advance_main_window_appearance_revision() -> u64 {
    use std::sync::atomic::Ordering;

    let mut current = MAIN_WINDOW_APPEARANCE_REVISION.load(Ordering::Acquire);
    loop {
        let next = current.wrapping_add(1).max(1);
        match MAIN_WINDOW_APPEARANCE_REVISION.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(target_os = "windows")]
fn reset_main_window_transition_state(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::IsZoomed;

    advance_main_window_appearance_revision();
    MAIN_WINDOW_NATIVE_MAXIMIZED.store(unsafe { IsZoomed(hwnd).as_bool() }, Ordering::Release);
    MAIN_WINDOW_IN_SIZE_MOVE.store(false, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_SAW_SIZING.store(false, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_DPI_CHANGED.store(false, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_STARTED_MAXIMIZED.store(false, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_START_WIDTH.store(0, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_START_HEIGHT.store(0, Ordering::Release);
    DEFERRED_APPEARANCE_SETTLE_PENDING.store(false, Ordering::Release);
    DEFERRED_APPEARANCE_REFRESH_PENDING.store(false, Ordering::Release);
    MAIN_WINDOW_APPEARANCE_REFRESH.store(false, Ordering::Release);
    MAIN_WINDOW_REGION_MUTATING.store(false, Ordering::Release);
    MAIN_WINDOW_REGION_SUSPENDED.store(false, Ordering::Release);
    invalidate_main_window_region_cache();
}

#[cfg(target_os = "windows")]
fn invalidate_main_window_region_cache() {
    use std::sync::atomic::Ordering;

    MAIN_WINDOW_REGION_CACHE_HWND.store(0, Ordering::Release);
    MAIN_WINDOW_REGION_CACHE_WIDTH.store(0, Ordering::Release);
    MAIN_WINDOW_REGION_CACHE_HEIGHT.store(0, Ordering::Release);
    MAIN_WINDOW_REGION_CACHE_RADIUS.store(0, Ordering::Release);
}

#[cfg(target_os = "windows")]
fn main_window_outer_size(hwnd: windows::Win32::Foundation::HWND) -> Option<(i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok().map(|_| {
        (
            (rect.right - rect.left).max(1),
            (rect.bottom - rect.top).max(1),
        )
    })
}

#[cfg(target_os = "windows")]
fn size_move_needs_backdrop_refresh(
    started_maximized: bool,
    maximized: bool,
    saw_sizing: bool,
    size_changed: bool,
    dpi_changed: bool,
) -> bool {
    dpi_changed || maximized != started_maximized || (!saw_sizing && size_changed)
}

#[cfg(target_os = "windows")]
fn begin_main_window_size_move(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::IsZoomed;

    advance_main_window_appearance_revision();
    let maximized = unsafe { IsZoomed(hwnd).as_bool() };
    let (width, height) = main_window_outer_size(hwnd).unwrap_or_default();
    MAIN_WINDOW_NATIVE_MAXIMIZED.store(maximized, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_STARTED_MAXIMIZED.store(maximized, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_START_WIDTH.store(width, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_START_HEIGHT.store(height, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_SAW_SIZING.store(false, Ordering::Release);
    MAIN_WINDOW_SIZE_MOVE_DPI_CHANGED.store(false, Ordering::Release);
    MAIN_WINDOW_IN_SIZE_MOVE.store(true, Ordering::Release);
    suspend_main_window_region(hwnd);
}

#[cfg(target_os = "windows")]
fn prepare_main_window_position_change(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOSIZE, WINDOWPOS};

    if lparam.0 == 0
        || MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire)
        || MAIN_WINDOW_REGION_MUTATING.load(Ordering::Acquire)
    {
        return;
    }

    let position = unsafe { &*(lparam.0 as *const WINDOWPOS) };
    if position.flags.contains(SWP_NOSIZE) || position.cx <= 0 || position.cy <= 0 {
        return;
    }
    let Some((current_width, current_height)) = main_window_outer_size(hwnd) else {
        return;
    };
    if position.cx == current_width && position.cy == current_height {
        return;
    }

    // ShowWindow/window::maximize and system shortcuts do not enter the modal
    // size/move loop. Remove the old-size HRGN before Windows grows the native
    // surface so it cannot clip the new renderer frame.
    advance_main_window_appearance_revision();
    suspend_main_window_region(hwnd);
}

#[cfg(target_os = "windows")]
fn suspend_main_window_region(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::Graphics::Gdi::{HRGN, SetWindowRgn};

    if MAIN_WINDOW_REGION_SUSPENDED.swap(true, Ordering::AcqRel) {
        return;
    }

    // Invalidate before entering SetWindowRgn because it synchronously emits
    // WINDOWPOS messages. A later settle must always rebuild the final region.
    invalidate_main_window_region_cache();
    MAIN_WINDOW_REGION_MUTATING.store(true, Ordering::Release);
    let changed = unsafe { SetWindowRgn(hwnd, HRGN::default(), false) } != 0;
    MAIN_WINDOW_REGION_MUTATING.store(false, Ordering::Release);
    if !changed {
        MAIN_WINDOW_REGION_SUSPENDED.store(false, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn finish_main_window_size_move(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::IsZoomed;

    MAIN_WINDOW_IN_SIZE_MOVE.store(false, Ordering::Release);
    let maximized = unsafe { IsZoomed(hwnd).as_bool() };
    let started_maximized = MAIN_WINDOW_SIZE_MOVE_STARTED_MAXIMIZED.load(Ordering::Acquire);
    let saw_sizing = MAIN_WINDOW_SIZE_MOVE_SAW_SIZING.load(Ordering::Acquire);
    let dpi_changed = MAIN_WINDOW_SIZE_MOVE_DPI_CHANGED.load(Ordering::Acquire);
    let start_width = MAIN_WINDOW_SIZE_MOVE_START_WIDTH.load(Ordering::Acquire);
    let start_height = MAIN_WINDOW_SIZE_MOVE_START_HEIGHT.load(Ordering::Acquire);
    let size_changed = main_window_outer_size(hwnd)
        .is_some_and(|(width, height)| width != start_width || height != start_height);

    // A border resize only needs its final HRGN. A title-bar transaction whose
    // size changed is a Snap operation; maximize/restore and DPI transitions
    // also rebuild DWM's visual and therefore request a backdrop refresh.
    let refresh_backdrop = size_move_needs_backdrop_refresh(
        started_maximized,
        maximized,
        saw_sizing,
        size_changed,
        dpi_changed,
    );
    MAIN_WINDOW_NATIVE_MAXIMIZED.store(maximized, Ordering::Release);
    defer_main_window_appearance_settle(hwnd, refresh_backdrop);
}

#[cfg(target_os = "windows")]
fn observe_main_window_size(hwnd: windows::Win32::Foundation::HWND, size_kind: u32) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};

    if MAIN_WINDOW_REGION_MUTATING.load(Ordering::Acquire) || size_kind == SIZE_MINIMIZED {
        return;
    }

    if !MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire)
        && !MAIN_WINDOW_REGION_SUSPENDED.load(Ordering::Acquire)
    {
        // Normally WM_WINDOWPOSCHANGING already opened the revision and
        // suspended the stale HRGN. Keep WM_SIZE as a fallback for unusual
        // shell transitions that bypass that notification.
        advance_main_window_appearance_revision();
        suspend_main_window_region(hwnd);
    }

    let refresh_backdrop = match size_kind {
        SIZE_MAXIMIZED => {
            MAIN_WINDOW_NATIVE_MAXIMIZED.store(true, Ordering::Release);
            // Reassert even if the cached state was already maximized. DWM can
            // rebuild the visual during repeated top-edge Snap transactions.
            true
        }
        SIZE_RESTORED => MAIN_WINDOW_NATIVE_MAXIMIZED.swap(false, Ordering::AcqRel),
        _ => return,
    };

    defer_main_window_appearance_settle(hwnd, refresh_backdrop);
}

#[cfg(target_os = "windows")]
fn defer_main_window_appearance_settle(
    hwnd: windows::Win32::Foundation::HWND,
    refresh_backdrop: bool,
) {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    if refresh_backdrop {
        DEFERRED_APPEARANCE_REFRESH_PENDING.store(true, Ordering::Release);
    }
    if MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire) {
        return;
    }

    let message = DEFERRED_APPEARANCE_SETTLE_MESSAGE.load(Ordering::Acquire);
    if message == 0 || DEFERRED_APPEARANCE_SETTLE_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    if unsafe { PostMessageW(hwnd, message, WPARAM(0), LPARAM(0)) }.is_err() {
        DEFERRED_APPEARANCE_SETTLE_PENDING.store(false, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn publish_main_window_appearance_settle(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};

    DEFERRED_APPEARANCE_SETTLE_PENDING.store(false, Ordering::Release);
    if MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire) || unsafe { IsIconic(hwnd).as_bool() } {
        return;
    }

    let maximized = unsafe { IsZoomed(hwnd).as_bool() };
    let refresh_backdrop = DEFERRED_APPEARANCE_REFRESH_PENDING.swap(false, Ordering::AcqRel);
    MAIN_WINDOW_NATIVE_MAXIMIZED.store(maximized, Ordering::Release);
    MAIN_WINDOW_APPEARANCE_SETTLED_MAXIMIZED.store(maximized, Ordering::Release);
    if refresh_backdrop {
        MAIN_WINDOW_APPEARANCE_REFRESH.store(true, Ordering::Release);
    }
    MAIN_WINDOW_APPEARANCE_SETTLED_REVISION.store(
        MAIN_WINDOW_APPEARANCE_REVISION.load(Ordering::Acquire),
        Ordering::Release,
    );
    MAIN_WINDOW_APPEARANCE_GENERATION.fetch_add(1, Ordering::Release);
    notify_main_window_appearance();
}

#[cfg(target_os = "windows")]
fn notify_main_window_appearance() {
    let Some(sender) = MAIN_WINDOW_APPEARANCE_SENDER.get() else {
        return;
    };
    let Ok(sender) = sender.lock() else {
        return;
    };
    if let Some(sender) = sender.as_ref() {
        let _ = sender.try_send(());
    }
}

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
        DefWindowProcW, IsIconic, WM_ACTIVATE, WM_DEVICECHANGE, WM_DPICHANGED, WM_ENTERSIZEMOVE,
        WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_NCACTIVATE, WM_NCDESTROY, WM_NCPAINT, WM_SIZE,
        WM_SIZING, WM_STYLECHANGING, WM_SYNCPAINT, WM_TIMER, WM_WINDOWPOSCHANGED,
        WM_WINDOWPOSCHANGING, WNDPROC,
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
    let redraw_timer = DEFERRED_CLIENT_REDRAW_TIMER.load(Ordering::Acquire);
    if redraw_timer != 0 && msg == WM_TIMER && wparam.0 == redraw_timer {
        use windows::Win32::Graphics::Gdi::{HRGN, RDW_INTERNALPAINT, RedrawWindow};

        cancel_client_redraw(hwnd);
        if !MAIN_WINDOW_ACTIVE.load(Ordering::Acquire) || unsafe { IsIconic(hwnd).as_bool() } {
            return LRESULT(0);
        }
        // Activation can emit both WM_ACTIVATE and WM_SYNCPAINT. The one-shot
        // timer is rearmed by each signal, so Iced presents one frame only
        // after DWM has had a compositor frame to leave Acrylic's inactive
        // fallback. This preserves the stale-frame fix without amplifying the
        // native material transition into two visible flashes.
        let _ = unsafe { RedrawWindow(hwnd, None, HRGN::default(), RDW_INTERNALPAINT) };
        return LRESULT(0);
    }
    let appearance_message = DEFERRED_APPEARANCE_SETTLE_MESSAGE.load(Ordering::Acquire);
    if appearance_message != 0 && msg == appearance_message {
        publish_main_window_appearance_settle(hwnd);
        return LRESULT(0);
    }
    if msg == WM_ACTIVATE {
        // During a taskbar restore WM_ACTIVATE can arrive while IsIconic still
        // reports the previous minimized state. Preserve the intended active
        // state so a later WM_SYNCPAINT can request the first visible frame.
        let active =
            (wparam.0 & 0xffff) != windows::Win32::UI::WindowsAndMessaging::WA_INACTIVE as usize;
        MAIN_WINDOW_ACTIVE.store(active, Ordering::Release);
        if !active {
            cancel_client_redraw(hwnd);
        }
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
        if msg == WM_ENTERSIZEMOVE {
            begin_main_window_size_move(hwnd);
        } else if msg == WM_SIZING {
            MAIN_WINDOW_SIZE_MOVE_SAW_SIZING.store(true, Ordering::Release);
        } else if msg == WM_WINDOWPOSCHANGING {
            prepare_main_window_position_change(hwnd, lparam);
        }
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
        if msg == WM_SIZE {
            observe_main_window_size(hwnd, wparam.0 as u32);
        } else if msg == WM_DPICHANGED {
            if MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire) {
                MAIN_WINDOW_SIZE_MOVE_DPI_CHANGED.store(true, Ordering::Release);
            } else {
                advance_main_window_appearance_revision();
                suspend_main_window_region(hwnd);
            }
            defer_main_window_appearance_settle(hwnd, true);
        } else if msg == WM_EXITSIZEMOVE {
            finish_main_window_size_move(hwnd);
        } else if msg == WM_WINDOWPOSCHANGED
            && !MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire)
            && !MAIN_WINDOW_REGION_MUTATING.load(Ordering::Acquire)
            && MAIN_WINDOW_REGION_SUSPENDED.load(Ordering::Acquire)
        {
            // WM_SIZE normally requested this already. Keep WINDOWPOSCHANGED
            // as the non-modal transaction boundary as well so a shell path
            // that omits WM_SIZE cannot leave the HRGN suspended.
            defer_main_window_appearance_settle(hwnd, false);
        }
        if should_defer_client_redraw(
            msg,
            wparam.0,
            is_minimized,
            MAIN_WINDOW_ACTIVE.load(Ordering::Acquire),
        ) {
            defer_client_redraw(hwnd);
        }
        if msg == WM_NCDESTROY
            && MAIN_WINDOW_HWND
                .compare_exchange(hwnd.0 as isize, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            // The main window can be closed while background operations keep
            // the process alive, then recreated later. Release every piece of
            // subclass state so the new HWND installs its own hook.
            MAIN_WINDOW_PREV_WNDPROC.store(0, Ordering::Release);
            MAIN_WINDOW_ACTIVE.store(false, Ordering::Release);
            cancel_client_redraw(hwnd);
            DEFERRED_APPEARANCE_SETTLE_PENDING.store(false, Ordering::Release);
            DEFERRED_APPEARANCE_REFRESH_PENDING.store(false, Ordering::Release);
            MAIN_WINDOW_APPEARANCE_REFRESH.store(false, Ordering::Release);
            MAIN_WINDOW_IN_SIZE_MOVE.store(false, Ordering::Release);
            MAIN_WINDOW_SIZE_MOVE_SAW_SIZING.store(false, Ordering::Release);
            MAIN_WINDOW_SIZE_MOVE_DPI_CHANGED.store(false, Ordering::Release);
            MAIN_WINDOW_REGION_MUTATING.store(false, Ordering::Release);
            MAIN_WINDOW_REGION_SUSPENDED.store(false, Ordering::Release);
            advance_main_window_appearance_revision();
            invalidate_main_window_region_cache();
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
fn defer_client_redraw(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::SetTimer;

    let current = DEFERRED_CLIENT_REDRAW_TIMER.load(Ordering::Acquire);
    let timer = unsafe { SetTimer(hwnd, current, DEFERRED_CLIENT_REDRAW_DELAY_MS, None) };
    if timer != 0 {
        DEFERRED_CLIENT_REDRAW_TIMER.store(timer, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn cancel_client_redraw(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::KillTimer;

    let timer = DEFERRED_CLIENT_REDRAW_TIMER.swap(0, Ordering::AcqRel);
    if timer != 0 {
        let _ = unsafe { KillTimer(hwnd, timer) };
    }
}

#[cfg(target_os = "windows")]
fn should_defer_client_redraw(
    message: u32,
    wparam: usize,
    is_minimized: bool,
    is_active: bool,
) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{WA_INACTIVE, WM_ACTIVATE, WM_SYNCPAINT};

    !is_minimized
        && ((message == WM_SYNCPAINT && is_active)
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
        should_defer_client_redraw, should_suppress_native_frame_paint,
        size_move_needs_backdrop_refresh, work_area_maximize_metrics,
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
    fn synchronized_paint_requests_a_deferred_client_redraw() {
        assert!(should_defer_client_redraw(WM_SYNCPAINT, 0, false, true));
        assert!(!should_defer_client_redraw(WM_PAINT, 0, false, true));
        assert!(!should_defer_client_redraw(WM_SYNCPAINT, 0, true, true));
        assert!(!should_defer_client_redraw(WM_SYNCPAINT, 0, false, false));
    }

    #[test]
    fn activation_requests_a_redraw_only_when_becoming_visible() {
        use windows::Win32::UI::WindowsAndMessaging::{WA_ACTIVE, WA_INACTIVE, WM_ACTIVATE};

        assert!(should_defer_client_redraw(
            WM_ACTIVATE,
            WA_ACTIVE as usize,
            false,
            true
        ));
        assert!(!should_defer_client_redraw(
            WM_ACTIVATE,
            WA_INACTIVE as usize,
            false,
            false
        ));
        assert!(!should_defer_client_redraw(
            WM_ACTIVATE,
            WA_ACTIVE as usize,
            true,
            true
        ));
    }

    #[test]
    fn manual_border_resize_only_needs_the_final_region() {
        assert!(!size_move_needs_backdrop_refresh(
            false, false, true, true, false
        ));
    }

    #[test]
    fn snap_maximize_and_restore_refresh_the_backdrop() {
        assert!(size_move_needs_backdrop_refresh(
            false, true, false, true, false
        ));
        assert!(size_move_needs_backdrop_refresh(
            true, false, false, true, false
        ));
    }

    #[test]
    fn title_bar_snap_and_dpi_changes_refresh_the_backdrop() {
        assert!(size_move_needs_backdrop_refresh(
            false, false, false, true, false
        ));
        assert!(size_move_needs_backdrop_refresh(
            false, false, true, false, true
        ));
    }
}

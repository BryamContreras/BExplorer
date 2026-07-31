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

/// Reads the latest coalesced native appearance state.
#[cfg(target_os = "windows")]
pub fn take_main_window_appearance_event() -> MainWindowAppearanceEvent {
    use std::sync::atomic::Ordering;

    loop {
        let generation_before = MAIN_WINDOW_APPEARANCE_GENERATION.load(Ordering::Acquire);
        let revision = MAIN_WINDOW_APPEARANCE_SETTLED_REVISION.load(Ordering::Acquire);
        let maximized = MAIN_WINDOW_APPEARANCE_SETTLED_MAXIMIZED.load(Ordering::Acquire);
        let generation_after = MAIN_WINDOW_APPEARANCE_GENERATION.load(Ordering::Acquire);

        if generation_before == generation_after {
            return MainWindowAppearanceEvent {
                generation: generation_after,
                revision,
                maximized,
            };
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

/// Whether the native window has completed its latest visible geometry burst.
/// An unavailable hook also returns `true` so the Iced watchdog can fall back
/// to its own authoritative window query instead of polling forever.
#[cfg(target_os = "windows")]
pub fn main_window_appearance_is_settled() -> bool {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;

    let hwnd = HWND(MAIN_WINDOW_HWND.load(Ordering::Acquire) as *mut _);
    hwnd.is_invalid()
        || (MAIN_WINDOW_APPEARANCE_REVISION.load(Ordering::Acquire)
            == MAIN_WINDOW_APPEARANCE_SETTLED_REVISION.load(Ordering::Acquire)
            && !MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire)
            && !main_window_settle_pending()
            && !unsafe { IsIconic(hwnd).as_bool() })
}

/// Whether a delayed main-window region update still belongs to the latest
/// settled native transition.
#[cfg(target_os = "windows")]
pub fn main_window_region_update_is_current(revision: u64) -> bool {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;

    let hwnd = HWND(MAIN_WINDOW_HWND.load(Ordering::Acquire) as *mut _);
    let minimized = !hwnd.is_invalid() && unsafe { IsIconic(hwnd).as_bool() };
    main_window_update_is_current(
        revision,
        MAIN_WINDOW_APPEARANCE_REVISION.load(Ordering::Acquire),
        MAIN_WINDOW_APPEARANCE_SETTLED_REVISION.load(Ordering::Acquire),
        MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire),
        main_window_settle_pending(),
        hwnd.is_invalid() || minimized,
    )
}

#[cfg(target_os = "windows")]
fn main_window_settle_pending() -> bool {
    use std::sync::atomic::Ordering;

    DEFERRED_APPEARANCE_SETTLE_TIMER.load(Ordering::Acquire) != 0
        || APPEARANCE_SETTLE_FALLBACK_TOKEN.load(Ordering::Acquire) != 0
}

/// Releases the native redraw barrier after the final HRGN reconciliation
/// attempt failed.
///
/// A missing rounded region is preferable to leaving the client redraw timer
/// spinning forever. The cache remains invalid, so the next native geometry
/// transition will attempt to build the region again.
#[cfg(target_os = "windows")]
pub fn abandon_main_window_region_update(revision: u64) {
    use std::sync::atomic::Ordering;

    if !main_window_region_update_is_current(revision) {
        return;
    }

    invalidate_main_window_region_cache();
    MAIN_WINDOW_REGION_SUSPENDED.store(false, Ordering::Release);
    rearm_pending_client_redraw();
}

#[cfg(target_os = "windows")]
fn main_window_update_is_current(
    requested_revision: u64,
    current_revision: u64,
    settled_revision: u64,
    in_size_move: bool,
    settle_pending: bool,
    unavailable: bool,
) -> bool {
    requested_revision != 0
        && current_revision == requested_revision
        && settled_revision == requested_revision
        && !in_size_move
        && !settle_pending
        && !unavailable
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
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
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
        // Keep DWM non-client rendering at its winit-configured default.
        // Transparent winit windows use DwmEnableBlurBehindWindow; forcing
        // DWMNCRP_DISABLED here invalidates that transparency path when DWM
        // rebuilds the surface during repeated Snap/maximize transitions. The
        // main-window hook removes WS_CAPTION/WS_EX_WINDOWEDGE and suppresses
        // stray WM_NCPAINT instead, while preserving resize and Snap styles.
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

    // A transition suspends the old rounded region by setting HRGN to NULL.
    // Maximized windows also require a NULL region, so that suspension is
    // already the final native shape. Mark it reconciled without calling
    // SetWindowRgn(NULL) a second time (which would emit another pair of
    // WINDOWPOS messages and extend the compositor transaction).
    if radius <= 1 && MAIN_WINDOW_REGION_SUSPENDED.swap(false, Ordering::AcqRel) {
        MAIN_WINDOW_REGION_CACHE_HWND.store(hwnd_value, Ordering::Release);
        MAIN_WINDOW_REGION_CACHE_WIDTH.store(width, Ordering::Release);
        MAIN_WINDOW_REGION_CACHE_HEIGHT.store(height, Ordering::Release);
        MAIN_WINDOW_REGION_CACHE_RADIUS.store(radius, Ordering::Release);
        rearm_pending_client_redraw();
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
            rearm_pending_client_redraw();
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
    if APPEARANCE_SETTLE_FALLBACK_MESSAGE.load(Ordering::Acquire) == 0 {
        let registered =
            unsafe { RegisterWindowMessageW(w!("BExplorer.AppearanceSettleFallback")) };
        if registered == 0 {
            return Ok(());
        }
        APPEARANCE_SETTLE_FALLBACK_MESSAGE.store(registered, Ordering::Release);
    }
    if CLIENT_REDRAW_FALLBACK_MESSAGE.load(Ordering::Acquire) == 0 {
        let registered = unsafe { RegisterWindowMessageW(w!("BExplorer.ClientRedrawFallback")) };
        if registered == 0 {
            return Ok(());
        }
        CLIENT_REDRAW_FALLBACK_MESSAGE.store(registered, Ordering::Release);
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
    let frame_result = remove_native_frame_styles(hwnd);
    // SWP_FRAMECHANGED below intentionally keeps the current size, so Windows
    // is not required to emit WM_SIZE. Establish an initial settled baseline
    // explicitly; otherwise the logical watchdog could wait for a resize that
    // never arrives.
    defer_main_window_appearance_settle(hwnd);
    frame_result
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
static APPEARANCE_SETTLE_FALLBACK_MESSAGE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "windows")]
static APPEARANCE_SETTLE_FALLBACK_TOKEN: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
static CLIENT_REDRAW_FALLBACK_MESSAGE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "windows")]
static CLIENT_REDRAW_FALLBACK_TOKEN: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
static DEFERRED_CLIENT_REDRAW_TIMER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
static DEFERRED_CLIENT_REDRAW_TIMER_SEQUENCE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_CLIENT_REDRAW_PENDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
const DEFERRED_CLIENT_REDRAW_DELAY_MS: u32 = 20;

#[cfg(target_os = "windows")]
static DEFERRED_APPEARANCE_SETTLE_TIMER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
static DEFERRED_APPEARANCE_SETTLE_TIMER_SEQUENCE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "windows")]
const DEFERRED_APPEARANCE_SETTLE_DELAY_MS: u32 = 90;

#[cfg(target_os = "windows")]
const CLIENT_REDRAW_TIMER_NAMESPACE: usize = 0xA000_0000;

#[cfg(target_os = "windows")]
const APPEARANCE_SETTLE_TIMER_NAMESPACE: usize = 0xB000_0000;

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
static MAIN_WINDOW_NATIVE_MAXIMIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
static MAIN_WINDOW_IN_SIZE_MOVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
fn next_window_timer_id(sequence: &std::sync::atomic::AtomicUsize, namespace: usize) -> usize {
    use std::sync::atomic::Ordering;

    const GENERATION_MASK: usize = 0x0FFF_FFFF;

    let generation = sequence.fetch_add(1, Ordering::AcqRel).wrapping_add(1) & GENERATION_MASK;
    namespace | generation.max(1)
}

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
    cancel_appearance_settle(hwnd);
    cancel_client_redraw(hwnd);
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
fn begin_main_window_size_move(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::IsZoomed;

    cancel_appearance_settle(hwnd);
    advance_main_window_appearance_revision();
    let maximized = unsafe { IsZoomed(hwnd).as_bool() };
    MAIN_WINDOW_NATIVE_MAXIMIZED.store(maximized, Ordering::Release);
    MAIN_WINDOW_IN_SIZE_MOVE.store(true, Ordering::Release);
}

#[cfg(target_os = "windows")]
fn prepare_main_window_position_change(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOSIZE, WINDOWPOS};

    if lparam.0 == 0 || MAIN_WINDOW_REGION_MUTATING.load(Ordering::Acquire) {
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

    if MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire) {
        // A pure title-bar move must not churn the HRGN. Suspend it only when
        // the interactive transaction actually changes size (border resize,
        // restore-from-maximized, or top-edge Snap).
        suspend_main_window_region(hwnd);
        return;
    }

    // ShowWindow/window::maximize and system shortcuts do not enter the modal
    // size/move loop. Remove the old-size HRGN before Windows grows the native
    // surface so it cannot clip the new renderer frame.
    cancel_appearance_settle(hwnd);
    advance_main_window_appearance_revision();
    suspend_main_window_region(hwnd);
    // WINDOWPOSCHANGED/WM_SIZE normally rearm this deadline. Arming it here as
    // well guarantees liveness if Windows cancels the transaction after only
    // sending WINDOWPOSCHANGING.
    defer_main_window_appearance_settle(hwnd);
}

#[cfg(target_os = "windows")]
fn suspend_main_window_region(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::Graphics::Gdi::{HRGN, SetWindowRgn};

    if MAIN_WINDOW_REGION_SUSPENDED.swap(true, Ordering::AcqRel) {
        return;
    }

    let hwnd_value = hwnd.0 as isize;
    let already_unclipped = MAIN_WINDOW_REGION_CACHE_HWND.load(Ordering::Acquire) == hwnd_value
        && MAIN_WINDOW_REGION_CACHE_RADIUS.load(Ordering::Acquire) <= 1;

    // Invalidate before entering SetWindowRgn because it synchronously emits
    // WINDOWPOS messages. A later settle must always rebuild the final region.
    invalidate_main_window_region_cache();
    if already_unclipped {
        // A maximized window already owns no HRGN. Mark the new transaction as
        // suspended without asking USER32/DWM to remove the same region again.
        return;
    }
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
    MAIN_WINDOW_NATIVE_MAXIMIZED.store(maximized, Ordering::Release);
    defer_main_window_appearance_settle(hwnd);
}

#[cfg(target_os = "windows")]
fn observe_main_window_size(hwnd: windows::Win32::Foundation::HWND, size_kind: u32) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};

    if MAIN_WINDOW_REGION_MUTATING.load(Ordering::Acquire) {
        return;
    }
    if !matches!(size_kind, SIZE_RESTORED | SIZE_MINIMIZED | SIZE_MAXIMIZED) {
        // SIZE_MAXSHOW/SIZE_MAXHIDE are popup notifications. They must not
        // open a main-window geometry revision that can never settle.
        return;
    }
    if size_kind == SIZE_MINIMIZED {
        // Invalidate queued Iced/native work and leave the current region state
        // untouched while hidden (it may already be suspended by the preceding
        // WINDOWPOSCHANGING). Restoration opens a fresh visible revision.
        advance_main_window_appearance_revision();
        cancel_appearance_settle(hwnd);
        cancel_client_redraw(hwnd);
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

    match size_kind {
        SIZE_MAXIMIZED => {
            MAIN_WINDOW_NATIVE_MAXIMIZED.store(true, Ordering::Release);
        }
        SIZE_RESTORED => {
            MAIN_WINDOW_NATIVE_MAXIMIZED.store(false, Ordering::Release);
        }
        _ => return,
    }

    defer_main_window_appearance_settle(hwnd);
}

#[cfg(target_os = "windows")]
fn defer_main_window_appearance_settle(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SetTimer};

    if MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire) {
        return;
    }

    // Recreate the timer to reset the quiet-period deadline. A fresh identifier
    // also makes a WM_TIMER already queued for the previous
    // deadline distinguishable after KillTimer, which does not remove queued
    // messages.
    cancel_appearance_settle(hwnd);
    let timer_id = next_window_timer_id(
        &DEFERRED_APPEARANCE_SETTLE_TIMER_SEQUENCE,
        APPEARANCE_SETTLE_TIMER_NAMESPACE,
    );
    let timer = unsafe { SetTimer(hwnd, timer_id, DEFERRED_APPEARANCE_SETTLE_DELAY_MS, None) };
    if timer != 0 {
        DEFERRED_APPEARANCE_SETTLE_TIMER.store(timer_id, Ordering::Release);
        APPEARANCE_SETTLE_FALLBACK_TOKEN.store(0, Ordering::Release);
        return;
    }

    // Timer allocation can fail under resource pressure. Post a registered
    // window message as a last-resort queue barrier so a failed SetTimer cannot
    // leave the HRGN suspended forever. The normal path always retains the
    // 90 ms quiet-period debounce.
    APPEARANCE_SETTLE_FALLBACK_TOKEN.store(timer_id, Ordering::Release);
    let message = APPEARANCE_SETTLE_FALLBACK_MESSAGE.load(Ordering::Acquire);
    if message == 0 || unsafe { PostMessageW(hwnd, message, WPARAM(timer_id), LPARAM(0)) }.is_err()
    {
        // Registration is required before the hook is installed, so reaching
        // this branch means the queue itself is unavailable. Publish directly
        // as the final liveness guarantee; subsequent geometry notifications
        // will open a newer revision if the transaction is still progressing.
        if APPEARANCE_SETTLE_FALLBACK_TOKEN
            .compare_exchange(timer_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            publish_main_window_appearance_settle(hwnd);
        }
    }
}

#[cfg(target_os = "windows")]
fn publish_main_window_appearance_settle(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};

    if MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire) || unsafe { IsIconic(hwnd).as_bool() } {
        return;
    }

    let maximized = unsafe { IsZoomed(hwnd).as_bool() };
    MAIN_WINDOW_NATIVE_MAXIMIZED.store(maximized, Ordering::Release);
    MAIN_WINDOW_APPEARANCE_SETTLED_MAXIMIZED.store(maximized, Ordering::Release);
    MAIN_WINDOW_APPEARANCE_SETTLED_REVISION.store(
        MAIN_WINDOW_APPEARANCE_REVISION.load(Ordering::Acquire),
        Ordering::Release,
    );
    MAIN_WINDOW_APPEARANCE_GENERATION.fetch_add(1, Ordering::Release);
    notify_main_window_appearance();
    if MAIN_WINDOW_CLIENT_REDRAW_PENDING.load(Ordering::Acquire) {
        defer_client_redraw(hwnd);
    }
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
        WM_STYLECHANGING, WM_SYNCPAINT, WM_TIMER, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING,
        WNDPROC,
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
    let settle_fallback_message = APPEARANCE_SETTLE_FALLBACK_MESSAGE.load(Ordering::Acquire);
    if settle_fallback_message != 0 && msg == settle_fallback_message {
        let token = wparam.0;
        if token != 0
            && APPEARANCE_SETTLE_FALLBACK_TOKEN
                .compare_exchange(token, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            && DEFERRED_APPEARANCE_SETTLE_TIMER.load(Ordering::Acquire) == 0
            && !MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire)
            && !unsafe { IsIconic(hwnd).as_bool() }
        {
            publish_main_window_appearance_settle(hwnd);
        }
        return LRESULT(0);
    }
    let redraw_fallback_message = CLIENT_REDRAW_FALLBACK_MESSAGE.load(Ordering::Acquire);
    if redraw_fallback_message != 0 && msg == redraw_fallback_message {
        let token = wparam.0;
        if token != 0
            && CLIENT_REDRAW_FALLBACK_TOKEN
                .compare_exchange(token, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            flush_pending_client_redraw(hwnd);
        }
        return LRESULT(0);
    }
    let redraw_timer = DEFERRED_CLIENT_REDRAW_TIMER.load(Ordering::Acquire);
    if redraw_timer != 0 && msg == WM_TIMER && wparam.0 == redraw_timer {
        stop_client_redraw_timer(hwnd);
        flush_pending_client_redraw(hwnd);
        return LRESULT(0);
    }
    let appearance_timer = DEFERRED_APPEARANCE_SETTLE_TIMER.load(Ordering::Acquire);
    if appearance_timer != 0 && msg == WM_TIMER && wparam.0 == appearance_timer {
        cancel_appearance_settle(hwnd);
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
            if !MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire) {
                advance_main_window_appearance_revision();
                suspend_main_window_region(hwnd);
                defer_main_window_appearance_settle(hwnd);
            }
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
            defer_main_window_appearance_settle(hwnd);
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
            cancel_appearance_settle(hwnd);
            MAIN_WINDOW_IN_SIZE_MOVE.store(false, Ordering::Release);
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

    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SetTimer};

    MAIN_WINDOW_CLIENT_REDRAW_PENDING.store(true, Ordering::Release);
    stop_client_redraw_timer(hwnd);
    CLIENT_REDRAW_FALLBACK_TOKEN.store(0, Ordering::Release);
    let timer_id = next_window_timer_id(
        &DEFERRED_CLIENT_REDRAW_TIMER_SEQUENCE,
        CLIENT_REDRAW_TIMER_NAMESPACE,
    );
    let timer = unsafe { SetTimer(hwnd, timer_id, DEFERRED_CLIENT_REDRAW_DELAY_MS, None) };
    if timer != 0 {
        DEFERRED_CLIENT_REDRAW_TIMER.store(timer_id, Ordering::Release);
        return;
    }

    CLIENT_REDRAW_FALLBACK_TOKEN.store(timer_id, Ordering::Release);
    let message = CLIENT_REDRAW_FALLBACK_MESSAGE.load(Ordering::Acquire);
    if (message == 0
        || unsafe { PostMessageW(hwnd, message, WPARAM(timer_id), LPARAM(0)) }.is_err())
        && CLIENT_REDRAW_FALLBACK_TOKEN
            .compare_exchange(timer_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        flush_pending_client_redraw(hwnd);
    }
}

#[cfg(target_os = "windows")]
fn rearm_pending_client_redraw() {
    use std::sync::atomic::Ordering;

    use windows::Win32::Foundation::HWND;

    if !MAIN_WINDOW_CLIENT_REDRAW_PENDING.load(Ordering::Acquire) {
        return;
    }
    let hwnd = HWND(MAIN_WINDOW_HWND.load(Ordering::Acquire) as *mut _);
    if !hwnd.is_invalid() {
        defer_client_redraw(hwnd);
    }
}

#[cfg(target_os = "windows")]
fn flush_pending_client_redraw(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::Graphics::Gdi::{HRGN, RDW_INTERNALPAINT, RedrawWindow};
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;

    if !MAIN_WINDOW_CLIENT_REDRAW_PENDING.load(Ordering::Acquire) {
        return;
    }
    if !MAIN_WINDOW_ACTIVE.load(Ordering::Acquire) || unsafe { IsIconic(hwnd).as_bool() } {
        MAIN_WINDOW_CLIENT_REDRAW_PENDING.store(false, Ordering::Release);
        return;
    }
    if MAIN_WINDOW_IN_SIZE_MOVE.load(Ordering::Acquire)
        || main_window_settle_pending()
        || MAIN_WINDOW_REGION_SUSPENDED.load(Ordering::Acquire)
    {
        // The native settle and the final HRGN completion explicitly rearm the
        // latched debt; do not poll the compositor while either is pending.
        return;
    }

    MAIN_WINDOW_CLIENT_REDRAW_PENDING.store(false, Ordering::Release);
    // Activation can emit both WM_ACTIVATE and WM_SYNCPAINT. Present one frame
    // only after DWM has had a compositor frame to leave Acrylic's inactive
    // fallback.
    let _ = unsafe { RedrawWindow(hwnd, None, HRGN::default(), RDW_INTERNALPAINT) };
}

#[cfg(target_os = "windows")]
fn stop_client_redraw_timer(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::KillTimer;

    let timer = DEFERRED_CLIENT_REDRAW_TIMER.swap(0, Ordering::AcqRel);
    if timer != 0 {
        let _ = unsafe { KillTimer(hwnd, timer) };
    }
}

#[cfg(target_os = "windows")]
fn cancel_client_redraw(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    stop_client_redraw_timer(hwnd);
    CLIENT_REDRAW_FALLBACK_TOKEN.store(0, Ordering::Release);
    MAIN_WINDOW_CLIENT_REDRAW_PENDING.store(false, Ordering::Release);
}

#[cfg(target_os = "windows")]
fn cancel_appearance_settle(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::WindowsAndMessaging::KillTimer;

    let timer = DEFERRED_APPEARANCE_SETTLE_TIMER.swap(0, Ordering::AcqRel);
    if timer != 0 {
        let _ = unsafe { KillTimer(hwnd, timer) };
    }
    APPEARANCE_SETTLE_FALLBACK_TOKEN.store(0, Ordering::Release);
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
        APPEARANCE_SETTLE_TIMER_NAMESPACE, CLIENT_REDRAW_TIMER_NAMESPACE, custom_frame_ex_style,
        custom_frame_style, main_window_update_is_current, next_window_timer_id,
        non_client_activation_lparam, should_defer_client_redraw,
        should_suppress_native_frame_paint, work_area_maximize_metrics,
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
    fn settled_work_requires_the_current_visible_revision() {
        assert!(main_window_update_is_current(8, 8, 8, false, false, false));
        assert!(!main_window_update_is_current(7, 8, 8, false, false, false));
        assert!(!main_window_update_is_current(8, 8, 7, false, false, false));
        assert!(!main_window_update_is_current(8, 8, 8, true, false, false));
        assert!(!main_window_update_is_current(8, 8, 8, false, true, false));
        assert!(!main_window_update_is_current(8, 8, 8, false, false, true));
        assert!(!main_window_update_is_current(0, 0, 0, false, false, false));
    }

    #[test]
    fn rearmed_timers_reject_messages_from_old_deadlines() {
        let sequence = std::sync::atomic::AtomicUsize::new(0);
        let first = next_window_timer_id(&sequence, APPEARANCE_SETTLE_TIMER_NAMESPACE);
        let second = next_window_timer_id(&sequence, APPEARANCE_SETTLE_TIMER_NAMESPACE);
        let redraw = next_window_timer_id(&sequence, CLIENT_REDRAW_TIMER_NAMESPACE);

        assert_ne!(first, second);
        assert_ne!(second, redraw);
        assert_eq!(first & 0xF000_0000, APPEARANCE_SETTLE_TIMER_NAMESPACE);
        assert_eq!(redraw & 0xF000_0000, CLIENT_REDRAW_TIMER_NAMESPACE);
    }

    #[test]
    fn queued_fallback_cannot_consume_a_newer_deadline() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sequence = AtomicUsize::new(0);
        let first = next_window_timer_id(&sequence, APPEARANCE_SETTLE_TIMER_NAMESPACE);
        let second = next_window_timer_id(&sequence, APPEARANCE_SETTLE_TIMER_NAMESPACE);
        let current = AtomicUsize::new(second);

        assert!(
            current
                .compare_exchange(first, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        );
        assert_eq!(current.load(Ordering::Acquire), second);
        assert!(
            current
                .compare_exchange(second, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        );
    }
}

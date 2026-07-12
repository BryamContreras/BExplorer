//! Optional KWin blur support for existing Iced/Wayland surfaces.
//!
//! `iced::window::run` intentionally exposes only raw window/display handles.
//! The small client context here attaches KWin's optional protocol to that
//! existing surface and leaves all non-KWin compositors untouched.

use std::cell::RefCell;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::protocol::{
    wl_registry::{self, WlRegistry},
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_plasma::blur::client::{
    org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
};
use wayland_sys::client::{wl_display, wl_proxy};

use crate::utils::errors::{BExplorerError, Result};

thread_local! {
    static CONTEXTS: RefCell<Vec<KWinBlurContext>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct KWinBlurState {
    _registry: Option<WlRegistry>,
    manager: Option<OrgKdeKwinBlurManager>,
}

struct KWinBlurContext {
    display_ptr: usize,
    surface_ptr: usize,
    connection: Connection,
    queue: EventQueue<KWinBlurState>,
    state: KWinBlurState,
    surface: WlSurface,
    blur: Option<OrgKdeKwinBlur>,
}

/// Applies or removes KWin's blur request for a live Wayland surface.
///
/// When the current compositor does not advertise the KDE protocol this is an
/// ordinary fallback condition, surfaced to the caller so it can keep the
/// transparent appearance without treating it as an application failure.
pub(super) fn set_window_blur(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
    enabled: bool,
) -> Result<()> {
    let Some((display_ptr, surface_ptr)) = wayland_handles(raw_display_handle, raw_window_handle)
    else {
        return Ok(());
    };

    CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        if let Some(context) = contexts.iter_mut().find(|context| {
            context.display_ptr == display_ptr && context.surface_ptr == surface_ptr
        }) {
            return context.set_enabled(enabled);
        }

        // There is nothing to remove if this window has never requested blur.
        if !enabled {
            return Ok(());
        }

        let mut context = KWinBlurContext::new(display_ptr, surface_ptr)?;
        context.set_enabled(true)?;
        contexts.push(context);
        Ok(())
    })
}

impl KWinBlurContext {
    fn new(display_ptr: usize, surface_ptr: usize) -> Result<Self> {
        // The display belongs to winit/Iced. `from_foreign_display` expressly
        // creates a non-owning backend, so dropping our context never closes
        // the application's Wayland connection.
        let backend = unsafe { Backend::from_foreign_display(display_ptr as *mut wl_display) };
        let connection = Connection::from_backend(backend);
        let mut queue = connection.new_event_queue();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());

        let surface_id = unsafe {
            ObjectId::from_ptr(WlSurface::interface(), surface_ptr as *mut wl_proxy).map_err(
                |_| BExplorerError::Operation("Could not wrap the Wayland window surface".into()),
            )?
        };
        let surface = WlSurface::from_id(&connection, surface_id).map_err(|_| {
            BExplorerError::Operation("Could not create a Wayland window-surface proxy".into())
        })?;

        let mut state = KWinBlurState {
            _registry: Some(registry),
            ..KWinBlurState::default()
        };
        queue.roundtrip(&mut state).map_err(|error| {
            BExplorerError::Operation(format!("Wayland blur setup failed: {error}"))
        })?;

        if state.manager.is_none() {
            return Err(BExplorerError::Operation(
                "KWin blur protocol is not advertised by this compositor".into(),
            ));
        }
        crate::utils::log::info("KWin blur protocol bound for a BExplorer window");

        Ok(Self {
            display_ptr,
            surface_ptr,
            connection,
            queue,
            state,
            surface,
            blur: None,
        })
    }

    fn set_enabled(&mut self, enabled: bool) -> Result<()> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| {
                BExplorerError::Operation(format!("Wayland blur dispatch failed: {error}"))
            })?;

        if enabled && self.blur.is_none() {
            let manager = self.state.manager.as_ref().ok_or_else(|| {
                BExplorerError::Operation("KWin blur manager became unavailable".into())
            })?;
            let blur = manager.create(&self.surface, &self.queue.handle(), ());
            // A null region means the complete surface, matching winit's own
            // KWin integration and avoiding size-dependent region updates.
            blur.commit();
            self.blur = Some(blur);
            crate::utils::log::info("KWin native blur request applied to a BExplorer window");
        } else if !enabled && self.blur.is_some() {
            if let Some(manager) = self.state.manager.as_ref() {
                manager.unset(&self.surface);
            }
            if let Some(blur) = self.blur.take() {
                blur.release();
            }
        }

        self.connection.flush().map_err(|error| {
            BExplorerError::Operation(format!("Wayland blur flush failed: {error}"))
        })
    }
}

impl Dispatch<WlRegistry, ()> for KWinBlurState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "org_kde_kwin_blur_manager"
        {
            state.manager =
                Some(registry.bind::<OrgKdeKwinBlurManager, _, _>(name, version.min(1), qh, ()));
        }
    }
}

impl Dispatch<OrgKdeKwinBlurManager, ()> for KWinBlurState {
    fn event(
        _state: &mut Self,
        _manager: &OrgKdeKwinBlurManager,
        _event: <OrgKdeKwinBlurManager as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdeKwinBlur, ()> for KWinBlurState {
    fn event(
        _state: &mut Self,
        _blur: &OrgKdeKwinBlur,
        _event: <OrgKdeKwinBlur as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

fn wayland_handles(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Option<(usize, usize)> {
    let display = match raw_display_handle {
        RawDisplayHandle::Wayland(handle) => handle.display.as_ptr() as usize,
        _ => return None,
    };
    let surface = match raw_window_handle {
        RawWindowHandle::Wayland(handle) => handle.surface.as_ptr() as usize,
        _ => return None,
    };
    Some((display, surface))
}

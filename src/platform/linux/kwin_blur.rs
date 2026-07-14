//! Optional KWin blur support for existing Iced/Wayland surfaces.
//!
//! `iced::window::run` intentionally exposes only raw window/display handles.
//! The small client context here attaches KWin's optional protocol to that
//! existing surface and leaves all non-KWin compositors untouched.

use std::cell::RefCell;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::protocol::{
    wl_compositor::WlCompositor,
    wl_region::WlRegion,
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
    compositor: Option<WlCompositor>,
    manager: Option<OrgKdeKwinBlurManager>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BlurGeometry {
    width: u32,
    height: u32,
    radius: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegionRectangle {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct KWinBlurContext {
    display_ptr: usize,
    surface_ptr: usize,
    connection: Connection,
    queue: EventQueue<KWinBlurState>,
    state: KWinBlurState,
    surface: WlSurface,
    blur: Option<OrgKdeKwinBlur>,
    blur_region: Option<WlRegion>,
    blur_geometry: Option<BlurGeometry>,
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
    width: u32,
    height: u32,
    radius: u32,
) -> Result<()> {
    let Some((display_ptr, surface_ptr)) = wayland_handles(raw_display_handle, raw_window_handle)
    else {
        return Ok(());
    };

    let geometry = BlurGeometry {
        width,
        height,
        radius,
    };
    CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        if let Some(context) = contexts.iter_mut().find(|context| {
            context.display_ptr == display_ptr && context.surface_ptr == surface_ptr
        }) {
            return context.set_enabled(enabled, geometry);
        }

        // There is nothing to remove if this window has never requested blur.
        if !enabled {
            return Ok(());
        }

        let mut context = KWinBlurContext::new(display_ptr, surface_ptr)?;
        context.set_enabled(true, geometry)?;
        contexts.push(context);
        Ok(())
    })
}

/// Updates the KWin blur mask after a client-side window resize. The native
/// blur context is created by `set_window_blur`; opaque/fallback windows have
/// no context and therefore need no work here.
pub(super) fn update_window_blur_region(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
    width: u32,
    height: u32,
    radius: u32,
) -> Result<()> {
    let Some((display_ptr, surface_ptr)) = wayland_handles(raw_display_handle, raw_window_handle)
    else {
        return Ok(());
    };
    let geometry = BlurGeometry {
        width,
        height,
        radius,
    };
    CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        let Some(context) = contexts.iter_mut().find(|context| {
            context.display_ptr == display_ptr && context.surface_ptr == surface_ptr
        }) else {
            return Ok(());
        };
        context.update_region(geometry)
    })
}

pub(super) fn release_window(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) {
    let Some((display_ptr, surface_ptr)) = wayland_handles(raw_display_handle, raw_window_handle)
    else {
        return;
    };
    let released = CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        contexts
            .iter()
            .position(|context| {
                context.display_ptr == display_ptr && context.surface_ptr == surface_ptr
            })
            .map(|index| contexts.swap_remove(index))
    });
    release_contexts(released);
}

pub(super) fn release_display(raw_display_handle: RawDisplayHandle) {
    let Some(display_ptr) = wayland_display(raw_display_handle) else {
        return;
    };
    let released = CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        let mut released = Vec::new();
        let mut index = 0;
        while index < contexts.len() {
            if contexts[index].display_ptr == display_ptr {
                released.push(contexts.swap_remove(index));
            } else {
                index += 1;
            }
        }
        released
    });
    release_contexts(released);
}

fn release_contexts(contexts: impl IntoIterator<Item = KWinBlurContext>) {
    for mut context in contexts {
        // Always drop the context even if the compositor stopped responding.
        // Its guest Backend still has to disappear before winit's display.
        if let Err(error) = context.set_enabled(false, BlurGeometry::default()) {
            crate::utils::log::info(format!(
                "Could not remove KWin blur during window close: {error}"
            ));
        }
        drop(context);
        crate::utils::log::info("KWin blur context released before window close");
    }
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
        if state.compositor.is_none() {
            return Err(BExplorerError::Operation(
                "Wayland compositor could not create the rounded blur region".into(),
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
            blur_region: None,
            blur_geometry: None,
        })
    }

    fn set_enabled(&mut self, enabled: bool, geometry: BlurGeometry) -> Result<()> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| {
                BExplorerError::Operation(format!("Wayland blur dispatch failed: {error}"))
            })?;

        if enabled {
            let created = self.blur.is_none();
            if created {
                let manager = self.state.manager.as_ref().ok_or_else(|| {
                    BExplorerError::Operation("KWin blur manager became unavailable".into())
                })?;
                self.blur = Some(manager.create(&self.surface, &self.queue.handle(), ()));
            }
            self.apply_rounded_region(geometry)?;
            if created {
                crate::utils::log::info(
                    "KWin native blur request applied with rounded corners to a BExplorer window",
                );
            }
        } else if self.blur.is_some() {
            if let Some(manager) = self.state.manager.as_ref() {
                manager.unset(&self.surface);
            }
            if let Some(blur) = self.blur.take() {
                blur.release();
            }
            if let Some(region) = self.blur_region.take() {
                region.destroy();
            }
            self.blur_geometry = None;
        }

        self.connection.flush().map_err(|error| {
            BExplorerError::Operation(format!("Wayland blur flush failed: {error}"))
        })
    }

    fn update_region(&mut self, geometry: BlurGeometry) -> Result<()> {
        if self.blur.is_none() {
            return Ok(());
        }
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| {
                BExplorerError::Operation(format!("Wayland blur dispatch failed: {error}"))
            })?;
        self.apply_rounded_region(geometry)?;
        self.connection.flush().map_err(|error| {
            BExplorerError::Operation(format!("Wayland blur-region flush failed: {error}"))
        })
    }

    fn apply_rounded_region(&mut self, geometry: BlurGeometry) -> Result<()> {
        if self.blur_geometry == Some(geometry) && self.blur_region.is_some() {
            return Ok(());
        }
        let blur = self
            .blur
            .as_ref()
            .ok_or_else(|| BExplorerError::Operation("KWin blur object is not available".into()))?;
        let compositor = self.state.compositor.as_ref().ok_or_else(|| {
            BExplorerError::Operation("Wayland compositor became unavailable".into())
        })?;
        let region = compositor.create_region(&self.queue.handle(), ());
        for rectangle in rounded_region_rectangles(geometry) {
            region.add(rectangle.x, rectangle.y, rectangle.width, rectangle.height);
        }
        blur.set_region(Some(&region));
        blur.commit();
        if let Some(previous) = self.blur_region.replace(region) {
            previous.destroy();
        }
        self.blur_geometry = Some(geometry);
        Ok(())
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
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_compositor" => {
                state.compositor =
                    Some(registry.bind::<WlCompositor, _, _>(name, version.min(1), qh, ()));
            }
            "org_kde_kwin_blur_manager" => {
                state.manager = Some(registry.bind::<OrgKdeKwinBlurManager, _, _>(
                    name,
                    version.min(1),
                    qh,
                    (),
                ));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCompositor, ()> for KWinBlurState {
    fn event(
        _state: &mut Self,
        _compositor: &WlCompositor,
        _event: <WlCompositor as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlRegion, ()> for KWinBlurState {
    fn event(
        _state: &mut Self,
        _region: &WlRegion,
        _event: <WlRegion as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
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

fn rounded_region_rectangles(geometry: BlurGeometry) -> Vec<RegionRectangle> {
    let width = i32::try_from(geometry.width).unwrap_or(i32::MAX);
    let height = i32::try_from(geometry.height).unwrap_or(i32::MAX);
    if width <= 0 || height <= 0 {
        return Vec::new();
    }

    let radius = i32::try_from(geometry.radius)
        .unwrap_or(i32::MAX)
        .min(width / 2)
        .min(height / 2);
    if radius <= 0 {
        return vec![RegionRectangle {
            x: 0,
            y: 0,
            width,
            height,
        }];
    }

    let mut rectangles = Vec::with_capacity((radius * 2 + 1) as usize);
    for y in 0..radius {
        let inset = rounded_corner_inset(radius, y);
        push_region_rectangle(&mut rectangles, inset, y, width - inset * 2, 1);
    }
    push_region_rectangle(&mut rectangles, 0, radius, width, height - radius * 2);
    for y in (height - radius)..height {
        let mirrored_y = height - 1 - y;
        let inset = rounded_corner_inset(radius, mirrored_y);
        push_region_rectangle(&mut rectangles, inset, y, width - inset * 2, 1);
    }
    rectangles
}

fn rounded_corner_inset(radius: i32, y: i32) -> i32 {
    let radius = f64::from(radius);
    let distance_from_center = radius - (f64::from(y) + 0.5);
    let half_width = (radius * radius - distance_from_center * distance_from_center)
        .max(0.0)
        .sqrt();
    (radius - half_width - 0.5).ceil().max(0.0) as i32
}

fn push_region_rectangle(
    rectangles: &mut Vec<RegionRectangle>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    if let Some(previous) = rectangles.last_mut()
        && previous.x == x
        && previous.width == width
        && previous.y + previous.height == y
    {
        previous.height += height;
        return;
    }
    rectangles.push(RegionRectangle {
        x,
        y,
        width,
        height,
    });
}

fn wayland_handles(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Option<(usize, usize)> {
    let display = wayland_display(raw_display_handle)?;
    let surface = match raw_window_handle {
        RawWindowHandle::Wayland(handle) => handle.surface.as_ptr() as usize,
        _ => return None,
    };
    Some((display, surface))
}

fn wayland_display(raw_display_handle: RawDisplayHandle) -> Option<usize> {
    match raw_display_handle {
        RawDisplayHandle::Wayland(handle) => Some(handle.display.as_ptr() as usize),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(rectangles: &[RegionRectangle], x: i32, y: i32) -> bool {
        rectangles.iter().any(|rectangle| {
            x >= rectangle.x
                && x < rectangle.x + rectangle.width
                && y >= rectangle.y
                && y < rectangle.y + rectangle.height
        })
    }

    #[test]
    fn rounded_blur_region_excludes_only_the_window_corners() {
        let rectangles = rounded_region_rectangles(BlurGeometry {
            width: 120,
            height: 80,
            radius: 10,
        });

        assert!(!contains(&rectangles, 0, 0));
        assert!(!contains(&rectangles, 119, 0));
        assert!(!contains(&rectangles, 0, 79));
        assert!(!contains(&rectangles, 119, 79));
        assert!(contains(&rectangles, 60, 0));
        assert!(contains(&rectangles, 0, 40));
        assert!(contains(&rectangles, 60, 40));
    }

    #[test]
    fn rounded_blur_region_is_symmetric_bounded_and_never_empty() {
        let width = 31;
        let height = 17;
        let rectangles = rounded_region_rectangles(BlurGeometry {
            width,
            height,
            radius: 99,
        });

        assert!(!rectangles.is_empty());
        assert!(rectangles.iter().all(|rectangle| {
            rectangle.x >= 0
                && rectangle.y >= 0
                && rectangle.width > 0
                && rectangle.height > 0
                && rectangle.x + rectangle.width <= width as i32
                && rectangle.y + rectangle.height <= height as i32
        }));
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let included = contains(&rectangles, x, y);
                assert_eq!(included, contains(&rectangles, width as i32 - 1 - x, y));
                assert_eq!(included, contains(&rectangles, x, height as i32 - 1 - y));
            }
        }
    }

    #[test]
    fn zero_radius_blur_region_covers_the_complete_surface() {
        assert_eq!(
            rounded_region_rectangles(BlurGeometry {
                width: 50,
                height: 30,
                radius: 0,
            }),
            [RegionRectangle {
                x: 0,
                y: 0,
                width: 50,
                height: 30,
            }]
        );
    }
}

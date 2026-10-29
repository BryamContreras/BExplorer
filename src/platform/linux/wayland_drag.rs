use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::protocol::{
    wl_data_device::{self, WlDataDevice},
    wl_data_device_manager::{self, DndAction, WlDataDeviceManager},
    wl_data_offer::{self, WlDataOffer},
    wl_data_source::{self, WlDataSource},
    wl_pointer::{self, WlPointer},
    wl_registry::{self, WlRegistry},
    wl_seat::{self, WlSeat},
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_sys::client::{wl_display, wl_proxy};

use crate::utils::errors::{BExplorerError, Result};

const MAX_DRAG_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_WRITES: usize = 8;
const DRAG_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

thread_local! {
    static CONTEXT: RefCell<Option<WaylandDragContext>> = const { RefCell::new(None) };
}

struct WaylandDragContext {
    display_ptr: usize,
    surface_ptr: usize,
    connection: Connection,
    queue: EventQueue<WaylandDragState>,
    state: WaylandDragState,
    surface: WlSurface,
}

#[derive(Default)]
struct WaylandDragState {
    _registry: Option<WlRegistry>,
    data_device_manager: Option<WlDataDeviceManager>,
    seats: HashMap<u32, WlSeat>,
    keyboard_capable_seats: HashSet<u32>,
    pointers: HashMap<u32, WlPointer>,
    data_devices: HashMap<u32, WlDataDevice>,
    active_sources: Vec<WlDataSource>,
    ignored_offers: Vec<WlDataOffer>,
    pending_writes: Vec<PendingDragWrite>,
    last_button_serial: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Debug)]
struct SeatData {
    name: u32,
}

#[derive(Clone, Debug)]
struct DragPayload {
    uri_list: Arc<str>,
    gnome_copied_files: Arc<str>,
}

#[derive(Debug, Default)]
struct OfferData;

struct PendingDragWrite {
    file: fs::File,
    contents: Arc<str>,
    written: usize,
    last_progress: Instant,
}

pub fn prepare(raw_display_handle: RawDisplayHandle, raw_window_handle: RawWindowHandle) {
    let Some((display_ptr, surface_ptr)) = wayland_handles(raw_display_handle, raw_window_handle)
    else {
        return;
    };

    CONTEXT.with(|cell| {
        let mut context = cell.borrow_mut();
        let needs_new = context.as_ref().is_none_or(|context| {
            context.display_ptr != display_ptr || context.surface_ptr != surface_ptr
        });
        if needs_new {
            match WaylandDragContext::new(display_ptr, surface_ptr) {
                Ok(new_context) => {
                    crate::utils::log::info("Wayland native drag bridge prepared");
                    *context = Some(new_context);
                }
                Err(error) => {
                    crate::utils::log::error(format!(
                        "Wayland native drag bridge could not be prepared: {error}"
                    ));
                    *context = None;
                }
            }
        }
        if let Some(context) = context.as_mut() {
            let _ = context.dispatch_pending();
        }
    });
}

pub(super) fn release_window(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) {
    let Some((display_ptr, surface_ptr)) = wayland_handles(raw_display_handle, raw_window_handle)
    else {
        return;
    };
    release_matching_context(|context| {
        context.display_ptr == display_ptr && context.surface_ptr == surface_ptr
    });
}

pub(super) fn release_display(raw_display_handle: RawDisplayHandle) {
    let Some(display_ptr) = wayland_display(raw_display_handle) else {
        return;
    };
    release_matching_context(|context| context.display_ptr == display_ptr);
}

fn release_matching_context(matches: impl FnOnce(&WaylandDragContext) -> bool) {
    let released = CONTEXT.with(|cell| {
        let mut context = cell.borrow_mut();
        if context.as_ref().is_some_and(matches) {
            context.take()
        } else {
            None
        }
    });

    if released.is_some() {
        // Drop the guest Backend while winit's foreign wl_display is still
        // valid. Leaving this to TLS destruction can run after the display
        // and makes an otherwise clean Wayland exit undefined.
        drop(released);
        crate::utils::log::info("Wayland native drag bridge released before window close");
    }
}

pub fn start_file_drag(
    paths: Vec<PathBuf>,
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Result<()> {
    let (display_ptr, surface_ptr) = wayland_handles(raw_display_handle, raw_window_handle)
        .ok_or_else(|| BExplorerError::Shell("This Linux session is not Wayland".into()))?;

    CONTEXT.with(|cell| {
        let mut context = cell.borrow_mut();
        let needs_new = context.as_ref().is_none_or(|context| {
            context.display_ptr != display_ptr || context.surface_ptr != surface_ptr
        });
        if needs_new {
            *context = Some(WaylandDragContext::new(display_ptr, surface_ptr)?);
        }
        let context = context
            .as_mut()
            .ok_or_else(|| BExplorerError::Shell("Wayland drag context is not available".into()))?;
        context.dispatch_pending()?;
        context.start_file_drag(paths)
    })
}

/// Pumps Wayland events for an outbound drag. The destination asks the source
/// for `text/uri-list` asynchronously, so this has to continue after
/// `start_drag` returns.
pub fn poll_active_file_drag(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Result<bool> {
    let (display_ptr, surface_ptr) = wayland_handles(raw_display_handle, raw_window_handle)
        .ok_or_else(|| BExplorerError::Shell("This Linux session is not Wayland".into()))?;

    CONTEXT.with(|cell| {
        let mut context = cell.borrow_mut();
        let Some(context) = context.as_mut().filter(|context| {
            context.display_ptr == display_ptr && context.surface_ptr == surface_ptr
        }) else {
            return Ok(false);
        };
        context.dispatch_pending()?;
        context.state.retain_active_sources();
        Ok(!context.state.active_sources.is_empty() || !context.state.pending_writes.is_empty())
    })
}

impl WaylandDragContext {
    fn new(display_ptr: usize, surface_ptr: usize) -> Result<Self> {
        let backend = unsafe { Backend::from_foreign_display(display_ptr as *mut wl_display) };
        let connection = Connection::from_backend(backend);
        let mut queue = connection.new_event_queue();
        let qh = queue.handle();

        let display = connection.display();
        let registry = display.get_registry(&qh, ());

        let surface_id = unsafe {
            ObjectId::from_ptr(WlSurface::interface(), surface_ptr as *mut wl_proxy)
                .map_err(|_| BExplorerError::Shell("Could not wrap Wayland surface".into()))?
        };
        let surface = WlSurface::from_id(&connection, surface_id)
            .map_err(|_| BExplorerError::Shell("Could not create Wayland surface proxy".into()))?;

        let mut state = WaylandDragState {
            _registry: Some(registry),
            ..Default::default()
        };
        queue
            .roundtrip(&mut state)
            .map_err(|error| BExplorerError::Shell(format!("Wayland setup failed: {error}")))?;
        queue.roundtrip(&mut state).map_err(|error| {
            BExplorerError::Shell(format!("Wayland input setup failed: {error}"))
        })?;

        Ok(Self {
            display_ptr,
            surface_ptr,
            connection,
            queue,
            state,
            surface,
        })
    }

    fn dispatch_pending(&mut self) -> Result<()> {
        self.state.poll_pending_writes();
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| BExplorerError::Shell(format!("Wayland dispatch failed: {error}")))?;
        let _ = self.connection.flush();
        self.state.poll_pending_writes();
        Ok(())
    }

    fn start_file_drag(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        let payload = DragPayload::from_paths(&paths)?;
        let (seat_name, serial) = self.state.last_button_serial.ok_or_else(|| {
            BExplorerError::Shell("Wayland has not provided a mouse drag serial yet".into())
        })?;
        let data_device =
            self.state.data_devices.get(&seat_name).ok_or_else(|| {
                BExplorerError::Shell("Wayland data device is not available".into())
            })?;
        let manager = self.state.data_device_manager.as_ref().ok_or_else(|| {
            BExplorerError::Shell("Wayland data device manager is not available".into())
        })?;
        let qh = self.queue.handle();
        let source = manager.create_data_source(&qh, payload);
        source.offer("text/uri-list".into());
        source.offer("x-special/gnome-copied-files".into());
        if source.version() >= 3 {
            source.set_actions(DndAction::Copy);
        }
        data_device.start_drag(Some(&source), &self.surface, None, serial);
        self.state.retain_active_sources();
        self.state.active_sources.push(source);
        self.connection.flush().map_err(|error| {
            BExplorerError::Shell(format!("Wayland drag flush failed: {error}"))
        })?;
        crate::utils::log::info(format!(
            "Wayland native external drag started for {} path(s)",
            paths.len()
        ));
        Ok(())
    }
}

impl WaylandDragState {
    fn finish_source(&mut self, source: &WlDataSource) {
        let source_id = source.id();
        if source.is_alive() {
            source.destroy();
        }
        self.active_sources
            .retain(|active| active.id() != source_id);
    }

    fn retain_active_sources(&mut self) {
        self.active_sources.retain(WlDataSource::is_alive);
    }

    fn discard_offer(&mut self, offer: WlDataOffer) {
        self.ignored_offers.retain(|candidate| candidate != &offer);
        if offer.is_alive() {
            offer.destroy();
        }
    }

    fn poll_pending_writes(&mut self) {
        self.pending_writes.retain_mut(|pending| {
            if pending.last_progress.elapsed() >= DRAG_WRITE_TIMEOUT {
                return false;
            }
            loop {
                match pending
                    .file
                    .write(&pending.contents.as_bytes()[pending.written..])
                {
                    Ok(0) => return false,
                    Ok(count) => {
                        pending.written += count;
                        pending.last_progress = Instant::now();
                        if pending.written == pending.contents.len() {
                            return false;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return true,
                    Err(_) => return false,
                }
            }
        });
    }

    fn ensure_data_device(&mut self, seat_name: u32, qh: &QueueHandle<Self>) {
        if self.data_devices.contains_key(&seat_name)
            || !self.keyboard_capable_seats.contains(&seat_name)
        {
            return;
        }
        let (Some(manager), Some(seat)) = (
            self.data_device_manager.as_ref(),
            self.seats.get(&seat_name),
        ) else {
            return;
        };
        let device = manager.get_data_device(seat, qh, SeatData { name: seat_name });
        self.data_devices.insert(seat_name, device);
        crate::utils::log::info(format!(
            "Wayland data device ready for keyboard-capable seat {seat_name}"
        ));
    }
}

impl DragPayload {
    fn from_paths(paths: &[PathBuf]) -> Result<Self> {
        let mut uri_list = String::new();
        let mut gnome_copied_files = String::from("copy\n");
        for path in paths {
            let uri = file_uri(path)?;
            if uri_list.len().saturating_add(uri.len() + 2) > MAX_DRAG_BYTES {
                return Err(BExplorerError::Shell(
                    "The dragged file list is too large".into(),
                ));
            }
            uri_list.push_str(&uri);
            uri_list.push_str("\r\n");
            gnome_copied_files.push_str(&uri);
            gnome_copied_files.push('\n');
        }
        Ok(Self {
            uri_list: uri_list.into(),
            gnome_copied_files: gnome_copied_files.into(),
        })
    }

    fn data_for_mime(&self, mime: &str) -> Option<Arc<str>> {
        match mime {
            "text/uri-list" => Some(self.uri_list.clone()),
            "x-special/gnome-copied-files" => Some(self.gnome_copied_files.clone()),
            _ => None,
        }
    }
}

impl Dispatch<WlRegistry, ()> for WaylandDragState {
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
        {
            match interface.as_str() {
                "wl_seat" => {
                    let seat = registry.bind::<WlSeat, _, _>(
                        name,
                        version.clamp(1, 7),
                        qh,
                        SeatData { name },
                    );
                    state.seats.insert(name, seat);
                }
                "wl_data_device_manager" => {
                    let manager = registry.bind::<WlDataDeviceManager, _, _>(
                        name,
                        version.clamp(1, 3),
                        qh,
                        (),
                    );
                    state.data_device_manager = Some(manager);
                    let seat_names = state.seats.keys().copied().collect::<Vec<_>>();
                    for seat_name in seat_names {
                        state.ensure_data_device(seat_name, qh);
                    }
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, SeatData> for WaylandDragState {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        data: &SeatData,
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Pointer)
                && !state.pointers.contains_key(&data.name)
            {
                let pointer = seat.get_pointer(qh, *data);
                state.pointers.insert(data.name, pointer);
            }
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                state.keyboard_capable_seats.insert(data.name);
                state.ensure_data_device(data.name, qh);
            } else {
                state.keyboard_capable_seats.remove(&data.name);
                state.data_devices.remove(&data.name);
            }
        }
    }
}

impl Dispatch<WlPointer, SeatData> for WaylandDragState {
    fn event(
        state: &mut Self,
        _pointer: &WlPointer,
        event: wl_pointer::Event,
        data: &SeatData,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_pointer::Event::Button {
            serial,
            state: WEnum::Value(wl_pointer::ButtonState::Pressed),
            ..
        } = event
        {
            state.last_button_serial = Some((data.name, serial));
        }
    }
}

impl Dispatch<WlDataDeviceManager, ()> for WaylandDragState {
    fn event(
        _state: &mut Self,
        _manager: &WlDataDeviceManager,
        _event: wl_data_device_manager::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDevice, SeatData> for WaylandDragState {
    wayland_client::event_created_child!(WaylandDragState, WlDataDevice, [
        0 => (WlDataOffer, OfferData)
    ]);

    fn event(
        state: &mut Self,
        _device: &WlDataDevice,
        event: wl_data_device::Event,
        _data: &SeatData,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Iced's Smithay clipboard is the sole inbound file-drop receiver.
        // This bridge only owns the seat serial and source of outbound drags.
        // Discard its mirrored offers without requesting data or queuing a
        // second copy operation.
        match event {
            wl_data_device::Event::DataOffer { id } => state.ignored_offers.push(id),
            wl_data_device::Event::Enter {
                id: Some(offer), ..
            }
            | wl_data_device::Event::Selection { id: Some(offer) } => state.discard_offer(offer),
            _ => {}
        }
    }
}

impl Dispatch<WlDataOffer, OfferData> for WaylandDragState {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        _: wl_data_offer::Event,
        _: &OfferData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataSource, DragPayload> for WaylandDragState {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        data: &DragPayload,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Target { mime_type } => {
                crate::utils::log::info(format!(
                    "Wayland external drag target requested {:?}",
                    mime_type
                ));
            }
            wl_data_source::Event::Send { mime_type, fd } => {
                crate::utils::log::info(format!("Wayland external drag is sending {mime_type}"));
                if let Some(text) = data.data_for_mime(&mime_type) {
                    let file = fs::File::from(fd);
                    let nonblocking = rustix::fs::fcntl_getfl(&file).and_then(|flags| {
                        rustix::fs::fcntl_setfl(&file, flags | rustix::fs::OFlags::NONBLOCK)
                    });
                    if nonblocking.is_ok() && state.pending_writes.len() < MAX_PENDING_WRITES {
                        state.pending_writes.push(PendingDragWrite {
                            file,
                            contents: text,
                            written: 0,
                            last_progress: Instant::now(),
                        });
                    }
                }
            }
            wl_data_source::Event::Cancelled => {
                crate::utils::log::info("Wayland external drag cancelled");
                state.finish_source(source);
            }
            wl_data_source::Event::DndFinished => {
                crate::utils::log::info("Wayland external drag finished");
                state.finish_source(source);
            }
            _ => {}
        }
    }
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

fn file_uri(path: &Path) -> Result<String> {
    let path = fs::canonicalize(path).map_err(BExplorerError::Io)?;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        match *byte {
            b'/' => uri.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(*byte as char)
            }
            value => uri.push_str(&format!("%{value:02X}")),
        }
    }
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::pipe::{PipeFlags, pipe_with};

    #[test]
    fn blocked_drag_receiver_cannot_block_the_ui_thread() {
        let (_reader, writer) = pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK).unwrap();
        let mut state = WaylandDragState::default();
        state.pending_writes.push(PendingDragWrite {
            file: fs::File::from(writer),
            contents: "x".repeat(1024 * 1024).into(),
            written: 0,
            last_progress: Instant::now(),
        });
        let started = Instant::now();
        state.poll_pending_writes();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(state.pending_writes.len(), 1);
        state.pending_writes[0].last_progress = Instant::now() - DRAG_WRITE_TIMEOUT;
        state.poll_pending_writes();
        assert!(state.pending_writes.is_empty());
    }

    #[test]
    fn nonblocking_drag_write_preserves_all_bytes() {
        use std::io::Read;
        let (reader, writer) = pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK).unwrap();
        let mut reader = fs::File::from(reader);
        let mut state = WaylandDragState::default();
        state.pending_writes.push(PendingDragWrite {
            file: fs::File::from(writer),
            contents: "file:///tmp/test\r\n".into(),
            written: 0,
            last_progress: Instant::now(),
        });
        state.poll_pending_writes();
        assert!(state.pending_writes.is_empty());
        // The writer was closed on completion, so this does not wait for EOF.
        let mut data = Vec::new();
        reader.read_to_end(&mut data).unwrap();
        assert_eq!(data, b"file:///tmp/test\r\n");
    }
}

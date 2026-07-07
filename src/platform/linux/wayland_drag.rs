use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use rustix::pipe::{PipeFlags, pipe_with};
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

const FILE_DROP_MIMES: &[&str] = &["text/uri-list", "x-special/gnome-copied-files"];

thread_local! {
    static CONTEXT: RefCell<Option<WaylandDragContext>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct WaylandDragResult {
    pub paths: Vec<PathBuf>,
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
    pointers: HashMap<u32, WlPointer>,
    data_devices: HashMap<u32, WlDataDevice>,
    active_sources: Vec<WlDataSource>,
    active_offer: Option<WlDataOffer>,
    selection_offer: Option<WlDataOffer>,
    undetermined_offers: Vec<WlDataOffer>,
    pending_reads: Vec<PendingDropRead>,
    pending_drops: Vec<Vec<PathBuf>>,
    last_button_serial: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Debug)]
struct SeatData {
    name: u32,
}

#[derive(Clone, Debug)]
struct DragPayload {
    uri_list: String,
    gnome_copied_files: String,
}

#[derive(Debug, Default)]
struct OfferData {
    mime_types: Mutex<Vec<String>>,
    state: Mutex<OfferState>,
}

#[derive(Debug)]
enum OfferState {
    Undetermined { source_actions: DndAction },
    Drag(DragOfferState),
    Selection,
}

impl Default for OfferState {
    fn default() -> Self {
        Self::Undetermined {
            source_actions: DndAction::empty(),
        }
    }
}

#[derive(Debug)]
struct DragOfferState {
    serial: u32,
    source_actions: DndAction,
    selected_action: DndAction,
    accepted_mime: Option<String>,
    dropped: bool,
    left: bool,
}

struct PendingDropRead {
    offer: WlDataOffer,
    receiver: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    started_at: Instant,
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
            *context = WaylandDragContext::new(display_ptr, surface_ptr).ok();
        }
        if let Some(context) = context.as_mut() {
            let _ = context.dispatch_pending();
        }
    });
}

pub fn start_file_drag(
    paths: Vec<PathBuf>,
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Result<WaylandDragResult> {
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

pub fn take_received_file_drops(
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Result<(Vec<Vec<PathBuf>>, bool)> {
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
        Ok(context.take_received_file_drops())
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
        if self.state.poll_pending_reads() {
            let _ = self.connection.flush();
        }
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| BExplorerError::Shell(format!("Wayland dispatch failed: {error}")))?;
        let _ = self.connection.flush();
        if self.state.poll_pending_reads() {
            let _ = self.connection.flush();
        }
        Ok(())
    }

    fn take_received_file_drops(&mut self) -> (Vec<Vec<PathBuf>>, bool) {
        if self.state.poll_pending_reads() {
            let _ = self.connection.flush();
        }
        (
            std::mem::take(&mut self.state.pending_drops),
            self.state.has_pending_reads(),
        )
    }

    fn start_file_drag(&mut self, paths: Vec<PathBuf>) -> Result<WaylandDragResult> {
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
        Ok(WaylandDragResult { paths })
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

    fn enter_offer(&mut self, offer: WlDataOffer, serial: u32) {
        self.destroy_active_offer();
        self.remove_undetermined_offer(&offer);
        if self
            .selection_offer
            .as_ref()
            .is_some_and(|selection| *selection == offer)
        {
            self.selection_offer = None;
        }
        set_offer_drag(&offer, serial);
        if offer.version() >= 3 {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }
        self.active_offer = Some(offer);
        crate::utils::log::info(format!("Wayland native drop entered with serial {serial}"));
        self.accept_active_offer();
    }

    fn destroy_active_offer(&mut self) {
        if let Some(offer) = self.active_offer.take() {
            crate::utils::log::info(format!(
                "Wayland native drop active offer destroyed: {}",
                offer_debug_id(&offer)
            ));
            if offer.is_alive() {
                offer.destroy();
            }
        }
    }

    fn set_selection_offer(&mut self, offer: Option<WlDataOffer>) {
        if let Some(offer) = offer.as_ref() {
            self.remove_undetermined_offer(offer);
            set_offer_selection(offer);
        }
        if let Some(previous) = self.selection_offer.take()
            && offer.as_ref() != Some(&previous)
            && previous.is_alive()
        {
            previous.destroy();
        }
        if let Some(offer) = offer.as_ref() {
            crate::utils::log::info(format!(
                "Wayland selection offer stored for later use: {}",
                offer_debug_id(offer)
            ));
        }
        self.selection_offer = offer;
    }

    fn track_data_offer(&mut self, offer: WlDataOffer) {
        if self
            .undetermined_offers
            .iter()
            .any(|candidate| candidate == &offer)
        {
            return;
        }
        crate::utils::log::info(format!(
            "Wayland data offer announced: {}",
            offer_debug_id(&offer)
        ));
        self.undetermined_offers.push(offer);
    }

    fn remove_undetermined_offer(&mut self, offer: &WlDataOffer) {
        self.undetermined_offers
            .retain(|candidate| candidate != offer);
    }

    fn accept_active_offer(&mut self) {
        let Some(offer) = self.active_offer.as_ref() else {
            return;
        };
        accept_offer_as_copy(offer);
    }

    fn receive_active_offer(&mut self, connection: &Connection) {
        let Some(offer) = self.active_offer.take() else {
            crate::utils::log::info("Wayland drop event arrived without an active offer");
            return;
        };
        mark_offer_dropped(&offer);
        let Some(mime_type) = file_drop_mime(&offer) else {
            crate::utils::log::info("Wayland drop offer did not advertise file URI MIME types");
            if offer.is_alive() {
                offer.destroy();
            }
            return;
        };

        if let Some(serial) = offer_drag_serial(&offer) {
            offer.accept(serial, Some(mime_type.clone()));
        }
        if offer.version() >= 3 {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }

        match receive_offer_pipe(&offer, mime_type) {
            Ok(file) => {
                crate::utils::log::info("Wayland native drop receive started");
                let _ = connection.flush();
                let (sender, receiver) = mpsc::channel();
                thread::spawn(move || {
                    let mut file = file;
                    let mut data = Vec::new();
                    let result = file.read_to_end(&mut data).map(|_| data);
                    let _ = sender.send(result);
                });
                self.pending_reads.push(PendingDropRead {
                    offer,
                    receiver,
                    started_at: Instant::now(),
                });
            }
            Err(error) => {
                crate::utils::log::error(format!("Wayland drop receive failed: {error}"));
                if offer.is_alive() {
                    offer.destroy();
                }
            }
        }
    }

    fn leave_active_offer(&mut self) {
        if let Some(offer) = self.active_offer.take() {
            let should_destroy = mark_offer_left(&offer);
            if should_destroy && offer.is_alive() {
                offer.destroy();
            }
        }
    }

    fn poll_pending_reads(&mut self) -> bool {
        let mut finalized_offer = false;
        let mut index = 0;
        while index < self.pending_reads.len() {
            if self.pending_reads[index].started_at.elapsed() > Duration::from_secs(5) {
                let read = self.pending_reads.remove(index);
                crate::utils::log::error("Wayland drop read timed out");
                finish_drop_offer(&read.offer);
                finalized_offer = true;
                continue;
            }

            match self.pending_reads[index].receiver.try_recv() {
                Ok(Ok(data)) => {
                    let read = self.pending_reads.remove(index);
                    finish_drop_offer(&read.offer);
                    finalized_offer = true;
                    let text = String::from_utf8_lossy(&data);
                    let paths = paths_from_uri_list(&text);
                    if !paths.is_empty() {
                        crate::utils::log::info(format!(
                            "Wayland native drop received {} file path(s)",
                            paths.len()
                        ));
                        self.pending_drops.push(paths);
                    } else {
                        crate::utils::log::info(
                            "Wayland native drop data did not contain local paths",
                        );
                    }
                }
                Ok(Err(error)) => {
                    let read = self.pending_reads.remove(index);
                    crate::utils::log::error(format!("Wayland drop read failed: {error}"));
                    finish_drop_offer(&read.offer);
                    finalized_offer = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    index += 1;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let read = self.pending_reads.remove(index);
                    crate::utils::log::error("Wayland drop read worker disconnected");
                    finish_drop_offer(&read.offer);
                    finalized_offer = true;
                }
            }
        }
        finalized_offer
    }

    fn has_pending_reads(&self) -> bool {
        !self.pending_reads.is_empty()
    }

    fn ensure_data_device(&mut self, seat_name: u32, qh: &QueueHandle<Self>) {
        if self.data_devices.contains_key(&seat_name) {
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
    }
}

impl DragPayload {
    fn from_paths(paths: &[PathBuf]) -> Result<Self> {
        let mut uri_list = String::new();
        let mut gnome_copied_files = String::from("copy\n");
        for path in paths {
            let uri = file_uri(path)?;
            uri_list.push_str(&uri);
            uri_list.push_str("\r\n");
            gnome_copied_files.push_str(&uri);
            gnome_copied_files.push('\n');
        }
        Ok(Self {
            uri_list,
            gnome_copied_files,
        })
    }

    fn data_for_mime(&self, mime: &str) -> Option<&str> {
        match mime {
            "text/uri-list" => Some(&self.uri_list),
            "x-special/gnome-copied-files" => Some(&self.gnome_copied_files),
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
                    state.ensure_data_device(name, qh);
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
            state.ensure_data_device(data.name, qh);
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
        0 => (WlDataOffer, OfferData::default())
    ]);

    fn event(
        state: &mut Self,
        _device: &WlDataDevice,
        event: wl_data_device::Event,
        _data: &SeatData,
        connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::DataOffer { id } => {
                state.track_data_offer(id);
            }
            wl_data_device::Event::Enter { serial, id, .. } => {
                if let Some(offer) = id {
                    state.enter_offer(offer, serial);
                } else {
                    crate::utils::log::info(format!(
                        "Wayland native drop entered without data offer, serial {serial}"
                    ));
                }
            }
            wl_data_device::Event::Motion { .. } => {
                state.accept_active_offer();
            }
            wl_data_device::Event::Drop => {
                crate::utils::log::info("Wayland native drop event received");
                state.receive_active_offer(connection);
            }
            wl_data_device::Event::Leave => {
                crate::utils::log::info("Wayland native drop leave event received");
                state.leave_active_offer();
            }
            wl_data_device::Event::Selection { id } => {
                state.set_selection_offer(id);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlDataOffer, OfferData> for WaylandDragState {
    fn event(
        state: &mut Self,
        offer: &WlDataOffer,
        event: wl_data_offer::Event,
        data: &OfferData,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_offer::Event::Offer { mime_type } => {
                if let Ok(mut mime_types) = data.mime_types.lock() {
                    crate::utils::log::info(format!(
                        "Wayland offer {} advertised MIME {mime_type}",
                        offer_debug_id(offer)
                    ));
                    mime_types.push(mime_type);
                    if state
                        .active_offer
                        .as_ref()
                        .is_some_and(|active| active == offer)
                    {
                        drop(mime_types);
                        state.accept_active_offer();
                    }
                }
            }
            wl_data_offer::Event::Action {
                dnd_action: WEnum::Value(action),
            } => {
                crate::utils::log::info(format!(
                    "Wayland offer {} selected action {:?}",
                    offer_debug_id(offer),
                    action
                ));
                set_offer_selected_action(offer, action);
            }
            wl_data_offer::Event::SourceActions {
                source_actions: WEnum::Value(actions),
            } => {
                crate::utils::log::info(format!(
                    "Wayland offer {} source actions {:?}",
                    offer_debug_id(offer),
                    actions
                ));
                set_offer_source_actions(offer, actions);
                if state
                    .active_offer
                    .as_ref()
                    .is_some_and(|active| active == offer)
                    && actions.contains(DndAction::Copy)
                    && offer.version() >= 3
                {
                    offer.set_actions(DndAction::Copy, DndAction::Copy);
                    crate::utils::log::info("Wayland native drop confirmed copy action");
                    state.accept_active_offer();
                }
            }
            _ => {}
        }
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
            wl_data_source::Event::Send { mime_type, fd } => {
                if let Some(text) = data.data_for_mime(&mime_type) {
                    let mut file = fs::File::from(fd);
                    let _ = file.write_all(text.as_bytes());
                }
            }
            wl_data_source::Event::Cancelled => {
                state.finish_source(source);
            }
            wl_data_source::Event::DndFinished => {
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

fn receive_offer_pipe(offer: &WlDataOffer, mime_type: String) -> std::io::Result<fs::File> {
    let (readfd, writefd) = pipe_with(PipeFlags::CLOEXEC)?;
    offer.receive(mime_type, writefd.as_fd());
    drop(writefd);
    Ok(fs::File::from(readfd))
}

fn set_offer_drag(offer: &WlDataOffer, serial: u32) {
    let Some(data) = offer.data::<OfferData>() else {
        return;
    };
    let Ok(mut state) = data.state.lock() else {
        return;
    };
    let source_actions = match &*state {
        OfferState::Undetermined { source_actions } => *source_actions,
        OfferState::Drag(drag) => drag.source_actions,
        OfferState::Selection => DndAction::empty(),
    };
    *state = OfferState::Drag(DragOfferState {
        serial,
        source_actions,
        selected_action: DndAction::empty(),
        accepted_mime: None,
        dropped: false,
        left: false,
    });
}

fn set_offer_selection(offer: &WlDataOffer) {
    let Some(data) = offer.data::<OfferData>() else {
        return;
    };
    if let Ok(mut state) = data.state.lock() {
        *state = OfferState::Selection;
    }
}

fn set_offer_source_actions(offer: &WlDataOffer, actions: DndAction) {
    let Some(data) = offer.data::<OfferData>() else {
        return;
    };
    let Ok(mut state) = data.state.lock() else {
        return;
    };
    match &mut *state {
        OfferState::Undetermined { source_actions } => *source_actions = actions,
        OfferState::Drag(drag) => drag.source_actions = actions,
        OfferState::Selection => {}
    }
}

fn set_offer_selected_action(offer: &WlDataOffer, action: DndAction) {
    let Some(data) = offer.data::<OfferData>() else {
        return;
    };
    let Ok(mut state) = data.state.lock() else {
        return;
    };
    if let OfferState::Drag(drag) = &mut *state {
        drag.selected_action = action;
    }
}

fn mark_offer_dropped(offer: &WlDataOffer) {
    let Some(data) = offer.data::<OfferData>() else {
        return;
    };
    let Ok(mut state) = data.state.lock() else {
        return;
    };
    if let OfferState::Drag(drag) = &mut *state {
        drag.dropped = true;
    }
}

fn mark_offer_left(offer: &WlDataOffer) -> bool {
    let Some(data) = offer.data::<OfferData>() else {
        return false;
    };
    let Ok(mut state) = data.state.lock() else {
        return false;
    };
    match &mut *state {
        OfferState::Drag(drag) => {
            drag.left = true;
            !drag.dropped
        }
        _ => false,
    }
}

fn offer_drag_serial(offer: &WlDataOffer) -> Option<u32> {
    offer.data::<OfferData>().and_then(|data| {
        data.state.lock().ok().and_then(|state| match &*state {
            OfferState::Drag(drag) => Some(drag.serial),
            _ => None,
        })
    })
}

fn offer_accepted_mime(offer: &WlDataOffer) -> Option<String> {
    offer.data::<OfferData>().and_then(|data| {
        data.state.lock().ok().and_then(|state| match &*state {
            OfferState::Drag(drag) => drag.accepted_mime.clone(),
            _ => None,
        })
    })
}

fn set_offer_accepted_mime(offer: &WlDataOffer, mime_type: String) {
    let Some(data) = offer.data::<OfferData>() else {
        return;
    };
    let Ok(mut state) = data.state.lock() else {
        return;
    };
    if let OfferState::Drag(drag) = &mut *state {
        drag.accepted_mime = Some(mime_type);
    }
}

fn accept_offer_as_copy(offer: &WlDataOffer) {
    let Some(mime_type) = file_drop_mime(offer) else {
        return;
    };
    let Some(serial) = offer_drag_serial(offer) else {
        return;
    };
    let already_accepted = offer_accepted_mime(offer).as_deref() == Some(mime_type.as_str());
    offer.accept(serial, Some(mime_type.clone()));
    if offer.version() >= 3 {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }
    if already_accepted {
        return;
    }
    set_offer_accepted_mime(offer, mime_type.clone());
    crate::utils::log::info(format!("Wayland native drop accepted MIME {mime_type}"));
}

fn finish_drop_offer(offer: &WlDataOffer) {
    if !offer.is_alive() {
        return;
    }
    if offer.version() >= 3 {
        if offer_selected_action_is_copy(offer) {
            offer.finish();
        } else {
            crate::utils::log::info("Wayland native drop finished without negotiated copy action");
        }
    }
    if offer.is_alive() {
        offer.destroy();
    }
}

fn offer_selected_action_is_copy(offer: &WlDataOffer) -> bool {
    offer
        .data::<OfferData>()
        .and_then(|data| {
            data.state.lock().ok().and_then(|state| match &*state {
                OfferState::Drag(drag) => Some(drag.selected_action),
                _ => None,
            })
        })
        .is_some_and(|action| action.contains(DndAction::Copy))
}

fn offer_debug_id(offer: &WlDataOffer) -> String {
    format!("{:?}", offer.id())
}

fn file_drop_mime(offer: &WlDataOffer) -> Option<String> {
    let data = offer.data::<OfferData>()?;
    let mime_types = data.mime_types.lock().ok()?;
    FILE_DROP_MIMES
        .iter()
        .find(|mime| mime_types.iter().any(|offered| offered == **mime))
        .map(|mime| (*mime).to_string())
}

fn paths_from_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.eq_ignore_ascii_case("copy")
                || line.eq_ignore_ascii_case("cut")
            {
                return None;
            }
            crate::platform::shell::path_from_file_uri(line).filter(|path| path.exists())
        })
        .collect()
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

    #[test]
    fn parses_text_uri_list_drop() {
        assert_eq!(
            paths_from_uri_list("# comment\r\nfile:///tmp\r\n"),
            vec![PathBuf::from("/tmp")]
        );
    }

    #[test]
    fn parses_gnome_copied_files_drop() {
        assert_eq!(
            paths_from_uri_list("copy\nfile:///tmp\n"),
            vec![PathBuf::from("/tmp")]
        );
    }
}

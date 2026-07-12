use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, OnceLock};

use raw_window_handle::{RawWindowHandle, WindowHandle};
use windows::Win32::Devices::PortableDevices::GUID_DEVINTERFACE_WPD;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Ioctl::{GUID_DEVINTERFACE_DISK, GUID_DEVINTERFACE_VOLUME};
use windows::Win32::UI::WindowsAndMessaging::{
    DBT_DEVTYP_DEVICEINTERFACE, DEV_BROADCAST_DEVICEINTERFACE_W, DEVICE_NOTIFY_WINDOW_HANDLE,
    RegisterDeviceNotificationW,
};

use crate::utils::errors::Result;

static STORAGE_CHANGE_SENDER: OnceLock<Mutex<Option<SyncSender<()>>>> = OnceLock::new();
static REGISTERED_WINDOW: AtomicIsize = AtomicIsize::new(0);

pub fn storage_change_receiver() -> Receiver<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    if let Ok(mut current) = STORAGE_CHANGE_SENDER
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *current = Some(sender);
    }
    receiver
}

pub fn install_storage_change_notifications(handle: &WindowHandle<'_>) -> Result<()> {
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Ok(());
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let hwnd_value = hwnd.0 as isize;
    if REGISTERED_WINDOW
        .compare_exchange(0, hwnd_value, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }

    let registration = [
        GUID_DEVINTERFACE_DISK,
        GUID_DEVINTERFACE_VOLUME,
        GUID_DEVINTERFACE_WPD,
    ]
    .into_iter()
    .try_for_each(|class_guid| {
        let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_classguid: class_guid,
            ..DEV_BROADCAST_DEVICEINTERFACE_W::default()
        };
        unsafe {
            RegisterDeviceNotificationW(
                HANDLE(hwnd.0),
                std::ptr::from_ref(&filter).cast(),
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        }
        .map(|_| ())
    });

    if let Err(error) = registration {
        REGISTERED_WINDOW.store(0, Ordering::Release);
        return Err(crate::utils::errors::BExplorerError::Shell(
            error.to_string(),
        ));
    }
    Ok(())
}

pub(super) fn notify_storage_change() {
    let Some(sender) = STORAGE_CHANGE_SENDER.get() else {
        return;
    };
    let Ok(sender) = sender.lock() else {
        return;
    };
    if let Some(sender) = sender.as_ref() {
        let _ = sender.try_send(());
    }
}

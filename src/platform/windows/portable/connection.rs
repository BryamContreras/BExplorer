use super::{portable_retry_delay, wide_null, windows_error_is_resource_busy};
use windows::Win32::Devices::PortableDevices::{
    IPortableDevice, IPortableDeviceValues, PortableDeviceFTM, PortableDeviceValues,
    WPD_CLIENT_EVENT_COOKIE, WPD_CLIENT_MAJOR_VERSION, WPD_CLIENT_MINOR_VERSION, WPD_CLIENT_NAME,
    WPD_CLIENT_REVISION, WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE,
};
use windows::Win32::Storage::FileSystem::SECURITY_IMPERSONATION;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::PCWSTR;

// WPD Shell Namespace Extension cookie. Matching the shell's operation cookie
// avoids its synchronous rescan after our delete event, which can stall all
// MTP clients until the shell times out. Also used by Far Manager's PortaDev:
// https://svn.code.sf.net/p/farplugs/code/PortaDev/wpd.cpp
const SHELL_EVENT_COOKIE: &str = "{35786D3C-B075-49B9-88DD-029876E11C01}";

#[cfg(test)]
pub(super) static CONNECTION_OPENS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub(super) fn open_device(device_id: &str) -> windows::core::Result<IPortableDevice> {
    for attempt in 0..4 {
        let result = unsafe {
            (|| -> windows::core::Result<IPortableDevice> {
                let device: IPortableDevice =
                    CoCreateInstance(&PortableDeviceFTM, None, CLSCTX_INPROC_SERVER)?;
                let info: IPortableDeviceValues =
                    CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
                let name = wide_null("BExplorer");
                info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(name.as_ptr()))?;
                info.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1)?;
                info.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0)?;
                info.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0)?;
                info.SetUnsignedIntegerValue(
                    &WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE,
                    SECURITY_IMPERSONATION.0,
                )?;
                let event_cookie = wide_null(SHELL_EVENT_COOKIE);
                info.SetStringValue(&WPD_CLIENT_EVENT_COOKIE, PCWSTR(event_cookie.as_ptr()))?;
                let id = wide_null(device_id);
                device.Open(PCWSTR(id.as_ptr()), &info)?;
                #[cfg(test)]
                CONNECTION_OPENS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(device)
            })()
        };
        match result {
            Err(error) if windows_error_is_resource_busy(&error) && attempt < 3 => {
                std::thread::sleep(portable_retry_delay(attempt));
            }
            result => return result,
        }
    }
    unreachable!("the last connection attempt returns its result")
}

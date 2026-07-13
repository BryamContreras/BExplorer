use super::util::{pwstr_to_string, wide_null, wide_to_string};

#[cfg(target_os = "windows")]
static PORTABLE_DEVICE_ACCESS: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortableDeviceInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub description: String,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortableObjectInfo {
    pub id: String,
    pub name: String,
    pub is_folder: bool,
    pub size: Option<u64>,
}

#[cfg(target_os = "windows")]
pub fn portable_devices() -> Vec<PortableDeviceInfo> {
    use windows::Win32::Devices::PortableDevices::{IPortableDeviceManager, PortableDeviceManager};
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows::core::{PCWSTR, PWSTR};

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let should_uninitialize = hr == S_OK || hr.0 == 1;

        let result = (|| -> windows::core::Result<Vec<PortableDeviceInfo>> {
            let manager: IPortableDeviceManager =
                CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER)?;
            manager.RefreshDeviceList()?;

            let mut count = 0_u32;
            manager.GetDevices(std::ptr::null_mut(), &mut count)?;
            if count == 0 {
                return Ok(Vec::new());
            }

            let mut ids = vec![PWSTR::null(); count as usize];
            manager.GetDevices(ids.as_mut_ptr(), &mut count)?;

            let mut devices = Vec::new();
            for id in ids.into_iter().take(count as usize) {
                if id.is_null() {
                    continue;
                }
                let id_string = pwstr_to_string(id);
                let name =
                    portable_device_string(&manager, PCWSTR(id.0), PortableDeviceField::Name)
                        .unwrap_or_else(|| "Portable device".to_string());
                let manufacturer = portable_device_string(
                    &manager,
                    PCWSTR(id.0),
                    PortableDeviceField::Manufacturer,
                )
                .unwrap_or_default();
                let description = portable_device_string(
                    &manager,
                    PCWSTR(id.0),
                    PortableDeviceField::Description,
                )
                .unwrap_or_default();
                devices.push(PortableDeviceInfo {
                    id: id_string,
                    name,
                    manufacturer,
                    description,
                });
                CoTaskMemFree(Some(id.0 as *const _));
            }

            devices.sort_by_key(|device| device.name.to_lowercase());
            Ok(devices)
        })()
        .unwrap_or_else(|error| {
            crate::utils::log::error(format!("Portable device scan failed: {error}"));
            Vec::new()
        });

        if should_uninitialize {
            CoUninitialize();
        }
        result
    }
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn portable_device_objects(device_id: &str, parent_object_id: &str) -> Vec<PortableObjectInfo> {
    use windows::Win32::Devices::PortableDevices::{
        IPortableDevice, IPortableDeviceKeyCollection, IPortableDeviceValues, PortableDevice,
        PortableDeviceKeyCollection, PortableDeviceValues, WPD_CLIENT_MAJOR_VERSION,
        WPD_CLIENT_MINOR_VERSION, WPD_CLIENT_NAME, WPD_CLIENT_REVISION,
        WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE, WPD_CONTENT_TYPE_FOLDER,
        WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT, WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_NAME,
        WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_SIZE,
    };
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows::core::{PCWSTR, PWSTR};

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let should_uninitialize = hr == S_OK || hr.0 == 1;

        let result = (|| -> windows::core::Result<Vec<PortableObjectInfo>> {
            let device: IPortableDevice =
                CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
            let client_info: IPortableDeviceValues =
                CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;

            let client_name = wide_null("BExplorer");
            client_info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(client_name.as_ptr()))?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE, 2)?;

            let device_wide = wide_null(device_id);
            device.Open(PCWSTR(device_wide.as_ptr()), &client_info)?;

            let content = device.Content()?;
            let filter: IPortableDeviceValues =
                CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
            let parent_id = if parent_object_id.trim().is_empty() {
                "DEVICE"
            } else {
                parent_object_id
            };
            let parent_wide = wide_null(parent_id);
            let enumerator = content.EnumObjects(0, PCWSTR(parent_wide.as_ptr()), &filter)?;

            let properties = content.Properties()?;
            let keys: IPortableDeviceKeyCollection =
                CoCreateInstance(&PortableDeviceKeyCollection, None, CLSCTX_INPROC_SERVER)?;
            keys.Add(&WPD_OBJECT_NAME)?;
            keys.Add(&WPD_OBJECT_ORIGINAL_FILE_NAME)?;
            keys.Add(&WPD_OBJECT_CONTENT_TYPE)?;
            keys.Add(&WPD_OBJECT_SIZE)?;

            let mut objects = Vec::new();
            loop {
                let mut ids = [PWSTR::null(); 16];
                let mut fetched = 0_u32;
                enumerator.Next(&mut ids, &mut fetched).ok()?;
                if fetched == 0 {
                    break;
                }

                for id in ids.into_iter().take(fetched as usize) {
                    if id.is_null() {
                        continue;
                    }

                    let id_string = pwstr_to_string(id);
                    let object_wide = wide_null(&id_string);
                    let values = match properties.GetValues(PCWSTR(object_wide.as_ptr()), &keys) {
                        Ok(values) => values,
                        Err(error) => {
                            crate::utils::log::error(format!(
                                "Portable object properties failed: {error}"
                            ));
                            CoTaskMemFree(Some(id.0 as *const _));
                            continue;
                        }
                    };

                    let name = portable_value_string(&values, &WPD_OBJECT_NAME)
                        .or_else(|| portable_value_string(&values, &WPD_OBJECT_ORIGINAL_FILE_NAME))
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| id_string.clone());
                    let content_type = values.GetGuidValue(&WPD_OBJECT_CONTENT_TYPE).ok();
                    let is_folder = content_type.is_some_and(|content_type| {
                        content_type == WPD_CONTENT_TYPE_FOLDER
                            || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT
                    });
                    let size = values.GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE).ok();

                    objects.push(PortableObjectInfo {
                        id: id_string,
                        name,
                        is_folder,
                        size,
                    });

                    CoTaskMemFree(Some(id.0 as *const _));
                }
            }

            let _ = device.Close();
            objects.sort_by_key(|object| object.name.to_lowercase());
            Ok(objects)
        })()
        .unwrap_or_else(|error| {
            crate::utils::log::error(format!("Portable object scan failed: {error}"));
            Vec::new()
        });

        if should_uninitialize {
            CoUninitialize();
        }
        result
    }
}

#[cfg(target_os = "windows")]
pub fn portable_device_thumbnail(
    device_id: &str,
    object_id: &str,
    max_bytes: usize,
    allow_default_resource: bool,
) -> Option<Vec<u8>> {
    use windows::Win32::Devices::PortableDevices::{
        IPortableDevice, IPortableDeviceValues, PortableDevice, PortableDeviceValues,
        WPD_CLIENT_MAJOR_VERSION, WPD_CLIENT_MINOR_VERSION, WPD_CLIENT_NAME, WPD_CLIENT_REVISION,
        WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE, WPD_RESOURCE_DEFAULT, WPD_RESOURCE_THUMBNAIL,
    };
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows::core::PCWSTR;

    let _access_guard = portable_access_lock();

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let should_uninitialize = hr == S_OK || hr.0 == 1;

        let result = (|| -> windows::core::Result<Option<Vec<u8>>> {
            let device: IPortableDevice =
                CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
            let client_info: IPortableDeviceValues =
                CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;

            let client_name = wide_null("BExplorer");
            client_info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(client_name.as_ptr()))?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE, 2)?;

            let device_wide = wide_null(device_id);
            device.Open(PCWSTR(device_wide.as_ptr()), &client_info)?;

            let content = device.Content()?;
            let resources = content.Transfer()?;
            let object_wide = wide_null(object_id);
            let object_pcwstr = PCWSTR(object_wide.as_ptr());

            let bytes = read_portable_resource(
                &resources,
                object_pcwstr,
                &WPD_RESOURCE_THUMBNAIL,
                max_bytes,
            )
            .or_else(|| {
                allow_default_resource.then(|| {
                    read_portable_resource(
                        &resources,
                        object_pcwstr,
                        &WPD_RESOURCE_DEFAULT,
                        max_bytes,
                    )
                })?
            });

            let _ = device.Close();
            Ok(bytes)
        })()
        .unwrap_or_else(|error| {
            crate::utils::log::error(format!("Portable thumbnail load failed: {error}"));
            None
        });

        if should_uninitialize {
            CoUninitialize();
        }
        result
    }
}

#[cfg(target_os = "windows")]
pub struct PortableDeviceSession {
    device: windows::Win32::Devices::PortableDevices::IPortableDevice,
    content: windows::Win32::Devices::PortableDevices::IPortableDeviceContent,
    should_uninitialize: bool,
    _access_guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(target_os = "windows")]
impl PortableDeviceSession {
    pub fn open(device_id: &str) -> crate::utils::errors::Result<Self> {
        use windows::Win32::Devices::PortableDevices::{
            IPortableDevice, IPortableDeviceValues, PortableDevice, PortableDeviceValues,
            WPD_CLIENT_MAJOR_VERSION, WPD_CLIENT_MINOR_VERSION, WPD_CLIENT_NAME,
            WPD_CLIENT_REVISION, WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE,
        };
        use windows::Win32::Foundation::S_OK;
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            CoCreateInstance, CoInitializeEx, CoUninitialize,
        };
        use windows::core::PCWSTR;

        let mut access_guard = Some(portable_access_lock());
        let mut last_error = None;
        for attempt in 0..4 {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                let should_uninitialize = hr == S_OK || hr.0 == 1;

                let result = (|| -> crate::utils::errors::Result<Self> {
                    let device: IPortableDevice =
                        CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
                    let client_info: IPortableDeviceValues =
                        CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;

                    let client_name = wide_null("BExplorer");
                    client_info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(client_name.as_ptr()))?;
                    client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1)?;
                    client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0)?;
                    client_info.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0)?;
                    client_info
                        .SetUnsignedIntegerValue(&WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE, 2)?;

                    let device_wide = wide_null(device_id);
                    device.Open(PCWSTR(device_wide.as_ptr()), &client_info)?;
                    let content = device.Content()?;
                    Ok(Self {
                        device,
                        content,
                        should_uninitialize,
                        _access_guard: access_guard.take().expect("portable device lock"),
                    })
                })();

                if result.is_err() && should_uninitialize {
                    CoUninitialize();
                }

                match result {
                    Ok(session) => return Ok(session),
                    Err(error) if portable_error_is_resource_busy(&error) && attempt < 3 => {
                        last_error = Some(error);
                        std::thread::sleep(portable_retry_delay(attempt));
                    }
                    Err(error) => return Err(error),
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            crate::utils::errors::BExplorerError::Operation(
                "Portable device resource is busy".into(),
            )
        }))
    }

    pub fn object_info(&self, object_id: &str) -> crate::utils::errors::Result<PortableObjectInfo> {
        let mut last_error = None;
        for attempt in 0..4 {
            match portable_object_info_from_content(&self.content, object_id) {
                Ok(info) => return Ok(info),
                Err(error) if portable_error_is_resource_busy(&error) && attempt < 3 => {
                    last_error = Some(error);
                    std::thread::sleep(portable_retry_delay(attempt));
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            crate::utils::errors::BExplorerError::Operation(
                "Portable device resource is busy".into(),
            )
        }))
    }

    pub fn list_objects(
        &self,
        parent_object_id: &str,
    ) -> crate::utils::errors::Result<Vec<PortableObjectInfo>> {
        let mut last_error = None;
        for attempt in 0..5 {
            match portable_objects_from_content(&self.content, parent_object_id) {
                Ok(objects) => return Ok(objects),
                Err(error) if portable_error_is_resource_busy(&error) && attempt < 4 => {
                    last_error = Some(error);
                    std::thread::sleep(portable_retry_delay(attempt));
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            crate::utils::errors::BExplorerError::Operation(
                "Portable device resource is busy".into(),
            )
        }))
    }

    pub fn download_file<F>(
        &self,
        object_id: &str,
        target: &std::path::Path,
        mut on_bytes: F,
    ) -> crate::utils::errors::Result<u64>
    where
        F: FnMut(u64) -> crate::utils::errors::Result<()>,
    {
        use std::ffi::c_void;
        use std::io::Write;

        use windows::Win32::Devices::PortableDevices::WPD_RESOURCE_DEFAULT;
        use windows::Win32::System::Com::{IStream, STGM_READ};
        use windows::core::PCWSTR;

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let resources = unsafe { self.content.Transfer()? };
        let object_wide = wide_null(object_id);
        let mut optimal_buffer_size = 0_u32;
        let mut stream: Option<IStream> = None;
        unsafe {
            resources.GetStream(
                PCWSTR(object_wide.as_ptr()),
                &WPD_RESOURCE_DEFAULT,
                STGM_READ.0,
                &mut optimal_buffer_size,
                &mut stream,
            )?;
        }
        let stream = stream.ok_or_else(|| {
            crate::utils::errors::BExplorerError::Operation(
                "Portable device did not return a readable stream".into(),
            )
        })?;

        let chunk_size = portable_chunk_size(optimal_buffer_size);
        let mut buffer = vec![0_u8; chunk_size];
        let mut output = std::fs::File::create(target)?;
        let mut total = 0_u64;

        loop {
            let mut read = 0_u32;
            unsafe {
                stream
                    .Read(
                        buffer.as_mut_ptr() as *mut c_void,
                        buffer.len() as u32,
                        Some(&mut read),
                    )
                    .ok()?;
            }
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read as usize])?;
            total = total.saturating_add(read as u64);
            on_bytes(read as u64)?;
        }
        output.flush()?;
        Ok(total)
    }

    pub fn upload_file<F>(
        &self,
        parent_object_id: &str,
        source: &std::path::Path,
        name: &str,
        mut on_bytes: F,
    ) -> crate::utils::errors::Result<u64>
    where
        F: FnMut(u64) -> crate::utils::errors::Result<()>,
    {
        use std::ffi::c_void;
        use std::io::Read;

        use windows::Win32::Devices::PortableDevices::{
            IPortableDeviceValues, PortableDeviceValues, WPD_CONTENT_TYPE_UNSPECIFIED,
            WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_FORMAT, WPD_OBJECT_FORMAT_UNSPECIFIED,
            WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_PARENT_ID, WPD_OBJECT_SIZE,
        };
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STGC_DEFAULT,
        };
        use windows::core::{PCWSTR, PWSTR};

        let name = name.to_string();
        let size = std::fs::metadata(source)?.len();
        let values: IPortableDeviceValues =
            unsafe { CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)? };
        let parent_wide = wide_null(parent_object_id);
        let name_wide = wide_null(&name);
        unsafe {
            values.SetStringValue(&WPD_OBJECT_PARENT_ID, PCWSTR(parent_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE, size)?;
            values.SetGuidValue(&WPD_OBJECT_CONTENT_TYPE, &WPD_CONTENT_TYPE_UNSPECIFIED)?;
            values.SetGuidValue(&WPD_OBJECT_FORMAT, &WPD_OBJECT_FORMAT_UNSPECIFIED)?;
        }

        let mut optimal_buffer_size = 0_u32;
        let mut stream: Option<IStream> = None;
        let mut cookie = PWSTR::null();
        for attempt in 0..4 {
            let result = unsafe {
                self.content.CreateObjectWithPropertiesAndData(
                    &values,
                    &mut stream,
                    &mut optimal_buffer_size,
                    &mut cookie,
                )
            };
            match result {
                Ok(()) => break,
                Err(error) if windows_error_is_resource_busy(&error) && attempt < 3 => {
                    std::thread::sleep(portable_retry_delay(attempt));
                }
                Err(error) => return Err(error.into()),
            }
        }
        let stream = stream.ok_or_else(|| {
            crate::utils::errors::BExplorerError::Operation(
                "Portable device did not return a writable stream".into(),
            )
        })?;

        let result = (|| {
            let chunk_size = portable_chunk_size(optimal_buffer_size);
            let mut buffer = vec![0_u8; chunk_size];
            let mut input = std::fs::File::open(source)?;
            let mut total = 0_u64;

            loop {
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                let mut offset = 0_usize;
                while offset < read {
                    let mut written = 0_u32;
                    unsafe {
                        stream
                            .Write(
                                buffer[offset..read].as_ptr() as *const c_void,
                                (read - offset) as u32,
                                Some(&mut written),
                            )
                            .ok()?;
                    }
                    if written == 0 {
                        return Err(crate::utils::errors::BExplorerError::Operation(
                            "Portable device stopped accepting data".into(),
                        ));
                    }
                    offset += written as usize;
                    total = total.saturating_add(written as u64);
                    on_bytes(written as u64)?;
                }
            }

            unsafe {
                stream.Commit(STGC_DEFAULT)?;
            }
            Ok(total)
        })();

        if result.is_err() {
            unsafe {
                let _ = stream.Revert();
            }
        }
        if !cookie.is_null() {
            unsafe {
                CoTaskMemFree(Some(cookie.0 as *const _));
            }
        }

        result
    }

    pub fn create_folder(
        &self,
        parent_object_id: &str,
        name: &str,
    ) -> crate::utils::errors::Result<String> {
        use windows::Win32::Devices::PortableDevices::{
            IPortableDeviceValues, PortableDeviceValues, WPD_CONTENT_TYPE_FOLDER,
            WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_FORMAT, WPD_OBJECT_FORMAT_PROPERTIES_ONLY,
            WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_PARENT_ID,
        };
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
        use windows::core::{PCWSTR, PWSTR};

        let name = name.to_string();
        let values: IPortableDeviceValues =
            unsafe { CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)? };
        let parent_wide = wide_null(parent_object_id);
        let name_wide = wide_null(&name);
        unsafe {
            values.SetStringValue(&WPD_OBJECT_PARENT_ID, PCWSTR(parent_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetGuidValue(&WPD_OBJECT_CONTENT_TYPE, &WPD_CONTENT_TYPE_FOLDER)?;
            values.SetGuidValue(&WPD_OBJECT_FORMAT, &WPD_OBJECT_FORMAT_PROPERTIES_ONLY)?;
        }

        let mut object_id = PWSTR::null();
        for attempt in 0..4 {
            let result = unsafe {
                self.content
                    .CreateObjectWithPropertiesOnly(&values, &mut object_id)
            };
            match result {
                Ok(()) => break,
                Err(error) if windows_error_is_resource_busy(&error) && attempt < 3 => {
                    std::thread::sleep(portable_retry_delay(attempt));
                }
                Err(error) => return Err(error.into()),
            }
        }
        let id = pwstr_to_string(object_id);
        if !object_id.is_null() {
            unsafe {
                CoTaskMemFree(Some(object_id.0 as *const _));
            }
        }
        if id.trim().is_empty() {
            Err(crate::utils::errors::BExplorerError::Operation(
                "Portable device did not return the created folder id".into(),
            ))
        } else {
            Ok(id)
        }
    }

    pub fn delete_objects(&self, object_ids: &[String]) -> crate::utils::errors::Result<usize> {
        use windows::Win32::Devices::PortableDevices::{
            IPortableDevicePropVariantCollection, PORTABLE_DEVICE_DELETE_WITH_RECURSION,
            PortableDevicePropVariantCollection,
        };
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
        use windows::core::PROPVARIANT;

        if object_ids.is_empty() {
            return Ok(0);
        }

        let collection: IPortableDevicePropVariantCollection = unsafe {
            CoCreateInstance(
                &PortableDevicePropVariantCollection,
                None,
                CLSCTX_INPROC_SERVER,
            )?
        };
        for object_id in object_ids {
            let value = PROPVARIANT::from(object_id.as_str());
            unsafe {
                collection.Add(&value)?;
            }
        }

        let mut results = None;
        unsafe {
            self.content.Delete(
                PORTABLE_DEVICE_DELETE_WITH_RECURSION.0 as u32,
                &collection,
                &mut results,
            )?;
        }
        std::thread::sleep(std::time::Duration::from_millis(180));
        Ok(object_ids.len())
    }
}

#[cfg(target_os = "windows")]
impl Drop for PortableDeviceSession {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.Close();
            if self.should_uninitialize {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn portable_device_objects_result(
    device_id: &str,
    parent_object_id: &str,
) -> crate::utils::errors::Result<Vec<PortableObjectInfo>> {
    let session = PortableDeviceSession::open(device_id)?;
    session.list_objects(parent_object_id)
}

#[cfg(target_os = "windows")]
pub fn portable_device_object_info(
    device_id: &str,
    object_id: &str,
) -> crate::utils::errors::Result<PortableObjectInfo> {
    let session = PortableDeviceSession::open(device_id)?;
    session.object_info(object_id)
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn portable_download_file<F>(
    device_id: &str,
    object_id: &str,
    target: &std::path::Path,
    mut on_bytes: F,
) -> crate::utils::errors::Result<u64>
where
    F: FnMut(u64) -> crate::utils::errors::Result<()>,
{
    use std::ffi::c_void;
    use std::io::Write;

    use windows::Win32::Devices::PortableDevices::WPD_RESOURCE_DEFAULT;
    use windows::Win32::System::Com::{IStream, STGM_READ};
    use windows::core::PCWSTR;

    let target = target.to_path_buf();
    with_portable_content(device_id, |content| {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let resources = unsafe { content.Transfer()? };
        let object_wide = wide_null(object_id);
        let mut optimal_buffer_size = 0_u32;
        let mut stream: Option<IStream> = None;
        unsafe {
            resources.GetStream(
                PCWSTR(object_wide.as_ptr()),
                &WPD_RESOURCE_DEFAULT,
                STGM_READ.0,
                &mut optimal_buffer_size,
                &mut stream,
            )?;
        }
        let stream = stream.ok_or_else(|| {
            crate::utils::errors::BExplorerError::Operation(
                "Portable device did not return a readable stream".into(),
            )
        })?;

        let chunk_size = portable_chunk_size(optimal_buffer_size);
        let mut buffer = vec![0_u8; chunk_size];
        let mut output = std::fs::File::create(&target)?;
        let mut total = 0_u64;

        loop {
            let mut read = 0_u32;
            unsafe {
                stream
                    .Read(
                        buffer.as_mut_ptr() as *mut c_void,
                        buffer.len() as u32,
                        Some(&mut read),
                    )
                    .ok()?;
            }
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read as usize])?;
            total = total.saturating_add(read as u64);
            on_bytes(read as u64)?;
        }
        output.flush()?;
        Ok(total)
    })
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn portable_upload_file<F>(
    device_id: &str,
    parent_object_id: &str,
    source: &std::path::Path,
    name: &str,
    mut on_bytes: F,
) -> crate::utils::errors::Result<u64>
where
    F: FnMut(u64) -> crate::utils::errors::Result<()>,
{
    use std::ffi::c_void;
    use std::io::Read;

    use windows::Win32::Devices::PortableDevices::{
        IPortableDeviceValues, PortableDeviceValues, WPD_CONTENT_TYPE_UNSPECIFIED,
        WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_FORMAT, WPD_OBJECT_FORMAT_UNSPECIFIED, WPD_OBJECT_NAME,
        WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_PARENT_ID, WPD_OBJECT_SIZE,
    };
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STGC_DEFAULT,
    };
    use windows::core::{PCWSTR, PWSTR};

    let source = source.to_path_buf();
    let name = name.to_string();
    let size = std::fs::metadata(&source)?.len();

    with_portable_content(device_id, |content| {
        let values: IPortableDeviceValues =
            unsafe { CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)? };
        let parent_wide = wide_null(parent_object_id);
        let name_wide = wide_null(&name);
        unsafe {
            values.SetStringValue(&WPD_OBJECT_PARENT_ID, PCWSTR(parent_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE, size)?;
            values.SetGuidValue(&WPD_OBJECT_CONTENT_TYPE, &WPD_CONTENT_TYPE_UNSPECIFIED)?;
            values.SetGuidValue(&WPD_OBJECT_FORMAT, &WPD_OBJECT_FORMAT_UNSPECIFIED)?;
        }

        let mut optimal_buffer_size = 0_u32;
        let mut stream: Option<IStream> = None;
        let mut cookie = PWSTR::null();
        unsafe {
            content.CreateObjectWithPropertiesAndData(
                &values,
                &mut stream,
                &mut optimal_buffer_size,
                &mut cookie,
            )?;
        }
        let stream = stream.ok_or_else(|| {
            crate::utils::errors::BExplorerError::Operation(
                "Portable device did not return a writable stream".into(),
            )
        })?;

        let result = (|| {
            let chunk_size = portable_chunk_size(optimal_buffer_size);
            let mut buffer = vec![0_u8; chunk_size];
            let mut input = std::fs::File::open(&source)?;
            let mut total = 0_u64;

            loop {
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                let mut offset = 0_usize;
                while offset < read {
                    let mut written = 0_u32;
                    unsafe {
                        stream
                            .Write(
                                buffer[offset..read].as_ptr() as *const c_void,
                                (read - offset) as u32,
                                Some(&mut written),
                            )
                            .ok()?;
                    }
                    if written == 0 {
                        return Err(crate::utils::errors::BExplorerError::Operation(
                            "Portable device stopped accepting data".into(),
                        ));
                    }
                    offset += written as usize;
                    total = total.saturating_add(written as u64);
                    on_bytes(written as u64)?;
                }
            }

            unsafe {
                stream.Commit(STGC_DEFAULT)?;
            }
            Ok(total)
        })();

        if result.is_err() {
            unsafe {
                let _ = stream.Revert();
            }
        }
        if !cookie.is_null() {
            unsafe {
                CoTaskMemFree(Some(cookie.0 as *const _));
            }
        }

        result
    })
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn portable_create_folder(
    device_id: &str,
    parent_object_id: &str,
    name: &str,
) -> crate::utils::errors::Result<String> {
    use windows::Win32::Devices::PortableDevices::{
        IPortableDeviceValues, PortableDeviceValues, WPD_CONTENT_TYPE_FOLDER,
        WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_FORMAT, WPD_OBJECT_FORMAT_PROPERTIES_ONLY,
        WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_PARENT_ID,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::core::{PCWSTR, PWSTR};

    let name = name.to_string();
    with_portable_content(device_id, |content| {
        let values: IPortableDeviceValues =
            unsafe { CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)? };
        let parent_wide = wide_null(parent_object_id);
        let name_wide = wide_null(&name);
        unsafe {
            values.SetStringValue(&WPD_OBJECT_PARENT_ID, PCWSTR(parent_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME, PCWSTR(name_wide.as_ptr()))?;
            values.SetGuidValue(&WPD_OBJECT_CONTENT_TYPE, &WPD_CONTENT_TYPE_FOLDER)?;
            values.SetGuidValue(&WPD_OBJECT_FORMAT, &WPD_OBJECT_FORMAT_PROPERTIES_ONLY)?;
        }

        let mut object_id = PWSTR::null();
        unsafe {
            content.CreateObjectWithPropertiesOnly(&values, &mut object_id)?;
        }
        let id = pwstr_to_string(object_id);
        if !object_id.is_null() {
            unsafe {
                CoTaskMemFree(Some(object_id.0 as *const _));
            }
        }
        if id.trim().is_empty() {
            Err(crate::utils::errors::BExplorerError::Operation(
                "Portable device did not return the created folder id".into(),
            ))
        } else {
            Ok(id)
        }
    })
}

#[cfg(target_os = "windows")]
pub fn portable_delete_objects(
    device_id: &str,
    object_ids: &[String],
) -> crate::utils::errors::Result<usize> {
    let session = PortableDeviceSession::open(device_id)?;
    session.delete_objects(object_ids)
}

enum PortableDeviceField {
    Name,
    Manufacturer,
    Description,
}

#[cfg(target_os = "windows")]
fn portable_device_string(
    manager: &windows::Win32::Devices::PortableDevices::IPortableDeviceManager,
    id: windows::core::PCWSTR,
    field: PortableDeviceField,
) -> Option<String> {
    let mut len = 0_u32;
    unsafe {
        let _ = match field {
            PortableDeviceField::Name => {
                manager.GetDeviceFriendlyName(id, windows::core::PWSTR::null(), &mut len)
            }
            PortableDeviceField::Manufacturer => {
                manager.GetDeviceManufacturer(id, windows::core::PWSTR::null(), &mut len)
            }
            PortableDeviceField::Description => {
                manager.GetDeviceDescription(id, windows::core::PWSTR::null(), &mut len)
            }
        };
        if len == 0 {
            return None;
        }
        let mut buffer = vec![0_u16; len as usize];
        let result = match field {
            PortableDeviceField::Name => manager.GetDeviceFriendlyName(
                id,
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &mut len,
            ),
            PortableDeviceField::Manufacturer => manager.GetDeviceManufacturer(
                id,
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &mut len,
            ),
            PortableDeviceField::Description => manager.GetDeviceDescription(
                id,
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &mut len,
            ),
        };
        if result.is_err() {
            return None;
        }
        wide_to_string(&buffer)
    }
}

#[cfg(target_os = "windows")]
fn portable_value_string(
    values: &windows::Win32::Devices::PortableDevices::IPortableDeviceValues,
    key: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;

    unsafe {
        let raw = values.GetStringValue(key).ok()?;
        if raw.is_null() {
            return None;
        }
        let value = pwstr_to_string(raw);
        CoTaskMemFree(Some(raw.0 as *const _));
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    }
}

#[cfg(target_os = "windows")]
fn with_portable_content<T, F>(device_id: &str, operation: F) -> crate::utils::errors::Result<T>
where
    F: FnOnce(
        &windows::Win32::Devices::PortableDevices::IPortableDeviceContent,
    ) -> crate::utils::errors::Result<T>,
{
    use windows::Win32::Devices::PortableDevices::{
        IPortableDevice, IPortableDeviceValues, PortableDevice, PortableDeviceValues,
        WPD_CLIENT_MAJOR_VERSION, WPD_CLIENT_MINOR_VERSION, WPD_CLIENT_NAME, WPD_CLIENT_REVISION,
        WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE,
    };
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows::core::PCWSTR;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let should_uninitialize = hr == S_OK || hr.0 == 1;

        let result = (|| -> crate::utils::errors::Result<T> {
            let device: IPortableDevice =
                CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
            let client_info: IPortableDeviceValues =
                CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;

            let client_name = wide_null("BExplorer");
            client_info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(client_name.as_ptr()))?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0)?;
            client_info.SetUnsignedIntegerValue(&WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE, 2)?;

            let device_wide = wide_null(device_id);
            device.Open(PCWSTR(device_wide.as_ptr()), &client_info)?;
            let content = device.Content()?;
            let operation_result = operation(&content);
            let close_result = device.Close();
            if operation_result.is_ok() {
                close_result?;
            }
            operation_result
        })();

        if should_uninitialize {
            CoUninitialize();
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn portable_object_info_from_content(
    content: &windows::Win32::Devices::PortableDevices::IPortableDeviceContent,
    object_id: &str,
) -> crate::utils::errors::Result<PortableObjectInfo> {
    use windows::Win32::Devices::PortableDevices::{
        IPortableDeviceKeyCollection, PortableDeviceKeyCollection, WPD_CONTENT_TYPE_FOLDER,
        WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT, WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_NAME,
        WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_SIZE,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::core::PCWSTR;

    unsafe {
        let properties = content.Properties()?;
        let keys: IPortableDeviceKeyCollection =
            CoCreateInstance(&PortableDeviceKeyCollection, None, CLSCTX_INPROC_SERVER)?;
        keys.Add(&WPD_OBJECT_NAME)?;
        keys.Add(&WPD_OBJECT_ORIGINAL_FILE_NAME)?;
        keys.Add(&WPD_OBJECT_CONTENT_TYPE)?;
        keys.Add(&WPD_OBJECT_SIZE)?;

        let object_wide = wide_null(object_id);
        let values = properties.GetValues(PCWSTR(object_wide.as_ptr()), &keys)?;
        let name = portable_value_string(&values, &WPD_OBJECT_NAME)
            .or_else(|| portable_value_string(&values, &WPD_OBJECT_ORIGINAL_FILE_NAME))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| object_id.to_string());
        let content_type = values.GetGuidValue(&WPD_OBJECT_CONTENT_TYPE).ok();
        let is_folder = content_type.is_some_and(|content_type| {
            content_type == WPD_CONTENT_TYPE_FOLDER
                || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT
        });
        let size = values.GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE).ok();

        Ok(PortableObjectInfo {
            id: object_id.to_string(),
            name,
            is_folder,
            size,
        })
    }
}

#[cfg(target_os = "windows")]
fn portable_objects_from_content(
    content: &windows::Win32::Devices::PortableDevices::IPortableDeviceContent,
    parent_object_id: &str,
) -> crate::utils::errors::Result<Vec<PortableObjectInfo>> {
    use windows::Win32::Devices::PortableDevices::{
        IPortableDeviceKeyCollection, IPortableDeviceValues, PortableDeviceKeyCollection,
        PortableDeviceValues, WPD_CONTENT_TYPE_FOLDER, WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT,
        WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_SIZE,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::core::{PCWSTR, PWSTR};

    unsafe {
        let filter: IPortableDeviceValues =
            CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
        let parent_id = if parent_object_id.trim().is_empty() {
            "DEVICE"
        } else {
            parent_object_id
        };
        let parent_wide = wide_null(parent_id);
        let enumerator = content.EnumObjects(0, PCWSTR(parent_wide.as_ptr()), &filter)?;

        let properties = content.Properties()?;
        let keys: IPortableDeviceKeyCollection =
            CoCreateInstance(&PortableDeviceKeyCollection, None, CLSCTX_INPROC_SERVER)?;
        keys.Add(&WPD_OBJECT_NAME)?;
        keys.Add(&WPD_OBJECT_ORIGINAL_FILE_NAME)?;
        keys.Add(&WPD_OBJECT_CONTENT_TYPE)?;
        keys.Add(&WPD_OBJECT_SIZE)?;

        let mut objects = Vec::new();
        loop {
            let mut ids = [PWSTR::null(); 16];
            let mut fetched = 0_u32;
            enumerator.Next(&mut ids, &mut fetched).ok()?;
            if fetched == 0 {
                break;
            }

            for id in ids.into_iter().take(fetched as usize) {
                if id.is_null() {
                    continue;
                }

                let id_string = pwstr_to_string(id);
                let object_wide = wide_null(&id_string);
                let values = properties.GetValues(PCWSTR(object_wide.as_ptr()), &keys)?;

                let name = portable_value_string(&values, &WPD_OBJECT_NAME)
                    .or_else(|| portable_value_string(&values, &WPD_OBJECT_ORIGINAL_FILE_NAME))
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| id_string.clone());
                let content_type = values.GetGuidValue(&WPD_OBJECT_CONTENT_TYPE).ok();
                let is_folder = content_type.is_some_and(|content_type| {
                    content_type == WPD_CONTENT_TYPE_FOLDER
                        || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT
                });
                let size = values.GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE).ok();

                objects.push(PortableObjectInfo {
                    id: id_string,
                    name,
                    is_folder,
                    size,
                });

                CoTaskMemFree(Some(id.0 as *const _));
            }
        }

        objects.sort_by_key(|object| object.name.to_lowercase());
        Ok(objects)
    }
}

#[cfg(target_os = "windows")]
fn portable_chunk_size(optimal_buffer_size: u32) -> usize {
    if optimal_buffer_size == 0 {
        1024 * 1024
    } else {
        optimal_buffer_size.clamp(64 * 1024, 4 * 1024 * 1024) as usize
    }
}

#[cfg(target_os = "windows")]
fn portable_retry_delay(attempt: usize) -> std::time::Duration {
    std::time::Duration::from_millis(120 * (attempt as u64 + 1))
}

#[cfg(target_os = "windows")]
fn portable_access_lock() -> std::sync::MutexGuard<'static, ()> {
    PORTABLE_DEVICE_ACCESS
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(target_os = "windows")]
fn windows_error_is_resource_busy(error: &windows::core::Error) -> bool {
    error.code().0 as u32 == 0x800700AA
}

#[cfg(target_os = "windows")]
fn portable_error_is_resource_busy(error: &crate::utils::errors::BExplorerError) -> bool {
    error.to_string().contains("0x800700AA")
}

#[cfg(target_os = "windows")]
fn read_portable_resource(
    resources: &windows::Win32::Devices::PortableDevices::IPortableDeviceResources,
    object_id: windows::core::PCWSTR,
    key: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    use std::ffi::c_void;

    use windows::Win32::System::Com::{IStream, STGM_READ};

    unsafe {
        let mut optimal_buffer_size = 0_u32;
        let mut stream: Option<IStream> = None;
        resources
            .GetStream(
                object_id,
                key,
                STGM_READ.0,
                &mut optimal_buffer_size,
                &mut stream,
            )
            .ok()?;

        let stream = stream?;
        let chunk_size = if optimal_buffer_size == 0 {
            64 * 1024
        } else {
            optimal_buffer_size.clamp(4 * 1024, 1024 * 1024) as usize
        };
        let mut buffer = vec![0_u8; chunk_size];
        let mut output = Vec::new();

        loop {
            let mut read = 0_u32;
            stream
                .Read(
                    buffer.as_mut_ptr() as *mut c_void,
                    buffer.len() as u32,
                    Some(&mut read),
                )
                .ok()
                .ok()?;
            if read == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..read as usize]);
            if output.len() > max_bytes {
                return None;
            }
        }

        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    }
}

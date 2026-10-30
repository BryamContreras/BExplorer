//! Network Function Discovery providers return their results asynchronously.
//! Keep the query and notification sink alive for the initial probe window.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Devices::FunctionDiscovery::{
    FCTN_CATEGORY_NETBIOS, FCTN_CATEGORY_NETWORKDEVICES, FunctionDiscovery, IFunctionDiscovery,
    IFunctionDiscoveryNotification, IFunctionDiscoveryNotification_Impl, IFunctionInstance,
    IFunctionInstanceCollection, QUA_REMOVE, QueryUpdateAction,
};
use windows::Win32::Foundation::BOOL;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    STGM_READ,
};
use windows::core::{HRESULT, Interface, PCWSTR};

use super::{NetworkComputerInfo, merge_network_computers, network_device_from_store};

const DISCOVERY_WINDOW: Duration = Duration::from_secs(12);
// E_PENDING is projected under the otherwise unused Urlmon SDK feature.
const E_PENDING: HRESULT = HRESULT(0x8000000A_u32 as i32);

#[derive(Default)]
struct DiscoveryState {
    devices: Vec<NetworkComputerInfo>,
}

#[windows::core::implement(IFunctionDiscoveryNotification)]
struct DiscoveryNotifications {
    state: Arc<(Mutex<DiscoveryState>, Condvar)>,
}

#[allow(non_snake_case)]
impl IFunctionDiscoveryNotification_Impl for DiscoveryNotifications_Impl {
    fn OnUpdate(
        &self,
        action: QueryUpdateAction,
        _context: u64,
        instance: Option<&IFunctionInstance>,
    ) -> windows::core::Result<()> {
        if action != QUA_REMOVE
            && let Some(device) = instance.and_then(function_device)
        {
            record_device(&self.state, device);
        }
        Ok(())
    }

    fn OnError(
        &self,
        error: HRESULT,
        _context: u64,
        _provider: &PCWSTR,
    ) -> windows::core::Result<()> {
        crate::utils::log::error(format!("Network device discovery provider failed: {error}"));
        Ok(())
    }

    fn OnEvent(&self, _event: u32, _context: u64, _provider: &PCWSTR) -> windows::core::Result<()> {
        // SEARCHCOMPLETE is per provider, not per layered query. In particular,
        // an early SSDP completion must not release a WSD query still finding PCs.
        Ok(())
    }
}

fn function_device(instance: &IFunctionInstance) -> Option<NetworkComputerInfo> {
    use windows::Win32::Devices::FunctionDiscovery::{
        PKEY_Device_FriendlyName, PKEY_DeviceDisplay_FriendlyName, PKEY_WNET_DisplayType,
        PKEY_WNET_RemoteName,
    };

    let store = unsafe { instance.OpenPropertyStore(STGM_READ).ok()? };
    let remote_name = super::property_store_string(&store, &PKEY_WNET_RemoteName);
    let has_computer_resource = remote_name.is_some();
    let name = function_device_name(
        super::property_store_string(&store, &PKEY_DeviceDisplay_FriendlyName),
        super::property_store_string(&store, &PKEY_Device_FriendlyName),
        remote_name,
        super::property_store_string(&store, &PKEY_WNET_DisplayType),
    )?;
    let mut device = network_device_from_store(&name, Some(&store))?;
    super::apply_network_computer_hint(&mut device, has_computer_resource);
    Some(device)
}

fn function_device_name(
    display_name: Option<String>,
    device_name: Option<String>,
    remote_name: Option<String>,
    display_type: Option<String>,
) -> Option<String> {
    if let Some(remote_name) = remote_name {
        let resource = super::WNetResourceInfo {
            remote_name,
            provider_name: String::new(),
            comment: String::new(),
            usage: 0,
            display_type: display_type
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            resource_type: windows::Win32::NetworkManagement::WNet::RESOURCETYPE_ANY,
        };
        return super::wnet_host_name(&resource);
    }
    // WSD maps PKEY_PNPX_FriendlyName to PKEY_Device_FriendlyName. The display
    // property may be absent. DeviceDescription/ModelName and provider instance
    // IDs describe software or namespaces, and must never become device names.
    [display_name, device_name]
        .into_iter()
        .flatten()
        .find_map(|name| {
            let name = super::clean_network_device_name(&name);
            (!name.is_empty() && !super::is_windows_network_provider(&name)).then_some(name)
        })
}

fn record_device(state: &(Mutex<DiscoveryState>, Condvar), device: NetworkComputerInfo) {
    {
        let mut current = state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        merge_network_computers(&mut current.devices, vec![device.clone()]);
    }
    // Publish as soon as the callback arrives, even if the original UI waiter
    // reached its own timeout. The active navigation consumes cache revisions.
    super::publish_network_computer(device);
    state.1.notify_all();
}

pub(super) fn network_devices() -> Vec<NetworkComputerInfo> {
    unsafe {
        // An MTA delivers callbacks while this worker waits without a UI pump.
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialized.is_err() {
            crate::utils::log::error(format!(
                "Network discovery COM initialization failed: {initialized}"
            ));
            return Vec::new();
        }
        let result = collect_devices().unwrap_or_else(|error| {
            crate::utils::log::error(format!("Network device discovery failed: {error}"));
            Vec::new()
        });
        CoUninitialize();
        result
    }
}

unsafe fn collect_devices() -> windows::core::Result<Vec<NetworkComputerInfo>> {
    unsafe {
        let discovery: IFunctionDiscovery =
            CoCreateInstance(&FunctionDiscovery, None, CLSCTX_INPROC_SERVER)?;
        let state = Arc::new((Mutex::new(DiscoveryState::default()), Condvar::new()));
        let notifications: IFunctionDiscoveryNotification = DiscoveryNotifications {
            state: Arc::clone(&state),
        }
        .into();
        let mut queries = Vec::new();
        let mut asynchronous = false;
        for (index, category) in [FCTN_CATEGORY_NETWORKDEVICES, FCTN_CATEGORY_NETBIOS]
            .into_iter()
            .enumerate()
        {
            #[cfg(test)]
            eprintln!("FD category={index} create query");
            let mut context = index as u64;
            let query = match discovery.CreateInstanceCollectionQuery(
                category,
                None,
                BOOL(1),
                &notifications,
                &mut context,
            ) {
                Ok(query) => query,
                Err(error) => {
                    // Failure of one category must not discard another provider.
                    crate::utils::log::error(format!("Network discovery category failed: {error}"));
                    continue;
                }
            };
            #[cfg(test)]
            eprintln!("FD category={index} execute query");
            // Execute may return E_PENDING with a partial collection. Retain and
            // release that collection too; the convenience Result wrapper cannot
            // expose it when the HRESULT indicates asynchronous completion.
            let mut raw_collection = std::ptr::null_mut();
            let status =
                (Interface::vtable(&query).Execute)(Interface::as_raw(&query), &mut raw_collection);
            #[cfg(test)]
            eprintln!("FD category={index} execute returned {status}");
            if !raw_collection.is_null() {
                let collection = IFunctionInstanceCollection::from_raw(raw_collection);
                if let Ok(count) = collection.GetCount() {
                    for index in 0..count.min(2048) {
                        if let Ok(instance) = collection.Item(index)
                            && let Some(device) = function_device(&instance)
                        {
                            record_device(&state, device);
                        }
                    }
                }
            }
            if status == E_PENDING {
                asynchronous = true;
            } else if status.is_err() {
                crate::utils::log::error(format!("Network discovery query failed: {status}"));
            }
            queries.push(query);
        }
        #[cfg(test)]
        eprintln!(
            "FD queries initialized; asynchronous={asynchronous}; waiting up to {DISCOVERY_WINDOW:?}"
        );
        if asynchronous {
            let deadline = Instant::now() + DISCOVERY_WINDOW;
            let mut current = state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                current = state
                    .1
                    .wait_timeout(current, remaining)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
        }
        // Releasing a query waits for any callback in progress. Never do it
        // while holding our state lock, which a callback may need to acquire.
        #[cfg(test)]
        eprintln!("FD releasing queries");
        drop(queries);
        #[cfg(test)]
        eprintln!("FD queries released");
        drop(notifications);
        let mut current = state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(std::mem::take(&mut current.devices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_names_require_a_friendly_device_or_real_wnet_host() {
        for provider in [
            "Microsoft Terminal Services::Microsoft Terminal Services",
            "Microsoft Windows Network::Microsoft Windows Network",
            "Plan 9 Network Provider::Plan 9 Network Provider",
            "Web Client Network::Web Client Network",
            "Microsoft Publication Service Device Host",
        ] {
            assert_eq!(
                function_device_name(Some(provider.into()), None, None, None),
                None
            );
        }
        assert_eq!(
            function_device_name(None, Some("MARTHAOPORTA".into()), None, None).as_deref(),
            Some("MARTHAOPORTA")
        );
        assert_eq!(
            function_device_name(
                Some("Microsoft Publication Service Device Host".into()),
                Some("S3RVERMB".into()),
                None,
                None
            )
            .as_deref(),
            Some("S3RVERMB")
        );
        assert_eq!(
            function_device_name(None, Some("EPSONDF2594 (L3250 Series)".into()), None, None)
                .as_deref(),
            Some("EPSONDF2594 (L3250 Series)")
        );
        assert_eq!(
            function_device_name(None, None, Some(r"\\OFFICE-PC".into()), Some("2".into()))
                .as_deref(),
            Some("OFFICE-PC")
        );
        assert_eq!(
            function_device_name(
                Some("WORKGROUP".into()),
                None,
                Some(r"\\WORKGROUP".into()),
                Some("1".into())
            ),
            None
        );
        assert_eq!(
            function_device_name(
                Some("Web Client Network".into()),
                None,
                Some("Web Client Network".into()),
                Some("6".into())
            ),
            None
        );
        assert_eq!(function_device_name(None, None, None, None), None);
    }
}

use super::util::{pwstr_to_string, wide_null};

mod function_discovery;

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkComputerInfo {
    pub name: String,
    pub comment: String,
    pub kind: crate::platform::NetworkDeviceKind,
}

fn publish_network_computer(device: NetworkComputerInfo) {
    crate::fs::explorer::merge_discovered_network_computer(crate::platform::NetworkComputerInfo {
        name: device.name,
        comment: device.comment,
        kind: device.kind,
    });
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkShareInfo {
    pub name: String,
    pub remark: String,
}

pub fn network_computers() -> Vec<NetworkComputerInfo> {
    let mut computers = network_computers_fast();
    merge_network_computers(&mut computers, network_computers_discovered());
    computers
}

#[cfg(target_os = "windows")]
pub fn network_computers_fast() -> Vec<NetworkComputerInfo> {
    use std::ffi::c_void;

    use windows::Win32::Foundation::ERROR_MORE_DATA;
    use windows::Win32::NetworkManagement::NetManagement::{
        MAX_PREFERRED_LENGTH, NERR_Success, NET_SERVER_TYPE, NetApiBufferFree, NetServerEnum,
        SERVER_INFO_101, SV_TYPE_SERVER, SV_TYPE_WORKSTATION,
    };
    use windows::core::PCWSTR;

    unsafe {
        let mut computers = Vec::new();
        let mut resume = 0_u32;
        loop {
            let mut buffer: *mut u8 = std::ptr::null_mut();
            let mut entries_read = 0_u32;
            let mut total_entries = 0_u32;
            let status = NetServerEnum(
                PCWSTR::null(),
                101,
                &mut buffer,
                MAX_PREFERRED_LENGTH,
                &mut entries_read,
                &mut total_entries,
                NET_SERVER_TYPE(SV_TYPE_WORKSTATION.0 | SV_TYPE_SERVER.0),
                PCWSTR::null(),
                Some(&mut resume),
            );

            if status != NERR_Success && status != ERROR_MORE_DATA.0 {
                if !buffer.is_null() {
                    NetApiBufferFree(Some(buffer as *const c_void));
                }
                crate::utils::log::error(format!("Network computer scan failed: {status}"));
                break;
            }

            if !buffer.is_null() && entries_read > 0 {
                let entries = std::slice::from_raw_parts(
                    buffer as *const SERVER_INFO_101,
                    entries_read as usize,
                );
                for entry in entries {
                    let name = pwstr_to_string(entry.sv101_name);
                    if name.trim().is_empty() {
                        continue;
                    }
                    computers.push(NetworkComputerInfo {
                        name,
                        comment: pwstr_to_string(entry.sv101_comment),
                        kind: crate::platform::NetworkDeviceKind::Computer,
                    });
                    if let Some(computer) = computers.last() {
                        publish_network_computer(computer.clone());
                    }
                }
            }

            if !buffer.is_null() {
                NetApiBufferFree(Some(buffer as *const c_void));
            }
            if status != ERROR_MORE_DATA.0 || entries_read == 0 {
                break;
            }
        }

        computers.sort_by_key(|computer| computer.name.to_lowercase());
        computers.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        computers
    }
}

#[cfg(target_os = "windows")]
pub fn network_computers_discovered() -> Vec<NetworkComputerInfo> {
    let mut computers = netbios_cached_devices();
    merge_network_computers(&mut computers, netbios_network_devices());
    merge_network_computers(&mut computers, installed_printer_devices());
    merge_network_computers(&mut computers, function_discovery_network_devices());
    if computers.is_empty() {
        merge_network_computers(&mut computers, shell_network_devices());
    }
    computers
}

#[cfg(target_os = "windows")]
pub fn network_computers_netbios_cached() -> Vec<NetworkComputerInfo> {
    netbios_cached_devices()
}

#[cfg(target_os = "windows")]
pub fn network_netbios_neighbor_addresses() -> Vec<String> {
    netbios_neighbor_addresses()
        .into_iter()
        .map(|address| address.to_string())
        .collect()
}

#[cfg(target_os = "windows")]
pub fn network_computer_netbios_at(address: &str) -> Option<NetworkComputerInfo> {
    const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(6500);

    let remote_ip = address.parse().ok()?;
    let name = query_netbios_node_name(std::net::Ipv4Addr::UNSPECIFIED, remote_ip, QUERY_TIMEOUT)?;
    Some(NetworkComputerInfo {
        kind: classify_network_device_kind(&name, "PC de red"),
        name,
        comment: "PC de red".into(),
    })
}

#[cfg(target_os = "windows")]
pub fn network_printer_devices() -> Vec<NetworkComputerInfo> {
    installed_printer_devices()
}

#[cfg(target_os = "windows")]
pub fn network_function_devices() -> Vec<NetworkComputerInfo> {
    function_discovery_network_devices()
}

#[cfg(target_os = "windows")]
pub fn network_computers_wnet() -> Vec<NetworkComputerInfo> {
    wnet_network_computers()
}

#[cfg(target_os = "windows")]
pub fn network_shell_devices() -> Vec<NetworkComputerInfo> {
    shell_network_devices()
}

#[cfg(target_os = "windows")]
fn function_discovery_network_devices() -> Vec<NetworkComputerInfo> {
    function_discovery::network_devices()
}

#[cfg(target_os = "windows")]
fn netbios_network_devices() -> Vec<NetworkComputerInfo> {
    use std::process::Command;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const QUERY_TIMEOUT: Duration = Duration::from_millis(6500);
    const MAX_NEIGHBORS: usize = 64;

    let Some(output) = crate::utils::process::command_output_with_timeout(
        Command::new("arp")
            .arg("-a")
            .creation_flags(CREATE_NO_WINDOW),
        Duration::from_secs(3),
    ) else {
        return Vec::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let neighbors = parse_arp_neighbors(&stdout);

    let neighbors = std::sync::Arc::new(std::sync::Mutex::new(
        neighbors.into_iter().take(MAX_NEIGHBORS),
    ));
    // Scope the aggregate's workers to its real lifetime. A UI timeout must
    // not detach these workers and allow a second aggregate to multiply them.
    let mut computers = thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        for _ in 0..8 {
            let tx = tx.clone();
            let neighbors = std::sync::Arc::clone(&neighbors);
            scope.spawn(move || {
                loop {
                    let neighbor = neighbors
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .next();
                    let Some((local_ip, remote_ip)) = neighbor else {
                        break;
                    };
                    if let Some(name) = query_netbios_node_name(local_ip, remote_ip, QUERY_TIMEOUT)
                    {
                        let _ = tx.send(NetworkComputerInfo {
                            kind: classify_network_device_kind(&name, "PC de red"),
                            name,
                            comment: "PC de red".into(),
                        });
                    }
                }
            });
        }
        drop(tx);
        rx.into_iter().collect::<Vec<_>>()
    });

    computers.sort_by_key(|computer| computer.name.to_lowercase());
    computers.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
    computers
}

#[cfg(target_os = "windows")]
fn netbios_cached_devices() -> Vec<NetworkComputerInfo> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let Some(output) = crate::utils::process::command_output_with_timeout(
        Command::new("nbtstat")
            .arg("-c")
            .creation_flags(CREATE_NO_WINDOW),
        std::time::Duration::from_secs(3),
    ) else {
        return Vec::new();
    };

    let output = String::from_utf8_lossy(&output.stdout);
    let mut computers: Vec<NetworkComputerInfo> = parse_nbtstat_names(&output)
        .into_iter()
        .map(|name| NetworkComputerInfo {
            kind: classify_network_device_kind(&name, "PC de red"),
            name,
            comment: "PC de red".into(),
        })
        .collect();
    computers.sort_by_key(|computer| computer.name.to_lowercase());
    computers.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
    computers
}

#[cfg(target_os = "windows")]
fn netbios_neighbor_addresses() -> Vec<std::net::Ipv4Addr> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let Some(output) = crate::utils::process::command_output_with_timeout(
        Command::new("arp")
            .arg("-a")
            .creation_flags(CREATE_NO_WINDOW),
        std::time::Duration::from_secs(3),
    ) else {
        return Vec::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_arp_neighbors(&stdout)
        .into_iter()
        .map(|(_, remote_ip)| remote_ip)
        .take(64)
        .collect()
}

#[cfg(target_os = "windows")]
fn parse_arp_neighbors(output: &str) -> Vec<(std::net::Ipv4Addr, std::net::Ipv4Addr)> {
    let mut current_interface = None;
    let mut neighbors = Vec::new();

    for line in output.lines() {
        let ips = ipv4_tokens(line);
        if line.contains("---") {
            current_interface = ips.into_iter().next();
            continue;
        }

        let Some(local_ip) = current_interface else {
            continue;
        };
        let Some(remote_ip) = ips.into_iter().next() else {
            continue;
        };
        if is_candidate_network_neighbor(remote_ip)
            && !neighbors
                .iter()
                .any(|(_, existing_remote)| *existing_remote == remote_ip)
        {
            neighbors.push((local_ip, remote_ip));
        }
    }

    neighbors
}

#[cfg(target_os = "windows")]
fn ipv4_tokens(line: &str) -> Vec<std::net::Ipv4Addr> {
    line.split_whitespace()
        .filter_map(|token| {
            token
                .trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.')
                .parse()
                .ok()
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn installed_printer_devices() -> Vec<NetworkComputerInfo> {
    use windows::Win32::Graphics::Printing::{
        EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_4W,
    };
    use windows::core::PCWSTR;

    unsafe {
        let mut needed = 0_u32;
        let mut returned = 0_u32;
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let _ = EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut needed, &mut returned);
        if needed == 0 {
            return Vec::new();
        }

        let mut buffer = vec![0_u8; needed as usize];
        if EnumPrintersW(
            flags,
            PCWSTR::null(),
            4,
            Some(&mut buffer),
            &mut needed,
            &mut returned,
        )
        .is_err()
        {
            return Vec::new();
        }

        let entries = std::slice::from_raw_parts(
            buffer.as_ptr() as *const PRINTER_INFO_4W,
            returned as usize,
        );
        let mut printers = Vec::new();
        for entry in entries {
            let name = clean_network_device_name(&pwstr_to_string(entry.pPrinterName));
            if name.is_empty() || is_builtin_printer_name(&name) {
                continue;
            }
            printers.push(NetworkComputerInfo {
                name,
                comment: "Impresora".into(),
                kind: crate::platform::NetworkDeviceKind::Printer,
            });
        }
        printers.sort_by_key(|printer| printer.name.to_lowercase());
        printers.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        printers
    }
}

#[cfg(target_os = "windows")]
fn is_builtin_printer_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("microsoft print")
        || lower.contains("onenote")
        || lower.contains("xps")
        || lower.contains("fax")
        || lower.contains("pdf")
}

#[cfg(target_os = "windows")]
fn classify_network_device_kind(name: &str, comment: &str) -> crate::platform::NetworkDeviceKind {
    let lower = format!("{} {}", name, comment).to_ascii_lowercase();
    let printer = lower.contains("printer") || lower.contains("impresora");
    let scanner = lower.contains("scanner")
        || lower.contains("scan")
        || lower.contains("escaner")
        || lower.contains("escáner");
    if lower.contains("multifunc")
        || lower.contains("mfp")
        || lower.contains("all-in-one")
        || lower.contains("todo en uno")
        || (printer && scanner)
    {
        crate::platform::NetworkDeviceKind::Multifunction
    } else if scanner {
        crate::platform::NetworkDeviceKind::Scanner
    } else if lower.contains("printer")
        || lower.contains("impresora")
        || lower.contains("laserjet")
        || lower.contains("ir-adv")
        || lower.contains("ir adv")
        || lower.contains("imagerunner")
        || lower.starts_with("hpd")
    {
        crate::platform::NetworkDeviceKind::Printer
    } else if lower.contains("pc")
        || lower.contains("computer")
        || lower.contains("workstation")
        || lower.contains("server")
        || lower.contains("equipo")
        || lower.contains("desktop")
        || lower.contains("operador")
        || lower.contains("informatica")
        || lower.contains("gerencia")
    {
        crate::platform::NetworkDeviceKind::Computer
    } else {
        crate::platform::NetworkDeviceKind::Other
    }
}

#[cfg(target_os = "windows")]
fn network_device_from_store(
    name: &str,
    store: Option<&windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore>,
) -> Option<NetworkComputerInfo> {
    use windows::Win32::Devices::FunctionDiscovery::{
        PKEY_DeviceDisplay_Category, PKEY_DeviceDisplay_Category_Desc_Singular,
        PKEY_DeviceDisplay_DeviceDescription1, PKEY_DeviceDisplay_DeviceDescription2,
        PKEY_PNPX_DeviceCategory, PKEY_PNPX_Types,
    };

    let name = clean_network_device_name(name);
    if name.is_empty() || is_windows_network_provider(&name) {
        return None;
    }
    let property = |key| store.and_then(|store| property_store_string(store, key));
    let categories = [
        PKEY_DeviceDisplay_Category,
        PKEY_PNPX_DeviceCategory,
        PKEY_PNPX_Types,
    ]
    .iter()
    .filter_map(property)
    .collect::<Vec<_>>()
    .join(" ");
    let descriptions = [
        PKEY_DeviceDisplay_Category_Desc_Singular,
        PKEY_DeviceDisplay_DeviceDescription1,
        PKEY_DeviceDisplay_DeviceDescription2,
    ]
    .iter()
    .filter_map(property)
    .collect::<Vec<_>>();
    let description = descriptions.join(" ");
    let kind = network_kind_with_metadata(&name, &categories, &description);
    Some(NetworkComputerInfo {
        name,
        kind,
        comment: descriptions.into_iter().next().unwrap_or_else(|| {
            match kind {
                crate::platform::NetworkDeviceKind::Computer => "PC de red",
                crate::platform::NetworkDeviceKind::Printer => "Impresora de red",
                crate::platform::NetworkDeviceKind::Scanner => "Escáner de red",
                crate::platform::NetworkDeviceKind::Multifunction => "Dispositivo multifunción",
                crate::platform::NetworkDeviceKind::Other => "Dispositivo de red",
            }
            .into()
        }),
    })
}

#[cfg(target_os = "windows")]
fn network_kind_with_metadata(
    name: &str,
    categories: &str,
    description: &str,
) -> crate::platform::NetworkDeviceKind {
    // DeviceCategory/PNPX Types describe the function. A manufacturer's name
    // alone must not turn an explicitly classified scanner into a printer.
    let category_kind = classify_network_device_kind("", categories);
    if category_kind != crate::platform::NetworkDeviceKind::Other {
        category_kind
    } else {
        classify_network_device_kind(name, description)
    }
}

fn apply_network_computer_hint(device: &mut NetworkComputerInfo, computer_resource: bool) {
    if computer_resource && device.kind == crate::platform::NetworkDeviceKind::Other {
        device.kind = crate::platform::NetworkDeviceKind::Computer;
        if device.comment == "Dispositivo de red" {
            device.comment = "PC de red".into();
        }
    }
}

fn shell_parsing_name_is_computer(name: &str) -> bool {
    let parts = unc_parts(name);
    name.trim().starts_with(r"\\")
        && parts.len() == 1
        && !matches!(parts[0], "." | "?")
        && !is_windows_network_provider(parts[0])
}

#[cfg(target_os = "windows")]
fn network_device_kind_priority(kind: crate::platform::NetworkDeviceKind) -> u8 {
    match kind {
        crate::platform::NetworkDeviceKind::Multifunction => 5,
        crate::platform::NetworkDeviceKind::Printer => 4,
        crate::platform::NetworkDeviceKind::Scanner => 4,
        crate::platform::NetworkDeviceKind::Computer => 3,
        crate::platform::NetworkDeviceKind::Other => 1,
    }
}

#[cfg(target_os = "windows")]
fn is_candidate_network_neighbor(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    !ip.is_loopback()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && octets[0] != 0
        && octets[3] != 0
        && octets[3] != 255
}

#[cfg(target_os = "windows")]
struct NetBiosPermit;

#[cfg(target_os = "windows")]
fn netbios_slots() -> &'static (std::sync::Mutex<usize>, std::sync::Condvar) {
    static SLOTS: std::sync::OnceLock<(std::sync::Mutex<usize>, std::sync::Condvar)> =
        std::sync::OnceLock::new();
    SLOTS.get_or_init(|| (std::sync::Mutex::new(0), std::sync::Condvar::new()))
}

#[cfg(target_os = "windows")]
impl NetBiosPermit {
    fn acquire() -> Self {
        let (mutex, ready) = netbios_slots();
        let mut active = mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *active >= 8 {
            active = ready
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *active += 1;
        Self
    }
}

#[cfg(target_os = "windows")]
impl Drop for NetBiosPermit {
    fn drop(&mut self) {
        let (mutex, ready) = netbios_slots();
        let mut active = mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active -= 1;
        ready.notify_one();
    }
}

#[cfg(target_os = "windows")]
fn query_netbios_node_name(
    _local_ip: std::net::Ipv4Addr,
    remote_ip: std::net::Ipv4Addr,
    timeout: std::time::Duration,
) -> Option<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    // Applies to both incremental discovery and the legacy aggregate API.
    // A second caller cannot multiply the native-process concurrency.
    let _permit = NetBiosPermit::acquire();
    let output = crate::utils::process::command_output_with_timeout(
        Command::new("nbtstat")
            .arg("-A")
            .arg(remote_ip.to_string())
            .creation_flags(CREATE_NO_WINDOW),
        timeout,
    )?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8_lossy(&output.stdout);

    parse_nbtstat_name(&output)
        .map(|name| clean_network_device_name(&name))
        .filter(|name| !name.is_empty() && !is_windows_network_provider(name))
}

#[cfg(target_os = "windows")]
fn parse_nbtstat_name(output: &str) -> Option<String> {
    parse_nbtstat_names(output).into_iter().next()
}

#[cfg(target_os = "windows")]
fn parse_nbtstat_names(output: &str) -> Vec<String> {
    let mut fallback = None;
    let mut names = Vec::new();
    for line in output.lines() {
        if let Some(index) = line.find("<20>") {
            let name = line[..index].trim().to_string();
            if !name.is_empty() && name != "*" {
                names.push(name);
                continue;
            }
        }

        if fallback.is_none()
            && line.contains("<00>")
            && !line.contains("Grupo")
            && !line.contains("Group")
        {
            let Some(index) = line.find("<00>") else {
                continue;
            };
            let name = line[..index].trim().to_string();
            if !name.is_empty() && name != "*" {
                fallback = Some(name);
            }
        }
    }

    if names.is_empty()
        && let Some(name) = fallback
    {
        names.push(name);
    }

    names = names
        .into_iter()
        .map(|name| clean_network_device_name(&name))
        .filter(|name| !name.is_empty() && !is_windows_network_provider(name))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    names
}

#[cfg(target_os = "windows")]
fn shell_network_devices() -> Vec<NetworkComputerInfo> {
    use windows::Win32::Foundation::{HANDLE, HWND, S_OK};
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize, IBindCtx,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_NetworkFolder, ILCombine, IShellFolder, SHCONTF_FOLDERS, SHCONTF_INCLUDEHIDDEN,
        SHCONTF_NETPRINTERSRCH, SHCONTF_NONFOLDERS, SHGDN_FORPARSING, SHGDN_INFOLDER, SHGDN_NORMAL,
        SHGetDesktopFolder, SHGetKnownFolderIDList,
    };

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let should_uninitialize = hr == S_OK || hr.0 == 1;

        let result = (|| -> windows::core::Result<Vec<NetworkComputerInfo>> {
            let network_pidl = ShellPidl::new(SHGetKnownFolderIDList(
                &FOLDERID_NetworkFolder,
                0,
                HANDLE::default(),
            )?);
            if network_pidl.is_null() {
                return Ok(Vec::new());
            }

            let desktop = SHGetDesktopFolder()?;
            let network_folder: IShellFolder =
                desktop.BindToObject(network_pidl.as_ptr(), None::<&IBindCtx>)?;

            let flags = (SHCONTF_FOLDERS.0
                | SHCONTF_NONFOLDERS.0
                | SHCONTF_INCLUDEHIDDEN.0
                | SHCONTF_NETPRINTERSRCH.0) as u32;
            let mut enum_id_list = None;
            network_folder
                .EnumObjects(HWND::default(), flags, &mut enum_id_list)
                .ok()?;
            let Some(enum_id_list) = enum_id_list else {
                return Ok(Vec::new());
            };

            let mut devices = Vec::new();
            loop {
                let mut child = std::ptr::null_mut();
                let mut fetched = 0_u32;
                let status =
                    enum_id_list.Next(std::slice::from_mut(&mut child), Some(&mut fetched));
                if status != S_OK || fetched == 0 || child.is_null() {
                    break;
                }

                let child = ShellPidl::new(child);
                let absolute =
                    ShellPidl::new(ILCombine(Some(network_pidl.as_ptr()), Some(child.as_ptr())));
                let mut name = shell_pidl_display_name(absolute.as_ptr())
                    .or_else(|| {
                        shell_folder_child_name(&network_folder, child.as_ptr(), SHGDN_INFOLDER)
                    })
                    .or_else(|| {
                        shell_folder_child_name(&network_folder, child.as_ptr(), SHGDN_NORMAL)
                    })
                    .unwrap_or_default();
                name = clean_network_device_name(&name);
                if name.is_empty() || is_windows_network_provider(&name) {
                    continue;
                }

                use windows::Win32::UI::Shell::PropertiesSystem::{
                    GPS_FASTPROPERTIESONLY, IPropertyStore,
                };
                use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromIDList};
                let store = SHCreateItemFromIDList::<IShellItem2>(absolute.as_ptr())
                    .ok()
                    .and_then(|item| {
                        item.GetPropertyStore::<IPropertyStore>(GPS_FASTPROPERTIESONLY)
                            .ok()
                    });
                if let Some(mut device) = network_device_from_store(&name, store.as_ref()) {
                    let computer_resource =
                        shell_folder_child_name(&network_folder, child.as_ptr(), SHGDN_FORPARSING)
                            .is_some_and(|name| shell_parsing_name_is_computer(&name));
                    apply_network_computer_hint(&mut device, computer_resource);
                    publish_network_computer(device.clone());
                    merge_network_computers(&mut devices, vec![device]);
                }
            }

            devices.sort_by_key(|device| device.name.to_lowercase());
            devices.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
            Ok(devices)
        })()
        .unwrap_or_else(|error| {
            crate::utils::log::error(format!("Shell network enumeration failed: {error}"));
            Vec::new()
        });

        if should_uninitialize {
            CoUninitialize();
        }
        result
    }
}

#[cfg(target_os = "windows")]
pub fn network_shares(host: &str) -> Vec<NetworkShareInfo> {
    use std::ffi::c_void;

    use windows::Win32::Foundation::{ERROR_MORE_DATA, WIN32_ERROR};
    use windows::Win32::NetworkManagement::NetManagement::{
        MAX_PREFERRED_LENGTH, NERR_Success, NetApiBufferFree,
    };
    use windows::Win32::Storage::FileSystem::{
        NetShareEnum, SHARE_INFO_1, STYPE_DISKTREE, STYPE_MASK,
    };
    use windows::core::PCWSTR;

    unsafe {
        let server = wide_null(&format!(r"\\{host}"));
        let mut shares = Vec::new();
        let mut failed_status = WIN32_ERROR(0);
        let mut resume = 0_u32;
        loop {
            let mut buffer: *mut u8 = std::ptr::null_mut();
            let mut entries_read = 0_u32;
            let mut total_entries = 0_u32;
            let status = NetShareEnum(
                PCWSTR(server.as_ptr()),
                1,
                &mut buffer,
                MAX_PREFERRED_LENGTH,
                &mut entries_read,
                &mut total_entries,
                Some(&mut resume),
            );

            if status != NERR_Success && status != ERROR_MORE_DATA.0 {
                if !buffer.is_null() {
                    NetApiBufferFree(Some(buffer as *const c_void));
                }
                failed_status = WIN32_ERROR(status);
                crate::utils::log::error(format!("Network share scan failed for {host}: {status}"));
                break;
            }

            if !buffer.is_null() && entries_read > 0 {
                let entries = std::slice::from_raw_parts(
                    buffer as *const SHARE_INFO_1,
                    entries_read as usize,
                );
                for entry in entries {
                    if entry.shi1_type.0 & STYPE_MASK.0 != STYPE_DISKTREE.0 {
                        continue;
                    }
                    let name = pwstr_to_string(entry.shi1_netname);
                    if name.trim().is_empty() {
                        continue;
                    }
                    shares.push(NetworkShareInfo {
                        name,
                        remark: pwstr_to_string(entry.shi1_remark),
                    });
                }
            }

            if !buffer.is_null() {
                NetApiBufferFree(Some(buffer as *const c_void));
            }
            if status != ERROR_MORE_DATA.0 || entries_read == 0 {
                break;
            }
        }

        if failed_status != WIN32_ERROR(0)
            && should_prompt_network_credentials(failed_status)
            && prompt_network_credentials_for_host(host)
        {
            shares = network_shares_without_prompt(host);
        }

        shares.sort_by_key(|share| share.name.to_lowercase());
        shares.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        merge_network_shares(&mut shares, wnet_network_shares(host));
        shares
    }
}

#[cfg(target_os = "windows")]
fn network_shares_without_prompt(host: &str) -> Vec<NetworkShareInfo> {
    use std::ffi::c_void;

    use windows::Win32::Foundation::ERROR_MORE_DATA;
    use windows::Win32::NetworkManagement::NetManagement::{
        MAX_PREFERRED_LENGTH, NERR_Success, NetApiBufferFree,
    };
    use windows::Win32::Storage::FileSystem::{
        NetShareEnum, SHARE_INFO_1, STYPE_DISKTREE, STYPE_MASK,
    };
    use windows::core::PCWSTR;

    unsafe {
        let server = wide_null(&format!(r"\\{host}"));
        let mut shares = Vec::new();
        let mut resume = 0_u32;
        loop {
            let mut buffer: *mut u8 = std::ptr::null_mut();
            let mut entries_read = 0_u32;
            let mut total_entries = 0_u32;
            let status = NetShareEnum(
                PCWSTR(server.as_ptr()),
                1,
                &mut buffer,
                MAX_PREFERRED_LENGTH,
                &mut entries_read,
                &mut total_entries,
                Some(&mut resume),
            );

            if status != NERR_Success && status != ERROR_MORE_DATA.0 {
                if !buffer.is_null() {
                    NetApiBufferFree(Some(buffer as *const c_void));
                }
                break;
            }

            if !buffer.is_null() && entries_read > 0 {
                let entries = std::slice::from_raw_parts(
                    buffer as *const SHARE_INFO_1,
                    entries_read as usize,
                );
                for entry in entries {
                    if entry.shi1_type.0 & STYPE_MASK.0 != STYPE_DISKTREE.0 {
                        continue;
                    }
                    let name = pwstr_to_string(entry.shi1_netname);
                    if name.trim().is_empty() {
                        continue;
                    }
                    shares.push(NetworkShareInfo {
                        name,
                        remark: pwstr_to_string(entry.shi1_remark),
                    });
                }
            }

            if !buffer.is_null() {
                NetApiBufferFree(Some(buffer as *const c_void));
            }
            if status != ERROR_MORE_DATA.0 || entries_read == 0 {
                break;
            }
        }

        shares
    }
}

#[cfg(target_os = "windows")]
pub fn prompt_network_credentials_for_path(path: &std::path::Path) -> bool {
    let display = path.display().to_string();
    let trimmed = display.trim_start_matches('\\');
    if trimmed.len() == display.len() {
        return false;
    }

    let mut parts = trimmed.split('\\').filter(|part| !part.trim().is_empty());
    let Some(host) = parts.next() else {
        return false;
    };
    if let Some(share) = parts.next() {
        prompt_network_credentials_for_remote(&format!(r"\\{host}\{share}"))
    } else {
        prompt_network_credentials_for_host(host)
    }
}

#[cfg(target_os = "windows")]
fn prompt_network_credentials_for_host(host: &str) -> bool {
    prompt_network_credentials_for_remote(&format!(r"\\{host}\IPC$"))
        || prompt_network_credentials_for_remote(&format!(r"\\{host}"))
}

#[cfg(target_os = "windows")]
fn prompt_network_credentials_for_remote(remote_name: &str) -> bool {
    use windows::Win32::Foundation::{
        ERROR_ALREADY_ASSIGNED, ERROR_CANCELLED, ERROR_SESSION_CREDENTIAL_CONFLICT, WIN32_ERROR,
    };
    use windows::Win32::NetworkManagement::WNet::{
        CONNECT_INTERACTIVE, CONNECT_PROMPT, NETRESOURCEW, RESOURCETYPE_ANY, WNetAddConnection2W,
    };
    use windows::core::{PCWSTR, PWSTR};

    unsafe {
        let mut remote = wide_null(remote_name);
        let resource = NETRESOURCEW {
            dwScope: windows::Win32::NetworkManagement::WNet::RESOURCE_GLOBALNET,
            dwType: RESOURCETYPE_ANY,
            dwDisplayType: 0,
            dwUsage: 0,
            lpLocalName: PWSTR::null(),
            lpRemoteName: PWSTR(remote.as_mut_ptr()),
            lpComment: PWSTR::null(),
            lpProvider: PWSTR::null(),
        };
        let flags = windows::Win32::NetworkManagement::WNet::NET_CONNECT_FLAGS(
            CONNECT_INTERACTIVE.0 | CONNECT_PROMPT.0,
        );
        let status = WNetAddConnection2W(&resource, PCWSTR::null(), PCWSTR::null(), flags);
        if status == WIN32_ERROR(0) || status == ERROR_ALREADY_ASSIGNED {
            true
        } else {
            if status != ERROR_CANCELLED && status != ERROR_SESSION_CREDENTIAL_CONFLICT {
                crate::utils::log::error(format!(
                    "Network credential prompt failed for {remote_name}: {}",
                    status.0
                ));
            }
            false
        }
    }
}

#[cfg(target_os = "windows")]
fn should_prompt_network_credentials(status: windows::Win32::Foundation::WIN32_ERROR) -> bool {
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_ACCOUNT_DISABLED, ERROR_ACCOUNT_RESTRICTION, ERROR_BAD_USERNAME,
        ERROR_INVALID_PASSWORD, ERROR_LOGON_FAILURE, ERROR_SESSION_CREDENTIAL_CONFLICT,
    };

    matches!(
        status,
        ERROR_ACCESS_DENIED
            | ERROR_ACCOUNT_RESTRICTION
            | ERROR_ACCOUNT_DISABLED
            | ERROR_BAD_USERNAME
            | ERROR_INVALID_PASSWORD
            | ERROR_LOGON_FAILURE
            | ERROR_SESSION_CREDENTIAL_CONFLICT
    )
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug)]
struct WNetResourceInfo {
    remote_name: String,
    provider_name: String,
    comment: String,
    usage: u32,
    display_type: u32,
    resource_type: windows::Win32::NetworkManagement::WNet::NET_RESOURCE_TYPE,
}

#[cfg(target_os = "windows")]
fn merge_network_computers(
    target: &mut Vec<NetworkComputerInfo>,
    mut extra: Vec<NetworkComputerInfo>,
) {
    let mut merged = std::mem::take(target);
    for mut computer in extra.drain(..) {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.name.eq_ignore_ascii_case(&computer.name))
        {
            if matches!(
                (existing.kind, computer.kind),
                (
                    crate::platform::NetworkDeviceKind::Printer,
                    crate::platform::NetworkDeviceKind::Scanner
                ) | (
                    crate::platform::NetworkDeviceKind::Scanner,
                    crate::platform::NetworkDeviceKind::Printer
                )
            ) {
                computer.kind = crate::platform::NetworkDeviceKind::Multifunction;
                computer.comment = "Dispositivo multifunción".into();
            }
            if network_device_kind_priority(computer.kind)
                > network_device_kind_priority(existing.kind)
                || existing.comment.trim().is_empty()
            {
                *existing = computer;
            }
        } else {
            merged.push(computer);
        }
    }
    merged.sort_by_key(|item| item.name.to_lowercase());
    *target = merged;
}

#[cfg(target_os = "windows")]
fn merge_network_shares(target: &mut Vec<NetworkShareInfo>, mut extra: Vec<NetworkShareInfo>) {
    target.append(&mut extra);
    target.sort_by_key(|item| item.name.to_lowercase());
    target.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
}

#[cfg(target_os = "windows")]
fn wnet_network_computers() -> Vec<NetworkComputerInfo> {
    const MAX_DEPTH: usize = 4;
    const MAX_NODES: usize = 2048;

    let mut computers = Vec::new();
    let mut visited = 0_usize;
    collect_wnet_computers(None, 0, MAX_DEPTH, MAX_NODES, &mut visited, &mut computers);
    computers.sort_by_key(|computer| computer.name.to_lowercase());
    computers.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
    computers
}

#[cfg(target_os = "windows")]
fn collect_wnet_computers(
    parent: Option<&windows::Win32::NetworkManagement::WNet::NETRESOURCEW>,
    depth: usize,
    max_depth: usize,
    max_nodes: usize,
    visited: &mut usize,
    computers: &mut Vec<NetworkComputerInfo>,
) {
    if depth > max_depth || *visited >= max_nodes {
        return;
    }

    let resources = wnet_enumerate(parent, true);
    for resource in resources {
        *visited = visited.saturating_add(1);
        if *visited > max_nodes {
            break;
        }

        if let Some(computer) = wnet_computer(&resource) {
            computers.push(computer);
            continue;
        }

        if depth < max_depth && should_descend_wnet_resource(&resource, depth) {
            let mut remote = wide_null(&resource.remote_name);
            let mut provider = wide_null(&resource.provider_name);
            let remote_name = if resource.remote_name.trim().is_empty() {
                windows::core::PWSTR::null()
            } else {
                windows::core::PWSTR(remote.as_mut_ptr())
            };
            let provider_name = if resource.provider_name.trim().is_empty() {
                windows::core::PWSTR::null()
            } else {
                windows::core::PWSTR(provider.as_mut_ptr())
            };
            let parent_resource = windows::Win32::NetworkManagement::WNet::NETRESOURCEW {
                dwScope: windows::Win32::NetworkManagement::WNet::RESOURCE_GLOBALNET,
                dwType: resource.resource_type,
                dwDisplayType: resource.display_type,
                dwUsage: resource.usage,
                lpLocalName: windows::core::PWSTR::null(),
                lpRemoteName: remote_name,
                lpComment: windows::core::PWSTR::null(),
                lpProvider: provider_name,
            };
            collect_wnet_computers(
                Some(&parent_resource),
                depth + 1,
                max_depth,
                max_nodes,
                visited,
                computers,
            );
        }
    }
}

#[cfg(target_os = "windows")]
fn should_descend_wnet_resource(resource: &WNetResourceInfo, depth: usize) -> bool {
    if resource.usage & windows::Win32::NetworkManagement::WNet::RESOURCEUSAGE_CONTAINER.0 == 0 {
        return false;
    }

    if depth == 0 {
        return is_windows_network_provider(&resource.provider_name)
            || is_windows_network_provider(&resource.remote_name)
            || resource.provider_name.trim().is_empty();
    }

    true
}

#[cfg(target_os = "windows")]
fn wnet_host_name(resource: &WNetResourceInfo) -> Option<String> {
    // The top-level namespace contains installed network providers, domains
    // and other containers. A display label is never proof of a computer.
    // Winnetwk.h: GENERIC=0, SERVER=2, SHARE=3; SDK metadata puts these in the
    // unrelated WinSock feature, so keep their documented values local.
    if !matches!(resource.display_type, 0 | 2 | 3)
        || !resource.remote_name.trim().starts_with(r"\\")
    {
        return None;
    }
    let parts = unc_parts(&resource.remote_name);
    parts
        .first()
        .filter(|host| !is_windows_network_provider(host))
        .map(|host| (*host).to_string())
}

fn wnet_computer(resource: &WNetResourceInfo) -> Option<NetworkComputerInfo> {
    let name = wnet_host_name(resource)?;
    let comment = if resource.comment.trim().is_empty() {
        "PC de red".into()
    } else {
        resource.comment.clone()
    };
    Some(NetworkComputerInfo {
        name,
        kind: classify_network_device_kind(&resource.remote_name, &comment),
        comment,
    })
}

#[cfg(target_os = "windows")]
struct ShellPidl(*mut windows::Win32::UI::Shell::Common::ITEMIDLIST);

#[cfg(target_os = "windows")]
impl ShellPidl {
    fn new(pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST) -> Self {
        Self(pidl)
    }

    fn as_ptr(&self) -> *const windows::Win32::UI::Shell::Common::ITEMIDLIST {
        self.0 as *const _
    }

    fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

#[cfg(target_os = "windows")]
impl Drop for ShellPidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows::Win32::UI::Shell::ILFree(Some(self.0 as *const _));
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn shell_pidl_display_name(
    pidl: *const windows::Win32::UI::Shell::Common::ITEMIDLIST,
) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetNameFromIDList, SIGDN_NORMALDISPLAY};

    if pidl.is_null() {
        return None;
    }

    unsafe {
        let text = SHGetNameFromIDList(pidl, SIGDN_NORMALDISPLAY).ok()?;
        let value = pwstr_to_string(text);
        if !text.is_null() {
            CoTaskMemFree(Some(text.0 as *const _));
        }
        let value = value.trim().to_string();
        if value.is_empty() { None } else { Some(value) }
    }
}

#[cfg(target_os = "windows")]
fn shell_folder_child_name(
    folder: &windows::Win32::UI::Shell::IShellFolder,
    child: *const windows::Win32::UI::Shell::Common::ITEMIDLIST,
    flags: windows::Win32::UI::Shell::SHGDNF,
) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::StrRetToStrW;
    use windows::core::PWSTR;

    if child.is_null() {
        return None;
    }

    unsafe {
        let mut strret = std::mem::zeroed();
        folder.GetDisplayNameOf(child, flags, &mut strret).ok()?;
        let mut text = PWSTR::null();
        StrRetToStrW(&mut strret, Some(child), &mut text).ok()?;
        let value = pwstr_to_string(text);
        if !text.is_null() {
            CoTaskMemFree(Some(text.0 as *const _));
        }
        let value = value.trim().to_string();
        if value.is_empty() { None } else { Some(value) }
    }
}

#[cfg(target_os = "windows")]
fn property_store_string(
    store: &windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore,
    key: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Com::StructuredStorage::{
        PropVariantClear, PropVariantToStringAlloc, PropVariantToStringVectorAlloc,
    };

    unsafe {
        let mut value = store.GetValue(key as *const _).ok()?;
        if value.is_empty() {
            return None;
        }

        let mut strings = std::ptr::null_mut();
        let mut count = 0;
        let vector = if PropVariantToStringVectorAlloc(&value, &mut strings, &mut count).is_ok()
            && !strings.is_null()
        {
            let values = std::slice::from_raw_parts(strings, count as usize);
            let result = values
                .iter()
                .take(256)
                .map(|text| pwstr_to_string(*text))
                .collect::<Vec<_>>()
                .join(" ");
            for text in values {
                CoTaskMemFree(Some(text.0 as *const _));
            }
            CoTaskMemFree(Some(strings as *const _));
            result
        } else {
            String::new()
        };
        let string = if vector.is_empty() {
            PropVariantToStringAlloc(&value)
                .ok()
                .map(|text| {
                    let value = pwstr_to_string(text);
                    if !text.is_null() {
                        CoTaskMemFree(Some(text.0 as *const _));
                    }
                    value
                })
                .unwrap_or_default()
        } else {
            vector
        };
        let _ = PropVariantClear(&mut value);

        let string = string.trim().to_string();
        if string.is_empty() {
            None
        } else {
            Some(string)
        }
    }
}

#[cfg(target_os = "windows")]
fn clean_network_device_name(name: &str) -> String {
    let trimmed = name.trim().trim_matches('"');
    let unc = trimmed.trim_start_matches('\\');
    let candidate = if unc.len() != trimmed.len() {
        unc.split('\\')
            .find(|part| !part.trim().is_empty())
            .unwrap_or(trimmed)
    } else {
        trimmed
    };

    let candidate = candidate.trim();
    if candidate.is_empty()
        || candidate.to_ascii_lowercase().contains("provider\\")
        || candidate.starts_with('{')
        || candidate.eq_ignore_ascii_case("network")
    {
        String::new()
    } else {
        candidate.to_string()
    }
}

fn wnet_network_shares(host: &str) -> Vec<NetworkShareInfo> {
    let mut remote = wide_null(&format!(r"\\{host}"));
    let parent = windows::Win32::NetworkManagement::WNet::NETRESOURCEW {
        dwScope: windows::Win32::NetworkManagement::WNet::RESOURCE_GLOBALNET,
        dwType: windows::Win32::NetworkManagement::WNet::RESOURCETYPE_DISK,
        dwDisplayType: 0,
        dwUsage: windows::Win32::NetworkManagement::WNet::RESOURCEUSAGE_CONTAINER.0,
        lpLocalName: windows::core::PWSTR::null(),
        lpRemoteName: windows::core::PWSTR(remote.as_mut_ptr()),
        lpComment: windows::core::PWSTR::null(),
        lpProvider: windows::core::PWSTR::null(),
    };

    wnet_enumerate(Some(&parent), false)
        .into_iter()
        .filter(|resource| {
            resource.resource_type == windows::Win32::NetworkManagement::WNet::RESOURCETYPE_DISK
        })
        .filter_map(|resource| {
            let share_name = unc_share_name(&resource.remote_name)?;
            Some(NetworkShareInfo {
                name: share_name,
                remark: resource.comment,
            })
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn wnet_enumerate(
    parent: Option<&windows::Win32::NetworkManagement::WNet::NETRESOURCEW>,
    publish: bool,
) -> Vec<WNetResourceInfo> {
    use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HANDLE, WIN32_ERROR};
    use windows::Win32::NetworkManagement::WNet::{
        NETRESOURCEW, RESOURCE_GLOBALNET, RESOURCETYPE_ANY, RESOURCEUSAGE_ALL, WNetCloseEnum,
        WNetEnumResourceW, WNetOpenEnumW,
    };

    unsafe {
        let mut handle = HANDLE::default();
        let open_status = WNetOpenEnumW(
            RESOURCE_GLOBALNET,
            RESOURCETYPE_ANY,
            RESOURCEUSAGE_ALL,
            parent.map(|resource| resource as *const NETRESOURCEW),
            &mut handle,
        );
        if open_status != WIN32_ERROR(0) {
            return Vec::new();
        }

        let mut output = Vec::new();
        loop {
            let mut count = u32::MAX;
            let mut buffer_size = 64 * 1024_u32;
            let mut buffer = vec![0_u8; buffer_size as usize];
            let status = WNetEnumResourceW(
                handle,
                &mut count,
                buffer.as_mut_ptr() as *mut _,
                &mut buffer_size,
            );

            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != WIN32_ERROR(0) || count == 0 {
                break;
            }

            let resources =
                std::slice::from_raw_parts(buffer.as_ptr() as *const NETRESOURCEW, count as usize);
            for resource in resources {
                output.push(WNetResourceInfo {
                    remote_name: pwstr_to_string(resource.lpRemoteName),
                    provider_name: pwstr_to_string(resource.lpProvider),
                    comment: pwstr_to_string(resource.lpComment),
                    usage: resource.dwUsage,
                    display_type: resource.dwDisplayType,
                    resource_type: resource.dwType,
                });
                if publish && let Some(computer) = output.last().and_then(wnet_computer) {
                    publish_network_computer(computer);
                }
            }
        }

        let _ = WNetCloseEnum(handle);
        output
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn is_windows_network_provider(value: &str) -> bool {
    if value.contains("::") {
        return value.split("::").any(is_windows_network_provider);
    }
    let normalized: String = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "microsoftwindowsnetwork"
            | "microsoftterminalservices"
            | "microsoftremotedesktopservices"
            | "microsoftremotedesktopnetwork"
            | "plan9networkprovider"
            | "webclientnetwork"
            | "webclientnetworkprovider"
            | "microsoftpublicationservicedevicehost"
    )
}

#[cfg(target_os = "windows")]
fn unc_share_name(remote_name: &str) -> Option<String> {
    let parts = unc_parts(remote_name);
    if parts.len() == 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn unc_parts(remote_name: &str) -> Vec<&str> {
    remote_name
        .trim()
        .trim_start_matches('\\')
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_netbios_cache_names() {
        let output = r#"
                  Tabla cachâ€š remota de NetBIOS

        Nombre            Tipo       Dir de Host     Vida [s]
    ------------------------------------------------------------
    RBODEGA-LT     <20>  Ã©nico           172.21.53.22        528
    DESKTOP-ALRHH08<20>  Ã©nico           172.21.53.71        529
    WORKGROUP      <1E>  Grupo           172.21.53.22        224
"#;

        let names = parse_nbtstat_names(output);
        assert_eq!(names, vec!["DESKTOP-ALRHH08", "RBODEGA-LT"]);
    }

    #[test]
    fn parses_netbios_node_status_name() {
        let output = r#"
   Tabla de nombres de equipos remotos de NetBIOS

       Nombre             Tipo         Estado
    ---------------------------------------------
    DESKTOP-0DHC1AV<00>  Ã©nico       Registrado
    WORKGROUP      <00>  Grupo       Registrado
    DESKTOP-0DHC1AV<20>  Ã©nico       Registrado
"#;

        assert_eq!(
            parse_nbtstat_name(output).as_deref(),
            Some("DESKTOP-0DHC1AV")
        );
    }

    #[test]
    fn recognizes_canon_image_runner_as_a_network_printer() {
        assert_eq!(
            classify_network_device_kind("iR-ADV 529", "Dispositivo de red"),
            crate::platform::NetworkDeviceKind::Printer
        );
    }

    fn wnet_resource(name: &str, display_type: u32) -> WNetResourceInfo {
        WNetResourceInfo {
            remote_name: name.into(),
            provider_name: "Microsoft Windows Network".into(),
            comment: String::new(),
            usage: windows::Win32::NetworkManagement::WNet::RESOURCEUSAGE_CONTAINER.0,
            display_type,
            resource_type: windows::Win32::NetworkManagement::WNet::RESOURCETYPE_ANY,
        }
    }

    #[test]
    fn wnet_provider_and_domain_containers_are_not_computers() {
        for name in [
            "Microsoft Terminal Services",
            "Plan9NetworkProvider",
            "WebClientNetwork",
            "Microsoft Windows Network",
        ] {
            assert_eq!(wnet_host_name(&wnet_resource(name, 6)), None, "{name}");
            assert_eq!(wnet_host_name(&wnet_resource(name, 0)), None, "{name}");
            assert!(is_windows_network_provider(name));
        }
        assert_eq!(wnet_host_name(&wnet_resource(r"\\WORKGROUP", 1)), None);
        assert_eq!(wnet_host_name(&wnet_resource("WORKGROUP", 0)), None);
        assert_eq!(
            wnet_host_name(&wnet_resource(r"\\MARTHAOPORTA", 2)).as_deref(),
            Some("MARTHAOPORTA")
        );
        assert_eq!(
            wnet_host_name(&wnet_resource(r"\\NAS\Documents", 3)).as_deref(),
            Some("NAS")
        );
        assert_eq!(
            wnet_host_name(&wnet_resource(r"\\NAS", 0)).as_deref(),
            Some("NAS")
        );
        assert!(!is_windows_network_provider("SERVER-PLAN9"));
        assert!(!is_windows_network_provider("WEBCLIENT-PC"));
    }

    #[test]
    fn device_categories_take_precedence_over_manufacturer_or_model_guesses() {
        use crate::platform::NetworkDeviceKind::{
            Computer, Multifunction, Other, Printer, Scanner,
        };
        assert_eq!(
            network_kind_with_metadata("Canon iR-ADV 529", "Imaging.Scanner", "Device"),
            Scanner
        );
        assert_eq!(
            network_kind_with_metadata(
                "EPSON device",
                "PrintFax.Printer Imaging.Scanner",
                "Device"
            ),
            Multifunction
        );
        assert_eq!(
            network_kind_with_metadata(
                "Canon iR-ADV 529",
                "Imaging.MultifunctionPrinter",
                "Device"
            ),
            Multifunction
        );
        assert_eq!(
            network_kind_with_metadata("Printer Support", "Computer.Desktop", "Device"),
            Computer
        );
        assert_eq!(
            network_kind_with_metadata("Canon Camera", "", "Device"),
            Other
        );
        assert_eq!(
            network_kind_with_metadata("Network scanner", "", "Device"),
            Scanner
        );
        assert_eq!(
            network_kind_with_metadata("HP LaserJet", "", "Device"),
            Printer
        );
    }

    #[test]
    fn printer_and_scanner_functions_of_the_same_device_enrich_as_multifunction() {
        use crate::platform::NetworkDeviceKind::{Multifunction, Printer, Scanner};
        let mut devices = vec![NetworkComputerInfo {
            name: "Office MFD".into(),
            comment: "Printer".into(),
            kind: Printer,
        }];
        merge_network_computers(
            &mut devices,
            vec![NetworkComputerInfo {
                name: "office mfd".into(),
                comment: "Scanner".into(),
                kind: Scanner,
            }],
        );
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].kind, Multifunction);
    }

    #[test]
    fn shell_unc_identity_classifies_computers_without_model_name_guesses() {
        use crate::platform::NetworkDeviceKind::{Computer, Multifunction, Other};
        for name in [
            "DIRPROYECTO",
            "GRAFICO2",
            "MARTHAOPORTA",
            "SECRETARIA-MUNI",
            "TECPROYECTO",
        ] {
            let parsing_name = format!(r"\\{name}");
            assert!(shell_parsing_name_is_computer(&parsing_name));
            let mut device = NetworkComputerInfo {
                name: name.into(),
                comment: "Dispositivo de red".into(),
                kind: Other,
            };
            apply_network_computer_hint(&mut device, shell_parsing_name_is_computer(&parsing_name));
            assert_eq!(device.kind, Computer);
        }
        for parsing_name in [
            "MARTHAOPORTA",
            "Web Client Network",
            r"\\?\SWD\PRINT",
            r"\\.\DEVICE",
            "::{network}\\device",
        ] {
            assert!(!shell_parsing_name_is_computer(parsing_name));
        }
        let mut multifunction = NetworkComputerInfo {
            name: "EPSONDF2594".into(),
            comment: "Multifunction".into(),
            kind: Multifunction,
        };
        apply_network_computer_hint(&mut multifunction, true);
        assert_eq!(multifunction.kind, Multifunction);
    }
}

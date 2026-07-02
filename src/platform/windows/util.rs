#[cfg(target_os = "windows")]
pub(super) fn pwstr_to_string(value: windows::core::PWSTR) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut len = 0;
    unsafe {
        while *value.0.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(value.0, len))
    }
}

#[cfg(target_os = "windows")]
pub(super) fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub(super) fn wide_to_string(buffer: &[u16]) -> Option<String> {
    let len = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    let value = String::from_utf16_lossy(&buffer[..len]).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

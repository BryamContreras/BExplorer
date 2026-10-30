use super::*;

const MIB: usize = 1024 * 1024;

pub(super) fn image_budget_for_memory(total: Option<u64>) -> usize {
    total
        .map(|bytes| (bytes / 32).min(160 * MIB as u64) as usize)
        .unwrap_or(160 * MIB)
        .clamp(32 * MIB, 160 * MIB)
}

pub(super) fn image_cache_budget() -> usize {
    #[cfg(target_os = "windows")]
    let total = {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        unsafe { GlobalMemoryStatusEx(&mut status) }
            .ok()
            .map(|_| status.ullTotalPhys)
    };
    #[cfg(target_os = "linux")]
    let total = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| {
                    line.strip_prefix("MemTotal:")?
                        .split_whitespace()
                        .next()?
                        .parse::<u64>()
                        .ok()
                })
                .map(|kib| kib.saturating_mul(1024))
        });
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let total = None;
    image_budget_for_memory(total)
}

impl BExplorerIced {
    pub(in crate::iced_ui) fn trim_image_memory(&mut self, minimized: bool) {
        if minimized {
            self.images_suspended = true;
        }
        // Auxiliary windows can remain visible while the main one is inactive.
        if self.duplicate_window_id.is_some() || self.storage_analysis_window_id.is_some() {
            return;
        }
        let mut keep = HashSet::new();
        if !minimized {
            for pane in [PaneId::Primary, PaneId::Secondary] {
                if pane == PaneId::Secondary && self.split.is_none() {
                    continue;
                }
                if let Some(visible) = self.visible_image_keys.get(&pane) {
                    keep.extend(visible.iter().cloned());
                }
                keep.extend(self.pane(pane).selected.iter().cloned());
            }
            keep.extend(self.sidebar_icon_cache_keys());
        }
        let budget = if minimized {
            0
        } else {
            self.image_cache_budget / 4
        };
        self.thumbnail_cache.trim_to(budget * 64 / 160, &keep);
        self.small_thumbnail_cache.trim_to(budget * 16 / 160, &keep);
        self.preview_cache.trim_to(budget * 32 / 160, &keep);
        self.native_icon_cache.trim_to(budget * 32 / 160, &keep);
        self.small_native_icon_cache
            .trim_to(budget * 16 / 160, &keep);
        // Selected PDF/text previews have their own bounds. Preserve their
        // state so restoring the window retains the page, selection and scroll.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_budget_scales_down_on_small_machines_without_exceeding_the_cap() {
        assert_eq!(image_budget_for_memory(Some(1024 * MIB as u64)), 32 * MIB);
        assert_eq!(image_budget_for_memory(Some(2048 * MIB as u64)), 64 * MIB);
        assert_eq!(image_budget_for_memory(Some(4096 * MIB as u64)), 128 * MIB);
        assert_eq!(
            image_budget_for_memory(Some(64 * 1024 * MIB as u64)),
            160 * MIB
        );
        assert_eq!(image_budget_for_memory(None), 160 * MIB);
    }
}

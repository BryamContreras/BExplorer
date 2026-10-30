use super::*;

const NETWORK_REFRESH_LIMIT: Duration = Duration::from_secs(90);
const NETWORK_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub(super) struct NetworkRefresh {
    deadline: Instant,
    revision: u64,
}

impl NetworkRefresh {
    pub(super) fn new(now: Instant, revision: u64) -> Self {
        Self {
            deadline: now + NETWORK_REFRESH_LIMIT,
            revision,
        }
    }

    fn waiting(&self, now: Instant, ui_pending: usize, native_pending: bool) -> bool {
        now < self.deadline && (ui_pending != 0 || native_pending)
    }

    fn observe(&mut self, revision: u64) -> bool {
        let changed = self.revision != revision;
        self.revision = revision;
        changed
    }
}

pub(super) fn network_discovery_poll_task(pane: PaneId, request_id: u64) -> Task<Message> {
    Task::perform(delay(NETWORK_REFRESH_INTERVAL), move |_| {
        Message::NetworkDiscoveryPoll(pane, request_id)
    })
}

impl BExplorerIced {
    pub(super) fn is_current_network_discovery(&self, pane: PaneId, request_id: u64) -> bool {
        (pane == PaneId::Primary || self.split.is_some())
            && self
                .tab_for_pane(pane)
                .path
                .as_deref()
                .is_some_and(explorer::is_network_root_path)
            && self.pane(pane).request_id == request_id
    }

    pub(super) fn update_network_discovery_status(
        &mut self,
        pane: PaneId,
        native_pending: bool,
    ) -> bool {
        let searching = self
            .localized(
                "Buscando dispositivos de red…",
                "Searching network devices…",
            )
            .to_owned();
        let elements = self.localized("elementos", "items");
        let results = self.localized("resultados", "results");
        let incomplete = self.localized(
            "Algunos proveedores no respondieron; se muestran los resultados disponibles.",
            "Some providers did not respond; showing available results.",
        );
        let state = self.pane_mut(pane);
        state.loading = state.network_refresh.as_ref().is_some_and(|refresh| {
            refresh.waiting(
                Instant::now(),
                state.network_discovery_pending,
                native_pending,
            )
        });
        state.status = if state.loading {
            format!("{} {results} · {searching}", state.entries.len())
        } else if state.network_discovery_pending != 0 || native_pending {
            format!("{} {elements} · {incomplete}", state.entries.len())
        } else {
            format!("{} {elements}", state.entries.len())
        };
        if !state.loading {
            let entries = state.entries.clone();
            let request_id = state.request_id;
            schedule_network_cache(entries);
            self.startup.complete_pane_load(pane, request_id);
            false
        } else {
            true
        }
    }

    pub(super) fn poll_network_discovery(
        &mut self,
        pane: PaneId,
        request_id: u64,
    ) -> Task<Message> {
        if !self.is_current_network_discovery(pane, request_id) {
            return Task::none();
        }
        // Read activity before the revision: a completed native query publishes
        // its observations before releasing its pending key. This ordering
        // also collects the final result when its original UI waiter timed out.
        let native_pending = explorer::network_discovery_pending();
        let revision = explorer::network_root_cache_revision();
        let changed = self
            .pane_mut(pane)
            .network_refresh
            .as_mut()
            .is_some_and(|refresh| refresh.observe(revision));
        let images = if changed {
            let entries = explorer::network_root_cached_entries();
            let state = self.pane_mut(pane);
            explorer::merge_network_entries(&mut state.entries, entries);
            explorer::sort_entries_by_name(&mut state.entries);
            state.folder_entries = None;
            state.has_vertical_overflow = false;
            state.mark_entries_changed();
            self.queue_visible_images(pane)
        } else {
            Task::none()
        };
        if self.update_network_discovery_status(pane, native_pending) {
            Task::batch([images, network_discovery_poll_task(pane, request_id)])
        } else {
            images
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ui_timeout_does_not_finish_native_discovery_or_require_refresh() {
        let start = Instant::now();
        let mut refresh = NetworkRefresh::new(start, 1);
        // The ten-second waiter has returned, but the cold native provider
        // is still discovering devices; its later observation is collected.
        assert!(refresh.waiting(start + Duration::from_secs(11), 0, true));
        assert!(refresh.observe(2));
        assert!(!refresh.observe(2));
        assert!(!refresh.waiting(start + Duration::from_secs(13), 0, false));
    }

    #[test]
    fn queued_host_results_keep_progress_but_a_stalled_provider_has_a_deadline() {
        let start = Instant::now();
        let refresh = NetworkRefresh::new(start, 0);
        assert!(refresh.waiting(start + Duration::from_secs(12), 3, false));
        assert!(!refresh.waiting(start + NETWORK_REFRESH_LIMIT, 3, true));
    }
}

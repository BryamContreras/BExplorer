use super::*;

impl BExplorerIced {
    pub(super) fn authorize_directory_listing(
        &mut self,
        pane: PaneId,
        denied_request: u64,
    ) -> Task<Message> {
        if pane == PaneId::Secondary && self.split.is_none() {
            return Task::none();
        }
        let state = self.pane(pane);
        if state.request_id != denied_request
            || state.directory_access != DirectoryAccess::Denied
            || state.loading
        {
            return Task::none();
        }
        let Some(path) = self.tab_for_pane(pane).path.clone() else {
            return Task::none();
        };

        self.focus_pane(pane);
        let status = self.localized("Esperando autorización…", "Waiting for authorization…");
        let (token, registration) = DirectoryLoadToken::new();
        let cancel = Arc::clone(&token.cancel);
        let state = self.pane_mut(pane);
        state.request_id += 1;
        state.loading = true;
        state.status = status.into();
        state.directory_load = Some(token);
        let request_id = state.request_id;
        let show_hidden = self.config.show_hidden;
        let requested_path = path.clone();

        Task::perform(
            iced::futures::future::Abortable::new(
                run_blocking_file_operation(move || {
                    if cancel.load(AtomicOrdering::Relaxed) {
                        return Ok(None);
                    }
                    crate::fs::elevated_browse::list_directory_elevated(
                        &requested_path,
                        show_hidden,
                    )
                }),
                registration,
            ),
            move |result| match result {
                Ok(result) => Message::AuthorizedDirectoryLoaded(pane, request_id, path, result),
                Err(_) => Message::Noop,
            },
        )
    }

    pub(super) fn finish_authorized_directory_listing(
        &mut self,
        pane: PaneId,
        request_id: u64,
        path: &Path,
        result: Result<Option<Vec<FileEntry>>, String>,
    ) -> Task<Message> {
        if (pane == PaneId::Secondary && self.split.is_none())
            || self.pane(pane).request_id != request_id
            || self.tab_for_pane(pane).path.as_deref() != Some(path)
            || self.pane(pane).directory_access != DirectoryAccess::Denied
        {
            return Task::none();
        }
        if let Ok(Some(entries)) = result {
            self.pane_mut(pane).directory_access = DirectoryAccess::AuthorizedListing;
            return self.update(Message::Loaded(pane, request_id, Ok(entries)));
        }

        let failure = self.localized(
            "No se pudo autorizar el listado",
            "Could not authorize listing",
        );
        let cancelled = self.localized("Autorización cancelada", "Authorization canceled");
        let state = self.pane_mut(pane);
        state.directory_load = None;
        state.loading = false;
        if let Err(error) = result {
            state.status = failure.into();
            self.show_error_dialog(failure.to_owned(), error)
        } else {
            state.status = cancelled.into();
            Task::none()
        }
    }
}

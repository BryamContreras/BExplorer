use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn prepare_portable_clipboard(
        &mut self,
        pane: PaneId,
        paths: Vec<PathBuf>,
    ) -> Task<Message> {
        self.pane_mut(pane).status = "Preparando elementos MTP…".into();
        Task::perform(
            run_blocking_file_operation(move || portable::stage_paths_for_clipboard(&paths)),
            move |result| Message::PortableClipboardPrepared(pane, result),
        )
    }

    pub(in crate::iced_ui) fn prepare_portable_open(
        &mut self,
        pane: PaneId,
        path: PathBuf,
    ) -> Task<Message> {
        self.pane_mut(pane).status = "Descargando archivo MTP…".into();
        Task::perform(
            run_blocking_file_operation(move || portable::stage_file_for_open(&path)),
            move |result| Message::PortableOpenPrepared(pane, result),
        )
    }

    pub(in crate::iced_ui) fn delete_portable_paths(
        &mut self,
        pane: PaneId,
        paths: Vec<PathBuf>,
    ) -> Task<Message> {
        self.pane_mut(pane).status = "Eliminando elementos MTP…".into();
        Task::perform(
            run_blocking_file_operation(move || portable::delete_paths(&paths)),
            move |result| Message::PortableDeleteFinished(pane, result),
        )
    }

    pub(in crate::iced_ui) fn transfer_with_portable(
        &mut self,
        pane: PaneId,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        kind: TransferKind,
        clear_clipboard: bool,
    ) -> Task<Message> {
        if sources.is_empty() {
            self.pane_mut(pane).status = "No hay elementos para copiar".into();
            return Task::none();
        }

        let portable_sources = sources
            .iter()
            .filter(|source| explorer::is_portable_path(source))
            .count();
        if portable_sources != 0 && portable_sources != sources.len() {
            self.pane_mut(pane).status =
                "No se pueden mezclar elementos MTP y archivos locales".into();
            return Task::none();
        }
        let destination_is_portable = explorer::is_portable_path(&destination);
        let source_directories = sources
            .iter()
            .filter_map(|source| source.parent().map(Path::to_path_buf));
        let refresh_directories = std::iter::once(destination.clone())
            .chain(source_directories)
            .collect::<Vec<_>>();
        self.pane_mut(pane).status = if destination_is_portable {
            "Copiando al dispositivo MTP…".into()
        } else {
            "Copiando desde el dispositivo MTP…".into()
        };

        Task::perform(
            run_blocking_file_operation(move || {
                let source_is_portable = portable_sources == sources.len();
                let mut completed = 0_usize;

                if destination_is_portable {
                    let local_sources = if source_is_portable {
                        portable::stage_paths_for_clipboard(&sources)?
                    } else {
                        sources.clone()
                    };
                    for source in &local_sources {
                        let mut on_event = |_event: portable::PortableTransferEvent<'_>| Ok(());
                        completed +=
                            portable::import_from_local(source, &destination, &mut on_event)?;
                    }
                } else if source_is_portable {
                    let mut reserved = Vec::new();
                    for source in &sources {
                        let target = portable::unique_local_destination(
                            &destination.join(portable::path_name(source)),
                            portable::path_is_folder(source),
                            &reserved,
                        );
                        reserved.push(target.clone());
                        let mut on_event = |_event: portable::PortableTransferEvent<'_>| Ok(());
                        completed += portable::export_to_local(source, &target, &mut on_event)?;
                    }
                } else {
                    return Err(BExplorerError::Operation(
                        "La transferencia no contiene una ruta MTP".into(),
                    ));
                }

                if kind == TransferKind::Move {
                    if source_is_portable {
                        portable::delete_paths(&sources)?;
                    } else {
                        operations::delete_permanently(&sources)?;
                    }
                }
                Ok(completed)
            }),
            move |result| {
                Message::PortableTransferFinished(
                    pane,
                    refresh_directories,
                    clear_clipboard,
                    result,
                )
            },
        )
    }

    pub(in crate::iced_ui) fn mount_disk_image(
        &mut self,
        pane: PaneId,
        path: PathBuf,
    ) -> Task<Message> {
        if !self.mounting_disk_images.insert(path.clone()) {
            self.pane_mut(pane).status = format!("Ya se está montando {}…", path.display());
            return Task::none();
        }
        let state = self.pane_mut(pane);
        state.mounting_disk_image = true;
        state.search_progress_phase = 0.0;
        state.status = format!("Montando {}…", path.display());
        let operation_path = path.clone();
        Task::perform(
            run_blocking_file_operation(move || {
                operations::mount_disk_image(&operation_path)?;
                let root = operations::mounted_disk_image_root(&operation_path)?;
                operations::suppress_file_explorer_windows_at(&root)?;
                Ok(root)
            }),
            move |result| Message::DiskImageMounted(pane, path, result),
        )
    }

    pub(in crate::iced_ui) fn eject_drive(&mut self, pane: PaneId, path: PathBuf) -> Task<Message> {
        self.pane_mut(pane).status = format!("Expulsando {}…", path.display());
        let operation_path = path.clone();
        Task::perform(
            run_blocking_file_operation(move || operations::eject_drive(&operation_path)),
            move |result| Message::DriveEjected(pane, path, result),
        )
    }

    pub(in crate::iced_ui) fn start_defender_scan(
        &mut self,
        pane: PaneId,
        paths: Vec<PathBuf>,
    ) -> Task<Message> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = paths;
            self.pane_mut(pane).status =
                "Microsoft Defender solo está disponible en Windows".into();
            Task::none()
        }

        #[cfg(target_os = "windows")]
        {
            let paths = paths
                .into_iter()
                .filter(|path| !explorer::is_virtual_path(path))
                .collect::<Vec<_>>();
            if paths.is_empty() {
                self.pane_mut(pane).status =
                    "Defender no puede analizar esta ubicación virtual".into();
                return Task::none();
            }
            if let Some(cancel) = self.defender_cancel.take() {
                cancel.store(true, AtomicOrdering::Relaxed);
            }
            let close_threats = self
                .defender_threats_window_id
                .take()
                .map(window::close)
                .unwrap_or_else(Task::none);

            let (sender, receiver) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let job = DefenderJob {
                paths: paths.clone(),
            };
            self.defender_rx = Some(receiver);
            self.defender_cancel = Some(cancel);
            self.defender_summary = None;
            self.defender_threat_remediation_pending = false;
            self.defender_threat_remediation_message = None;
            self.defender_progress = Some(DefenderProgress {
                state: DefenderScanState::Running,
                current_path: paths.first().cloned(),
                scanned: 0,
                total: paths.len(),
                threats_found: 0,
                started: Instant::now(),
            });
            self.pane_mut(pane).status = "Analizando con Microsoft Defender…".into();
            thread::spawn(move || defender::run_scan(job, sender, worker_cancel));
            Task::batch([close_threats, self.ensure_defender_window_task()])
        }
    }

    pub(in crate::iced_ui) fn defender_active(&self) -> bool {
        self.defender_rx.is_some()
            || self
                .defender_progress
                .as_ref()
                .is_some_and(|progress| progress.state == DefenderScanState::Running)
    }

    pub(in crate::iced_ui) fn cancel_defender_scan(&mut self) {
        if let Some(cancel) = &self.defender_cancel {
            cancel.store(true, AtomicOrdering::Relaxed);
        }
    }

    pub(in crate::iced_ui) fn close_defender_panel(&mut self) -> Task<Message> {
        self.cancel_defender_scan();
        self.defender_progress = None;
        self.defender_summary = None;
        self.defender_rx = None;
        self.defender_cancel = None;
        self.defender_threat_remediation_pending = false;
        self.defender_threat_remediation_message = None;
        let close_scan = self
            .defender_window_id
            .take()
            .map(window::close)
            .unwrap_or_else(Task::none);
        let close_threats = self
            .defender_threats_window_id
            .take()
            .map(window::close)
            .unwrap_or_else(Task::none);
        Task::batch([close_scan, close_threats])
    }

    #[cfg(target_os = "windows")]
    pub(in crate::iced_ui) fn ensure_defender_window_task(&mut self) -> Task<Message> {
        if let Some(id) = self.defender_window_id {
            return Task::batch([
                self.sync_defender_window_size_task(),
                window::minimize(id, false),
                window::gain_focus(id),
            ]);
        }
        let (id, task) = window::open(defender_window_settings(self.defender_window_size()));
        self.defender_window_id = Some(id);
        task.map(Message::DefenderWindowOpened)
    }

    pub(in crate::iced_ui) fn defender_window_size(&self) -> Size {
        let detail_lines = self
            .defender_summary
            .as_ref()
            .map(|summary| usize::from(summary.error.is_some()))
            .unwrap_or_default();
        defender_window_size_for_detail_lines(detail_lines)
    }

    pub(in crate::iced_ui) fn sync_defender_window_size_task(&mut self) -> Task<Message> {
        self.defender_window_id
            .map(|id| sync_fixed_progress_window_size_task(id, self.defender_window_size()))
            .unwrap_or_else(Task::none)
    }

    pub(in crate::iced_ui) fn ensure_defender_threats_window_task(&mut self) -> Task<Message> {
        let threat_count = self
            .defender_summary
            .as_ref()
            .map(|summary| summary.threats.len())
            .unwrap_or_default();
        let close_scan = self
            .defender_window_id
            .take()
            .map(window::close)
            .unwrap_or_else(Task::none);
        if let Some(id) = self.defender_threats_window_id {
            return Task::batch([
                close_scan,
                window::minimize(id, false),
                window::gain_focus(id),
            ]);
        }
        let (id, task) = window::open(defender_threats_window_settings(threat_count));
        self.defender_threats_window_id = Some(id);
        Task::batch([close_scan, task.map(Message::DefenderThreatsWindowOpened)])
    }

    pub(in crate::iced_ui) fn remediate_defender_threats_task(&mut self) -> Task<Message> {
        if self.defender_threat_remediation_pending {
            return Task::none();
        }
        let mut paths = self
            .defender_summary
            .as_ref()
            .map(|summary| {
                let detected = summary
                    .threats
                    .iter()
                    .filter_map(|threat| threat.path.clone())
                    .collect::<Vec<_>>();
                if detected.is_empty() {
                    summary.paths.clone()
                } else {
                    detected
                }
            })
            .unwrap_or_default();
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            self.defender_threat_remediation_message = Some((
                self.localized(
                    "No se encontró una ruta para que Defender la procese.",
                    "No path was available for Defender to remediate.",
                )
                .to_owned(),
                true,
            ));
            return Task::none();
        }

        self.defender_threat_remediation_pending = true;
        self.defender_threat_remediation_message = None;
        #[cfg(target_os = "windows")]
        {
            Task::perform(
                run_blocking_file_operation(move || {
                    shell::remediate_paths_with_windows_defender(&paths)
                }),
                |result| {
                    Message::DefenderThreatRemediationFinished(
                        result.map_err(|error| error.to_string()),
                    )
                },
            )
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.defender_threat_remediation_pending = false;
            self.defender_threat_remediation_message = Some((
                self.localized(
                    "Microsoft Defender solo está disponible en Windows.",
                    "Microsoft Defender is only available on Windows.",
                )
                .to_owned(),
                true,
            ));
            Task::none()
        }
    }

    pub(in crate::iced_ui) fn poll_defender_messages(&mut self) -> Task<Message> {
        let mut finished = false;
        while let Some(message) = self
            .defender_rx
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            match message {
                DefenderMessage::Progress(progress) => self.defender_progress = Some(progress),
                DefenderMessage::Finished(summary) => {
                    let threats = summary.threats.len();
                    self.defender_progress = Some(defender_progress_from_summary(&summary));
                    self.defender_summary = Some(summary);
                    self.pane_mut(self.focused_pane()).status = if threats == 0 {
                        "Microsoft Defender no encontró amenazas".into()
                    } else {
                        format!("Microsoft Defender encontró {threats} amenaza(s)")
                    };
                    finished = true;
                }
                DefenderMessage::Failed(summary) => {
                    self.pane_mut(self.focused_pane()).status = summary
                        .error
                        .clone()
                        .unwrap_or_else(|| "El análisis de Defender falló".into());
                    self.defender_progress = Some(defender_progress_from_summary(&summary));
                    self.defender_summary = Some(summary);
                    finished = true;
                }
                DefenderMessage::Cancelled(summary) => {
                    self.pane_mut(self.focused_pane()).status =
                        "Análisis de Defender cancelado".into();
                    self.defender_progress = Some(defender_progress_from_summary(&summary));
                    self.defender_summary = Some(summary);
                    finished = true;
                }
            }
        }
        if finished {
            self.defender_rx = None;
            self.defender_cancel = None;
            if self
                .defender_summary
                .as_ref()
                .is_some_and(|summary| !summary.threats.is_empty())
            {
                return self.ensure_defender_threats_window_task();
            }
        }
        Task::none()
    }
}

fn defender_progress_from_summary(summary: &DefenderSummary) -> DefenderProgress {
    DefenderProgress {
        state: summary.state,
        current_path: summary
            .paths
            .get(summary.scanned.saturating_sub(1))
            .cloned(),
        scanned: summary.scanned,
        total: summary.total,
        threats_found: summary.threats.len(),
        started: Instant::now(),
    }
}

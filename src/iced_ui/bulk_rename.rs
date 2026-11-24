use super::*;
use crate::fs::bulk_rename::{Preview, Rule, Selection};

pub(super) const PREVIEW_PAGE_SIZE: usize = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mode {
    Replace,
    Number,
}

#[derive(Clone, Debug)]
pub(super) enum BulkRenameMessage {
    Prepared(Instant, Result<Arc<Selection>, String>),
    SetMode(Mode),
    FirstInput(String),
    SecondInput(String),
    ToggleExtension,
    PreviousPage,
    NextPage,
    Submit,
    Finished(Instant, Result<Vec<PathBuf>, String>),
    Cancel,
    CapturedEscape(window::Id),
}

pub(super) struct BulkRenameDialog {
    request: Instant,
    pane: PaneId,
    pub(super) selection: Option<Result<Arc<Selection>, String>>,
    pub(super) mode: Mode,
    find: String,
    pub(super) replacement: String,
    prefix: String,
    pub(super) preserve_extension: bool,
    pub(super) preview: Preview,
    pub(super) page: usize,
    pub(super) applying: bool,
}

impl BulkRenameDialog {
    fn new(pane: PaneId) -> Self {
        Self {
            request: Instant::now(),
            pane,
            selection: None,
            mode: Mode::Replace,
            find: String::new(),
            replacement: String::new(),
            prefix: String::new(),
            preserve_extension: true,
            preview: Preview::default(),
            page: 0,
            applying: false,
        }
    }

    pub(super) fn first_input(&self) -> &str {
        match self.mode {
            Mode::Replace => &self.find,
            Mode::Number => &self.prefix,
        }
    }

    fn refresh_preview(&mut self) {
        let Some(Ok(selection)) = &self.selection else {
            return;
        };
        let rule = match self.mode {
            Mode::Replace => Rule::Replace {
                find: &self.find,
                replacement: &self.replacement,
            },
            Mode::Number => Rule::Numbered {
                prefix: &self.prefix,
            },
        };
        self.preview = selection.preview(rule, self.preserve_extension);
    }

    pub(super) fn page_count(&self) -> usize {
        self.preview.rows.len().div_ceil(PREVIEW_PAGE_SIZE)
    }
}

pub(super) fn input_id(second: bool) -> Id {
    Id::new(if second {
        "bulk-rename-second"
    } else {
        "bulk-rename-first"
    })
}

fn focus_input(second: bool) -> Task<Message> {
    iced::widget::operation::focus(input_id(second))
}

pub(super) fn captured_escape_event(
    event: Event,
    status: event::Status,
    window: window::Id,
) -> Option<Message> {
    if status == event::Status::Captured
        && let Event::Keyboard(keyboard::Event::KeyPressed {
            key, repeat: false, ..
        }) = event
        && key.as_ref() == keyboard::Key::Named(keyboard::key::Named::Escape)
    {
        return Some(Message::BulkRename(BulkRenameMessage::CapturedEscape(
            window,
        )));
    }
    None
}

fn bulk_rename_key_message(key: keyboard::Key<&str>, mode: Mode) -> Option<BulkRenameMessage> {
    use keyboard::key::Named;

    Some(match key {
        keyboard::Key::Named(Named::Escape) => BulkRenameMessage::Cancel,
        keyboard::Key::Named(Named::Enter) => BulkRenameMessage::Submit,
        keyboard::Key::Named(Named::PageUp) => BulkRenameMessage::PreviousPage,
        keyboard::Key::Named(Named::PageDown) => BulkRenameMessage::NextPage,
        keyboard::Key::Named(Named::F3) => BulkRenameMessage::SetMode(match mode {
            Mode::Replace => Mode::Number,
            Mode::Number => Mode::Replace,
        }),
        keyboard::Key::Named(Named::F4) => BulkRenameMessage::ToggleExtension,
        _ => return None,
    })
}

impl BExplorerIced {
    pub(super) fn bulk_rename_applying(&self) -> bool {
        self.bulk_rename_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.applying)
    }

    pub(in crate::iced_ui) fn context_has_bulk_rename_selection(
        &self,
        pane: PaneId,
        target: ContextTarget,
    ) -> bool {
        let selected = &self.pane(pane).selected;
        cfg!(any(windows, target_os = "linux"))
            && selected.len() > 1
            && self
                .context_entry(pane, target)
                .is_some_and(|entry| selected.contains(&entry.path))
    }

    pub(in crate::iced_ui) fn context_rename_label(
        &self,
        pane: PaneId,
        target: ContextTarget,
    ) -> &'static str {
        if self.context_has_bulk_rename_selection(pane, target) {
            self.localized("Renombrar en lote", "Bulk rename")
        } else {
            self.localized("Renombrar", "Rename")
        }
    }

    pub(in crate::iced_ui) fn open_bulk_rename(
        &mut self,
        pane: PaneId,
        paths: Vec<PathBuf>,
    ) -> Task<Message> {
        if self.bulk_rename_dialog.is_some() || self.pending_file_operations.contains(&pane) {
            return Task::none();
        }
        let dialog = BulkRenameDialog::new(pane);
        let request = dialog.request;
        self.bulk_rename_dialog = Some(dialog);
        Task::perform(
            run_blocking_file_operation(move || Selection::open(paths).map(Arc::new)),
            move |result| Message::BulkRename(BulkRenameMessage::Prepared(request, result)),
        )
    }

    pub(super) fn handle_bulk_rename_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Task<Message> {
        let Some(dialog) = &self.bulk_rename_dialog else {
            return Task::none();
        };
        if modifiers.command() || modifiers.alt() || modifiers.logo() {
            return Task::none();
        }
        if key.as_ref() == keyboard::Key::Named(keyboard::key::Named::Tab) && !dialog.applying {
            return match dialog.mode {
                Mode::Replace => {
                    iced::widget::operation::is_focused(input_id(false)).then(focus_input)
                }
                Mode::Number => focus_input(false),
            };
        }
        bulk_rename_key_message(key.as_ref(), dialog.mode)
            .map(|message| self.update_bulk_rename(message))
            .unwrap_or_else(Task::none)
    }

    pub(super) fn update_bulk_rename(&mut self, message: BulkRenameMessage) -> Task<Message> {
        let Some(dialog) = &mut self.bulk_rename_dialog else {
            return Task::none();
        };
        match message {
            BulkRenameMessage::Finished(request, result) if dialog.request == request => {
                let pane = dialog.pane;
                let directory = dialog
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.as_ref().ok())
                    .map(|selection| selection.directory().to_owned());
                self.bulk_rename_dialog = None;
                self.pending_file_operations.remove(&pane);
                let status = match result {
                    Ok(renamed) => {
                        let count = renamed.len();
                        self.pane_mut(pane).selected = renamed.into_iter().collect();
                        if self.is_spanish() {
                            format!("{count} elementos renombrados")
                        } else {
                            format!("Renamed {count} items")
                        }
                    }
                    Err(error) => {
                        let refresh =
                            self.refresh_panes_for_directories(pane, directory.as_slice());
                        return Task::batch([refresh, self.report_error(pane, error)]);
                    }
                };
                self.pane_mut(pane).status = status;
                self.suppress_open_after_rename_until =
                    Some(Instant::now() + Duration::from_millis(200));
                self.refresh_panes_for_directories(pane, directory.as_slice())
            }
            _ if dialog.applying => Task::none(),
            BulkRenameMessage::Prepared(request, result) if dialog.request == request => {
                let can_focus = result.is_ok();
                dialog.selection = Some(result);
                dialog.refresh_preview();
                if can_focus {
                    focus_input(false)
                } else {
                    Task::none()
                }
            }
            BulkRenameMessage::SetMode(mode) => {
                dialog.mode = mode;
                dialog.page = 0;
                dialog.refresh_preview();
                focus_input(false)
            }
            BulkRenameMessage::FirstInput(value) => {
                match dialog.mode {
                    Mode::Replace => dialog.find = value,
                    Mode::Number => dialog.prefix = value,
                }
                dialog.refresh_preview();
                Task::none()
            }
            BulkRenameMessage::SecondInput(value) => {
                dialog.replacement = value;
                dialog.refresh_preview();
                Task::none()
            }
            BulkRenameMessage::ToggleExtension => {
                dialog.preserve_extension = !dialog.preserve_extension;
                dialog.refresh_preview();
                Task::none()
            }
            BulkRenameMessage::PreviousPage => {
                dialog.page = dialog.page.saturating_sub(1);
                Task::none()
            }
            BulkRenameMessage::NextPage => {
                dialog.page = (dialog.page + 1).min(dialog.page_count().saturating_sub(1));
                Task::none()
            }
            BulkRenameMessage::Submit if dialog.preview.can_apply() => {
                let Some(Ok(selection)) = &dialog.selection else {
                    return Task::none();
                };
                let pane = dialog.pane;
                let request = dialog.request;
                let preview = dialog.preview.clone();
                let selection = Arc::clone(selection);
                let status = self.localized("Renombrando en lote…", "Renaming items…");
                if !self.begin_file_operation(pane, status) {
                    return Task::none();
                }
                if let Some(dialog) = &mut self.bulk_rename_dialog {
                    dialog.applying = true;
                }
                Task::perform(
                    run_blocking_file_operation(move || selection.apply(&preview)),
                    move |result| Message::BulkRename(BulkRenameMessage::Finished(request, result)),
                )
            }
            BulkRenameMessage::Cancel => {
                self.bulk_rename_dialog = None;
                Task::none()
            }
            BulkRenameMessage::CapturedEscape(window) if self.main_window_id == Some(window) => {
                self.bulk_rename_dialog = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyboard::key::Named;

    #[test]
    fn keyboard_actions_control_only_the_dialog() {
        let key = |named| bulk_rename_key_message(keyboard::Key::Named(named), Mode::Replace);
        assert!(matches!(key(Named::Enter), Some(BulkRenameMessage::Submit)));
        assert!(matches!(
            key(Named::Escape),
            Some(BulkRenameMessage::Cancel)
        ));
        assert!(matches!(
            key(Named::F3),
            Some(BulkRenameMessage::SetMode(Mode::Number))
        ));
        assert!(matches!(
            key(Named::F4),
            Some(BulkRenameMessage::ToggleExtension)
        ));
        assert!(matches!(
            key(Named::PageDown),
            Some(BulkRenameMessage::NextPage)
        ));
        assert!(key(Named::Delete).is_none());
    }
}

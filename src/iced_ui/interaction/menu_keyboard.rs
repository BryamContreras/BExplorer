use super::*;

impl BExplorerIced {
    pub(in crate::iced_ui) fn context_open_with_scroll_id() -> Id {
        Id::new("context-open-with-keyboard-scroll")
    }

    pub(in crate::iced_ui) fn active_keyboard_menu(&self) -> Option<KeyboardMenu> {
        if self.context_menu.is_some() {
            return Some(if self.context_open_with_submenu {
                KeyboardMenu::ContextOpenWith
            } else if self.context_archive_submenu && self.context_extract_submenu {
                KeyboardMenu::ContextExtract
            } else if self.context_archive_submenu {
                KeyboardMenu::ContextArchive
            } else if self.context_new_submenu {
                KeyboardMenu::ContextNew
            } else {
                KeyboardMenu::Context
            });
        }
        if self.title_menu_open {
            return Some(if self.show_menu_open {
                KeyboardMenu::Show
            } else {
                KeyboardMenu::Title
            });
        }
        self.new_menu_open
            .map(KeyboardMenu::New)
            .or_else(|| self.search_mode_menu_open.map(KeyboardMenu::Search))
            .or_else(|| self.view_menu_open.map(KeyboardMenu::View))
            .or_else(|| self.group_menu_open.map(KeyboardMenu::Group))
    }

    pub(in crate::iced_ui) fn keyboard_menu_item_selected(
        &self,
        menu: KeyboardMenu,
        index: usize,
    ) -> bool {
        self.active_keyboard_menu() == Some(menu)
            && self.keyboard_menu_selection == Some(KeyboardMenuSelection { menu, index })
    }

    pub(in crate::iced_ui) fn keyboard_menu_has_selection(&self, menu: KeyboardMenu) -> bool {
        self.active_keyboard_menu() == Some(menu)
            && self
                .keyboard_menu_selection
                .is_some_and(|selection| selection.menu == menu)
    }

    pub(in crate::iced_ui) fn context_command_keyboard_selected(
        &self,
        command: ContextCommand,
    ) -> bool {
        let Some(selection) = self.keyboard_menu_selection else {
            return false;
        };
        if self.active_keyboard_menu() != Some(selection.menu) {
            return false;
        }
        self.context_keyboard_items(selection.menu)
            .get(selection.index)
            .is_some_and(|(_, selected)| *selected == command)
    }

    pub(in crate::iced_ui) fn select_keyboard_menu_item_by_character(
        &mut self,
        character: &str,
    ) -> Task<Message> {
        if character.chars().count() != 1 || !character.chars().all(char::is_alphanumeric) {
            return Task::none();
        }
        let Some(menu) = self.active_keyboard_menu() else {
            return Task::none();
        };
        let labels = self.keyboard_menu_labels(menu);
        let selected = self
            .keyboard_menu_selection
            .filter(|selection| selection.menu == menu)
            .map(|selection| selection.index);
        let Some(index) = next_keyboard_menu_match(&labels, selected, character) else {
            return Task::none();
        };
        self.keyboard_menu_selection = Some(KeyboardMenuSelection { menu, index });
        keyboard_menu_scroll_task(menu, index, labels.len())
    }

    pub(in crate::iced_ui) fn move_keyboard_menu_selection(
        &mut self,
        direction: i32,
    ) -> Task<Message> {
        let Some(menu) = self.active_keyboard_menu() else {
            return Task::none();
        };
        let count = self.keyboard_menu_labels(menu).len();
        if count == 0 {
            return Task::none();
        }
        let current = self
            .keyboard_menu_selection
            .filter(|selection| selection.menu == menu)
            .map(|selection| selection.index)
            .filter(|index| *index < count);
        let index = match (current, direction < 0) {
            (Some(index), true) => (index + count - 1) % count,
            (Some(index), false) => (index + 1) % count,
            (None, true) => count - 1,
            (None, false) => 0,
        };
        self.keyboard_menu_selection = Some(KeyboardMenuSelection { menu, index });
        keyboard_menu_scroll_task(menu, index, count)
    }

    pub(in crate::iced_ui) fn activate_keyboard_menu_selection(&mut self) -> Task<Message> {
        let Some(selection) = self.keyboard_menu_selection else {
            return Task::none();
        };
        if self.active_keyboard_menu() != Some(selection.menu) {
            return Task::none();
        }
        let message = match selection.menu {
            KeyboardMenu::Title => match selection.index {
                0 => Message::OpenShortcuts,
                1 => Message::OpenShowMenu,
                2 => Message::ToggleSettings,
                3 => Message::OpenAbout,
                _ => return Task::none(),
            },
            KeyboardMenu::Show => match selection.index {
                0 => Message::ToggleActionBar,
                1 => Message::ToggleBookmarkBar,
                2 => Message::ToggleSplitPaneMenus,
                3 => Message::ToggleSplitPreviewPanels,
                _ => return Task::none(),
            },
            KeyboardMenu::View(pane) => {
                let Some(mode) = view_menu_modes().get(selection.index).copied() else {
                    return Task::none();
                };
                Message::SetViewMode(pane, mode)
            }
            KeyboardMenu::Group(pane) => match selection.index {
                0 => Message::SetGroupMode(pane, GroupMode::None),
                1 => Message::SetGroupMode(pane, GroupMode::Type),
                2 => Message::SetGroupMode(pane, GroupMode::Name),
                3 => Message::SetGroupMode(pane, GroupMode::TotalSize),
                4 => Message::SetGroupAscending(pane, true),
                5 => Message::SetGroupAscending(pane, false),
                _ => return Task::none(),
            },
            KeyboardMenu::Search(pane) => match selection.index {
                0 => Message::SetSearchMode(pane, SearchMode::Quick),
                1 => Message::SetSearchMode(pane, SearchMode::Complete),
                _ => return Task::none(),
            },
            KeyboardMenu::New(pane) => match selection.index {
                0 => Message::NewFolder(pane),
                1 => Message::NewTextDocument(pane),
                _ => return Task::none(),
            },
            KeyboardMenu::Context
            | KeyboardMenu::ContextOpenWith
            | KeyboardMenu::ContextArchive
            | KeyboardMenu::ContextExtract
            | KeyboardMenu::ContextNew => {
                let Some((_, command)) = self
                    .context_keyboard_items(selection.menu)
                    .get(selection.index)
                    .cloned()
                else {
                    return Task::none();
                };
                return self.run_context_command(command);
            }
        };
        self.update(message)
    }

    fn keyboard_menu_labels(&self, menu: KeyboardMenu) -> Vec<String> {
        match menu {
            KeyboardMenu::Title => vec![
                self.localized("Atajos", "Shortcuts").into(),
                self.localized("Mostrar", "Show").into(),
                self.localized("Configuracion", "Settings").into(),
                self.localized("Acerca de", "About").into(),
            ],
            KeyboardMenu::Show => vec![
                self.localized("Barra de acciones", "Action bar").into(),
                self.localized("Barra de marcadores", "Bookmarks bar")
                    .into(),
                self.localized("Menu lateral en pantalla dividida", "Sidebar in split view")
                    .into(),
                self.localized(
                    "Panel de vista previa en pantalla dividida",
                    "Preview panel in split view",
                )
                .into(),
            ],
            KeyboardMenu::View(_) => view_menu_modes()
                .into_iter()
                .map(|mode| {
                    self.localized(view_mode_label(mode), view_mode_label_english(mode))
                        .to_owned()
                })
                .collect(),
            KeyboardMenu::Group(_) => vec![
                self.localized("Ninguno", "None").into(),
                self.localized("Tipo", "Type").into(),
                self.localized("Nombre", "Name").into(),
                self.localized("Tamaño", "Size").into(),
                self.localized("Ascendente", "Ascending").into(),
                self.localized("Descendente", "Descending").into(),
            ],
            KeyboardMenu::Search(_) => vec![
                self.localized("Búsqueda rápida", "Quick search").into(),
                self.localized("Búsqueda completa", "Full search").into(),
            ],
            KeyboardMenu::New(_) => vec![
                self.localized("Nueva carpeta", "New folder").into(),
                self.localized("Documento de texto", "Text document").into(),
            ],
            KeyboardMenu::Context
            | KeyboardMenu::ContextOpenWith
            | KeyboardMenu::ContextArchive
            | KeyboardMenu::ContextExtract
            | KeyboardMenu::ContextNew => self
                .context_keyboard_items(menu)
                .into_iter()
                .map(|(label, _)| label)
                .collect(),
        }
    }

    fn context_keyboard_items(&self, menu: KeyboardMenu) -> Vec<(String, ContextCommand)> {
        let Some(menu_state) = &self.context_menu else {
            return Vec::new();
        };
        match menu {
            KeyboardMenu::ContextOpenWith => {
                let mut items = menu_state
                    .open_with_applications
                    .iter()
                    .enumerate()
                    .map(|(index, application)| {
                        (
                            application.name.clone(),
                            ContextCommand::OpenWithApplication(index),
                        )
                    })
                    .collect::<Vec<_>>();
                items.push((
                    self.localized("Elegir otra aplicación", "Choose another app")
                        .into(),
                    ContextCommand::OpenWith,
                ));
                return items;
            }
            KeyboardMenu::ContextArchive => {
                return vec![
                    (
                        self.localized("Comprimir", "Compress").into(),
                        ContextCommand::CompressDialog,
                    ),
                    (
                        self.localized("Comprimir como 7z", "Compress as 7z").into(),
                        ContextCommand::CompressDefault(ArchiveFormat::SevenZip),
                    ),
                    (
                        self.localized("Comprimir como zip", "Compress as zip")
                            .into(),
                        ContextCommand::CompressDefault(ArchiveFormat::Zip),
                    ),
                ];
            }
            KeyboardMenu::ContextExtract => {
                return vec![
                    (
                        self.localized("Extraer aquí", "Extract here").into(),
                        ContextCommand::Extract(ExtractMode::Here),
                    ),
                    (
                        self.localized("Extraer en carpeta", "Extract to folder")
                            .into(),
                        ContextCommand::Extract(ExtractMode::ToNamedFolder),
                    ),
                ];
            }
            KeyboardMenu::ContextNew => {
                return vec![
                    (
                        self.localized("Nueva carpeta", "New folder").into(),
                        ContextCommand::NewFolder,
                    ),
                    (
                        self.localized("Documento de texto", "Text document").into(),
                        ContextCommand::NewTextDocument,
                    ),
                ];
            }
            KeyboardMenu::Context => {}
            _ => return Vec::new(),
        }

        let is_entry = matches!(menu_state.target, ContextTarget::Entry(_));
        let is_sidebar_drive = matches!(menu_state.target, ContextTarget::SidebarDrive(_));
        let is_search_result = is_entry && self.pane(menu_state.pane).folder_entries.is_some();
        let context_entry = self.context_entry(menu_state.pane, menu_state.target);
        let drive_entry = context_entry
            .as_ref()
            .is_some_and(|entry| entry.kind == EntryKind::Drive);
        let extractable_archive = context_entry.as_ref().is_some_and(|entry| {
            crate::fs::archive_listing::has_extractable_archive_extension(&entry.path)
        });
        let mountable_disk_image = context_entry
            .as_ref()
            .is_some_and(is_mountable_disk_image_entry);
        let ejectable_drive = context_entry
            .as_ref()
            .and_then(|entry| entry.drive_kind)
            .is_some_and(DriveKind::is_ejectable);
        let formatable_drive = context_entry.as_ref().is_some_and(|entry| {
            entry.kind == EntryKind::Drive && entry.drive_kind.is_some_and(DriveKind::is_formatable)
        });
        let terminal_available = !is_sidebar_drive
            && (!is_entry
                || context_entry.as_ref().is_some_and(|entry| {
                    entry.kind.is_container() && !explorer::is_virtual_path(&entry.path)
                }));
        let defender_available = cfg!(target_os = "windows")
            && context_entry
                .as_ref()
                .is_some_and(|entry| !explorer::is_virtual_path(&entry.path));

        let mut items = Vec::new();
        if is_sidebar_drive {
            if formatable_drive {
                items.push((
                    self.localized("Formatear", "Format").into(),
                    ContextCommand::FormatDrive,
                ));
            }
            items.push((
                self.localized("Expulsar", "Eject").into(),
                ContextCommand::EjectDrive,
            ));
            return items;
        }

        if is_entry && !drive_entry {
            items.push((
                self.localized("Copiar", "Copy").into(),
                ContextCommand::Copy,
            ));
            items.push((self.localized("Cortar", "Cut").into(), ContextCommand::Cut));
        }
        if menu_state.paste_available {
            items.push((
                self.localized("Pegar", "Paste").into(),
                ContextCommand::Paste,
            ));
        }

        if is_entry {
            items.push((self.localized("Abrir", "Open").into(), ContextCommand::Open));
            if !drive_entry {
                items.push((
                    self.localized("Abrir con", "Open with").into(),
                    ContextCommand::OpenWithMenu,
                ));
                if is_search_result {
                    items.push((
                        self.localized("Abrir ubicación del archivo", "Open file location")
                            .into(),
                        ContextCommand::OpenFileLocation,
                    ));
                }
                items.push((
                    self.localized("Comprimir", "Compress").into(),
                    ContextCommand::CompressMenu,
                ));
                if extractable_archive {
                    items.push((
                        self.localized("Extraer", "Extract").into(),
                        ContextCommand::ExtractMenu,
                    ));
                }
                if mountable_disk_image {
                    items.push((
                        self.localized("Montar imagen", "Mount image").into(),
                        ContextCommand::MountDiskImage,
                    ));
                }
            }
            if ejectable_drive {
                items.push((
                    self.localized("Expulsar", "Eject").into(),
                    ContextCommand::EjectDrive,
                ));
            }
            if formatable_drive {
                items.push((
                    self.localized("Formatear", "Format").into(),
                    ContextCommand::FormatDrive,
                ));
            }
            if defender_available && !drive_entry {
                items.push((
                    self.localized(
                        "Analizar con Microsoft Defender",
                        "Scan with Microsoft Defender",
                    )
                    .into(),
                    ContextCommand::ScanWithDefender,
                ));
            }
            if !drive_entry {
                items.push((
                    self.localized("Renombrar", "Rename").into(),
                    ContextCommand::Rename,
                ));
                items.push((
                    self.localized("Eliminar", "Delete").into(),
                    ContextCommand::Delete,
                ));
                items.push((
                    self.localized("Eliminar permanentemente", "Delete permanently")
                        .into(),
                    ContextCommand::DeletePermanent,
                ));
            }
        } else {
            items.push((
                self.localized("Actualizar", "Refresh").into(),
                ContextCommand::Refresh,
            ));
            items.push((
                self.localized("Nuevo", "New").into(),
                ContextCommand::NewMenu,
            ));
        }
        if terminal_available {
            items.push((
                self.localized("Abrir en Terminal", "Open in Terminal")
                    .into(),
                ContextCommand::OpenTerminal,
            ));
        }
        items.push((
            self.localized("Propiedades", "Properties").into(),
            ContextCommand::Properties,
        ));
        items
    }
}

fn keyboard_menu_scroll_task(menu: KeyboardMenu, index: usize, count: usize) -> Task<Message> {
    if menu != KeyboardMenu::ContextOpenWith {
        return Task::none();
    }
    let y = if count > 1 {
        index as f32 / (count - 1) as f32
    } else {
        0.0
    };
    iced::widget::operation::snap_to(
        BExplorerIced::context_open_with_scroll_id(),
        iced::widget::operation::RelativeOffset { x: 0.0, y },
    )
}

fn next_keyboard_menu_match(
    labels: &[String],
    selected_position: Option<usize>,
    character: &str,
) -> Option<usize> {
    let prefix = character.to_lowercase();
    let last_position = selected_position
        .filter(|position| *position < labels.len())
        .unwrap_or_else(|| labels.len().saturating_sub(1));
    (1..=labels.len())
        .map(|offset| (last_position + offset) % labels.len())
        .find(|position| labels[*position].to_lowercase().starts_with(&prefix))
}

#[cfg(test)]
mod tests {
    use super::next_keyboard_menu_match;

    #[test]
    fn repeated_letters_cycle_action_menu_items() {
        let labels = ["Abrir", "Acerca de", "Copiar", "Actualizar"].map(str::to_owned);

        assert_eq!(next_keyboard_menu_match(&labels, None, "a"), Some(0));
        assert_eq!(next_keyboard_menu_match(&labels, Some(0), "a"), Some(1));
        assert_eq!(next_keyboard_menu_match(&labels, Some(1), "A"), Some(3));
        assert_eq!(next_keyboard_menu_match(&labels, Some(3), "a"), Some(0));
    }
}

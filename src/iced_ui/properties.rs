use super::*;

use crate::fs::properties::{
    self as backend, ApplyOutcome, DirectorySize, PropertiesChanges, PropertiesSnapshot,
    PropertyApplication, PropertyIcon, PropertyIdentity,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PropertiesTab {
    General,
    Permissions,
    Details,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PropertiesIdentityMenu {
    Owner,
    Group,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PropertiesSelectorMenu {
    Application,
    Owner,
    Group,
}

#[derive(Clone, Debug)]
pub(super) enum PropertiesMessage {
    WindowOpened(window::Id),
    Loaded(u64, Box<Result<PropertiesLoadedData, String>>),
    SelectTab(PropertiesTab),
    NameChanged(String),
    PermissionToggled(u32),
    ToggleIdentityMenu(PropertiesIdentityMenu),
    CloseIdentityMenu,
    IdentitySelected(PropertiesIdentityMenu, PropertyIdentity),
    ToggleApplicationMenu,
    CloseApplicationMenu,
    MoveMenuSelection(i32),
    SelectMenuByCharacter(String),
    ConfirmMenuSelection,
    ApplicationSelected(PropertyApplication),
    RecursiveChanged(bool),
    RefreshSize,
    StopSize,
    SizeFinished(u64, u64, Result<DirectorySize, String>),
    Apply,
    Accept,
    Applied(u64, Result<ApplyOutcome, String>),
    Drag,
    Close,
}

#[derive(Clone, Debug)]
pub(super) struct PropertiesLoadedData {
    pub(super) snapshot: PropertiesSnapshot,
    pub(super) users: Vec<PropertyIdentity>,
    pub(super) groups: Vec<PropertyIdentity>,
    pub(super) application_icons: HashMap<String, PropertyIcon>,
}

#[derive(Debug)]
pub(super) struct PropertiesWindowState {
    pub(super) pane: PaneId,
    pub(super) request_id: u64,
    pub(super) paths: Vec<PathBuf>,
    pub(super) tab: PropertiesTab,
    pub(super) snapshot: Option<PropertiesSnapshot>,
    pub(super) loading: bool,
    pub(super) applying: bool,
    pub(super) close_after_apply: bool,
    pub(super) notice: Option<(String, bool)>,
    pub(super) name: String,
    pub(super) initial_name: String,
    pub(super) mode: Option<u32>,
    pub(super) initial_mode: Option<u32>,
    pub(super) users: Vec<PropertyIdentity>,
    pub(super) groups: Vec<PropertyIdentity>,
    pub(super) owner: Option<PropertyIdentity>,
    pub(super) initial_owner: Option<u32>,
    pub(super) group: Option<PropertyIdentity>,
    pub(super) initial_group: Option<u32>,
    pub(super) application: Option<PropertyApplication>,
    pub(super) initial_application: Option<String>,
    pub(super) application_menu_open: bool,
    pub(super) application_menu_index: usize,
    pub(super) identity_menu_open: Option<PropertiesIdentityMenu>,
    pub(super) identity_menu_index: usize,
    pub(super) application_icons: HashMap<String, iced_image::Handle>,
    pub(super) recursive: bool,
    pub(super) size: Option<DirectorySize>,
    pub(super) size_loading: bool,
    pub(super) size_request_id: u64,
    pub(super) size_cancel: Option<Arc<AtomicBool>>,
    pub(super) icon: Option<iced_image::Handle>,
}

impl PropertiesWindowState {
    fn loading(pane: PaneId, request_id: u64, paths: Vec<PathBuf>, spanish: bool) -> Self {
        let name = properties_paths_title(&paths, spanish);
        Self {
            pane,
            request_id,
            paths,
            tab: PropertiesTab::General,
            snapshot: None,
            loading: true,
            applying: false,
            close_after_apply: false,
            notice: None,
            name: name.clone(),
            initial_name: name,
            mode: None,
            initial_mode: None,
            users: Vec::new(),
            groups: Vec::new(),
            owner: None,
            initial_owner: None,
            group: None,
            initial_group: None,
            application: None,
            initial_application: None,
            application_menu_open: false,
            application_menu_index: 0,
            identity_menu_open: None,
            identity_menu_index: 0,
            application_icons: HashMap::new(),
            recursive: false,
            size: None,
            size_loading: false,
            size_request_id: 0,
            size_cancel: None,
            icon: None,
        }
    }

    pub(super) fn is_dirty(&self) -> bool {
        if self.loading || self.snapshot.is_none() {
            return false;
        }
        self.name != self.initial_name
            || self.mode != self.initial_mode
            || self.owner.as_ref().map(|identity| identity.id) != self.initial_owner
            || self.group.as_ref().map(|identity| identity.id) != self.initial_group
            || self
                .application
                .as_ref()
                .map(|application| application.desktop_id.as_str())
                != self.initial_application.as_deref()
    }

    pub(super) fn can_rename(&self) -> bool {
        self.paths.len() == 1
            && self
                .paths
                .first()
                .is_some_and(|path| path.file_name().is_some())
    }

    pub(super) fn permission_editable(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.mode.is_some()
                && !matches!(
                    snapshot.kind,
                    backend::PropertyKind::SymlinkFile
                        | backend::PropertyKind::SymlinkDirectory
                        | backend::PropertyKind::BrokenSymlink
                )
        }) && !self.applying
    }

    pub(super) fn identity_editable(&self) -> bool {
        self.snapshot.is_some() && !self.applying
    }

    pub(super) fn selector_menu(&self) -> Option<PropertiesSelectorMenu> {
        if self.application_menu_open {
            Some(PropertiesSelectorMenu::Application)
        } else {
            self.identity_menu_open.map(|menu| match menu {
                PropertiesIdentityMenu::Owner => PropertiesSelectorMenu::Owner,
                PropertiesIdentityMenu::Group => PropertiesSelectorMenu::Group,
            })
        }
    }

    fn close_selector_menus(&mut self) {
        self.application_menu_open = false;
        self.identity_menu_open = None;
    }

    fn move_selector_selection(
        &mut self,
        direction: i32,
    ) -> Option<(PropertiesSelectorMenu, usize, usize)> {
        let menu = self.selector_menu()?;
        let (current, count) = match menu {
            PropertiesSelectorMenu::Application => (
                self.application_menu_index,
                self.snapshot.as_ref()?.applications.len(),
            ),
            PropertiesSelectorMenu::Owner => (self.identity_menu_index, self.users.len()),
            PropertiesSelectorMenu::Group => (self.identity_menu_index, self.groups.len()),
        };
        if count == 0 {
            return None;
        }
        let next = if direction < 0 {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        match menu {
            PropertiesSelectorMenu::Application => self.application_menu_index = next,
            PropertiesSelectorMenu::Owner | PropertiesSelectorMenu::Group => {
                self.identity_menu_index = next;
            }
        }
        Some((menu, next, count))
    }

    fn select_selector_by_character(
        &mut self,
        character: &str,
    ) -> Option<(PropertiesSelectorMenu, usize, usize)> {
        let menu = self.selector_menu()?;
        let current = match menu {
            PropertiesSelectorMenu::Application => self.application_menu_index,
            PropertiesSelectorMenu::Owner | PropertiesSelectorMenu::Group => {
                self.identity_menu_index
            }
        };
        let labels = match menu {
            PropertiesSelectorMenu::Application => self
                .snapshot
                .as_ref()?
                .applications
                .iter()
                .map(|application| application.name.as_str())
                .collect::<Vec<_>>(),
            PropertiesSelectorMenu::Owner => self
                .users
                .iter()
                .map(|identity| identity.name.as_str())
                .collect::<Vec<_>>(),
            PropertiesSelectorMenu::Group => self
                .groups
                .iter()
                .map(|identity| identity.name.as_str())
                .collect::<Vec<_>>(),
        };
        let next = next_matching_label_position(&labels, Some(current), character)?;
        let count = labels.len();
        match menu {
            PropertiesSelectorMenu::Application => self.application_menu_index = next,
            PropertiesSelectorMenu::Owner | PropertiesSelectorMenu::Group => {
                self.identity_menu_index = next;
            }
        }
        Some((menu, next, count))
    }

    fn confirm_selector_selection(&mut self) {
        match self.selector_menu() {
            Some(PropertiesSelectorMenu::Application) => {
                if let Some(application) = self.snapshot.as_ref().and_then(|snapshot| {
                    snapshot
                        .applications
                        .get(self.application_menu_index)
                        .cloned()
                }) {
                    self.application = Some(application);
                }
            }
            Some(PropertiesSelectorMenu::Owner) => {
                if let Some(owner) = self.users.get(self.identity_menu_index).cloned() {
                    self.owner = Some(owner);
                }
            }
            Some(PropertiesSelectorMenu::Group) => {
                if let Some(group) = self.groups.get(self.identity_menu_index).cloned() {
                    self.group = Some(group);
                }
            }
            None => {}
        }
        self.close_selector_menus();
    }
}

pub(super) fn properties_selector_scroll_id(menu: PropertiesSelectorMenu) -> Id {
    Id::new(match menu {
        PropertiesSelectorMenu::Application => "properties-application-selector-scroll",
        PropertiesSelectorMenu::Owner => "properties-owner-selector-scroll",
        PropertiesSelectorMenu::Group => "properties-group-selector-scroll",
    })
}

fn selector_scroll_task(menu: PropertiesSelectorMenu, index: usize, count: usize) -> Task<Message> {
    let y = if count > 1 {
        index as f32 / (count - 1) as f32
    } else {
        0.0
    };
    iced::widget::operation::snap_to(
        properties_selector_scroll_id(menu),
        iced::widget::operation::RelativeOffset { x: 0.0, y },
    )
}

fn next_matching_label_position(
    labels: &[&str],
    selected_position: Option<usize>,
    character: &str,
) -> Option<usize> {
    if character.chars().count() != 1 || !character.chars().all(char::is_alphanumeric) {
        return None;
    }
    let prefix = character.to_lowercase();
    let last_position = selected_position
        .filter(|position| *position < labels.len())
        .unwrap_or_else(|| labels.len().saturating_sub(1));
    (1..=labels.len())
        .map(|offset| (last_position + offset) % labels.len())
        .find(|position| labels[*position].to_lowercase().starts_with(&prefix))
}

fn properties_paths_title(paths: &[PathBuf], spanish: bool) -> String {
    if paths.len() > 1 {
        return if spanish {
            format!("{} elementos", paths.len())
        } else {
            format!("{} items", paths.len())
        };
    }
    paths
        .first()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .or_else(|| paths.first().map(|path| path.display().to_string()))
        .unwrap_or_else(|| "Propiedades".into())
}

impl BExplorerIced {
    pub(super) fn open_properties_window(
        &mut self,
        pane: PaneId,
        mut paths: Vec<PathBuf>,
    ) -> Task<Message> {
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            return self.report_error(pane, "No properties target");
        }

        let blocked_by_applying = self
            .properties_window
            .as_ref()
            .is_some_and(|state| state.applying);
        let blocked_by_changes = self
            .properties_window
            .as_ref()
            .is_some_and(PropertiesWindowState::is_dirty);
        if blocked_by_applying || blocked_by_changes {
            let notice = self
                .localized(
                    "Termina o descarta los cambios de la ventana de propiedades actual",
                    "Finish or discard the changes in the current properties window",
                )
                .to_owned();
            self.pane_mut(pane).status = notice.clone();
            if blocked_by_changes
                && !blocked_by_applying
                && let Some(state) = &mut self.properties_window
            {
                state.notice = Some((notice, false));
            }
            return self
                .properties_window_id
                .map(window::gain_focus)
                .unwrap_or_else(Task::none);
        }

        if let Some(state) = &self.properties_window
            && let Some(cancel) = &state.size_cancel
        {
            cancel.store(true, AtomicOrdering::Relaxed);
        }

        self.properties_request_id = self.properties_request_id.wrapping_add(1).max(1);
        let request_id = self.properties_request_id;
        let spanish = self.is_spanish();
        self.properties_window = Some(PropertiesWindowState::loading(
            pane,
            request_id,
            paths.clone(),
            spanish,
        ));
        self.pane_mut(pane).status = self
            .localized("Cargando propiedades...", "Loading properties...")
            .into();

        let load = load_properties_task(request_id, paths);
        if let Some(id) = self.properties_window_id {
            Task::batch([load, window::minimize(id, false), window::gain_focus(id)])
        } else {
            let (id, open) = window::open(properties_window_settings());
            self.properties_window_id = Some(id);
            Task::batch([
                load,
                open.map(|id| Message::Properties(PropertiesMessage::WindowOpened(id))),
            ])
        }
    }

    pub(super) fn properties_window_title(&self) -> String {
        let name = self
            .properties_window
            .as_ref()
            .map(|state| state.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("BExplorer");
        if self.is_spanish() {
            format!("Propiedades de {name}")
        } else {
            format!("{name} Properties")
        }
    }

    pub(super) fn close_properties_window(&mut self) -> Task<Message> {
        if self
            .properties_window
            .as_ref()
            .is_some_and(|state| state.applying)
        {
            return Task::none();
        }
        if let Some(state) = &self.properties_window
            && let Some(cancel) = &state.size_cancel
        {
            cancel.store(true, AtomicOrdering::Relaxed);
        }
        let Some(id) = self.properties_window_id else {
            self.properties_window = None;
            return Task::none();
        };
        self.close_window_task(id)
    }

    pub(super) fn properties_window_closed(&mut self, id: window::Id) {
        if self.properties_window_id != Some(id) {
            return;
        }
        if let Some(state) = &self.properties_window
            && let Some(cancel) = &state.size_cancel
        {
            cancel.store(true, AtomicOrdering::Relaxed);
        }
        self.properties_window_id = None;
        self.properties_window = None;
    }

    pub(super) fn update_properties(&mut self, message: PropertiesMessage) -> Task<Message> {
        match message {
            PropertiesMessage::WindowOpened(id) => {
                self.properties_window_id = Some(id);
                Task::batch([
                    self.apply_window_corners_task_for(id),
                    sync_fixed_progress_window_size_task(id, properties_window_size()),
                    window::minimize(id, false),
                    window::gain_focus(id),
                ])
            }
            PropertiesMessage::Loaded(request_id, result) => {
                let spanish = self.is_spanish();
                let Some(state) = &mut self.properties_window else {
                    return Task::none();
                };
                if state.request_id != request_id {
                    return Task::none();
                }
                state.loading = false;
                match *result {
                    Ok(loaded) => {
                        let snapshot = loaded.snapshot;
                        let display_name = if snapshot.paths.len() > 1 {
                            if spanish {
                                format!("{} elementos", snapshot.paths.len())
                            } else {
                                format!("{} items", snapshot.paths.len())
                            }
                        } else {
                            snapshot.display_name.clone()
                        };
                        state.name = display_name.clone();
                        state.initial_name = display_name;
                        state.mode = snapshot.mode;
                        state.initial_mode = snapshot.mode;
                        state.initial_owner = snapshot.uid;
                        state.initial_group = snapshot.gid;
                        state.users = loaded.users;
                        state.groups = loaded.groups;
                        state.application_icons = loaded
                            .application_icons
                            .into_iter()
                            .map(|(desktop_id, icon)| {
                                (
                                    desktop_id,
                                    iced_image::Handle::from_rgba(
                                        icon.width,
                                        icon.height,
                                        icon.rgba,
                                    ),
                                )
                            })
                            .collect();
                        state.owner = snapshot.uid.map(|id| {
                            state
                                .users
                                .iter()
                                .find(|identity| identity.id == id)
                                .cloned()
                                .unwrap_or(PropertyIdentity {
                                    id,
                                    name: snapshot.owner.clone().unwrap_or_else(|| id.to_string()),
                                })
                        });
                        state.group = snapshot.gid.map(|id| {
                            state
                                .groups
                                .iter()
                                .find(|identity| identity.id == id)
                                .cloned()
                                .unwrap_or(PropertyIdentity {
                                    id,
                                    name: snapshot.group.clone().unwrap_or_else(|| id.to_string()),
                                })
                        });
                        state.application = snapshot.default_application.clone();
                        state.initial_application = snapshot
                            .default_application
                            .as_ref()
                            .map(|application| application.desktop_id.clone());
                        state.icon = snapshot.icon.as_ref().map(|icon| {
                            iced_image::Handle::from_rgba(
                                icon.width,
                                icon.height,
                                icon.rgba.clone(),
                            )
                        });
                        let scan_size = snapshot.contains_directory;
                        state.snapshot = Some(snapshot);
                        state.notice = None;
                        let pane = state.pane;
                        self.pane_mut(pane).status = self
                            .localized("Propiedades cargadas", "Properties loaded")
                            .into();
                        if scan_size {
                            return self.start_properties_size_scan();
                        }
                    }
                    Err(error) => {
                        state.notice = Some((error.clone(), true));
                        let pane = state.pane;
                        self.pane_mut(pane).status = error;
                    }
                }
                Task::none()
            }
            PropertiesMessage::SelectTab(tab) => {
                if let Some(state) = &mut self.properties_window {
                    state.tab = tab;
                    state.close_selector_menus();
                }
                Task::none()
            }
            PropertiesMessage::NameChanged(value) => {
                if let Some(state) = &mut self.properties_window
                    && state.can_rename()
                    && !state.applying
                {
                    state.name = value;
                    state.close_selector_menus();
                }
                Task::none()
            }
            PropertiesMessage::PermissionToggled(bit) => {
                if let Some(state) = &mut self.properties_window
                    && state.permission_editable()
                    && let Some(mode) = &mut state.mode
                {
                    *mode ^= bit;
                    state.close_selector_menus();
                }
                Task::none()
            }
            PropertiesMessage::ToggleIdentityMenu(menu) => {
                if let Some(state) = &mut self.properties_window
                    && state.identity_editable()
                {
                    if state.identity_menu_open == Some(menu) {
                        state.identity_menu_open = None;
                    } else {
                        state.application_menu_open = false;
                        state.identity_menu_open = Some(menu);
                        state.identity_menu_index = match menu {
                            PropertiesIdentityMenu::Owner => state
                                .owner
                                .as_ref()
                                .and_then(|selected| {
                                    state
                                        .users
                                        .iter()
                                        .position(|identity| identity.id == selected.id)
                                })
                                .unwrap_or(0),
                            PropertiesIdentityMenu::Group => state
                                .group
                                .as_ref()
                                .and_then(|selected| {
                                    state
                                        .groups
                                        .iter()
                                        .position(|identity| identity.id == selected.id)
                                })
                                .unwrap_or(0),
                        };
                    }
                }
                Task::none()
            }
            PropertiesMessage::CloseIdentityMenu => {
                if let Some(state) = &mut self.properties_window {
                    state.identity_menu_open = None;
                }
                Task::none()
            }
            PropertiesMessage::IdentitySelected(menu, identity) => {
                if let Some(state) = &mut self.properties_window
                    && state.identity_editable()
                {
                    match menu {
                        PropertiesIdentityMenu::Owner => state.owner = Some(identity),
                        PropertiesIdentityMenu::Group => state.group = Some(identity),
                    }
                    state.close_selector_menus();
                }
                Task::none()
            }
            PropertiesMessage::ToggleApplicationMenu => {
                if let Some(state) = &mut self.properties_window
                    && !state.applying
                    && state
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| !snapshot.applications.is_empty())
                {
                    if !state.application_menu_open {
                        state.identity_menu_open = None;
                        state.application_menu_index = state
                            .application
                            .as_ref()
                            .and_then(|selected| {
                                state.snapshot.as_ref().and_then(|snapshot| {
                                    snapshot.applications.iter().position(|application| {
                                        application.desktop_id == selected.desktop_id
                                    })
                                })
                            })
                            .unwrap_or(0);
                    }
                    state.application_menu_open = !state.application_menu_open;
                }
                Task::none()
            }
            PropertiesMessage::CloseApplicationMenu => {
                if let Some(state) = &mut self.properties_window {
                    state.application_menu_open = false;
                }
                Task::none()
            }
            PropertiesMessage::MoveMenuSelection(direction) => self
                .properties_window
                .as_mut()
                .and_then(|state| state.move_selector_selection(direction))
                .map(|(menu, index, count)| selector_scroll_task(menu, index, count))
                .unwrap_or_else(Task::none),
            PropertiesMessage::SelectMenuByCharacter(character) => self
                .properties_window
                .as_mut()
                .and_then(|state| state.select_selector_by_character(&character))
                .map(|(menu, index, count)| selector_scroll_task(menu, index, count))
                .unwrap_or_else(Task::none),
            PropertiesMessage::ConfirmMenuSelection => {
                if let Some(state) = &mut self.properties_window
                    && state.selector_menu().is_some()
                {
                    state.confirm_selector_selection();
                }
                Task::none()
            }
            PropertiesMessage::ApplicationSelected(application) => {
                if let Some(state) = &mut self.properties_window
                    && !state.applying
                {
                    state.application = Some(application);
                    state.close_selector_menus();
                }
                Task::none()
            }
            PropertiesMessage::RecursiveChanged(recursive) => {
                if let Some(state) = &mut self.properties_window
                    && state.identity_editable()
                    && state
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.contains_directory)
                {
                    state.recursive = recursive;
                    state.close_selector_menus();
                }
                Task::none()
            }
            PropertiesMessage::RefreshSize => {
                if let Some(state) = &mut self.properties_window {
                    state.close_selector_menus();
                }
                self.start_properties_size_scan()
            }
            PropertiesMessage::StopSize => {
                let notice = self
                    .localized("Deteniendo el cálculo...", "Stopping calculation...")
                    .to_owned();
                if let Some(state) = &mut self.properties_window
                    && let Some(cancel) = state.size_cancel.clone()
                {
                    state.close_selector_menus();
                    cancel.store(true, AtomicOrdering::Relaxed);
                    state.notice = Some((notice, false));
                }
                Task::none()
            }
            PropertiesMessage::SizeFinished(properties_request_id, size_request_id, result) => {
                let spanish = self.is_spanish();
                let cancelled_notice = self
                    .localized("Cálculo detenido", "Calculation stopped")
                    .to_owned();
                let Some(state) = &mut self.properties_window else {
                    return Task::none();
                };
                if state.request_id != properties_request_id
                    || state.size_request_id != size_request_id
                {
                    return Task::none();
                }
                state.size_loading = false;
                state.size_cancel = None;
                match result {
                    Ok(size) => {
                        if size.cancelled {
                            state.notice = Some((cancelled_notice, false));
                        } else if size.unreadable > 0 {
                            state.notice = Some((
                                if spanish {
                                    format!(
                                        "Tamaño parcial: {} elementos sin acceso",
                                        size.unreadable
                                    )
                                } else {
                                    format!("Partial size: {} inaccessible items", size.unreadable)
                                },
                                false,
                            ));
                        } else {
                            state.notice = None;
                        }
                        state.size = Some(size);
                    }
                    Err(error) => state.notice = Some((error, true)),
                }
                Task::none()
            }
            PropertiesMessage::Apply => self.apply_properties(false),
            PropertiesMessage::Accept => {
                if self
                    .properties_window
                    .as_ref()
                    .is_some_and(|state| state.applying)
                {
                    return Task::none();
                }
                if self
                    .properties_window
                    .as_ref()
                    .is_some_and(PropertiesWindowState::is_dirty)
                {
                    self.apply_properties(true)
                } else {
                    self.close_properties_window()
                }
            }
            PropertiesMessage::Applied(request_id, result) => {
                let Some(state) = &mut self.properties_window else {
                    return Task::none();
                };
                if state.request_id != request_id {
                    return Task::none();
                }
                state.applying = false;
                match result {
                    Ok(outcome) => {
                        let pane = state.pane;
                        let close_after = state.close_after_apply;
                        state.paths = outcome.paths.clone();
                        let status = if outcome.elevated {
                            self.localized(
                                "Propiedades aplicadas con permisos de administrador",
                                "Properties applied with administrator access",
                            )
                        } else {
                            self.localized("Propiedades aplicadas", "Properties applied")
                        };
                        self.pane_mut(pane).status = status.into();
                        let refresh = self.start_load(pane);
                        if close_after {
                            Task::batch([refresh, self.close_properties_window()])
                        } else {
                            Task::batch([refresh, self.reload_properties_data(pane, outcome.paths)])
                        }
                    }
                    Err(error) => {
                        state.notice = Some((error.clone(), true));
                        let pane = state.pane;
                        self.pane_mut(pane).status = error;
                        self.start_load(pane)
                    }
                }
            }
            PropertiesMessage::Drag => self
                .properties_window_id
                .map(window::drag)
                .unwrap_or_else(Task::none),
            PropertiesMessage::Close => self.close_properties_window(),
        }
    }

    fn start_properties_size_scan(&mut self) -> Task<Message> {
        let Some(state) = &mut self.properties_window else {
            return Task::none();
        };
        if state.loading || state.applying || state.snapshot.is_none() {
            return Task::none();
        }
        if let Some(cancel) = &state.size_cancel {
            cancel.store(true, AtomicOrdering::Relaxed);
        }
        state.size_request_id = state.size_request_id.wrapping_add(1).max(1);
        let request_id = state.size_request_id;
        let properties_request_id = state.request_id;
        let paths = state.paths.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        state.size_cancel = Some(cancel.clone());
        state.size_loading = true;
        state.notice = None;

        Task::perform(
            run_blocking_file_operation(move || {
                Ok(backend::calculate_size(&paths, cancel.as_ref()))
            }),
            move |result| {
                Message::Properties(PropertiesMessage::SizeFinished(
                    properties_request_id,
                    request_id,
                    result,
                ))
            },
        )
    }

    fn apply_properties(&mut self, close_after: bool) -> Task<Message> {
        let can_apply = self.properties_window.as_ref().is_some_and(|state| {
            !state.loading && !state.applying && state.snapshot.is_some() && state.is_dirty()
        });
        if !can_apply {
            return if close_after {
                self.close_properties_window()
            } else {
                Task::none()
            };
        }
        let applying_notice = self
            .localized("Aplicando cambios...", "Applying changes...")
            .to_owned();
        let Some(state) = &mut self.properties_window else {
            return Task::none();
        };

        if let Some(cancel) = state.size_cancel.take() {
            cancel.store(true, AtomicOrdering::Relaxed);
        }
        state.size_request_id = state.size_request_id.wrapping_add(1).max(1);
        state.size_loading = false;

        let snapshot = state.snapshot.as_ref().expect("snapshot checked above");
        let permission_mask = state
            .initial_mode
            .zip(state.mode)
            .map(|(initial, current)| initial ^ current)
            .unwrap_or(0);
        let changes = PropertiesChanges {
            paths: state.paths.clone(),
            new_name: (state.can_rename() && state.name != state.initial_name)
                .then(|| state.name.clone()),
            permission_mask,
            permission_value: state.mode.unwrap_or_default(),
            owner: state
                .owner
                .as_ref()
                .map(|identity| identity.id)
                .filter(|id| Some(*id) != state.initial_owner),
            group: state
                .group
                .as_ref()
                .map(|identity| identity.id)
                .filter(|id| Some(*id) != state.initial_group),
            recursive: state.recursive,
            mime_type: snapshot.mime_type.clone(),
            default_application: state
                .application
                .as_ref()
                .map(|application| application.desktop_id.clone())
                .filter(|id| Some(id.as_str()) != state.initial_application.as_deref()),
        };
        state.applying = true;
        state.close_after_apply = close_after;
        state.notice = Some((applying_notice, false));
        let request_id = state.request_id;
        Task::perform(
            run_blocking_file_operation(move || backend::apply(changes)),
            move |result| Message::Properties(PropertiesMessage::Applied(request_id, result)),
        )
    }

    fn reload_properties_data(&mut self, pane: PaneId, paths: Vec<PathBuf>) -> Task<Message> {
        if let Some(state) = &mut self.properties_window
            && let Some(cancel) = state.size_cancel.take()
        {
            cancel.store(true, AtomicOrdering::Relaxed);
            state.size_request_id = state.size_request_id.wrapping_add(1).max(1);
        }
        self.properties_request_id = self.properties_request_id.wrapping_add(1).max(1);
        let request_id = self.properties_request_id;
        let tab = self
            .properties_window
            .as_ref()
            .map(|state| state.tab)
            .unwrap_or(PropertiesTab::General);
        let mut state =
            PropertiesWindowState::loading(pane, request_id, paths.clone(), self.is_spanish());
        state.tab = tab;
        self.properties_window = Some(state);
        load_properties_task(request_id, paths)
    }
}

fn load_properties_task(request_id: u64, paths: Vec<PathBuf>) -> Task<Message> {
    Task::perform(
        run_blocking_file_operation(move || {
            let snapshot = backend::load(&paths)?;
            let application_icons = snapshot
                .applications
                .iter()
                .filter_map(|application| {
                    backend::load_application_icon(application, 24)
                        .map(|icon| (application.desktop_id.clone(), icon))
                })
                .collect();
            Ok(PropertiesLoadedData {
                snapshot,
                users: backend::list_users(),
                groups: backend::list_groups(),
                application_icons,
            })
        }),
        move |result| Message::Properties(PropertiesMessage::Loaded(request_id, Box::new(result))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_titles_cover_root_single_and_localized_multiple_items() {
        assert_eq!(properties_paths_title(&[PathBuf::from("/")], true), "/");
        assert_eq!(
            properties_paths_title(&[PathBuf::from("/tmp/document.txt")], true),
            "document.txt"
        );
        let paths = [PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")];
        assert_eq!(properties_paths_title(&paths, true), "2 elementos");
        assert_eq!(properties_paths_title(&paths, false), "2 items");
    }

    #[test]
    fn filesystem_root_cannot_be_renamed_from_properties() {
        let state =
            PropertiesWindowState::loading(PaneId::Primary, 7, vec![PathBuf::from("/")], true);

        assert!(!state.can_rename());
        assert_eq!(state.request_id, 7);
    }

    #[test]
    fn selector_typeahead_cycles_repeated_initials() {
        let labels = ["audio", "bin", "bluetooth", "backup"];

        assert_eq!(next_matching_label_position(&labels, None, "b"), Some(1));
        assert_eq!(next_matching_label_position(&labels, Some(1), "B"), Some(2));
        assert_eq!(next_matching_label_position(&labels, Some(2), "b"), Some(3));
        assert_eq!(next_matching_label_position(&labels, Some(3), "b"), Some(1));
        assert_eq!(next_matching_label_position(&labels, Some(1), "z"), None);
    }
}

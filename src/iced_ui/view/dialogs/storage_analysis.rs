use super::duplicate_cleanup::{dialog_action_button, format_duplicate_time, table_value_cell};
use super::*;
use iced::widget::{column, row, tooltip};

use crate::fs::storage_analysis::{StorageAnalysisSummary, StorageCategory, StorageFile};

const STORAGE_OVERVIEW_BREAKDOWN_WIDTH_RATIO: f32 = 0.36;
const STORAGE_OVERVIEW_BREAKDOWN_MIN_WIDTH: f32 = 440.0;
const STORAGE_OVERVIEW_BREAKDOWN_MAX_WIDTH: f32 = 520.0;

fn storage_overview_breakdown_width(window_width: f32, font_size: f32) -> f32 {
    (window_width * STORAGE_OVERVIEW_BREAKDOWN_WIDTH_RATIO).clamp(
        scaled_ui_metric(STORAGE_OVERVIEW_BREAKDOWN_MIN_WIDTH, font_size),
        scaled_ui_metric(STORAGE_OVERVIEW_BREAKDOWN_MAX_WIDTH, font_size),
    )
}

fn storage_overview_extension_column_width(card_width: f32, font_size: f32) -> f32 {
    let metric = |base| scaled_ui_metric(base, font_size);
    (card_width
        - metric(12.0) * 2.0
        - metric(8.0) * 2.0
        - metric(88.0)
        - metric(72.0)
        - metric(82.0)
        - metric(7.0) * 3.0
        - EXPLORER_SCROLLBAR_RAIL_WIDTH
        - 4.0)
        .max(1.0)
}

impl BExplorerIced {
    pub(in crate::iced_ui) fn storage_analysis_window_view(
        &self,
        palette: Palette,
    ) -> Element<'_, Message> {
        let (window_bg, window_title_bg) = palette.native_utility_backgrounds();
        let Some(state) = self.storage_analysis.as_ref() else {
            return container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style::default().background(window_bg))
                .into();
        };
        let font_size = self.font_size();
        let title_height = scaled_ui_metric(TRANSFER_WINDOW_TITLE_HEIGHT, font_size);
        let window_size = state.window_size;
        let window_radius = if state.window_maximized {
            1.0
        } else {
            WINDOW_RADIUS
        };
        let inner_height = (window_size.height - WINDOW_BORDER_WIDTH * 2.0).max(0.0);
        let body_height = (inner_height - title_height).max(0.0);
        let active = state.phase == StorageAnalysisPhase::Scanning;

        let title_drag_area = mouse_area(
            container(
                text(self.localized("Análisis de almacenamiento", "Storage analysis"))
                    .size(font_size)
                    .color(palette.text)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .height(title_height)
            .width(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::StorageAnalysisWindowDrag);
        let title_bar = container(
            row![
                title_drag_area,
                native_window_minimize_button(
                    Message::StorageAnalysisWindowMinimize,
                    palette,
                    title_height,
                    font_size,
                ),
                native_window_maximize_button(
                    Message::StorageAnalysisWindowMaximize,
                    state.window_maximized,
                    palette,
                    title_height,
                    font_size,
                ),
                native_window_close_button_maybe(
                    Some(Message::CloseStorageAnalysis),
                    palette,
                    title_height,
                    font_size,
                ),
            ]
            .align_y(Alignment::Center),
        )
        .height(title_height)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(window_title_bg)
                .border(border::rounded(border::top((window_radius - 1.0).max(0.0))))
        });

        let (heading, progress_detail) = match state.phase {
            StorageAnalysisPhase::Scanning => (
                self.localized("Analizando archivos", "Analyzing files"),
                if self.is_spanish() {
                    format!("{} archivos analizados", state.scanned)
                } else {
                    format!("{} files analyzed", state.scanned)
                },
            ),
            StorageAnalysisPhase::Complete => (
                self.localized("Análisis completado", "Analysis complete"),
                if self.is_spanish() {
                    format!("{} archivos clasificados", state.summary.total_files)
                } else {
                    format!("{} files classified", state.summary.total_files)
                },
            ),
            StorageAnalysisPhase::Cancelled => (
                self.localized("Análisis cancelado", "Analysis cancelled"),
                self.localized(
                    "Se muestran los resultados recopilados hasta el momento.",
                    "Results collected so far are shown.",
                )
                .to_owned(),
            ),
            StorageAnalysisPhase::Failed => (
                self.localized(
                    "No se pudo completar el análisis",
                    "The analysis could not finish",
                ),
                state.error.clone().unwrap_or_default(),
            ),
        };
        let current_path = state
            .current_path
            .as_ref()
            .unwrap_or(&state.root)
            .display()
            .to_string();
        let current_path = ellipsize_to_glyph_width(&current_path, 720.0, font_size - 1.0);
        let skipped = if state.skipped == 0 {
            String::new()
        } else if self.is_spanish() {
            format!(" · {} incidencias", state.skipped)
        } else {
            format!(" · {} issues", state.skipped)
        };
        let progress: Element<'_, Message> = if active {
            indeterminate_progress_bar(
                self.transfer_progress_phase,
                palette,
                TRANSFER_PROGRESS_BAR_HEIGHT,
            )
        } else {
            transfer_progress_bar(
                f32::from(state.phase == StorageAnalysisPhase::Complete),
                palette,
                TRANSFER_PROGRESS_BAR_HEIGHT,
            )
        };
        let status_card = container(
            column![
                row![
                    column![
                        text(heading).size(font_size + 1.0).color(palette.text),
                        text(format!("{progress_detail}{skipped}"))
                            .size(font_size - 1.0)
                            .color(palette.muted_text),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    text(format_size(Some(state.summary.total_bytes)))
                        .size(font_size + 1.0)
                        .color(palette.text),
                ]
                .align_y(Alignment::Center),
                progress,
                text(current_path)
                    .size(font_size - 1.0)
                    .color(palette.muted_text)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(7),
        )
        .padding([10, 12])
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(palette.native_utility_card_background(self.config.vibrancy_active))
                .border(border::rounded(7).color(palette.border).width(1))
        });

        let category_spacing = self.ui_metric(5.0);
        let mut category_rows = column![].spacing(category_spacing).width(Length::Fill);
        for category in StorageCategory::ALL {
            let usage = state.summary.usage(category);
            if usage.files == 0 {
                continue;
            }
            let category_color = state.category_colors[category.index()];
            let percentage = if state.summary.total_bytes == 0 {
                0.0
            } else {
                usage.bytes as f64 * 100.0 / state.summary.total_bytes as f64
            };
            let detail = format!(
                "{} · {} · {percentage:.1}%",
                localized_storage_file_count(usage.files, self.is_spanish()),
                format_size(Some(usage.bytes)),
            );
            let can_select = !active && !state.files.get(category).is_empty();
            let selected = state.overview_selected_category == Some(category);
            let card: Element<'_, Message> = Button::new(
                row![
                    container(Space::new())
                        .width(12)
                        .height(12)
                        .style(move |_| {
                            container::Style::default()
                                .background(category_color)
                                .border(border::rounded(6))
                        }),
                    container(
                        column![
                            text(storage_category_label(category, self.is_spanish()))
                                .size(font_size)
                                .color(palette.text)
                                .wrapping(iced::widget::text::Wrapping::None),
                            text(detail)
                                .size((font_size - 1.0).max(10.0))
                                .color(palette.muted_text)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .spacing(1)
                        .width(Length::Fill),
                    )
                    .width(Length::Fill)
                    .clip(true),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([5, 8])
            .width(Length::Fill)
            .on_press_maybe(can_select.then_some(Message::SelectStorageOverviewCategory(category)))
            .style(move |_, status| {
                let base = translucent_color(palette.table_bg, 0.72);
                let background = if selected {
                    mix_color(base, palette.accent, 0.2)
                } else if can_select
                    && matches!(status, button::Status::Hovered | button::Status::Pressed)
                {
                    mix_color(base, palette.accent, 0.16)
                } else {
                    base
                };
                button::Style {
                    background: Some(background.into()),
                    text_color: palette.text,
                    border: border::rounded(6)
                        .color(if selected {
                            palette.accent
                        } else if can_select
                            && matches!(status, button::Status::Hovered | button::Status::Pressed)
                        {
                            mix_color(palette.border, palette.accent, 0.5)
                        } else {
                            palette.border
                        })
                        .width(1),
                    ..button::Style::default()
                }
            })
            .into();
            category_rows = category_rows.push(card);
        }

        let category_scroller: Element<'_, Message> = scrollable(category_rows)
            .direction(scrollable::Direction::Vertical(
                explorer_scrollbar(f32::from(state.scrollbar_vertical_hovered)).spacing(4.0),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(|_| Message::StorageAnalysisScrolled)
            .style(move |theme, status| {
                explorer_scrollable_style(palette, theme, status, state.scrollbar_reveal_progress)
            })
            .into();
        let category_panel = container(scrollbar_proximity_layer(
            category_scroller,
            Some((
                Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Vertical, true),
                Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Vertical, false),
            )),
            None,
        ))
        .padding([0, 2])
        .width(Length::Fill)
        .height(Length::Fill);
        let total_label = self.localized("Total analizado", "Scanned total");
        let total_value = format_size(Some(state.summary.total_bytes));
        let category_chart_title =
            self.localized("Distribución por categoría", "Distribution by category");
        let duplicate_chart_title = self.localized(
            "Distribución de duplicados por categoría",
            "Duplicate distribution by category",
        );
        let duplicate_donut_visible =
            state.duplicate_estimate.phase != StorageDuplicateEstimatePhase::Waiting;
        let donut_size = if duplicate_donut_visible {
            (window_size.width * 0.28)
                .min(((body_height - 250.0) / 2.0).max(0.0))
                .clamp(110.0, 330.0)
        } else {
            (window_size.width * 0.36)
                .min((body_height - 170.0).max(0.0))
                .clamp(180.0, 360.0)
        };
        let chart_title_width = estimated_ui_text_width(
            if duplicate_donut_visible {
                duplicate_chart_title
            } else {
                category_chart_title
            },
            font_size,
        );
        let chart_card_width = (donut_size + self.ui_metric(40.0))
            .max(chart_title_width + self.ui_metric(28.0))
            .clamp(self.ui_metric(300.0), self.ui_metric(420.0));
        let hovered_category = state
            .donut_pointer
            .and_then(|point| storage_donut_category_at_point(&state.summary, point, donut_size));
        let donut_svg = svg::Svg::new(storage_donut_handle(
            &state.summary,
            &state.category_colors,
            palette,
            hovered_category,
        ))
        .width(Length::Fixed(donut_size))
        .height(Length::Fixed(donut_size))
        .content_fit(ContentFit::Contain);
        let donut_center = container(
            column![
                text(total_label)
                    .size((font_size - 1.0).max(10.0))
                    .color(palette.muted_text),
                text(total_value.clone())
                    .size(font_size + 8.0)
                    .color(palette.text),
            ]
            .spacing(self.ui_metric(4.0))
            .align_x(Alignment::Center),
        )
        .width(Length::Fixed(donut_size))
        .height(Length::Fixed(donut_size))
        .center(Length::Fill);
        let donut_graphic = stack(vec![donut_svg.into(), donut_center.into()])
            .width(Length::Fixed(donut_size))
            .height(Length::Fixed(donut_size));
        let donut_interaction = mouse_area(donut_graphic)
            .on_move(Message::StorageDonutPointerMoved)
            .on_exit(Message::StorageDonutPointerLeft);
        let tooltip_content: Element<'_, Message> = hovered_category
            .map(|category| {
                let usage = state.summary.usage(category);
                let percentage = storage_category_percentage(&state.summary, category);
                let color = state.category_colors[category.index()];
                container(
                    row![
                        container(Space::new())
                            .width(self.ui_metric(11.0))
                            .height(self.ui_metric(11.0))
                            .style(move |_| {
                                container::Style::default()
                                    .background(color)
                                    .border(border::rounded(6))
                            }),
                        column![
                            text(storage_category_label(category, self.is_spanish()))
                                .size(font_size)
                                .color(palette.text),
                            text(format!(
                                "{percentage:.1}% · {}",
                                format_size(Some(usage.bytes))
                            ))
                            .size((font_size - 1.0).max(10.0))
                            .color(palette.muted_text),
                        ]
                        .spacing(1),
                    ]
                    .spacing(self.ui_metric(8.0))
                    .align_y(Alignment::Center),
                )
                .padding([self.ui_metric(7.0), self.ui_metric(10.0)])
                .style(move |_| {
                    container::Style::default()
                        .background(palette.menu_bg)
                        .border(border::rounded(7).color(palette.strong_border).width(1))
                        .shadow(iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.28),
                            offset: Vector::new(0.0, 3.0),
                            blur_radius: 12.0,
                        })
                })
                .into()
            })
            .unwrap_or_else(|| Space::new().width(0).height(0).into());
        let donut: Element<'_, Message> = tooltip(
            donut_interaction,
            tooltip_content,
            tooltip::Position::FollowCursor,
        )
        .gap(self.ui_metric(10.0))
        .padding(0)
        .style(|_| container::Style::default().background(Color::TRANSPARENT))
        .into();
        let primary_chart_height = if duplicate_donut_visible {
            Length::FillPortion(1)
        } else {
            Length::Fill
        };
        let primary_chart = container(
            column![
                text(category_chart_title)
                    .size(font_size)
                    .color(palette.text)
                    .width(Length::Fill)
                    .align_x(Horizontal::Center),
                container(donut)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            ]
            .spacing(self.ui_metric(6.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center),
        )
        .padding([self.ui_metric(8.0), self.ui_metric(12.0)])
        .width(Length::Fill)
        .height(primary_chart_height)
        .style(move |_| {
            container::Style::default()
                .background(translucent_color(palette.table_bg, 0.45))
                .border(border::rounded(8).color(palette.border).width(1))
        });
        let mut chart_cards = column![primary_chart]
            .spacing(self.ui_metric(10.0))
            .width(Length::Fill)
            .height(Length::Fill);
        if duplicate_donut_visible {
            let duplicate_summary = &state.duplicate_estimate.summary;
            let duplicate_hovered_category =
                if state.duplicate_estimate.phase == StorageDuplicateEstimatePhase::Complete {
                    state.duplicate_donut_pointer.and_then(|point| {
                        storage_donut_category_at_point(duplicate_summary, point, donut_size)
                    })
                } else {
                    None
                };
            let duplicate_handle =
                if state.duplicate_estimate.phase == StorageDuplicateEstimatePhase::Complete {
                    storage_donut_handle(
                        duplicate_summary,
                        &state.category_colors,
                        palette,
                        duplicate_hovered_category,
                    )
                } else {
                    duplicate_estimate_donut_handle(
                        &state.duplicate_estimate,
                        self.transfer_progress_phase,
                        palette,
                    )
                };
            let (duplicate_center_label, duplicate_center_value) =
                duplicate_estimate_center_text(&state.duplicate_estimate, self.is_spanish());
            let duplicate_svg = svg::Svg::new(duplicate_handle)
                .width(Length::Fixed(donut_size))
                .height(Length::Fixed(donut_size))
                .content_fit(ContentFit::Contain);
            let duplicate_center = container(
                column![
                    text(duplicate_center_label)
                        .size((font_size - 1.0).max(10.0))
                        .color(palette.muted_text),
                    text(duplicate_center_value)
                        .size(font_size + 8.0)
                        .color(palette.text),
                ]
                .spacing(self.ui_metric(4.0))
                .align_x(Alignment::Center),
            )
            .width(Length::Fixed(donut_size))
            .height(Length::Fixed(donut_size))
            .center(Length::Fill);
            let duplicate_graphic = stack(vec![duplicate_svg.into(), duplicate_center.into()])
                .width(Length::Fixed(donut_size))
                .height(Length::Fixed(donut_size));
            let duplicate_interaction = mouse_area(duplicate_graphic)
                .on_move(Message::StorageDuplicateDonutPointerMoved)
                .on_exit(Message::StorageDuplicateDonutPointerLeft);
            let duplicate_tooltip_content: Element<'_, Message> = duplicate_hovered_category
                .map(|category| {
                    let usage = duplicate_summary.usage(category);
                    let percentage = storage_category_percentage(duplicate_summary, category);
                    let color = state.category_colors[category.index()];
                    container(
                        row![
                            container(Space::new())
                                .width(self.ui_metric(11.0))
                                .height(self.ui_metric(11.0))
                                .style(move |_| {
                                    container::Style::default()
                                        .background(color)
                                        .border(border::rounded(6))
                                }),
                            column![
                                text(storage_category_label(category, self.is_spanish()))
                                    .size(font_size)
                                    .color(palette.text),
                                text(format!(
                                    "{percentage:.1}% · {}",
                                    format_size(Some(usage.bytes))
                                ))
                                .size((font_size - 1.0).max(10.0))
                                .color(palette.muted_text),
                            ]
                            .spacing(1),
                        ]
                        .spacing(self.ui_metric(8.0))
                        .align_y(Alignment::Center),
                    )
                    .padding([self.ui_metric(7.0), self.ui_metric(10.0)])
                    .style(move |_| {
                        container::Style::default()
                            .background(palette.menu_bg)
                            .border(border::rounded(7).color(palette.strong_border).width(1))
                            .shadow(iced::Shadow {
                                color: Color::from_rgba8(0, 0, 0, 0.28),
                                offset: Vector::new(0.0, 3.0),
                                blur_radius: 12.0,
                            })
                    })
                    .into()
                })
                .unwrap_or_else(|| Space::new().width(0).height(0).into());
            let duplicate_donut: Element<'_, Message> = tooltip(
                duplicate_interaction,
                duplicate_tooltip_content,
                tooltip::Position::FollowCursor,
            )
            .gap(self.ui_metric(10.0))
            .padding(0)
            .style(|_| container::Style::default().background(Color::TRANSPARENT))
            .into();
            let duplicate_chart = container(
                column![
                    text(duplicate_chart_title)
                        .size(font_size)
                        .color(palette.text)
                        .width(Length::Fill)
                        .align_x(Horizontal::Center),
                    container(duplicate_donut)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                ]
                .spacing(self.ui_metric(6.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center),
            )
            .padding([self.ui_metric(8.0), self.ui_metric(12.0)])
            .width(Length::Fill)
            .height(Length::FillPortion(1))
            .style(move |_| {
                container::Style::default()
                    .background(translucent_color(palette.table_bg, 0.45))
                    .border(border::rounded(8).color(palette.border).width(1))
            });
            chart_cards = chart_cards.push(duplicate_chart);
        }
        let chart_panel = container(chart_cards)
            .width(Length::Fixed(chart_card_width))
            .height(Length::Fill);
        let category_breakdown_width =
            storage_overview_breakdown_width(window_size.width, font_size);
        let category_breakdown = container(self.storage_overview_category_breakdown(
            state,
            palette,
            category_breakdown_width,
        ))
        .width(Length::Fixed(category_breakdown_width))
        .height(Length::Fill);
        let overview = row![category_panel, category_breakdown, chart_panel]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        let action_message = if active {
            Message::CancelStorageAnalysis
        } else {
            Message::CloseStorageAnalysis
        };
        let actions = row![
            Space::new().width(Length::Fill),
            Button::new(text(if active {
                self.localized("Cancelar", "Cancel")
            } else {
                self.localized("Cerrar", "Close")
            }))
            .padding([7, 16])
            .on_press(action_message)
            .style(move |_, status| dialog_button_style(palette, !active, status)),
        ]
        .align_y(Alignment::Center);
        let content: Element<'_, Message> = if let Some(category) = state.selected_category {
            self.storage_category_detail_content(state, category, palette, window_size)
        } else {
            column![overview, actions]
                .spacing(10)
                .height(Length::Fill)
                .into()
        };
        let body_content = if state.selected_category.is_some() {
            column![content]
        } else {
            column![status_card, content].spacing(10)
        };
        let body = container(body_content.padding([10, 12]).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fixed(body_height))
            .style(move |_| container::Style::default().background(window_bg));
        let inner_panel = container(
            column![title_bar, body]
                .width(Length::Fill)
                .height(Length::Fixed(inner_height)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(inner_height))
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(window_bg)
                .border(border::rounded(
                    (window_radius - WINDOW_BORDER_WIDTH).max(0.0),
                ))
        });
        let panel: Element<'_, Message> = mouse_area(
            container(inner_panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(WINDOW_BORDER_WIDTH)
                .clip(true)
                .style(move |_| {
                    container::Style::default()
                        .background(Color::TRANSPARENT)
                        .border(
                            border::rounded(window_radius)
                                .color(window_border_color(palette))
                                .width(WINDOW_BORDER_WIDTH),
                        )
                }),
        )
        .on_move(Message::StorageCategoryPointerMoved)
        .on_release(Message::StopStorageCategoryColumnResize)
        .into();

        let mut layers = vec![panel];
        if state.selected_category.is_some()
            && let Some(context_path) = state.category_context_path.as_ref()
        {
            let menu_width = self.ui_metric(270.0);
            let row_height = context_menu_row_height(font_size);
            let menu_height = row_height * 2.0 + self.ui_metric(8.0);
            let x = state.category_context_position.x.clamp(
                self.ui_metric(8.0),
                window_size.width - menu_width - self.ui_metric(8.0),
            );
            let y = state.category_context_position.y.clamp(
                self.ui_metric(8.0),
                window_size.height - menu_height - self.ui_metric(8.0),
            );
            let open_content = row![
                inline_icon(
                    "open",
                    palette.muted_text,
                    scaled_ui_metric(18.0, font_size),
                ),
                text(self.localized("Abrir", "Open"))
                    .size(font_size)
                    .color(palette.text)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill),
            ]
            .spacing(scaled_ui_metric(12.0, font_size))
            .align_y(Alignment::Center)
            .height(Length::Fill);
            let open_button = Button::new(
                container(open_content)
                    .height(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fixed(row_height))
            .padding([0.0, self.ui_metric(10.0)])
            .on_press(Message::OpenStorageCategoryFile(context_path.clone()))
            .style(move |_, status| selected_button_style(palette, false, status));
            let location_content = row![
                inline_icon(
                    "folder",
                    palette.muted_text,
                    scaled_ui_metric(18.0, font_size),
                ),
                text(self.localized("Abrir ubicación del archivo", "Open file location"))
                    .size(font_size)
                    .color(palette.text)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill),
            ]
            .spacing(scaled_ui_metric(12.0, font_size))
            .align_y(Alignment::Center)
            .height(Length::Fill);
            let location_button = Button::new(
                container(location_content)
                    .height(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fixed(row_height))
            .padding([0.0, self.ui_metric(10.0)])
            .on_press(Message::OpenStorageCategoryFileLocation)
            .style(move |_, status| selected_button_style(palette, false, status));
            let context_menu = container(column![open_button, location_button])
                .width(Length::Fixed(menu_width))
                .padding(self.ui_metric(4.0))
                .style(move |_| {
                    container::Style::default()
                        .background(palette.menu_bg)
                        .border(border::rounded(7).color(palette.strong_border).width(1))
                        .shadow(iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.28),
                            offset: Vector::new(0.0, 7.0),
                            blur_radius: 18.0,
                        })
                });
            let backdrop = mouse_area(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::CloseStorageCategoryRowContext);
            let floating_menu: Element<'_, Message> = float(opaque(context_menu))
                .translate(move |_, _| Vector::new(x, y))
                .into();
            layers.push(backdrop.into());
            layers.push(floating_menu);
        }
        layers.push(storage_analysis_window_resize_handles(
            state.window_maximized,
        ));

        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn storage_overview_category_breakdown<'a>(
        &'a self,
        state: &'a StorageAnalysisState,
        palette: Palette,
        card_width: f32,
    ) -> Element<'a, Message> {
        let font_size = self.font_size();
        let selected_category = state.overview_selected_category;
        let title = selected_category.map_or_else(
            || self.localized("Categoría", "Category"),
            |category| storage_category_label(category, self.is_spanish()),
        );
        let title_color = selected_category
            .map(|category| state.category_colors[category.index()])
            .unwrap_or(palette.muted_text);
        let heading = row![
            container(Space::new())
                .width(self.ui_metric(12.0))
                .height(self.ui_metric(12.0))
                .style(move |_| {
                    container::Style::default()
                        .background(title_color)
                        .border(border::rounded(6))
                }),
            text(title).size(font_size + 1.0).color(palette.text),
        ]
        .spacing(self.ui_metric(8.0))
        .align_y(Alignment::Center);

        let body: Element<'a, Message> = if let Some(category) = selected_category {
            let usage = state.summary.usage(category);
            let summary = format!(
                "{} · {}",
                localized_storage_file_count(usage.files, self.is_spanish()),
                format_size(Some(usage.bytes)),
            );
            let count_column_width = self.ui_metric(72.0);
            let size_column_width = self.ui_metric(82.0);
            let usage_bar_width = self.ui_metric(88.0);
            let column_spacing = self.ui_metric(7.0);
            let extension_column_width =
                storage_overview_extension_column_width(card_width, font_size);
            let max_extension_bytes = state
                .overview_extensions
                .iter()
                .map(|extension| extension.bytes)
                .max()
                .unwrap_or_default();
            let extension_header = container(
                row![
                    text(self.localized("Extensión", "Extension"))
                        .size((font_size - 1.0).max(10.0))
                        .color(palette.muted_text)
                        .width(Length::Fixed(extension_column_width)),
                    Space::new().width(Length::Fixed(usage_bar_width)),
                    text(self.localized("Archivos", "Files"))
                        .size((font_size - 1.0).max(10.0))
                        .color(palette.muted_text)
                        .width(Length::Fixed(count_column_width))
                        .align_x(Horizontal::Right),
                    text(self.localized("Tamaño", "Size"))
                        .size((font_size - 1.0).max(10.0))
                        .color(palette.muted_text)
                        .width(Length::Fixed(size_column_width))
                        .align_x(Horizontal::Right),
                ]
                .spacing(self.ui_metric(7.0))
                .align_y(Alignment::Center),
            )
            .padding([0.0, self.ui_metric(8.0)])
            .width(Length::Fill);
            let extension_row_height = self.ui_metric(32.0);
            let extension_range = virtual_table_range(
                state.overview_extensions.len(),
                extension_row_height,
                state.overview_extension_scroll_offset_y,
                state.overview_extension_viewport_height,
                state.overview_extension_scroll_velocity_y,
            );
            let mut extension_rows = column![];
            if extension_range.before > 0.0 {
                extension_rows = extension_rows.push(
                    Space::new()
                        .width(Length::Fill)
                        .height(Length::Fixed(extension_range.before)),
                );
            }
            for index in extension_range.start..extension_range.end {
                let extension = &state.overview_extensions[index];
                let label = extension.extension.as_ref().map_or_else(
                    || self.localized("Sin extensión", "No extension").to_owned(),
                    |extension| extension.to_uppercase(),
                );
                let label = ellipsize_to_glyph_width(&label, extension_column_width, font_size);
                let selected = state.overview_selected_extension == Some(index);
                let usage_ratio = if max_extension_bytes == 0 {
                    0.0
                } else {
                    extension.bytes as f32 / max_extension_bytes as f32
                };
                let usage_fill_width = if extension.bytes == 0 {
                    0.0
                } else {
                    (usage_bar_width * usage_ratio).max(self.ui_metric(2.0))
                };
                let usage_track = container(Space::new())
                    .width(Length::Fixed(usage_bar_width))
                    .height(Length::Fixed(self.ui_metric(7.0)))
                    .style(move |_| {
                        container::Style::default()
                            .background(translucent_color(palette.muted_text, 0.18))
                            .border(border::rounded(2))
                    });
                let usage_fill = container(Space::new())
                    .width(Length::Fixed(usage_fill_width))
                    .height(Length::Fixed(self.ui_metric(7.0)))
                    .style(move |_| {
                        container::Style::default()
                            .background(title_color)
                            .border(border::rounded(2))
                    });
                extension_rows = extension_rows.push(
                    Button::new(
                        row![
                            text(label)
                                .size(font_size)
                                .color(palette.text)
                                .wrapping(iced::widget::text::Wrapping::None)
                                .width(Length::Fixed(extension_column_width)),
                            container(stack(vec![usage_track.into(), usage_fill.into()]))
                                .width(Length::Fixed(usage_bar_width))
                                .height(Length::Fill)
                                .center_y(Length::Fill),
                            text(extension.files.to_string())
                                .size(font_size)
                                .color(palette.text)
                                .width(Length::Fixed(count_column_width))
                                .align_x(Horizontal::Right),
                            text(format_size(Some(extension.bytes)))
                                .size(font_size)
                                .color(palette.text)
                                .width(Length::Fixed(size_column_width))
                                .align_x(Horizontal::Right),
                        ]
                        .spacing(column_spacing)
                        .align_y(Alignment::Center),
                    )
                    .padding([self.ui_metric(5.0), self.ui_metric(8.0)])
                    .width(Length::Fill)
                    .height(Length::Fixed(extension_row_height))
                    .on_press(Message::SelectStorageOverviewExtension(index))
                    .style(move |_, status| {
                        let background = if selected {
                            Some(mix_color(palette.table_bg, palette.accent, 0.24).into())
                        } else if matches!(
                            status,
                            button::Status::Hovered | button::Status::Pressed
                        ) {
                            Some(translucent_color(palette.hover, 0.55).into())
                        } else {
                            Some(Color::TRANSPARENT.into())
                        };
                        button::Style {
                            background,
                            text_color: palette.text,
                            border: border::rounded(3)
                                .color(if selected {
                                    mix_color(palette.border, palette.accent, 0.65)
                                } else {
                                    Color::TRANSPARENT
                                })
                                .width(1),
                            ..button::Style::default()
                        }
                    }),
                );
            }
            if extension_range.after > 0.0 {
                extension_rows = extension_rows.push(
                    Space::new()
                        .width(Length::Fill)
                        .height(Length::Fixed(extension_range.after)),
                );
            }
            let extensions: Element<'_, Message> = scrollable(extension_rows)
                .id(storage_overview_extension_scroll_id())
                .direction(scrollable::Direction::Vertical(
                    explorer_scrollbar(f32::from(state.scrollbar_vertical_hovered)).spacing(4.0),
                ))
                .height(Length::Fill)
                .width(Length::Fill)
                .on_scroll(|viewport| Message::StorageOverviewExtensionScrolled {
                    offset_y: viewport.absolute_offset().y,
                    viewport_height: viewport.bounds().height,
                })
                .style(move |theme, status| {
                    explorer_scrollable_style(
                        palette,
                        theme,
                        status,
                        state.scrollbar_reveal_progress,
                    )
                })
                .into();
            let extensions = scrollbar_proximity_layer(
                extensions,
                Some((
                    Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Vertical, true),
                    Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Vertical, false),
                )),
                None,
            );
            column![
                text(summary)
                    .size((font_size - 1.0).max(10.0))
                    .color(palette.muted_text),
                extension_header,
                container(Space::new())
                    .width(Length::Fill)
                    .height(1)
                    .style(move |_| container::Style::default().background(palette.border)),
                extensions,
            ]
            .spacing(self.ui_metric(7.0))
            .height(Length::Fill)
            .into()
        } else {
            container(
                text(self.localized(
                    "Selecciona una categoría para ver sus extensiones y tamaños.",
                    "Select a category to see its extensions and sizes.",
                ))
                .size(font_size)
                .color(palette.muted_text)
                .align_x(Horizontal::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into()
        };
        let details_message = selected_category.map(Message::OpenStorageCategory);
        let details = row![
            Space::new().width(Length::Fill),
            dialog_action_button(
                self.localized("Ver detalles", "View details"),
                details_message,
                true,
                palette,
                font_size,
            ),
        ]
        .align_y(Alignment::Center);

        container(
            column![heading, body, details]
                .spacing(self.ui_metric(9.0))
                .height(Length::Fill),
        )
        .padding([self.ui_metric(10.0), self.ui_metric(12.0)])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| {
            container::Style::default()
                .background(translucent_color(palette.table_bg, 0.45))
                .border(border::rounded(8).color(palette.border).width(1))
        })
        .into()
    }

    fn storage_category_detail_content<'a>(
        &'a self,
        state: &'a StorageAnalysisState,
        category: StorageCategory,
        palette: Palette,
        window_size: Size,
    ) -> Element<'a, Message> {
        let font_size = self.font_size();
        let files = state.files.get(category);
        let usage = state.summary.usage(category);
        let category_color = state.category_colors[category.index()];
        let category_summary = if self.is_spanish() {
            format!(
                "{} · {}",
                localized_storage_file_count(usage.files, true),
                format_size(Some(usage.bytes))
            )
        } else {
            format!(
                "{} · {}",
                localized_storage_file_count(usage.files, false),
                format_size(Some(usage.bytes))
            )
        };
        let mut category_text = column![
            text(storage_category_label(category, self.is_spanish()))
                .size(font_size + 1.0)
                .color(palette.text),
            text(category_summary)
                .size((font_size - 1.0).max(10.0))
                .color(palette.muted_text),
        ]
        .spacing(1);
        if category == StorageCategory::Other
            && let Some(extension_summary) =
                storage_other_extensions_summary(files, self.is_spanish())
        {
            category_text = category_text.push(
                text(extension_summary)
                    .size((font_size - 2.0).max(9.0))
                    .color(palette.muted_text),
            );
        }
        let category_heading = container(
            row![
                container(Space::new())
                    .width(self.ui_metric(12.0))
                    .height(self.ui_metric(12.0))
                    .style(move |_| {
                        container::Style::default()
                            .background(category_color)
                            .border(border::rounded(6))
                    }),
                category_text,
            ]
            .spacing(self.ui_metric(9.0))
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([self.ui_metric(4.0), self.ui_metric(2.0)]);
        let category_filter = text_input(
            self.localized("Filtrar por nombre", "Filter by name"),
            &state.category_filter,
        )
        .on_input(Message::StorageCategoryFilterChanged)
        .size(font_size)
        .padding([self.ui_metric(6.0), self.ui_metric(9.0)])
        .width(Length::Fill)
        .style(move |_, status| {
            let border_color = if matches!(status, iced::widget::text_input::Status::Focused { .. })
            {
                palette.accent
            } else {
                palette.strong_border
            };
            iced::widget::text_input::Style {
                background: chrome_glass_background(palette, palette.input_bg).into(),
                border: border::rounded(6).color(border_color).width(1),
                icon: palette.muted_text,
                placeholder: palette.muted_text,
                value: palette.text,
                selection: translucent_color(palette.accent, 0.58),
            }
        });

        let mut column_widths = std::array::from_fn::<_, 6, _>(|index| {
            state.category_column_widths[index].max(storage_category_table_column_min_width(
                index,
                font_size,
                self.is_spanish(),
            ))
        });
        let available_table_width = (window_size.width - self.ui_metric(30.0)).max(0.0);
        let current_table_width = column_widths.iter().sum::<f32>();
        column_widths[5] += (available_table_width - current_table_width).max(0.0);
        let [
            name_column_width,
            type_column_width,
            size_column_width,
            created_column_width,
            modified_column_width,
            location_column_width,
        ] = column_widths;
        let table_content_width = column_widths.iter().sum::<f32>();

        let table_header = row![
            storage_table_header_cell(
                self.localized("Nombre", "Name"),
                name_column_width,
                0,
                StorageCategorySortColumn::Name,
                (state.category_sort_column, state.category_sort_ascending),
                palette,
                font_size,
            ),
            storage_table_header_cell(
                self.localized("Tipo", "Type"),
                type_column_width,
                1,
                StorageCategorySortColumn::Type,
                (state.category_sort_column, state.category_sort_ascending),
                palette,
                font_size,
            ),
            storage_table_header_cell(
                self.localized("Tamaño", "Size"),
                size_column_width,
                2,
                StorageCategorySortColumn::Size,
                (state.category_sort_column, state.category_sort_ascending),
                palette,
                font_size,
            ),
            storage_table_header_cell(
                self.localized("Fecha de creación", "Creation date"),
                created_column_width,
                3,
                StorageCategorySortColumn::Created,
                (state.category_sort_column, state.category_sort_ascending),
                palette,
                font_size,
            ),
            storage_table_header_cell(
                self.localized("Fecha de modificación", "Modification date"),
                modified_column_width,
                4,
                StorageCategorySortColumn::Modified,
                (state.category_sort_column, state.category_sort_ascending),
                palette,
                font_size,
            ),
            storage_table_header_cell(
                self.localized("Ubicación", "Location"),
                location_column_width,
                5,
                StorageCategorySortColumn::Location,
                (state.category_sort_column, state.category_sort_ascending),
                palette,
                font_size,
            ),
        ]
        .width(Length::Fixed(table_content_width))
        .height(self.ui_metric(DUPLICATE_TABLE_HEADER_HEIGHT))
        .align_y(Alignment::Center);

        let filtered_indices = state.category_filter_matches.as_deref();
        let total_entries = filtered_indices.map_or(files.len(), <[usize]>::len);
        let row_height = self.ui_metric(DUPLICATE_TABLE_ROW_HEIGHT);
        let visible_range = virtual_table_range(
            total_entries,
            row_height,
            state.category_scroll_offset_y,
            state.category_viewport_height,
            state.category_scroll_velocity_y,
        );
        let mut rows = column![].width(Length::Fill);
        if visible_range.before > 0.0 {
            rows = rows.push(
                Space::new()
                    .width(Length::Fixed(table_content_width))
                    .height(Length::Fixed(visible_range.before)),
            );
        }
        for visible_index in visible_range.start..visible_range.end {
            let entry = filtered_indices.map_or_else(
                || &files[visible_index],
                |indices| &files[indices[visible_index]],
            );
            let file_entry = crate::iced_ui::storage_analysis::storage_file_entry(entry);
            let name = file_entry.name.clone();
            let type_label = self.localized_entry_type_label(&file_entry);
            let location = entry
                .path
                .parent()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "—".into());
            let highlighted = state.category_highlighted.as_ref() == Some(&entry.path);
            let thumbnail = self.detail_file_entry_icon(
                &file_entry,
                palette,
                highlighted,
                self.ui_metric(24.0),
            );
            let row_content = row![
                container(
                    row![
                        thumbnail,
                        text(name)
                            .size(font_size)
                            .color(palette.text)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .spacing(self.ui_metric(8.0))
                    .align_y(Alignment::Center),
                )
                .width(Length::Fixed(name_column_width))
                .padding([0.0, self.ui_metric(8.0)])
                .clip(true),
                table_value_cell(type_label, type_column_width, palette, font_size),
                table_value_cell(
                    format_size(Some(entry.size)),
                    size_column_width,
                    palette,
                    font_size,
                ),
                table_value_cell(
                    format_duplicate_time(entry.created, self.is_spanish()),
                    created_column_width,
                    palette,
                    font_size,
                ),
                table_value_cell(
                    format_duplicate_time(entry.modified, self.is_spanish()),
                    modified_column_width,
                    palette,
                    font_size,
                ),
                table_value_cell(location, location_column_width, palette, font_size),
            ]
            .width(Length::Fixed(table_content_width))
            .height(self.ui_metric(DUPLICATE_TABLE_ROW_HEIGHT))
            .align_y(Alignment::Center);
            let selected_path = entry.path.clone();
            let context_path = entry.path.clone();
            let open_path = entry.path.clone();
            rows = rows.push(
                mouse_area(
                    container(row_content)
                        .width(Length::Fixed(table_content_width))
                        .style(move |_| {
                            container::Style::default()
                                .background(if highlighted {
                                    mix_color(palette.table_bg, palette.accent, 0.2)
                                } else {
                                    palette.table_bg
                                })
                                .border(border::rounded(0).color(palette.border).width(1))
                        }),
                )
                .on_press(Message::StorageCategoryRowSelected(selected_path))
                .on_double_click(Message::OpenStorageCategoryFile(open_path))
                .on_right_press(Message::OpenStorageCategoryRowContext(context_path))
                .interaction(mouse::Interaction::Pointer),
            );
        }
        if visible_range.after > 0.0 {
            rows = rows.push(
                Space::new()
                    .width(Length::Fixed(table_content_width))
                    .height(Length::Fixed(visible_range.after)),
            );
        }
        if files.is_empty() {
            rows = rows.push(
                container(
                    text(self.localized(
                        "No hay archivos en esta categoría.",
                        "There are no files in this category.",
                    ))
                    .size(font_size)
                    .color(palette.muted_text),
                )
                .height(Length::Fixed(self.ui_metric(90.0)))
                .center(Length::Fill),
            );
        } else if total_entries == 0 {
            rows = rows.push(
                container(
                    text(self.localized(
                        "No hay coincidencias para este filtro.",
                        "No files match this filter.",
                    ))
                    .size(font_size)
                    .color(palette.muted_text),
                )
                .height(Length::Fixed(self.ui_metric(90.0)))
                .center(Length::Fill),
            );
        }

        let header = scrollable(
            container(table_header)
                .width(Length::Fixed(table_content_width))
                .style(move |_| {
                    container::Style::default()
                        .background(palette.header_bg)
                        .border(border::rounded(0).color(palette.strong_border).width(1))
                }),
        )
        .id(storage_category_table_header_scroll_id())
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::hidden(),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(self.ui_metric(DUPLICATE_TABLE_HEADER_HEIGHT)));
        let table_rows: Element<'_, Message> =
            scrollable(rows.width(Length::Fixed(table_content_width)))
                .id(storage_category_table_scroll_id())
                .direction(scrollable::Direction::Both {
                    vertical: explorer_scrollbar(f32::from(state.scrollbar_vertical_hovered)),
                    horizontal: explorer_scrollbar(f32::from(state.scrollbar_horizontal_hovered)),
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .on_scroll(|viewport| Message::StorageCategoryTableScrolled {
                    offset_x: viewport.absolute_offset().x,
                    offset_y: viewport.absolute_offset().y,
                    viewport_height: viewport.bounds().height,
                })
                .style(move |theme, status| {
                    explorer_scrollable_style(
                        palette,
                        theme,
                        status,
                        state.scrollbar_reveal_progress,
                    )
                })
                .into();
        let table_rows = scrollbar_proximity_layer(
            table_rows,
            Some((
                Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Vertical, true),
                Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Vertical, false),
            )),
            Some((
                Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Horizontal, true),
                Message::StorageAnalysisScrollbarHover(ScrollbarAxis::Horizontal, false),
            )),
        );
        let table = container(column![header, table_rows].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| {
                container::Style::default()
                    .background(palette.table_bg)
                    .border(border::rounded(5).color(palette.border).width(1))
            });

        let action_row: Element<'_, Message> = row![
            if let Some(error) = state.error.as_deref() {
                text(ellipsize_text(error, 65))
                    .size(font_size - 1.0)
                    .color(Color::from_rgb8(210, 72, 72))
            } else {
                text("")
            },
            Space::new().width(Length::Fill),
            dialog_action_button(
                self.localized("Volver", "Back"),
                Some(Message::BackToStorageOverview),
                false,
                palette,
                font_size,
            ),
        ]
        .spacing(self.ui_metric(8.0))
        .align_y(Alignment::Center)
        .into();

        column![category_heading, category_filter, table, action_row]
            .spacing(self.ui_metric(8.0))
            .height(Length::Fill)
            .into()
    }
}

fn storage_table_header_cell<'a>(
    label: &'a str,
    width: f32,
    column: usize,
    sort_column: StorageCategorySortColumn,
    sort_state: (StorageCategorySortColumn, bool),
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let (active_sort_column, sort_ascending) = sort_state;
    let indicator = if sort_column == active_sort_column {
        if sort_ascending { "▲" } else { "▼" }
    } else {
        ""
    };
    storage_table_header_content_cell(
        row![
            text(label)
                .size(font_size)
                .color(palette.text)
                .width(Length::Fill)
                .align_x(Horizontal::Left)
                .wrapping(iced::widget::text::Wrapping::None),
            text(indicator)
                .size((font_size - 2.0).max(9.0))
                .color(palette.accent),
        ]
        .spacing(scaled_ui_metric(5.0, font_size))
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into(),
        width,
        column,
        sort_column,
        palette,
        font_size,
    )
}

fn storage_table_header_content_cell<'a>(
    content: Element<'a, Message>,
    width: f32,
    column: usize,
    sort_column: StorageCategorySortColumn,
    palette: Palette,
    font_size: f32,
) -> Element<'a, Message> {
    let handle_width = scaled_ui_metric(DUPLICATE_TABLE_COLUMN_HANDLE_WIDTH, font_size);
    let handle = mouse_area(
        container(
            container(Space::new())
                .width(1.0)
                .height(Length::Fill)
                .style(move |_| container::Style::default().background(palette.border)),
        )
        .width(handle_width)
        .height(Length::Fill)
        .align_x(Horizontal::Center),
    )
    .on_press(Message::StartStorageCategoryColumnResize(column))
    .interaction(mouse::Interaction::ResizingColumn);

    let sort_area = mouse_area(
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([0.0, scaled_ui_metric(8.0, font_size)])
            .center_y(Length::Fill)
            .clip(true),
    )
    .on_press(Message::SortStorageCategoryColumn(sort_column))
    .interaction(mouse::Interaction::Pointer);

    container(
        row![sort_area, handle]
            .height(Length::Fill)
            .align_y(Alignment::Center),
    )
    .width(Length::Fixed(width))
    .clip(true)
    .into()
}

fn storage_analysis_window_resize_handles(maximized: bool) -> Element<'static, Message> {
    if maximized {
        return Space::new().width(Length::Fill).height(Length::Fill).into();
    }
    let edge = WINDOW_RESIZE_HANDLE_WIDTH;
    let corner = WINDOW_RESIZE_HANDLE_WIDTH * 1.8;
    column![
        row![
            storage_analysis_window_resize_handle(corner, edge, window::Direction::NorthWest),
            storage_analysis_window_resize_handle(Length::Fill, edge, window::Direction::North),
            storage_analysis_window_resize_handle(corner, edge, window::Direction::NorthEast),
        ]
        .height(edge),
        row![
            storage_analysis_window_resize_handle(edge, Length::Fill, window::Direction::West),
            Space::new().width(Length::Fill).height(Length::Fill),
            storage_analysis_window_resize_handle(edge, Length::Fill, window::Direction::East),
        ]
        .height(Length::Fill),
        row![
            storage_analysis_window_resize_handle(corner, edge, window::Direction::SouthWest),
            storage_analysis_window_resize_handle(Length::Fill, edge, window::Direction::South),
            storage_analysis_window_resize_handle(corner, edge, window::Direction::SouthEast),
        ]
        .height(edge),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn storage_analysis_window_resize_handle<W, H>(
    width: W,
    height: H,
    direction: window::Direction,
) -> Element<'static, Message>
where
    W: Into<Length>,
    H: Into<Length>,
{
    let interaction = match direction {
        window::Direction::East | window::Direction::West => {
            mouse::Interaction::ResizingHorizontally
        }
        window::Direction::North | window::Direction::South => {
            mouse::Interaction::ResizingVertically
        }
        window::Direction::NorthEast | window::Direction::SouthWest => {
            mouse::Interaction::ResizingDiagonallyUp
        }
        window::Direction::NorthWest | window::Direction::SouthEast => {
            mouse::Interaction::ResizingDiagonallyDown
        }
    };
    mouse_area(
        container(Space::new())
            .width(width)
            .height(height)
            .style(|_| container::Style::default().background(Color::TRANSPARENT)),
    )
    .on_press(Message::StorageAnalysisWindowResize(direction))
    .interaction(interaction)
    .into()
}

fn storage_category_label(category: StorageCategory, spanish: bool) -> &'static str {
    match (category, spanish) {
        (StorageCategory::Documents, true) => "Documentos",
        (StorageCategory::Documents, false) => "Documents",
        (StorageCategory::Images, true) => "Imágenes",
        (StorageCategory::Images, false) => "Images",
        (StorageCategory::Videos, true) => "Vídeos",
        (StorageCategory::Videos, false) => "Videos",
        (StorageCategory::Audio, _) => "Audio",
        (StorageCategory::Archives, true) => "Comprimidos",
        (StorageCategory::Archives, false) => "Archives",
        (StorageCategory::WindowsExecutables, true) => "Ejecutables Windows",
        (StorageCategory::WindowsExecutables, false) => "Windows executables",
        (StorageCategory::LinuxPackages, true) => "Paquetes de Linux",
        (StorageCategory::LinuxPackages, false) => "Linux packages",
        (StorageCategory::MacOsPackages, true) => "Paquetes de macOS",
        (StorageCategory::MacOsPackages, false) => "macOS packages",
        (StorageCategory::OtherApplications, true) => "Otras aplicaciones",
        (StorageCategory::OtherApplications, false) => "Other applications",
        (StorageCategory::Databases, true) => "Bases de datos",
        (StorageCategory::Databases, false) => "Databases",
        (StorageCategory::Backups, true) => "Copias de seguridad",
        (StorageCategory::Backups, false) => "Backups",
        (StorageCategory::DiskImages, true) => "Imágenes de disco",
        (StorageCategory::DiskImages, false) => "Disk images",
        (StorageCategory::VirtualMachines, true) => "Máquinas virtuales",
        (StorageCategory::VirtualMachines, false) => "Virtual machines",
        (StorageCategory::SystemFiles, true) => "Sistema y temporales",
        (StorageCategory::SystemFiles, false) => "System and temporary",
        (StorageCategory::Other, true) => "Otros",
        (StorageCategory::Other, false) => "Other",
    }
}

fn localized_storage_file_count(files: u64, spanish: bool) -> String {
    match (files, spanish) {
        (1, true) => "1 archivo".to_owned(),
        (_, true) => format!("{files} archivos"),
        (1, false) => "1 file".to_owned(),
        (_, false) => format!("{files} files"),
    }
}

fn storage_other_extensions_summary(files: &[StorageFile], spanish: bool) -> Option<String> {
    let mut extensions: HashMap<Option<String>, (u64, u64)> = HashMap::new();
    for file in files {
        let extension = file
            .path
            .extension()
            .filter(|extension| !extension.is_empty())
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
        let usage = extensions.entry(extension).or_default();
        usage.0 = usage.0.saturating_add(file.size);
        usage.1 = usage.1.saturating_add(1);
    }
    if extensions.is_empty() {
        return None;
    }

    let mut ranked = extensions.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .0
            .cmp(&left.1.0)
            .then_with(|| right.1.1.cmp(&left.1.1))
            .then_with(|| left.0.cmp(&right.0))
    });
    let hidden = ranked.len().saturating_sub(3);
    let labels = ranked
        .into_iter()
        .take(3)
        .map(|(extension, _)| {
            extension.map_or_else(
                || {
                    if spanish {
                        "sin extensión".to_owned()
                    } else {
                        "no extension".to_owned()
                    }
                },
                |extension| format!(".{extension}"),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let prefix = if spanish { "Extensiones" } else { "Extensions" };
    let remainder = match (hidden, spanish) {
        (0, _) => String::new(),
        (1, true) => " · +1 más".to_owned(),
        (1, false) => " · +1 more".to_owned(),
        (_, true) => format!(" · +{hidden} más"),
        (_, false) => format!(" · +{hidden} more"),
    };
    Some(format!("{prefix}: {labels}{remainder}"))
}

fn storage_category_percentage(summary: &StorageAnalysisSummary, category: StorageCategory) -> f64 {
    if summary.total_bytes == 0 {
        0.0
    } else {
        summary.usage(category).bytes as f64 * 100.0 / summary.total_bytes as f64
    }
}

fn storage_donut_handle(
    summary: &StorageAnalysisSummary,
    category_colors: &StorageCategoryColors,
    palette: Palette,
    hovered_category: Option<StorageCategory>,
) -> svg::Handle {
    svg::Handle::from_memory(
        storage_donut_svg(summary, category_colors, palette, hovered_category).into_bytes(),
    )
}

fn duplicate_estimate_donut_handle(
    estimate: &StorageDuplicateEstimate,
    animation_progress: f32,
    palette: Palette,
) -> svg::Handle {
    svg::Handle::from_memory(
        duplicate_estimate_donut_svg(estimate, animation_progress, palette).into_bytes(),
    )
}

const STORAGE_DONUT_RADIUS: f64 = 82.0;
const STORAGE_DONUT_CIRCUMFERENCE: f64 = std::f64::consts::TAU * STORAGE_DONUT_RADIUS;
const STORAGE_DONUT_VIEWBOX_SIZE: f64 = 240.0;
const STORAGE_DONUT_CENTER: f64 = 120.0;
const STORAGE_DONUT_INNER_RADIUS: f64 = 67.0;
const STORAGE_DONUT_OUTER_RADIUS: f64 = 97.0;

fn duplicate_estimate_center_text(
    estimate: &StorageDuplicateEstimate,
    spanish: bool,
) -> (&'static str, String) {
    match estimate.phase {
        StorageDuplicateEstimatePhase::Counting | StorageDuplicateEstimatePhase::Scanning => {
            if spanish {
                ("Calculando", "Duplicados".to_owned())
            } else {
                ("Calculating", "Duplicates".to_owned())
            }
        }
        StorageDuplicateEstimatePhase::Complete => (
            if spanish {
                "Total duplicados"
            } else {
                "Duplicate total"
            },
            format_size(Some(estimate.summary.total_bytes)),
        ),
        StorageDuplicateEstimatePhase::Cancelled => {
            if spanish {
                ("Cálculo", "Cancelado".to_owned())
            } else {
                ("Calculation", "Cancelled".to_owned())
            }
        }
        StorageDuplicateEstimatePhase::Failed => {
            if spanish {
                ("Duplicados", "No disponible".to_owned())
            } else {
                ("Duplicates", "Unavailable".to_owned())
            }
        }
        StorageDuplicateEstimatePhase::Waiting => ("", String::new()),
    }
}

fn duplicate_estimate_donut_svg(
    estimate: &StorageDuplicateEstimate,
    animation_progress: f32,
    palette: Palette,
) -> String {
    let active = matches!(
        estimate.phase,
        StorageDuplicateEstimatePhase::Counting | StorageDuplicateEstimatePhase::Scanning
    );
    let animation_progress = if animation_progress.is_finite() {
        animation_progress.rem_euclid(1.0) as f64
    } else {
        0.0
    };
    let indeterminate = active && (estimate.total == 0 || estimate.scanned == 0);
    let segment_length = if indeterminate {
        STORAGE_DONUT_CIRCUMFERENCE * 0.28
    } else if active {
        (estimate.scanned.min(estimate.total) as f64 / estimate.total as f64)
            * STORAGE_DONUT_CIRCUMFERENCE
    } else {
        0.0
    };
    let remainder = (STORAGE_DONUT_CIRCUMFERENCE - segment_length).max(0.0);
    let offset = if indeterminate {
        -animation_progress * STORAGE_DONUT_CIRCUMFERENCE
    } else {
        0.0
    };
    let base = palette.accent;
    let light = color_to_svg_hex(mix_color(base, Color::WHITE, 0.14));
    let color = color_to_svg_hex(base);
    let dark = color_to_svg_hex(mix_color(base, Color::BLACK, 0.10));
    let track = color_to_svg_hex(palette.border);
    let outline = color_to_svg_hex(mix_color(palette.border, palette.text, 0.18));
    let segment = if segment_length > f64::EPSILON {
        format!(
            r#"<circle cx="120" cy="120" r="82" fill="none" stroke="url(#duplicate-segment)" stroke-width="30" stroke-linecap="butt" stroke-dasharray="{segment_length:.6} {remainder:.6}" stroke-dashoffset="{offset:.6}" transform="rotate(-90 120 120)"/>"#
        )
    } else {
        String::new()
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 240 240" shape-rendering="geometricPrecision">
<defs><linearGradient id="duplicate-segment" gradientUnits="userSpaceOnUse" x1="35" y1="35" x2="205" y2="205"><stop offset="0%" stop-color="{light}"/><stop offset="52%" stop-color="{color}"/><stop offset="100%" stop-color="{dark}"/></linearGradient></defs>
<circle cx="120" cy="120" r="82" fill="none" stroke="{track}" stroke-opacity="0.45" stroke-width="30"/>
{segment}
<circle cx="120" cy="120" r="67" fill="none" stroke="{outline}" stroke-opacity="0.28" stroke-width="0.8"/>
<circle cx="120" cy="120" r="97" fill="none" stroke="{outline}" stroke-opacity="0.22" stroke-width="0.8"/>
</svg>"#
    )
}

fn storage_donut_category_at_point(
    summary: &StorageAnalysisSummary,
    point: Point,
    rendered_size: f32,
) -> Option<StorageCategory> {
    if summary.total_bytes == 0
        || !rendered_size.is_finite()
        || rendered_size <= f32::EPSILON
        || !point.x.is_finite()
        || !point.y.is_finite()
    {
        return None;
    }
    let scale = STORAGE_DONUT_VIEWBOX_SIZE / f64::from(rendered_size);
    let x = f64::from(point.x) * scale - STORAGE_DONUT_CENTER;
    let y = f64::from(point.y) * scale - STORAGE_DONUT_CENTER;
    let radius = x.hypot(y);
    if !(STORAGE_DONUT_INNER_RADIUS..=STORAGE_DONUT_OUTER_RADIUS).contains(&radius) {
        return None;
    }

    let angle = (y.atan2(x) + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU);
    let target = angle / std::f64::consts::TAU * summary.total_bytes as f64;
    let mut accumulated = 0.0_f64;
    let mut last_non_empty = None;
    for category in StorageCategory::ALL {
        let bytes = summary.usage(category).bytes;
        if bytes == 0 {
            continue;
        }
        last_non_empty = Some(category);
        accumulated += bytes as f64;
        if target < accumulated {
            return Some(category);
        }
    }
    last_non_empty
}

fn storage_donut_svg(
    summary: &StorageAnalysisSummary,
    category_colors: &StorageCategoryColors,
    palette: Palette,
    hovered_category: Option<StorageCategory>,
) -> String {
    let track = color_to_svg_hex(palette.border);
    let outline = color_to_svg_hex(mix_color(palette.border, palette.text, 0.18));
    let mut gradients = String::new();
    let mut segments = String::new();
    let mut offset = 0.0_f64;
    if summary.total_bytes > 0 {
        for category in StorageCategory::ALL {
            let bytes = summary.usage(category).bytes;
            if bytes == 0 {
                continue;
            }
            let segment_length =
                bytes as f64 * STORAGE_DONUT_CIRCUMFERENCE / summary.total_bytes as f64;
            let remainder = (STORAGE_DONUT_CIRCUMFERENCE - segment_length).max(0.0);
            let highlighted = hovered_category == Some(category);
            let base = category_colors[category.index()];
            let light = color_to_svg_hex(mix_color(
                base,
                Color::WHITE,
                if highlighted { 0.20 } else { 0.11 },
            ));
            let color = color_to_svg_hex(if highlighted {
                mix_color(base, Color::WHITE, 0.07)
            } else {
                base
            });
            let dark = color_to_svg_hex(mix_color(
                base,
                Color::BLACK,
                if highlighted { 0.03 } else { 0.08 },
            ));
            let gradient_id = format!("storage-segment-{}", category.index());
            gradients.push_str(&format!(
                r#"<linearGradient id="{gradient_id}" gradientUnits="userSpaceOnUse" x1="35" y1="35" x2="205" y2="205"><stop offset="0%" stop-color="{light}"/><stop offset="52%" stop-color="{color}"/><stop offset="100%" stop-color="{dark}"/></linearGradient>"#
            ));
            let opacity = if hovered_category.is_some() && !highlighted {
                0.88
            } else {
                1.0
            };
            segments.push_str(&format!(
                r#"<circle cx="120" cy="120" r="82" fill="none" stroke="url(#{gradient_id})" stroke-width="30" stroke-opacity="{opacity:.2}" stroke-linecap="butt" stroke-dasharray="{segment_length:.6} {remainder:.6}" stroke-dashoffset="{:.6}" transform="rotate(-90 120 120)"/>"#,
                -offset,
            ));
            offset += segment_length;
        }
    }
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 240 240" shape-rendering="geometricPrecision">
<defs>{gradients}</defs>
<circle cx="120" cy="120" r="82" fill="none" stroke="{track}" stroke-opacity="0.45" stroke-width="30"/>
{segments}
<circle cx="120" cy="120" r="67" fill="none" stroke="{outline}" stroke-opacity="0.28" stroke-width="0.8"/>
<circle cx="120" cy="120" r="97" fill="none" stroke="{outline}" stroke-opacity="0.22" stroke-width="0.8"/>
</svg>"#
    )
}

fn color_to_svg_hex(color: Color) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_file(path: &str, size: u64) -> StorageFile {
        StorageFile {
            path: PathBuf::from(path),
            size,
            created: None,
            modified: None,
        }
    }

    #[test]
    fn extension_card_minimum_preserves_useful_name_width() {
        for font_size in [10.0, 13.0, 18.0] {
            let card_width = scaled_ui_metric(STORAGE_OVERVIEW_BREAKDOWN_MIN_WIDTH, font_size);
            assert!(
                storage_overview_extension_column_width(card_width, font_size) >= 100.0,
                "the extension label must remain readable at font size {font_size}"
            );
        }

        assert!(
            storage_overview_breakdown_width(STORAGE_ANALYSIS_WINDOW_WIDTH, 13.0)
                >= scaled_ui_metric(STORAGE_OVERVIEW_BREAKDOWN_MIN_WIDTH, 13.0)
        );
    }

    #[test]
    fn other_extensions_are_grouped_case_insensitively_and_ranked_by_size() {
        let files = [
            storage_file("root/a.LOG", 100),
            storage_file("root/b.log", 20),
            storage_file("root/catalog.xyz", 200),
            storage_file("root/README", 50),
            storage_file("root/cache.tmp", 10),
        ];

        assert_eq!(
            storage_other_extensions_summary(&files, true).as_deref(),
            Some("Extensiones: .xyz, .log, sin extensión · +1 más")
        );
        assert_eq!(
            storage_other_extensions_summary(&files, false).as_deref(),
            Some("Extensions: .xyz, .log, no extension · +1 more")
        );
        assert_eq!(storage_other_extensions_summary(&[], true), None);
    }

    #[test]
    fn duplicate_donut_exposes_native_center_text_while_svg_keeps_only_shapes() {
        let estimate = StorageDuplicateEstimate {
            phase: StorageDuplicateEstimatePhase::Scanning,
            scanned: 50,
            total: 100,
            ..StorageDuplicateEstimate::default()
        };
        let source = duplicate_estimate_donut_svg(
            &estimate,
            0.25,
            Palette::from_config(&AppConfig::default(), true),
        );
        let center = duplicate_estimate_center_text(&estimate, true);

        assert_eq!(center, ("Calculando", "Duplicados".to_owned()));
        assert!(!source.contains("<text"));
        assert!(source.contains("id=\"duplicate-segment\""));
    }

    #[test]
    fn completed_duplicate_donut_is_partitioned_by_category() {
        let mut summary = StorageAnalysisSummary::default();
        summary.add_file_for_test(StorageCategory::Documents, 100);
        summary.add_file_for_test(StorageCategory::Videos, 150);
        let colors = crate::iced_ui::storage_analysis::storage_category_colors_from_seed(42);
        let source = storage_donut_svg(
            &summary,
            &colors,
            Palette::from_config(&AppConfig::default(), true),
            None,
        );

        assert_eq!(source.matches("stroke-dasharray").count(), 2);
        assert!(!source.contains("<text"));
    }

    #[test]
    fn donut_segments_partition_the_complete_total_without_repeating_arcs() {
        let mut summary = StorageAnalysisSummary::default();
        summary.add_file_for_test(StorageCategory::Documents, 40);
        summary.add_file_for_test(StorageCategory::Videos, 35);
        summary.add_file_for_test(StorageCategory::Other, 25);
        let total = StorageCategory::ALL
            .into_iter()
            .map(|category| summary.usage(category).bytes)
            .sum::<u64>();
        assert_eq!(total, summary.total_bytes);
        assert_eq!(total, 100);

        let colors = crate::iced_ui::storage_analysis::storage_category_colors_from_seed(42);
        let source = storage_donut_svg(
            &summary,
            &colors,
            Palette::from_config(&AppConfig::default(), true),
            None,
        );
        assert_eq!(source.matches("stroke-dasharray").count(), 3);
        assert_eq!(source.matches("stroke-linecap=\"butt\"").count(), 3);
        assert_eq!(source.matches("<linearGradient id=").count(), 3);
        assert_eq!(source.matches("stroke=\"url(#storage-segment-").count(), 3);
        assert!(source.contains("shape-rendering=\"geometricPrecision\""));
        assert!(source.contains("r=\"67\""));
        assert!(source.contains("r=\"97\""));
        assert!(!source.contains("pathLength"));
        for category in [
            StorageCategory::Documents,
            StorageCategory::Videos,
            StorageCategory::Other,
        ] {
            let color = color_to_svg_hex(colors[category.index()]);
            assert!(source.contains(&format!("id=\"storage-segment-{}\"", category.index())));
            assert!(source.contains(&format!("stop-color=\"{color}\"")));
        }
        for bytes in [40_u64, 35, 25] {
            let segment_length =
                bytes as f64 * STORAGE_DONUT_CIRCUMFERENCE / summary.total_bytes as f64;
            let remainder = STORAGE_DONUT_CIRCUMFERENCE - segment_length;
            assert!(source.contains(&format!(
                "stroke-dasharray=\"{segment_length:.6} {remainder:.6}\""
            )));
            assert!(
                ((segment_length + remainder) - STORAGE_DONUT_CIRCUMFERENCE).abs() < f64::EPSILON
            );
        }
        assert!(!source.contains("NaN"));
        assert!(
            resvg::usvg::Tree::from_data(source.as_bytes(), &resvg::usvg::Options::default())
                .is_ok()
        );

        let hovered = storage_donut_svg(
            &summary,
            &colors,
            Palette::from_config(&AppConfig::default(), true),
            Some(StorageCategory::Videos),
        );
        assert!(!hovered.contains("stroke-width=\"33\""));
        assert_eq!(hovered.matches("stroke-width=\"30\"").count(), 4);
        assert_eq!(hovered.matches("stroke-opacity=\"0.88\"").count(), 2);
        assert!(
            resvg::usvg::Tree::from_data(hovered.as_bytes(), &resvg::usvg::Options::default())
                .is_ok()
        );
    }

    #[test]
    fn donut_hit_testing_returns_only_the_segment_under_the_ring() {
        let mut summary = StorageAnalysisSummary::default();
        summary.add_file_for_test(StorageCategory::Documents, 25);
        summary.add_file_for_test(StorageCategory::Videos, 25);
        summary.add_file_for_test(StorageCategory::Audio, 25);
        summary.add_file_for_test(StorageCategory::Other, 25);

        let point_in_segment = |clockwise_degrees: f32, rendered_size: f32| {
            let angle = clockwise_degrees.to_radians();
            let center = rendered_size / 2.0;
            let radius =
                rendered_size * STORAGE_DONUT_RADIUS as f32 / STORAGE_DONUT_VIEWBOX_SIZE as f32;
            Point::new(center + radius * angle.sin(), center - radius * angle.cos())
        };
        for rendered_size in [240.0, 420.0] {
            for (angle, expected) in [
                (45.0, StorageCategory::Documents),
                (135.0, StorageCategory::Videos),
                (225.0, StorageCategory::Audio),
                (315.0, StorageCategory::Other),
            ] {
                assert_eq!(
                    storage_donut_category_at_point(
                        &summary,
                        point_in_segment(angle, rendered_size),
                        rendered_size,
                    ),
                    Some(expected)
                );
            }
        }
        assert_eq!(
            storage_donut_category_at_point(&summary, Point::new(120.0, 120.0), 240.0),
            None
        );
        assert_eq!(
            storage_donut_category_at_point(&summary, Point::new(5.0, 5.0), 240.0),
            None
        );
        assert_eq!(
            storage_donut_category_at_point(
                &StorageAnalysisSummary::default(),
                Point::new(120.0, 38.0),
                240.0,
            ),
            None
        );
        assert_eq!(
            storage_donut_category_at_point(&summary, Point::new(f32::NAN, 120.0), 240.0,),
            None
        );
        assert_eq!(
            storage_donut_category_at_point(&summary, Point::new(120.0, 38.0), f32::INFINITY),
            None
        );
    }
}

use anyhow::Result;
use iced::widget::{
    button, column, container, horizontal_rule, pick_list, row, scrollable, text, text_editor,
    text_input, toggler, Column,
};
use iced::{color, Element, Length, Task, Theme};

use crate::registry::{self, Location, MenuEntry, Status};

// ── Messages ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    EntriesLoaded(Result<Vec<MenuEntry>, String>),
    Refresh,
    ToggleEntry(usize),
    ToggleResult {
        index: usize,
        result: Result<Status, String>,
    },
    FilterChanged(String),
    SearchChanged(String),
    DismissError,
    ToggleLogs,
    LogAction(text_editor::Action),
}

// ── State ───────────────────────────────────────────────────────────────────

struct App {
    entries: Vec<MenuEntry>,
    location_filter: Option<Location>,
    search_query: String,
    is_loading: bool,
    error: Option<String>,
    log_content: text_editor::Content,
    show_logs: bool,
}

impl App {
    fn push_log(&mut self, msg: &str) {
        use text_editor::Action;
        // Move cursor to end, insert newline if not empty, then insert msg
        self.log_content.perform(Action::Move(text_editor::Motion::DocumentEnd));
        let current = self.log_content.text();
        if !current.is_empty() && !current.ends_with('\n') {
            self.log_content.perform(Action::Edit(text_editor::Edit::Insert('\n')));
        }
        for ch in msg.chars() {
            self.log_content.perform(Action::Edit(text_editor::Edit::Insert(ch)));
        }
    }
}

const FILTER_OPTIONS: &[&str] = &["All", "Files", "Folders", "Background"];

fn parse_filter(s: &str) -> Option<Location> {
    match s {
        "Files" => Some(Location::Files),
        "Folders" => Some(Location::Folders),
        "Background" => Some(Location::Background),
        _ => None,
    }
}

fn filter_label(filter: &Option<Location>) -> &'static str {
    match filter {
        None => "All",
        Some(Location::Files) => "Files",
        Some(Location::Folders) => "Folders",
        Some(Location::Background) => "Background",
    }
}

// ── App implementation ──────────────────────────────────────────────────────

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            entries: Vec::new(),
            location_filter: None,
            search_query: String::new(),
            is_loading: true,
            error: None,
            log_content: text_editor::Content::with_text("Loading entries..."),
            show_logs: false,
        };
        (app, load_entries(None))
    }

    fn title(&self) -> String {
        "ctxmenu — Context Menu Manager".to_string()
    }

    fn theme(&self) -> Theme {
        Theme::Light
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::EntriesLoaded(result) => {
                self.is_loading = false;
                match result {
                    Ok(entries) => {
                        self.push_log(&format!("Loaded {} entries", entries.len()));
                        self.entries = entries;
                    }
                    Err(e) => {
                        self.push_log(&format!("ERROR loading: {e}"));
                        self.error = Some(e);
                    }
                }
                Task::none()
            }

            Message::Refresh => {
                self.is_loading = true;
                self.error = None;
                load_entries(self.location_filter.clone())
            }

            Message::ToggleEntry(index) => {
                let Some(entry) = self.entries.get(index) else {
                    return Task::none();
                };
                let name = entry.name.clone();
                let path = entry.registry_path.clone();
                self.push_log(&format!("Toggling '{name}' at {path}"));
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || registry::toggle_entry(&path))
                            .await
                            .unwrap()
                    },
                    move |result| Message::ToggleResult {
                        index,
                        result: result.map_err(|e| e.to_string()),
                    },
                )
            }

            Message::ToggleResult { index, result } => {
                match result {
                    Ok(new_status) => {
                        if let Some(entry) = self.entries.get_mut(index) {
                            let name = entry.name.clone();
                            entry.status = new_status.clone();
                            self.push_log(&format!("'{name}' -> {new_status}"));
                        }
                    }
                    Err(e) => {
                        self.push_log(&format!("ERROR toggle: {e}"));
                        self.error = Some(e);
                    }
                }
                Task::none()
            }

            Message::FilterChanged(selected) => {
                self.location_filter = parse_filter(&selected);
                self.is_loading = true;
                load_entries(self.location_filter.clone())
            }

            Message::SearchChanged(query) => {
                self.search_query = query;
                Task::none()
            }

            Message::DismissError => {
                self.error = None;
                Task::none()
            }

            Message::ToggleLogs => {
                self.show_logs = !self.show_logs;
                Task::none()
            }

            Message::LogAction(action) => {
                // Allow cursor movement and selection, block edits
                if !action.is_edit() {
                    self.log_content.perform(action);
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut content = column![
            self.view_toolbar(),
            horizontal_rule(1),
            self.view_body(),
            horizontal_rule(1),
            self.view_status_bar(),
        ]
        .spacing(0);

        if self.show_logs {
            content = content
                .push(horizontal_rule(1))
                .push(self.view_logs());
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_toolbar(&self) -> Element<'_, Message> {
        let filter = pick_list(
            FILTER_OPTIONS.to_vec(),
            Some(filter_label(&self.location_filter)),
            |s| Message::FilterChanged(s.to_string()),
        )
        .width(120);

        let search = text_input("Search entries...", &self.search_query)
            .on_input(Message::SearchChanged)
            .width(Length::Fill);

        let refresh = button("Refresh").on_press(Message::Refresh);

        container(row![filter, search, refresh].spacing(8))
            .padding(10)
            .into()
    }

    fn view_body(&self) -> Element<'_, Message> {
        if self.is_loading {
            return container(text("Loading..."))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        }

        let filtered: Vec<(usize, &MenuEntry)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let search_match = self.search_query.is_empty()
                    || e.name
                        .to_lowercase()
                        .contains(&self.search_query.to_lowercase())
                    || e.command
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&self.search_query.to_lowercase());
                search_match
            })
            .collect();

        if filtered.is_empty() {
            return container(text("No entries found."))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        }

        // Header
        let header = container(
            row![
                text("Name").width(Length::FillPortion(3)),
                text("Type").width(Length::FillPortion(1)),
                text("Location").width(Length::FillPortion(1)),
                text("Command / CLSID").width(Length::FillPortion(4)),
                text("Enabled").width(Length::FillPortion(1)),
            ]
            .spacing(8),
        )
        .padding([6, 12])
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(color!(0xE8E8E8))),
            ..Default::default()
        });

        // Rows
        let rows: Vec<Element<Message>> = filtered
            .iter()
            .map(|(index, entry)| {
                let idx = *index;
                let toggle = toggler(entry.status == Status::Enabled)
                    .on_toggle(move |_| Message::ToggleEntry(idx))
                    .size(18);

                let cmd_text = truncate(entry.command.as_deref().unwrap_or(""), 60);

                container(
                    row![
                        text(&entry.name).width(Length::FillPortion(3)),
                        text(entry.entry_type.to_string()).width(Length::FillPortion(1)),
                        text(entry.location.to_string()).width(Length::FillPortion(1)),
                        text(cmd_text).width(Length::FillPortion(4)).size(12),
                        container(toggle)
                            .width(Length::FillPortion(1))
                            .center_x(Length::Fill),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding([4, 12])
                .into()
            })
            .collect();

        let list = Column::with_children(rows).spacing(0);

        scrollable(column![header, list])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_status_bar(&self) -> Element<'_, Message> {
        let count = text(format!("{} entries", self.entries.len())).size(13);

        let logs_btn = button(text(if self.show_logs { "Hide Logs" } else { "Logs" }).size(12))
            .on_press(Message::ToggleLogs)
            .style(button::secondary);

        let mut status_row = row![count, iced::widget::horizontal_space(), logs_btn]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        if let Some(err) = &self.error {
            let err_row = row![
                text(err).size(13).color(color!(0xC62828)),
                button(text("x").size(12))
                    .on_press(Message::DismissError)
                    .style(button::text),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);

            status_row = status_row.push(err_row);
        }

        container(status_row).padding([6, 10]).into()
    }
    fn view_logs(&self) -> Element<'_, Message> {
        text_editor(&self.log_content)
            .on_action(Message::LogAction)
            .height(150)
            .size(12)
            .into()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn load_entries(filter: Option<Location>) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || registry::scan_entries(filter.as_ref()))
                .await
                .unwrap()
        },
        |result| Message::EntriesLoaded(result.map_err(|e| e.to_string())),
    )
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    iced::application(App::title, App::update, App::view)
        .theme(App::theme)
        .window_size(iced::Size::new(900.0, 600.0))
        .run_with(App::new)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

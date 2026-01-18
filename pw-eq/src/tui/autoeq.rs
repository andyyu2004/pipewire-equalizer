use crate::filter::Filter;
use crate::tui::action::Action;
use crate::tui::theme::Theme;
use crate::tui::Rotation;
use anyhow;
use pw_util::module::FilterType;
use zi_input::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Padding, Paragraph, Row, Table},
};
use std::io;
use std::ops::ControlFlow;
use std::path::PathBuf;
use tokio::sync::mpsc;

use super::Notif;

pub struct AutoEqBrowser {
    pub filter_query: String,
    pub filter_cursor_pos: usize,
    pub filtering: bool,
    pub entries: Option<autoeq_api::Entries>,
    pub targets: Option<Vec<autoeq_api::Target>>,
    pub filtered_results: Vec<(String, autoeq_api::Entry)>,
    pub selected_index: usize,
    pub selected_target_index: usize,
    pub loading: bool,
}

impl Default for AutoEqBrowser {
    fn default() -> Self {
        Self {
            filter_query: String::new(),
            filter_cursor_pos: 0,
            filtering: false,
            entries: None,
            targets: None,
            filtered_results: Vec::new(),
            selected_index: 0,
            selected_target_index: 0,
            loading: false,
        }
    }
}

impl AutoEqBrowser {
    pub fn update_filtered_results(&mut self) {
        let Some(entries) = &self.entries else {
            self.filtered_results.clear();
            return;
        };

        let query = self.filter_query.to_lowercase();
        self.filtered_results = entries
            .iter()
            .flat_map(|(name, entries)| {
                entries
                    .iter()
                    .map(move |entry| (name.clone(), entry.clone()))
            })
            .filter(|(name, _)| {
                if query.is_empty() {
                    true
                } else {
                    name.to_lowercase().contains(&query)
                }
            })
            .collect();

        // Reset selection if out of bounds
        if self.selected_index >= self.filtered_results.len() {
            self.selected_index = 0;
        }
    }

    pub fn selected_entry(&self) -> Option<&(String, autoeq_api::Entry)> {
        self.filtered_results.get(self.selected_index)
    }

    pub fn selected_target(&self) -> Option<&autoeq_api::Target> {
        self.targets.as_ref()?.get(self.selected_target_index)
    }

    pub fn load_data(&mut self, http_client: reqwest::Client, notifs_tx: mpsc::Sender<Notif>) {
        if self.entries.is_some() && self.targets.is_some() {
            // Already loaded
            return;
        }

        self.loading = true;

        tokio::spawn(async move {
            // Try to load from cache first
            let (entries, targets) = match AutoEqCache::load().await {
                Ok(Some(cache)) => {
                    tracing::info!("Loaded AutoEQ data from cache");
                    (cache.entries, cache.targets)
                }
                Ok(None) => {
                    tracing::info!("Cache miss or expired, fetching from API");
                    // Fetch from API
                    match tokio::try_join!(
                        autoeq_api::entries(&http_client),
                        autoeq_api::targets(&http_client)
                    ) {
                        Ok((entries, targets)) => {
                            // Save to cache
                            if let Err(err) =
                                AutoEqCache::save(entries.clone(), targets.clone()).await
                            {
                                tracing::warn!(error = &*err, "Failed to save cache");
                            }
                            (entries, targets)
                        }
                        Err(err) => {
                            let _ = notifs_tx.send(Notif::Error(err.into())).await;
                            return;
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(error = &*err, "Failed to load cache");
                    // Try fetching from API if cache load fails
                    match tokio::try_join!(
                        autoeq_api::entries(&http_client),
                        autoeq_api::targets(&http_client)
                    ) {
                        Ok((entries, targets)) => {
                            if let Err(err) =
                                AutoEqCache::save(entries.clone(), targets.clone()).await
                            {
                                tracing::warn!(error = &*err, "Failed to save cache");
                            }
                            (entries, targets)
                        }
                        Err(err) => {
                            let _ = notifs_tx.send(Notif::Error(err.into())).await;
                            return;
                        }
                    }
                }
            };

            let _ = notifs_tx
                .send(Notif::AutoEqDbLoaded { entries, targets })
                .await;
        });
    }

    pub fn apply_selected(
        &self,
        http_client: reqwest::Client,
        notifs_tx: mpsc::Sender<Notif>,
    ) -> Option<Result<String, String>> {
        let (name, entry) = self.selected_entry()?.clone();
        let target = self.selected_target()?;

        let target_label = target.label.clone();
        let source = entry.source.clone();
        let rig = entry.rig.clone();

        let display_name = name.clone();
        let display_source = entry.source.clone();

        tokio::spawn(async move {
            let request = autoeq_api::EqualizeRequest {
                target: target_label.clone(),
                name: name.clone(),
                source,
                rig,
                sample_rate: 48000,
            };

            match autoeq_api::equalize(&http_client, &request).await {
                Ok(response) => {
                    let _ = notifs_tx
                        .send(Notif::AutoEqLoaded { name, response })
                        .await;
                }
                Err(err) => {
                    let _ = notifs_tx.send(Notif::Error(err.into())).await;
                }
            }
        });

        Some(Ok(format!("Fetching EQ for {} from {}...", display_name, display_source)))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<Option<Action>>> {
        if self.filtering {
            // Filter input mode
            match key.code() {
                KeyCode::Char(c)
                    if !key.modifiers().contains(KeyModifiers::CONTROL)
                        && !key.modifiers().contains(KeyModifiers::ALT) =>
                {
                    self.filter_query.insert(self.filter_cursor_pos, c);
                    self.filter_cursor_pos += 1;
                    self.update_filtered_results();
                    return Ok(ControlFlow::Continue(()));
                }
                KeyCode::Backspace => {
                    if self.filter_cursor_pos > 0 {
                        self.filter_query.remove(self.filter_cursor_pos - 1);
                        self.filter_cursor_pos -= 1;
                        self.update_filtered_results();
                    }
                    return Ok(ControlFlow::Continue(()));
                }
                KeyCode::Left => {
                    self.filter_cursor_pos = self.filter_cursor_pos.saturating_sub(1);
                    return Ok(ControlFlow::Continue(()));
                }
                KeyCode::Right => {
                    if self.filter_cursor_pos < self.filter_query.len() {
                        self.filter_cursor_pos += 1;
                    }
                    return Ok(ControlFlow::Continue(()));
                }
                KeyCode::Esc | KeyCode::Enter => {
                    self.filtering = false;
                    return Ok(ControlFlow::Continue(()));
                }
                _ => return Ok(ControlFlow::Continue(())),
            }
        } else {
            // Normal AutoEQ navigation
            match key.code() {
                KeyCode::Char('/') => {
                    return Ok(ControlFlow::Break(Some(Action::EnterAutoEqFilter)));
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.selected_index + 1 < self.filtered_results.len() {
                        self.selected_index += 1;
                    }
                    return Ok(ControlFlow::Continue(()));
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.selected_index = self.selected_index.saturating_sub(1);
                    return Ok(ControlFlow::Continue(()));
                }
                KeyCode::Char('t') => {
                    return Ok(ControlFlow::Break(Some(Action::CycleAutoEqTarget(
                        Rotation::Clockwise,
                    ))));
                }
                KeyCode::Char('T') => {
                    return Ok(ControlFlow::Break(Some(Action::CycleAutoEqTarget(
                        Rotation::CounterClockwise,
                    ))));
                }
                KeyCode::Enter => {
                    return Ok(ControlFlow::Break(Some(Action::ApplyAutoEq)));
                }
                KeyCode::Esc => {
                    return Ok(ControlFlow::Break(Some(Action::CloseAutoEq)));
                }
                _ => {}
            }
        }

        Ok(ControlFlow::Continue(()))
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::EnterAutoEqFilter => {
                self.filtering = true;
            }
            Action::CycleAutoEqTarget(rotation) => {
                if let Some(targets) = &self.targets {
                    let len = targets.len();
                    match rotation {
                        Rotation::Clockwise => {
                            self.selected_target_index = (self.selected_target_index + 1) % len;
                        }
                        Rotation::CounterClockwise => {
                            self.selected_target_index = self
                                .selected_target_index
                                .checked_sub(1)
                                .unwrap_or(len - 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn on_data_loaded(&mut self, entries: autoeq_api::Entries, targets: Vec<autoeq_api::Target>) {
        self.entries = Some(entries);
        self.targets = Some(targets);
        self.loading = false;
        self.update_filtered_results();

        // Select default target (Harman over-ear 2018 if available)
        if let Some(targets) = &self.targets {
            if let Some(idx) = targets
                .iter()
                .position(|t| t.label.contains("Harman") && t.label.contains("over-ear"))
            {
                self.selected_target_index = idx;
            }
        }
    }

    pub fn draw(
        &self,
        f: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        theme: &Theme,
    ) {
        use ratatui::layout::{Constraint, Direction, Layout};

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header with target
                Constraint::Min(10),   // Results table
                Constraint::Length(1), // Footer with filter/help
            ])
            .split(area);

        // Header showing current target
        let target_text = if let Some(targets) = &self.targets {
            if let Some(target) = targets.get(self.selected_target_index) {
                format!("AutoEQ Browser - Target: {}", target.label)
            } else {
                "AutoEQ Browser - Target: (none)".to_string()
            }
        } else {
            "AutoEQ Browser - Loading...".to_string()
        };

        let header = Paragraph::new(Line::from(vec![Span::styled(
            target_text,
            Style::default()
                .fg(theme.header)
                .add_modifier(Modifier::BOLD),
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .padding(Padding::horizontal(1)),
        );
        f.render_widget(header, chunks[0]);

        // Results table
        if self.loading {
            let loading = Paragraph::new("Loading headphone database...").block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .padding(Padding::horizontal(1)),
            );
            f.render_widget(loading, chunks[1]);
        } else if self.filtered_results.is_empty() {
            let empty = Paragraph::new("No results found. Press / to filter.").block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .padding(Padding::horizontal(1)),
            );
            f.render_widget(empty, chunks[1]);
        } else {
            let rows: Vec<Row> = self
                .filtered_results
                .iter()
                .enumerate()
                .map(|(idx, (name, entry))| {
                    let is_selected = idx == self.selected_index;
                    let style = if is_selected {
                        Style::default()
                            .bg(theme.selected_row)
                            .fg(theme.background)
                    } else {
                        Style::default()
                    };

                    Row::new(vec![
                        Cell::from(name.as_str()),
                        Cell::from(entry.source.as_str()),
                        Cell::from(entry.rig.as_deref().unwrap_or("-")),
                    ])
                    .style(style)
                })
                .collect();

            let results_table = Table::new(
                rows,
                [
                    Constraint::Percentage(50),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ],
            )
            .header(
                Row::new(vec!["Headphone", "Source", "Rig"])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(format!(" {} results ", self.filtered_results.len()))
                    .padding(Padding::horizontal(1)),
            );
            f.render_widget(results_table, chunks[1]);
        }

        // Footer with filter query or help
        let footer_text = if self.filtering {
            format!("/{}", self.filter_query)
        } else if self.filter_query.is_empty() {
            "/: filter | t/T: cycle target | Enter: apply | Esc: close | j/k: navigate"
                .to_string()
        } else {
            format!(
                "Filtered by: {} | /: change filter | t/T: target | Enter: apply | Esc: close",
                self.filter_query
            )
        };

        let footer = Paragraph::new(Line::from(vec![Span::styled(
            footer_text,
            Style::default().fg(theme.footer),
        )]));
        f.render_widget(footer, chunks[2]);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AutoEqCache {
    entries: autoeq_api::Entries,
    targets: Vec<autoeq_api::Target>,
    timestamp: u64,
}

impl AutoEqCache {
    const CACHE_DURATION_SECS: u64 = 24 * 60 * 60; // 24 hours

    fn cache_path() -> anyhow::Result<PathBuf> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find cache directory"))?
            .join("pw-eq");
        std::fs::create_dir_all(&cache_dir)?;
        Ok(cache_dir.join("autoeq-cache.json"))
    }

    async fn load() -> anyhow::Result<Option<Self>> {
        let path = Self::cache_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let data = tokio::fs::read_to_string(&path).await?;
        let cache: Self = serde_json::from_str(&data)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        if now - cache.timestamp > Self::CACHE_DURATION_SECS {
            return Ok(None);
        }

        Ok(Some(cache))
    }

    async fn save(
        entries: autoeq_api::Entries,
        targets: Vec<autoeq_api::Target>,
    ) -> anyhow::Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let cache = Self {
            entries,
            targets,
            timestamp,
        };

        let path = Self::cache_path()?;
        let data = serde_json::to_string_pretty(&cache)?;
        tokio::fs::write(&path, data).await?;

        Ok(())
    }
}

pub fn convert_response_to_filters(response: autoeq_api::ParametricEq) -> Vec<Filter> {
    let num_filters = response.filters.len();
    response
        .filters
        .into_iter()
        .enumerate()
        .map(|(idx, f)| {
            // First filter might be low shelf, last might be high shelf
            // Everything else is peaking
            let filter_type = if idx == 0 && f.fc < 100.0 {
                FilterType::LowShelf
            } else if idx == num_filters - 1 && f.fc > 8000.0 {
                FilterType::HighShelf
            } else {
                FilterType::Peaking
            };

            Filter {
                frequency: f.fc as f64,
                gain: f.gain as f64,
                q: f.q as f64,
                filter_type,
                muted: false,
            }
        })
        .collect()
}

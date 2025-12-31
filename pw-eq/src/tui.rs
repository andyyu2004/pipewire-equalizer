use crate::{FilterId, UpdateFilter, filter::Filter, update_filters, use_eq};
use std::{
    backtrace::Backtrace, error::Error, io, mem, num::NonZero, ops::ControlFlow, path::PathBuf,
    pin::pin, sync::mpsc::Receiver,
};

use crossterm::{
    event::{DisableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen},
};
use futures_util::{Stream, StreamExt as _};
use pw_util::{
    apo,
    module::{
        self, Control, FilterType, Module, ModuleArgs, NodeKind, ParamEqConfig, ParamEqFilter,
        RateAndBiquadCoefficients, RawNodeConfig,
    },
    pipewire,
};
use ratatui::{
    Terminal,
    layout::Direction,
    prelude::{Backend, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table},
};
use tokio::sync::mpsc;

use crate::pw::{self, pw_thread};

pub enum Format {
    PwParamEq,
    Apo,
}

#[derive(Clone, Copy)]
enum Rotation {
    Clockwise,
    CounterClockwise,
}

#[derive(Clone, Copy)]
enum ViewMode {
    Normal,
    Expert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Command,
}

// EQ state
#[derive(Clone)]
struct EqState {
    name: String,
    filters: Vec<Filter>,
    selected_band: usize,
    max_bands: usize,
    view_mode: ViewMode,
    preamp: f64, // dB
    bypassed: bool,
}

impl EqState {
    fn with_filters(name: String, filters: impl IntoIterator<Item = Filter>) -> Self {
        let filters = filters.into_iter().collect::<Vec<_>>();
        Self {
            name,
            // Set initial preamp to max gain among bands to avoid clipping
            preamp: -filters
                .iter()
                .fold(0.0f64, |acc, band| acc.max(band.gain))
                .max(0.0),
            filters,
            selected_band: 0,
            max_bands: 31,
            view_mode: ViewMode::Normal,
            bypassed: false,
        }
    }

    fn new(name: String) -> Self {
        Self::with_filters(
            name,
            [
                Filter {
                    frequency: 50.0,
                    filter_type: FilterType::LowShelf,
                    ..Default::default()
                },
                Filter {
                    frequency: 100.0,
                    ..Default::default()
                },
                Filter {
                    frequency: 200.0,
                    ..Default::default()
                },
                Filter {
                    frequency: 500.0,
                    ..Default::default()
                },
                Filter {
                    frequency: 2000.0,
                    ..Default::default()
                },
                Filter {
                    frequency: 5000.0,
                    ..Default::default()
                },
                Filter {
                    frequency: 10000.0,
                    filter_type: FilterType::HighShelf,
                    ..Default::default()
                },
            ],
        )
    }

    fn add_band(&mut self) {
        if self.filters.len() >= self.max_bands {
            return;
        }

        let current_band = &self.filters[self.selected_band];

        // Calculate new frequency between current and next band
        let new_freq = if self.selected_band + 1 < self.filters.len() {
            let next_band = &self.filters[self.selected_band + 1];
            // Geometric mean (better for logarithmic frequency scale)
            (current_band.frequency * next_band.frequency).sqrt()
        } else {
            // If at the end, go halfway to 20kHz in log space
            (current_band.frequency * 20000.0).sqrt().min(20000.0)
        };

        let new_filter = Filter {
            frequency: new_freq,
            gain: 0.0,
            q: 1.0,
            filter_type: FilterType::Peaking,
            muted: false,
        };

        self.filters.insert(self.selected_band + 1, new_filter);
        self.selected_band += 1;
    }

    fn delete_selected_band(&mut self) {
        if self.filters.len() > 1 {
            self.filters.remove(self.selected_band);
            if self.selected_band >= self.filters.len() {
                self.selected_band = self.filters.len().saturating_sub(1);
            }
        }
    }

    fn select_next_band(&mut self) {
        if self.selected_band < self.filters.len().saturating_sub(1) {
            self.selected_band += 1;
        }
    }

    fn select_prev_band(&mut self) {
        self.selected_band = self.selected_band.saturating_sub(1);
    }

    fn adjust_freq(&mut self, f: impl FnOnce(f64) -> f64) {
        if let Some(band) = self.filters.get_mut(self.selected_band) {
            band.frequency = f(band.frequency).clamp(20.0, 20000.0);
        }
    }

    fn adjust_gain(&mut self, f: impl FnOnce(f64) -> f64) {
        if let Some(band) = self.filters.get_mut(self.selected_band) {
            band.gain = f(band.gain).clamp(-12.0, 12.0);
        }
    }

    fn adjust_q(&mut self, f: impl FnOnce(f64) -> f64) {
        if let Some(band) = self.filters.get_mut(self.selected_band) {
            band.q = f(band.q).clamp(0.001, 10.0);
        }
    }

    fn cycle_filter_type(&mut self, rotation: Rotation) {
        if let Some(band) = self.filters.get_mut(self.selected_band) {
            band.filter_type = match rotation {
                Rotation::Clockwise => match band.filter_type {
                    FilterType::Peaking => FilterType::HighShelf,
                    FilterType::LowShelf => FilterType::Peaking,
                    FilterType::HighShelf => FilterType::LowShelf,
                },
                Rotation::CounterClockwise => match band.filter_type {
                    FilterType::Peaking => FilterType::LowShelf,
                    FilterType::LowShelf => FilterType::HighShelf,
                    FilterType::HighShelf => FilterType::Peaking,
                },
            };
        }
    }

    fn toggle_mute(&mut self) {
        if let Some(band) = self.filters.get_mut(self.selected_band) {
            band.muted = !band.muted;
        }
    }

    fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Normal => ViewMode::Expert,
            ViewMode::Expert => ViewMode::Normal,
        };
    }

    fn adjust_preamp(&mut self, f: impl FnOnce(f64) -> f64) {
        self.preamp = f(self.preamp).clamp(-12.0, 12.0);
    }

    fn toggle_bypass(&mut self) {
        self.bypassed = !self.bypassed;
    }

    fn to_module_args(&self, rate: u32) -> ModuleArgs {
        Module::from_kinds(
            &format!("{}-{}", self.name, self.filters.len()),
            self.preamp,
            self.filters.iter().map(|band| NodeKind::Raw {
                config: RawNodeConfig {
                    coefficients: vec![RateAndBiquadCoefficients {
                        rate,
                        coefficients: band.biquad_coeffs(rate as f64),
                    }],
                },
            }),
        )
        .args
    }

    /// Save current EQ configuration to a PipeWire filter-chain config file using param_eq
    async fn save_config(
        &self,
        path: impl AsRef<std::path::Path>,
        format: Format,
    ) -> anyhow::Result<()> {
        let data = match format {
            Format::PwParamEq => {
                let config = module::Config::from_kinds(
                    &self.name,
                    self.preamp,
                    [NodeKind::ParamEq {
                        config: ParamEqConfig {
                            filters: self
                                .filters
                                .iter()
                                .map(|band| ParamEqFilter {
                                    ty: band.filter_type,
                                    control: Control {
                                        freq: band.frequency,
                                        q: band.q,
                                        gain: band.gain,
                                    },
                                })
                                .collect(),
                        },
                    }],
                );

                pw_util::to_spa_json(&config)
            }
            Format::Apo => apo::Config {
                preamp: self.preamp,
                filters: self
                    .filters
                    .iter()
                    .enumerate()
                    .map(|(i, filter)| apo::Filter {
                        number: (i + 1) as u32,
                        enabled: !filter.muted,
                        filter_type: filter.filter_type,
                        frequency: filter.frequency,
                        gain: filter.gain,
                        q: filter.q,
                    })
                    .collect(),
            }
            .to_string(),
        };

        tokio::fs::write(path, data).await?;

        Ok(())
    }

    /// Build update for preamp
    fn build_preamp_update(&self) -> UpdateFilter {
        UpdateFilter {
            frequency: None,
            gain: Some(self.preamp),
            q: None,
            coeffs: None,
        }
    }

    fn build_filter_update(&self, filter_idx: usize, sample_rate: u32) -> UpdateFilter {
        // Locally copy the band to modify muted state based on bypass
        // This is necessary to get the correct biquad coefficients
        let mut band = self.filters[filter_idx];
        band.muted |= self.bypassed;
        let gain = if band.muted { 0.0 } else { band.gain };

        UpdateFilter {
            frequency: Some(band.frequency),
            gain: Some(gain),
            q: Some(band.q),
            coeffs: Some(band.biquad_coeffs(sample_rate as f64)),
        }
    }

    fn apply_updates(
        &self,
        node_id: u32,
        updates: impl IntoIterator<Item = (FilterId, UpdateFilter), IntoIter: Send> + Send + 'static,
    ) {
        tokio::spawn(async move {
            if let Err(err) = update_filters(node_id, updates).await {
                tracing::error!(error = %err, "failed to apply filter updates");
            }
        });
    }

    /// Sync preamp gain to PipeWire
    fn sync_preamp(&self, node_id: u32) {
        let update = self.build_preamp_update();
        self.apply_updates(node_id, [(FilterId::Preamp, update)]);
    }

    /// Sync a specific filter band to PipeWire
    fn sync_filter(&self, node_id: u32, band_idx: usize, sample_rate: u32) {
        let band_id = FilterId::Index(NonZero::new(band_idx + 1).unwrap());
        let update = self.build_filter_update(band_idx, sample_rate);
        self.apply_updates(node_id, [(band_id, update)]);
    }

    fn sync(&self, node_id: u32, sample_rate: u32) {
        let mut updates = Vec::with_capacity(self.filters.len() + 1);

        updates.push((FilterId::Preamp, self.build_preamp_update()));

        for idx in 0..self.filters.len() {
            let id = FilterId::Index(NonZero::new(idx + 1).unwrap());
            updates.push((id, self.build_filter_update(idx, sample_rate)));
        }

        self.apply_updates(node_id, updates);
    }

    /// Generate frequency response curve data for visualization
    /// Returns Vec of (frequency, magnitude_db) pairs
    fn frequency_response_curve(&self, num_points: usize, sample_rate: f64) -> Vec<(f64, f64)> {
        // Generate logarithmically spaced frequency points from 20 Hz to 20 kHz
        let log_min = 20_f64.log10();
        let log_max = 20000_f64.log10();

        (0..num_points)
            .map(|i| {
                let t = i as f64 / (num_points - 1) as f64;
                let log_freq = log_min + t * (log_max - log_min);
                let freq = 10_f64.powf(log_freq);

                // Sum magnitude response from all bands
                let total_db: f64 = self
                    .filters
                    .iter()
                    .map(|band| band.magnitude_db_at(freq, sample_rate))
                    .sum();

                (freq, total_db)
            })
            .collect()
    }
}

pub enum Notif {
    ModuleLoaded {
        id: u32,
        name: String,
        media_name: String,
        reused: bool,
    },
    Error(anyhow::Error),
}

pub struct App<B: Backend + io::Write> {
    term: Terminal<B>,
    notifs: mpsc::Receiver<Notif>,
    pw_tx: pipewire::channel::Sender<pw::Message>,
    panic_rx: Receiver<(String, Backtrace)>,
    eq_state: EqState,
    active_node_id: Option<u32>,
    original_default_sink: Option<u32>,
    pw_handle: Option<std::thread::JoinHandle<io::Result<()>>>,
    sample_rate: u32,
    input_mode: InputMode,
    command_buffer: String,
    show_help: bool,
    status_error: Option<String>,
}

impl<B> App<B>
where
    B: Backend + io::Write,
    B::Error: Send + Sync + 'static,
{
    pub fn new(
        term: Terminal<B>,
        filters: impl IntoIterator<Item = Filter>,
        panic_rx: Receiver<(String, Backtrace)>,
    ) -> io::Result<Self> {
        let (pw_tx, rx) = pipewire::channel::channel();
        let (notifs_tx, notifs) = mpsc::channel(100);
        let pw_handle = std::thread::spawn(|| pw_thread(notifs_tx, rx));

        let filters = filters.into_iter().collect::<Vec<_>>();
        let eq_state = if !filters.is_empty() {
            EqState::with_filters("pweq".to_string(), filters)
        } else {
            EqState::new("pweq".to_string())
        };

        Ok(Self {
            term,
            panic_rx,
            pw_tx,
            notifs,
            eq_state,
            active_node_id: None,
            original_default_sink: None,
            pw_handle: Some(pw_handle),
            // TODO query
            sample_rate: 48000,
            input_mode: InputMode::Normal,
            command_buffer: String::new(),
            show_help: false,
            status_error: None,
        })
    }

    pub fn enter(&mut self) -> io::Result<()> {
        execute!(
            self.term.backend_mut(),
            EnterAlternateScreen,
            DisableMouseCapture
        )?;
        terminal::enable_raw_mode()?;
        Ok(())
    }

    pub async fn run(
        mut self,
        events: impl Stream<Item = io::Result<Event>>,
    ) -> anyhow::Result<()> {
        // Save the current default sink so we can restore it on exit
        self.original_default_sink = pw_util::get_default_audio_sink()
            .await
            .inspect_err(|err| {
                tracing::warn!(error = %err, "Failed to get default audio sink");
            })
            .ok();

        let mut events = pin!(events.fuse());

        loop {
            self.draw()?;

            tokio::select! {
                Ok(event) = events.select_next_some() => {
                    if let ControlFlow::Break(()) = self.handle_event(event)? {
                        break;
                    }
                }
                Some(notif) = self.notifs.recv() => self.on_notif(notif).await,
            }
        }

        let _ = self.pw_tx.send(pw::Message::Terminate);

        // Restore the original default sink before exiting
        if let Some(sink_id) = self.original_default_sink {
            tracing::info!(sink_id, "Restoring original default sink");
            pw_util::set_default(sink_id).await.inspect_err(|err| {
                tracing::error!(error = %err, "Failed to restore original default sink");
            })?;
        }

        if let Some(handle) = self.pw_handle.take() {
            match handle.join() {
                Ok(Ok(())) => tracing::info!("PipeWire thread exited cleanly"),
                Ok(Err(err)) => tracing::error!(
                    error = &err as &dyn Error,
                    "PipeWire thread exited with error"
                ),
                Err(err) => tracing::error!(error = ?err, "PipeWire thread panicked"),
            }
        }

        Ok(())
    }

    async fn on_notif(&mut self, notif: Notif) {
        match notif {
            Notif::ModuleLoaded {
                id,
                name,
                media_name,
                reused,
            } => {
                tracing::info!(id, name, media_name, "module loaded");

                let Ok(node_id) = use_eq(&media_name).await.inspect_err(|err| {
                    tracing::error!(error = %err, "failed to use EQ");
                }) else {
                    return;
                };

                if reused {
                    // If the module was reused, it may have stale filter settings
                    self.eq_state.sync(node_id, self.sample_rate);
                }

                self.active_node_id = Some(node_id);
            }
            Notif::Error(err) => {
                tracing::error!(error = &*err, "PipeWire error");
            }
        }
    }

    fn handle_event(&mut self, event: Event) -> io::Result<ControlFlow<()>> {
        match event {
            Event::Key(key) => self.handle_key(key),
            _ => Ok(ControlFlow::Continue(())),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        tracing::trace!(?key, mode = ?self.input_mode, "key event");

        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Command => self.handle_command_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        assert!(self.input_mode == InputMode::Normal);
        let before_idx = self.eq_state.selected_band;
        let before_band = self.eq_state.filters[self.eq_state.selected_band];
        let before_preamp = self.eq_state.preamp;
        let before_bypass = self.eq_state.bypassed;
        let before_filter_count = self.eq_state.filters.len();

        match key.code {
            KeyCode::Esc => return Ok(ControlFlow::Break(())),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(ControlFlow::Break(()));
            }

            // Enter command mode
            KeyCode::Char(':') => {
                self.input_mode = InputMode::Command;
                self.command_buffer.clear();
                self.status_error = None;
            }
            // Toggle help
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Char('w') => {
                self.input_mode = InputMode::Command;
                self.command_buffer = format!(
                    "write $HOME/.config/pipewire/pipewire.conf.d/{}.conf",
                    self.eq_state.name
                );
            }

            // Navigation
            KeyCode::Tab | KeyCode::Char('j') => self.eq_state.select_next_band(),
            KeyCode::BackTab | KeyCode::Char('k') => self.eq_state.select_prev_band(),
            KeyCode::Char(c @ '1'..='9') => {
                let idx = c.to_digit(10).unwrap() as usize - 1;
                if idx < self.eq_state.filters.len() {
                    self.eq_state.selected_band = idx;
                }
            }

            KeyCode::Char('f') => self.eq_state.adjust_freq(|f| f * 1.025),
            KeyCode::Char('F') => self.eq_state.adjust_freq(|f| f / 1.025),

            KeyCode::Char('g') => self.eq_state.adjust_gain(|g| g + 0.1),
            KeyCode::Char('G') => self.eq_state.adjust_gain(|g| g - 0.1),

            KeyCode::Char('q') => self.eq_state.adjust_q(|q| q + 0.01),
            KeyCode::Char('Q') => self.eq_state.adjust_q(|q| q - 0.01),

            KeyCode::Char('p') => self.eq_state.adjust_preamp(|p| p + 0.1),
            KeyCode::Char('P') => self.eq_state.adjust_preamp(|p| p - 0.1),

            KeyCode::Char('t') => self.eq_state.cycle_filter_type(Rotation::Clockwise),
            KeyCode::Char('T') => self.eq_state.cycle_filter_type(Rotation::CounterClockwise),

            KeyCode::Char('m') => self.eq_state.toggle_mute(),

            KeyCode::Char('e') => self.eq_state.toggle_view_mode(),

            KeyCode::Char('b') => self.eq_state.toggle_bypass(),

            // Band management
            KeyCode::Char('a') => self.eq_state.add_band(),
            KeyCode::Char('d') => self.eq_state.delete_selected_band(),
            KeyCode::Char('0') => {
                // Zero the gain on current band
                if let Some(band) = self.eq_state.filters.get_mut(self.eq_state.selected_band) {
                    band.gain = 0.0;
                }
            }

            _ => {}
        }

        if let Some(node_id) = self.active_node_id
            && before_preamp != self.eq_state.preamp
        {
            self.eq_state.sync_preamp(node_id);
        }

        if let Some(node_id) = self.active_node_id
            && self.eq_state.selected_band == before_idx
            && self.eq_state.filters[self.eq_state.selected_band] != before_band
        {
            self.eq_state
                .sync_filter(node_id, self.eq_state.selected_band, self.sample_rate);
        }

        if let Some(node_id) = self.active_node_id
            && before_bypass != self.eq_state.bypassed
        {
            // If bypass state changed, sync all bands
            self.eq_state.sync(node_id, self.sample_rate);
        }

        // Reload module if filter count changed (add/delete band), or if nothing is loaded yet
        if before_filter_count != self.eq_state.filters.len()
            || (self.active_node_id.is_none()
                && self.eq_state.filters.iter().any(|f| f.gain != 0.0))
        {
            tracing::debug!(
                old_filter_count = before_filter_count,
                new_filter_count = self.eq_state.filters.len(),
                "Loading module"
            );
            let _ = self.pw_tx.send(pw::Message::LoadModule {
                name: "libpipewire-module-filter-chain".into(),
                args: Box::new(self.eq_state.to_module_args(self.sample_rate)),
            });
        }

        Ok(ControlFlow::Continue(()))
    }

    fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.command_buffer.clear();
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        assert!(self.input_mode == InputMode::Command);

        match key.code {
            KeyCode::Esc => self.enter_normal_mode(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.enter_normal_mode()
            }
            KeyCode::Enter => {
                let command = mem::take(&mut self.command_buffer);
                self.enter_normal_mode();
                return self.execute_command(&command);
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
            }
            KeyCode::Char(c) => self.command_buffer.push(c),
            _ => {}
        }

        Ok(ControlFlow::Continue(()))
    }

    fn execute_command(&mut self, cmd: &str) -> io::Result<ControlFlow<()>> {
        let cmd = shellexpand::full(cmd).map_err(io::Error::other)?;
        let words = match shellish_parse::parse(&cmd, true) {
            Ok(words) => words,
            Err(err) => {
                self.status_error = Some(format!("command parse error: {err}"));
                return Ok(ControlFlow::Continue(()));
            }
        };

        let words = words.iter().map(|s| s.as_str()).collect::<Vec<_>>();

        match &words[..] {
            ["q" | "quit"] => return Ok(ControlFlow::Break(())),
            [cmd @ ("w" | "write" | "w!" | "write!"), args @ ..] => {
                let force = cmd.ends_with('!');
                let path = match args {
                    [path] => PathBuf::from(path),
                    _ => {
                        self.status_error = Some("usage: write <path>".to_string());
                        return Ok(ControlFlow::Continue(()));
                    }
                };

                let format = match path.extension() {
                    Some(ext) if ext == "apo" => Format::Apo,
                    _ => Format::PwParamEq,
                };

                if path.exists() && !force {
                    self.status_error = Some(format!(
                        "file {} already exists (use ! to overwrite)",
                        path.display()
                    ));
                    return Ok(ControlFlow::Continue(()));
                }

                tokio::spawn({
                    let eq_state = self.eq_state.clone();
                    async move {
                        match eq_state.save_config(&path, format).await {
                            Ok(()) => {
                                tracing::info!(path = %path.display(), "EQ configuration saved")
                            }
                            Err(err) => {
                                tracing::error!(error = %err, "failed to save EQ configuration");
                            }
                        }
                    }
                });
            }
            _ => {
                self.status_error = Some(format!("unknown command: {cmd}"));
            }
        }

        Ok(ControlFlow::Continue(()))
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let eq_state = &self.eq_state;
        let sample_rate = self.sample_rate;
        self.term.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),      // Header
                    Constraint::Min(10),        // Band table
                    Constraint::Percentage(40), // Frequency response chart
                    Constraint::Length(1),      // Footer
                ])
                .split(f.area());

            // Header
            let preamp_color = if eq_state.preamp > 0.05 {
                Color::Green
            } else if eq_state.preamp < -0.05 {
                Color::Red
            } else {
                Color::Gray
            };

            let mut header_spans = vec![
                Span::raw(format!(
                    "PipeWire EQ: {} | Bands: {}/{} | Sample Rate: {:.0} Hz | Preamp: ",
                    eq_state.name,
                    eq_state.filters.len(),
                    eq_state.max_bands,
                    sample_rate
                )),
                Span::styled(
                    format!("{:+.1} dB", eq_state.preamp),
                    Style::default().fg(preamp_color),
                ),
            ];

            if eq_state.bypassed {
                header_spans.push(Span::styled(
                    " | BYPASSED",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            }

            let header = Paragraph::new(Line::from(header_spans))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Band table
            Self::draw_band_table(f, chunks[1], eq_state, sample_rate);

            // Frequency response chart
            Self::draw_frequency_response(f, chunks[2], eq_state, sample_rate);

            // Footer: Status message, Command line, or Help
            let footer = match self.input_mode {
                InputMode::Command => {
                    Paragraph::new(format!(":{}", self.command_buffer))
                }
                InputMode::Normal if self.status_error.is_some() => {
                    Paragraph::new(self.status_error.as_ref().unwrap().as_str())
                        .style(Style::default().fg(Color::Red))
                }
                InputMode::Normal if self.show_help => {
                    Paragraph::new(
                        "Tab/j/k: select | t: type | m: mute | b: bypass | e: expert | f/F: freq | g/G: gain | q/Q: Q | p/P: preamp | a: add | d: delete | 0: zero | :: command | ?: hide help"
                    )
                    .style(Style::default().fg(Color::DarkGray))
                }
                InputMode::Normal => {
                    Paragraph::new("Press ? for help")
                        .style(Style::default().fg(Color::DarkGray))
                }
            };
            f.render_widget(footer, chunks[3]);
        })?;
        Ok(())
    }

    fn draw_band_table(f: &mut ratatui::Frame, area: Rect, eq_state: &EqState, sample_rate: u32) {
        let rows: Vec<Row> = eq_state
            .filters
            .iter()
            .enumerate()
            .map(|(idx, band)| {
                // Format frequency with better precision
                let freq_str = if band.frequency >= 10000.0 {
                    format!("{:.1}k", band.frequency / 1000.0)
                } else if band.frequency >= 1000.0 {
                    format!("{:.2}k", band.frequency / 1000.0)
                } else {
                    format!("{:.0}", band.frequency)
                };

                // Format filter type
                let type_str = match band.filter_type {
                    FilterType::Peaking => "PK",
                    FilterType::LowShelf => "LS",
                    FilterType::HighShelf => "HS",
                };

                // Color-code the gain value
                let gain_color = if band.gain > 0.05 {
                    Color::Green
                } else if band.gain < -0.05 {
                    Color::Red
                } else {
                    Color::Gray
                };

                let is_selected = idx == eq_state.selected_band;
                let is_dimmed = band.muted || eq_state.bypassed;

                // Dim muted or bypassed filters
                let (num_color, type_color, freq_color, q_color) = if is_dimmed {
                    (
                        Color::DarkGray,
                        Color::DarkGray,
                        Color::DarkGray,
                        Color::DarkGray,
                    )
                } else if is_selected {
                    (Color::Yellow, Color::Blue, Color::Cyan, Color::Magenta)
                } else {
                    (Color::DarkGray, Color::Gray, Color::White, Color::White)
                };

                let final_gain_color = if is_dimmed {
                    Color::DarkGray
                } else {
                    gain_color
                };

                let coeff_color = if is_dimmed {
                    Color::DarkGray
                } else if is_selected {
                    Color::Green
                } else {
                    Color::Gray
                };

                // Create base cells
                let mut cells = vec![
                    Cell::from(format!("{}", idx + 1)).style(
                        Style::default()
                            .fg(num_color)
                            .add_modifier(if is_selected && !is_dimmed {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Cell::from(type_str).style(Style::default().fg(type_color).add_modifier(
                        if is_selected && !is_dimmed {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        },
                    )),
                    Cell::from(freq_str).style(Style::default().fg(freq_color).add_modifier(
                        if is_selected && !is_dimmed {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        },
                    )),
                    Cell::from(format!("{:+.1}", band.gain)).style(
                        Style::default().fg(final_gain_color).add_modifier(
                            if is_selected && !is_dimmed {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            },
                        ),
                    ),
                    Cell::from(format!("{:.2}", band.q)).style(
                        Style::default()
                            .fg(q_color)
                            .add_modifier(if is_selected && !is_dimmed {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ];

                // Add biquad coefficients in expert mode
                if matches!(eq_state.view_mode, ViewMode::Expert) {
                    let coeffs = band.biquad_coeffs(sample_rate as f64);
                    cells.extend([
                        Cell::from(format!("{:.6}", coeffs.b0))
                            .style(Style::default().fg(coeff_color)),
                        Cell::from(format!("{:.6}", coeffs.b1))
                            .style(Style::default().fg(coeff_color)),
                        Cell::from(format!("{:.6}", coeffs.b2))
                            .style(Style::default().fg(coeff_color)),
                        Cell::from(format!("{:.6}", coeffs.a1))
                            .style(Style::default().fg(coeff_color)),
                        Cell::from(format!("{:.6}", coeffs.a2))
                            .style(Style::default().fg(coeff_color)),
                    ]);
                }

                Row::new(cells)
            })
            .collect();

        let (constraints, header_cells, title) = match eq_state.view_mode {
            ViewMode::Normal => (
                vec![
                    Constraint::Length(3), // #
                    Constraint::Length(4), // Type
                    Constraint::Length(8), // Freq
                    Constraint::Length(9), // Gain (dB)
                    Constraint::Length(6), // Q
                ],
                vec!["#", "Type", "Freq", "Gain", "Q"],
                "EQ Bands",
            ),
            ViewMode::Expert => (
                vec![
                    Constraint::Length(3),  // #
                    Constraint::Length(4),  // Type
                    Constraint::Length(8),  // Freq
                    Constraint::Length(9),  // Gain (dB)
                    Constraint::Length(6),  // Q
                    Constraint::Length(11), // b0
                    Constraint::Length(11), // b1
                    Constraint::Length(11), // b2
                    Constraint::Length(11), // a1
                    Constraint::Length(11), // a2
                ],
                vec![
                    "#", "Type", "Freq", "Gain", "Q", "b0", "b1", "b2", "a1", "a2",
                ],
                "EQ Bands (Expert Mode)",
            ),
        };

        let table = Table::new(rows, constraints)
            .header(
                Row::new(header_cells)
                    .style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    )
                    .bottom_margin(1),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
            );

        f.render_widget(table, area);
    }

    fn draw_frequency_response(
        f: &mut ratatui::Frame,
        area: Rect,
        eq_state: &EqState,
        sample_rate: u32,
    ) {
        const NUM_POINTS: usize = 200;

        // Generate frequency response curve data
        let curve_data = eq_state.frequency_response_curve(NUM_POINTS, sample_rate as f64);

        // Convert to chart data format (log x-axis manually handled via data)
        let data: Vec<(f64, f64)> = curve_data
            .iter()
            .map(|(freq, db)| (freq.log10(), *db))
            .collect();

        // Find min/max for y-axis bounds
        let max_db = curve_data
            .iter()
            .map(|(_, db)| db)
            .fold(f64::NEG_INFINITY, |a, &b| a.max(b))
            .max(1.0);

        let min_db = curve_data
            .iter()
            .map(|(_, db)| db)
            .fold(f64::INFINITY, |a, &b| a.min(b))
            .min(-1.0);

        let dataset = Dataset::default()
            .name("EQ Response")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&data);

        // X-axis: log scale from 20 Hz to 20 kHz
        let log_min = 20_f64.log10();
        let log_max = 20000_f64.log10();

        let x_axis = Axis::default()
            .title("Frequency")
            .style(Style::default().fg(Color::Gray))
            .bounds([log_min, log_max])
            .labels(vec!["20Hz".to_string(), "20kHz".to_string()]);

        // Y-axis: dB scale
        let y_axis = Axis::default()
            .title("Gain (dB)")
            .style(Style::default().fg(Color::Gray))
            .bounds([min_db - 1.0, max_db + 1.0])
            .labels(vec![
                format!("{:.1}", min_db),
                "0".into(),
                format!("{:.1}", max_db),
            ]);

        let chart = Chart::new(vec![dataset])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Frequency Response"),
            )
            .x_axis(x_axis)
            .y_axis(y_axis);

        f.render_widget(chart, area);
    }
}

impl<W: Backend + io::Write> Drop for App<W> {
    fn drop(&mut self) {
        let _ = ratatui::try_restore();

        if let Ok((panic, backtrace)) = self.panic_rx.try_recv() {
            use std::io::Write as _;
            let mut stderr = io::stderr().lock();
            let _ = writeln!(stderr, "{panic}");
            let _ = writeln!(stderr, "{backtrace}");
        }
    }
}

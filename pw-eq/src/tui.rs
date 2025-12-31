use crate::{FilterId, UpdateFilter, filter::Filter, update_filters, use_eq};
use std::{
    backtrace::Backtrace,
    error::Error,
    io, mem,
    num::NonZero,
    ops::ControlFlow,
    path::PathBuf,
    pin::{Pin, pin},
    sync::mpsc::Receiver,
};

use crossterm::{
    cursor,
    event::{DisableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen},
};
use futures_util::{Stream, StreamExt as _, future::BoxFuture, stream::FusedStream};
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
use strum::IntoEnumIterator;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    Command { buffer: String, cursor_pos: usize },
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
        let types = FilterType::iter().collect::<Vec<_>>();
        if let Some(band) = self.filters.get_mut(self.selected_band) {
            let idx = types
                .iter()
                .position(|&t| t == band.filter_type)
                .expect("filter type must exist in enum");

            band.filter_type = match rotation {
                Rotation::Clockwise => types[(idx + 1) % types.len()],
                Rotation::CounterClockwise => types[(idx + types.len() - 1) % types.len()],
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

pub type TaskResult = Result<Option<String>, String>;
pub type Task = BoxFuture<'static, TaskResult>;

pub struct App<B: Backend + io::Write> {
    term: Terminal<B>,
    notifs: mpsc::Receiver<Notif>,
    tasks: Pin<Box<dyn FusedStream<Item = TaskResult> + Send>>,
    task_tx: mpsc::Sender<Task>,
    pw_tx: pipewire::channel::Sender<pw::Message>,
    panic_rx: Receiver<(String, Backtrace)>,
    eq: EqState,
    active_node_id: Option<u32>,
    original_default_sink: Option<u32>,
    pw_handle: Option<std::thread::JoinHandle<io::Result<()>>>,
    sample_rate: u32,
    input_mode: InputMode,
    command_history: Vec<String>,
    command_history_index: Option<usize>,
    command_history_scratch: String,
    show_help: bool,
    status: Option<Result<String, String>>,
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

        let (task_tx, task_rx) = mpsc::channel::<BoxFuture<'static, TaskResult>>(100);
        let tasks = Box::pin(ReceiverStream::new(task_rx).buffered(8));

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
            tasks,
            task_tx,
            eq: eq_state,
            active_node_id: None,
            original_default_sink: None,
            pw_handle: Some(pw_handle),
            // TODO query
            sample_rate: 48000,
            input_mode: InputMode::Normal,
            command_history: Vec::new(),
            command_history_index: None,
            command_history_scratch: String::new(),
            show_help: false,
            status: None,
        })
    }

    fn schedule(&self, fut: impl std::future::Future<Output = TaskResult> + Send + 'static) {
        match self.task_tx.try_send(Box::pin(fut)) {
            Ok(()) => {}
            Err(err) => {
                tracing::error!(error = %err, "failed to schedule task");
            }
        }
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
        execute!(
            self.term.backend_mut(),
            cursor::Show,
            cursor::SetCursorStyle::SteadyBar,
        )?;

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
                result = self.tasks.select_next_some() => match result {
                    Ok(Some(status)) => self.status = Some(Ok(status)),
                    Ok(None) => {}
                    Err(err) => self.status = Some(Err(err)),
                }
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
                    self.sync(node_id, self.sample_rate);
                }

                self.active_node_id = Some(node_id);
            }
            Notif::Error(err) => {
                tracing::error!(error = &*err, "PipeWire error");
            }
        }
    }

    fn apply_updates(
        &self,
        node_id: u32,
        updates: impl IntoIterator<Item = (FilterId, UpdateFilter), IntoIter: Send> + Send + 'static,
    ) {
        self.schedule(async move {
            match update_filters(node_id, updates).await {
                Ok(()) => Ok(None),
                Err(err) => Err(err.to_string()),
            }
        });
    }

    /// Sync preamp gain to PipeWire
    fn sync_preamp(&self, node_id: u32) {
        let update = self.eq.build_preamp_update();
        self.apply_updates(node_id, [(FilterId::Preamp, update)]);
    }

    /// Sync a specific filter band to PipeWire
    fn sync_filter(&self, node_id: u32, band_idx: usize, sample_rate: u32) {
        let band_id = FilterId::Index(NonZero::new(band_idx + 1).unwrap());
        let update = self.eq.build_filter_update(band_idx, sample_rate);
        self.apply_updates(node_id, [(band_id, update)]);
    }

    fn sync(&self, node_id: u32, sample_rate: u32) {
        let mut updates = Vec::with_capacity(self.eq.filters.len() + 1);

        updates.push((FilterId::Preamp, self.eq.build_preamp_update()));

        for idx in 0..self.eq.filters.len() {
            let id = FilterId::Index(NonZero::new(idx + 1).unwrap());
            updates.push((id, self.eq.build_filter_update(idx, sample_rate)));
        }

        self.apply_updates(node_id, updates);
    }

    fn handle_event(&mut self, event: Event) -> io::Result<ControlFlow<()>> {
        match event {
            Event::Key(key) => self.handle_key(key),
            _ => Ok(ControlFlow::Continue(())),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        tracing::trace!(?key, mode = ?self.input_mode, "key event");

        match &self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Command { .. } => self.handle_command_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        assert!(matches!(self.input_mode, InputMode::Normal));
        let before_idx = self.eq.selected_band;
        let before_band = self.eq.filters[self.eq.selected_band];
        let before_preamp = self.eq.preamp;
        let before_bypass = self.eq.bypassed;
        let before_filter_count = self.eq.filters.len();

        match key.code {
            KeyCode::Esc => self.status = None,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(ControlFlow::Break(()));
            }

            KeyCode::Char(':') => self.enter_command_mode(),
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Char('w') => {
                let buffer = format!(
                    "write $HOME/.config/pipewire/pipewire.conf.d/{}.conf",
                    self.eq.name
                );
                let cursor_pos = buffer.len();
                self.input_mode = InputMode::Command { buffer, cursor_pos };
            }

            KeyCode::Char('j') => self.eq.select_next_band(),
            KeyCode::Char('k') => self.eq.select_prev_band(),
            KeyCode::Char(c @ '1'..='9') => {
                let idx = c.to_digit(10).unwrap() as usize - 1;
                if idx < self.eq.filters.len() {
                    self.eq.selected_band = idx;
                }
            }

            KeyCode::Char('f') => self.eq.adjust_freq(|f| f * 1.025),
            KeyCode::Char('F') => self.eq.adjust_freq(|f| f / 1.025),

            KeyCode::Char('g') => self.eq.adjust_gain(|g| g + 0.1),
            KeyCode::Char('G') => self.eq.adjust_gain(|g| g - 0.1),

            KeyCode::Char('q') => self.eq.adjust_q(|q| q + 0.01),
            KeyCode::Char('Q') => self.eq.adjust_q(|q| q - 0.01),

            KeyCode::Char('p' | '+') => self.eq.adjust_preamp(|p| p + 0.1),
            KeyCode::Char('P' | '-') => self.eq.adjust_preamp(|p| p - 0.1),

            KeyCode::Tab => self.eq.cycle_filter_type(Rotation::Clockwise),
            KeyCode::BackTab => self.eq.cycle_filter_type(Rotation::CounterClockwise),

            KeyCode::Char('m') => self.eq.toggle_mute(),

            KeyCode::Char('e') => self.eq.toggle_view_mode(),

            KeyCode::Char('b') => self.eq.toggle_bypass(),

            // Band management
            KeyCode::Char('a') => self.eq.add_band(),
            KeyCode::Char('d') => self.eq.delete_selected_band(),
            KeyCode::Char('0') => {
                // Zero the gain on current band
                if let Some(band) = self.eq.filters.get_mut(self.eq.selected_band) {
                    band.gain = 0.0;
                }
            }

            _ => {}
        }

        if let Some(node_id) = self.active_node_id
            && before_preamp != self.eq.preamp
        {
            self.sync_preamp(node_id);
        }

        if let Some(node_id) = self.active_node_id
            && self.eq.selected_band == before_idx
            && self.eq.filters[self.eq.selected_band] != before_band
        {
            self.sync_filter(node_id, self.eq.selected_band, self.sample_rate);
        }

        if let Some(node_id) = self.active_node_id
            && before_bypass != self.eq.bypassed
        {
            // If bypass state changed, sync all bands
            self.sync(node_id, self.sample_rate);
        }

        // Reload module if filter count changed (add/delete band), or if nothing is loaded yet
        // Attempting to avoid loading no-op EQ as long as possible
        if before_filter_count != self.eq.filters.len() || self.active_node_id.is_none() {
            tracing::debug!(
                old_filter_count = before_filter_count,
                new_filter_count = self.eq.filters.len(),
                "Loading module"
            );
            let _ = self.pw_tx.send(pw::Message::LoadModule {
                name: "libpipewire-module-filter-chain".into(),
                args: Box::new(self.eq.to_module_args(self.sample_rate)),
            });
        }

        Ok(ControlFlow::Continue(()))
    }

    fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    fn enter_command_mode(&mut self) {
        self.input_mode = InputMode::Command {
            buffer: String::new(),
            cursor_pos: 0,
        };
        self.command_history_index = None;
        self.command_history_scratch.clear();
        self.status = None;
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        let InputMode::Command { buffer, cursor_pos } = &mut self.input_mode else {
            panic!("handle_command_key called in non-command mode");
        };

        match key.code {
            KeyCode::Esc => self.enter_normal_mode(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.enter_normal_mode()
            }
            KeyCode::Enter => {
                let InputMode::Command { buffer, .. } =
                    mem::replace(&mut self.input_mode, InputMode::Normal)
                else {
                    unreachable!();
                };
                return self.execute_command(&buffer);
            }
            KeyCode::Up => {
                if self.command_history.is_empty() {
                    return Ok(ControlFlow::Continue(()));
                }

                match self.command_history_index {
                    None => {
                        // Save current buffer and start at the end of history
                        self.command_history_scratch = buffer.clone();
                        self.command_history_index = Some(self.command_history.len() - 1);
                        *buffer = self.command_history[self.command_history.len() - 1].clone();
                        *cursor_pos = buffer.len();
                    }
                    Some(idx) if idx > 0 => {
                        // Go back in history
                        self.command_history_index = Some(idx - 1);
                        *buffer = self.command_history[idx - 1].clone();
                        *cursor_pos = buffer.len();
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                if let Some(idx) = self.command_history_index {
                    if idx + 1 < self.command_history.len() {
                        // Go forward in history
                        self.command_history_index = Some(idx + 1);
                        *buffer = self.command_history[idx + 1].clone();
                        *cursor_pos = buffer.len();
                    } else {
                        // At the end, restore scratch
                        self.command_history_index = None;
                        *buffer = mem::take(&mut self.command_history_scratch);
                        *cursor_pos = buffer.len();
                    }
                }
            }
            KeyCode::Backspace => {
                if *cursor_pos > 0 {
                    buffer.remove(*cursor_pos - 1);
                    *cursor_pos -= 1;
                }
                self.command_history_index = None;
            }
            KeyCode::Delete => {
                if *cursor_pos < buffer.len() {
                    buffer.remove(*cursor_pos);
                }
                self.command_history_index = None;
            }
            KeyCode::Left => *cursor_pos = cursor_pos.saturating_sub(1),
            KeyCode::Right => *cursor_pos = (*cursor_pos + 1).min(buffer.len()),
            KeyCode::Home => *cursor_pos = 0,
            KeyCode::End => *cursor_pos = buffer.len(),
            KeyCode::Char(c) => {
                buffer.insert(*cursor_pos, c);
                *cursor_pos += 1;
                self.command_history_index = None;
            }
            _ => {}
        }

        Ok(ControlFlow::Continue(()))
    }

    fn execute_command(&mut self, cmd: &str) -> io::Result<ControlFlow<()>> {
        let mut cmd = cmd;

        // Special handling for '!!' to repeat last command with force
        let add_force = if cmd == "!!" {
            if let Some(last_cmd) = self.command_history.last() {
                cmd = last_cmd;
                true
            } else {
                self.status = Some(Err("no previous command".to_string()));
                return Ok(ControlFlow::Continue(()));
            }
        } else {
            // Add to history if non-empty and not a duplicate of the last command
            if !cmd.is_empty() && self.command_history.last().is_none_or(|last| last != cmd) {
                self.command_history.push(cmd.to_string());
            }

            false
        };

        let cmd = shellexpand::full(&cmd).map_err(io::Error::other)?;
        let mut words = match shellish_parse::parse(&cmd, true) {
            Ok(words) => words,
            Err(err) => {
                self.status = Some(Err(format!("command parse error: {err}")));
                return Ok(ControlFlow::Continue(()));
            }
        };

        // Append '!' to the first word
        if add_force && let Some(first) = words.get_mut(0) {
            first.push('!');
        }

        let words = words.iter().map(|s| s.as_str()).collect::<Vec<_>>();

        match &words[..] {
            ["q" | "quit"] => return Ok(ControlFlow::Break(())),
            [cmd @ ("w" | "write" | "w!" | "write!"), args @ ..] => {
                let force = cmd.ends_with('!');
                let path = match args {
                    [path] => PathBuf::from(path),
                    _ => {
                        self.status = Some(Err("usage: write <path>".to_string()));
                        return Ok(ControlFlow::Continue(()));
                    }
                };

                let format = match path.extension() {
                    Some(ext) if ext == "apo" => Format::Apo,
                    _ => Format::PwParamEq,
                };

                if path.exists() && !force {
                    self.status = Some(Err(format!(
                        "file {} already exists (use ! to overwrite)",
                        path.display()
                    )));
                    return Ok(ControlFlow::Continue(()));
                }

                self.schedule({
                    let eq_state = self.eq.clone();
                    let path = path.clone();
                    async move {
                        match eq_state.save_config(&path, format).await {
                            Ok(()) => Ok(Some(format!("Saved to {}", path.display()))),
                            Err(err) => Err(err.to_string()),
                        }
                    }
                });
            }
            _ => self.status = Some(Err(format!("unknown command: {cmd}"))),
        }

        Ok(ControlFlow::Continue(()))
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let eq_state = &self.eq;
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
                    format!("{} dB", Gain(eq_state.preamp)),
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
            let footer = match &self.input_mode {
                InputMode::Command { buffer, .. } => {
                    Paragraph::new(format!(":{}", buffer))
                }
                InputMode::Normal if self.status.is_some() => {
                    let (msg, color) = match self.status.as_ref().unwrap() {
                        Ok(msg) => (msg.as_str(), Color::White),
                        Err(msg) => (msg.as_str(), Color::Red),
                    };
                    Paragraph::new(msg).style(Style::default().fg(color))
                }
                InputMode::Normal if self.show_help => {
                    Paragraph::new(
                        "j/k: select | STab: type | m: mute | b: bypass | e: expert | f/F: freq | g/G: gain | q/Q: Q | +/-: preamp | a: add | d: delete | 0: zero | :: command | ?: hide help"
                    )
                    .style(Style::default().fg(Color::DarkGray))
                }
                InputMode::Normal => {
                    Paragraph::new("Press ? for help")
                        .style(Style::default().fg(Color::DarkGray))
                }
            };
            f.render_widget(footer, chunks[3]);

            if let InputMode::Command { cursor_pos, .. } = &self.input_mode {
                f.set_cursor_position((chunks[3].x + 1 + *cursor_pos as u16, chunks[3].y));
            }
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

                // Format filter type (following APO conventions)
                let type_str = match band.filter_type {
                    FilterType::LowShelf => "LSC",
                    FilterType::LowPass => "LPQ",
                    FilterType::Peaking => "PK",
                    FilterType::BandPass => "BP",
                    FilterType::Notch => "NO",
                    FilterType::HighPass => "HPQ",
                    FilterType::HighShelf => "HSC",
                };

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
                    Cell::from(format!("{}", Gain(band.gain))).style(
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

struct Gain(f64);

impl std::fmt::Display for Gain {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if self.0.abs() < 0.05 {
            write!(f, "0.0")
        } else {
            write!(f, "{:+.1}", self.0)
        }
    }
}

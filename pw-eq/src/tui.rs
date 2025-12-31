mod action;
mod eq;

use crate::{FilterId, UpdateFilter, filter::Filter, update_filters, use_eq};
use std::{
    error::Error,
    io, mem,
    num::NonZero,
    ops::ControlFlow,
    path::PathBuf,
    pin::{Pin, pin},
};
use zi_input::{Event, KeyCode, KeyEvent, KeyModifiers};

use crossterm::{
    cursor,
    event::DisableMouseCapture,
    execute,
    terminal::{self, EnterAlternateScreen},
};
use futures_util::{Stream, StreamExt as _, future::BoxFuture, stream::FusedStream};
use keymap::KeyMap;
use pw_util::{module::FilterType, pipewire};
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
use tokio_stream::wrappers::ReceiverStream;

use crate::pw::{self, pw_thread};

use self::{action::Action, eq::Eq};

pub enum Format {
    PwParamEq,
    Apo,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Rotation {
    Clockwise,
    CounterClockwise,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum ViewMode {
    #[default]
    Normal,
    Expert,
}

#[derive(
    Debug, Copy, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
enum InputMode {
    #[default]
    Normal,
    Command,
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
    eq: Eq,
    active_node_id: Option<u32>,
    original_default_sink: Option<u32>,
    pw_handle: Option<std::thread::JoinHandle<io::Result<()>>>,
    sample_rate: u32,
    input_mode: InputMode,
    command_history: Vec<String>,
    command_history_index: Option<usize>,
    command_history_scratch: String,
    command_buffer: String,
    command_cursor_pos: usize,
    show_help: bool,
    status: Option<Result<String, String>>,
    view_mode: ViewMode,
    config: Config,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    keymap: KeyMap<InputMode, zi_input::KeyEvent, Action>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keymap: serde_json::from_value(serde_json::json!({
                "normal": {
                    "<Esc>": { "enter-mode": { "mode": "normal" } },
                    ":": { "enter-mode": { "mode": "command" } },
                    "<C-c>": "quit",
                    "?": "toggle-help",
                    "j": "select-next",
                    "k": "select-previous",
                    "1": { "select-index": 0 },
                    "2": { "select-index": 1 },
                    "3": { "select-index": 2 },
                    "4": { "select-index": 3 },
                    "5": { "select-index": 4 },
                    "6": { "select-index": 5 },
                    "7": { "select-index": 6 },
                    "8": { "select-index": 7 },
                    "9": { "select-index": 8 },
                    "l": { "adjust-frequency": { "multiplier": 1.0125 } },
                    "<S-l>": { "adjust-frequency": { "multiplier": 1.1 } },
                    "h": { "adjust-frequency": { "multiplier": 0.9875 } },
                    "<S-h>": { "adjust-frequency": { "multiplier": 0.9 } },
                    "g": { "adjust-gain": { "delta": 0.1 } },
                    "<S-g>": { "adjust-gain": { "delta": -0.1 } },
                    "r": { "adjust-q": { "delta": 0.01 } },
                    "<S-R>": { "adjust-q": { "delta": 0.1 } },
                    "w": { "adjust-q": { "delta": -0.01 } },
                    "<S-W>": { "adjust-q": { "delta": -0.1 } },
                    "p": { "adjust-preamp": { "delta": 0.1 } },
                    "+": { "adjust-preamp": { "delta": 0.1 } },
                    "<S-P>": { "adjust-preamp": { "delta": -0.1 } },
                    "-": { "adjust-preamp": { "delta": -0.1 } },
                    "<Tab>": { "cycle-filter-type": { "rotation": "clockwise" } },
                    "<S-Tab>": { "cycle-filter-type": { "rotation": "counter-clockwise" } },
                    "m": "toggle-mute",
                    "b": "toggle-bypass",
                    "a": "add-filter",
                    "d": "remove-filter",
                    "0": { "adjust-gain": { "set": 0.0 } },
                    "x": { "cycle-view-mode": { "rotation": "clockwise" } },
                },
                "command": {}
            }))
            .unwrap(),
        }
    }
}

impl<B> App<B>
where
    B: Backend + io::Write,
    B::Error: Send + Sync + 'static,
{
    pub fn new(
        term: Terminal<B>,
        config: Config,
        filters: impl IntoIterator<Item = Filter>,
    ) -> io::Result<Self> {
        let (pw_tx, rx) = pipewire::channel::channel();
        let (notifs_tx, notifs) = mpsc::channel(100);
        let pw_handle = std::thread::spawn(|| pw_thread(notifs_tx, rx));

        let (task_tx, task_rx) = mpsc::channel::<BoxFuture<'static, TaskResult>>(100);
        let tasks = Box::pin(ReceiverStream::new(task_rx).buffered(8));

        let filters = filters.into_iter().collect::<Vec<_>>();
        let eq = if !filters.is_empty() {
            Eq::with_filters("pweq".to_string(), filters)
        } else {
            Eq::new("pweq".to_string())
        };

        Ok(Self {
            term,
            pw_tx,
            notifs,
            tasks,
            task_tx,
            eq,
            config,
            pw_handle: Some(pw_handle),
            // TODO query sample rate
            sample_rate: 48000,
            active_node_id: Default::default(),
            original_default_sink: Default::default(),
            input_mode: Default::default(),
            command_history: Default::default(),
            command_history_index: Default::default(),
            command_history_scratch: Default::default(),
            command_buffer: Default::default(),
            command_cursor_pos: Default::default(),
            show_help: Default::default(),
            view_mode: Default::default(),
            status: Default::default(),
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

    pub async fn run(mut self, events: impl Stream<Item = zi_input::Event>) -> anyhow::Result<()> {
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

        if self.eq.filters.iter().any(|band| {
            use FilterType as Ft;
            band.gain > 0.0
                || matches!(
                    band.filter_type,
                    Ft::BandPass | Ft::Notch | Ft::HighPass | Ft::LowPass
                )
        }) {
            // Load module if any band is not a no-op.
            // This is just a development convenience to avoid loading the module when starting
            // unnecessarily as audio pauses when unloading the module.
            self.load_module();
        }

        let mut events = pin!(events.fuse());

        loop {
            self.draw()?;

            tokio::select! {
                event = events.select_next_some() => {
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
            InputMode::Command => self.handle_command_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        assert!(matches!(self.input_mode, InputMode::Normal));
        let before_idx = self.eq.selected_idx;
        let before_filter = self.eq.filters[self.eq.selected_idx];
        let before_preamp = self.eq.preamp;
        let before_bypass = self.eq.bypassed;
        let before_filter_count = self.eq.filters.len();

        if let Some(action) = self.config.keymap.get(&self.input_mode, &key) {
            match self.perform(*action)? {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => return Ok(ControlFlow::Break(())),
            }
        }

        if let Some(node_id) = self.active_node_id {
            if before_preamp != self.eq.preamp {
                self.sync_preamp(node_id);
            }

            if self.eq.selected_idx == before_idx
                && self.eq.filters[self.eq.selected_idx] != before_filter
            {
                self.sync_filter(node_id, self.eq.selected_idx, self.sample_rate);
            }

            if before_bypass != self.eq.bypassed {
                // If bypass state changed, sync all bands
                self.sync(node_id, self.sample_rate);
            }
        }

        // Load new module if filter count changed (add/delete band) because we cannot dynamically
        // change the number of filters in the filter-chain module.
        // Or if nothing is currently loaded.
        if before_filter_count != self.eq.filters.len() || self.active_node_id.is_none() {
            tracing::debug!(
                old_filter_count = before_filter_count,
                new_filter_count = self.eq.filters.len(),
                "Reloading pipewire module"
            );
            self.load_module();
        }

        Ok(ControlFlow::Continue(()))
    }

    fn cycle_view_mode(&mut self, _rotation: Rotation) {
        self.view_mode = match self.view_mode {
            ViewMode::Normal => ViewMode::Expert,
            ViewMode::Expert => ViewMode::Normal,
        };
    }

    fn perform(&mut self, action: Action) -> io::Result<ControlFlow<()>> {
        let before_idx = self.eq.selected_idx;
        let before_filter = self.eq.filters[self.eq.selected_idx];
        let before_preamp = self.eq.preamp;
        let before_bypass = self.eq.bypassed;
        let before_filter_count = self.eq.filters.len();
        match action {
            Action::EnterMode { mode } => match mode {
                InputMode::Normal => self.enter_normal_mode(),
                InputMode::Command => self.enter_command_mode(),
            },
            Action::ClearStatus => self.status = None,
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::Quit => return Ok(ControlFlow::Break(())),
            Action::SelectNext => self.eq.select_next_filter(),
            Action::SelectPrevious => self.eq.select_prev_filter(),
            Action::AddFilter => self.eq.add_filter(),
            Action::RemoveFilter => self.eq.delete_selected_filter(),
            Action::SelectIndex(idx) => {
                if idx < self.eq.filters.len() {
                    self.eq.selected_idx = idx;
                }
            }
            Action::AdjustFrequency(adj) => self.eq.adjust_freq(|f| adj.apply(f)),
            Action::AdjustGain(adj) => self.eq.adjust_gain(|g| adj.apply(g)),
            Action::AdjustQ(adj) => self.eq.adjust_q(|q| adj.apply(q)),
            Action::AdjustPreamp(adj) => self.eq.adjust_preamp(|p| adj.apply(p)),
            Action::CycleFilterType { rotation } => self.eq.cycle_filter_type(rotation),
            Action::ToggleBypass => self.eq.toggle_bypass(),
            Action::ToggleMute => self.eq.toggle_mute(),
            Action::CycleViewMode { rotation } => self.cycle_view_mode(rotation),
        }

        if let Some(node_id) = self.active_node_id {
            if before_preamp != self.eq.preamp {
                self.sync_preamp(node_id);
            }

            if self.eq.selected_idx == before_idx
                && self.eq.filters[self.eq.selected_idx] != before_filter
            {
                self.sync_filter(node_id, self.eq.selected_idx, self.sample_rate);
            }

            if before_bypass != self.eq.bypassed {
                // If bypass state changed, sync all bands
                self.sync(node_id, self.sample_rate);
            }
        }

        // Load new module if filter count changed (add/delete band) because we cannot dynamically
        // change the number of filters in the filter-chain module.
        // Or if nothing is currently loaded.
        if before_filter_count != self.eq.filters.len() || self.active_node_id.is_none() {
            tracing::debug!(
                old_filter_count = before_filter_count,
                new_filter_count = self.eq.filters.len(),
                "Reloading pipewire module"
            );
            self.load_module();
        }

        Ok(ControlFlow::Continue(()))
    }

    fn load_module(&mut self) {
        let _ = self.pw_tx.send(pw::Message::LoadModule {
            name: "libpipewire-module-filter-chain".into(),
            args: Box::new(self.eq.to_module_args(self.sample_rate)),
        });
    }

    fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    fn enter_command_mode(&mut self) {
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        self.input_mode = InputMode::Command;
        self.command_history_index = None;
        self.command_history_scratch.clear();
        self.status = None;
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        let InputMode::Command = &mut self.input_mode else {
            panic!("handle_command_key called in non-command mode");
        };

        match key.code() {
            KeyCode::Esc => self.enter_normal_mode(),
            KeyCode::Char('c') if key.modifiers().contains(KeyModifiers::CONTROL) => {
                self.enter_normal_mode()
            }
            KeyCode::Enter => {
                let InputMode::Command = mem::replace(&mut self.input_mode, InputMode::Normal)
                else {
                    unreachable!();
                };
                let buffer = mem::take(&mut self.command_buffer);
                return self.execute_command(&buffer);
            }
            KeyCode::Up => {
                if self.command_history.is_empty() {
                    return Ok(ControlFlow::Continue(()));
                }

                match self.command_history_index {
                    None => {
                        // Save current buffer and start at the end of history
                        self.command_history_scratch = mem::take(&mut self.command_buffer);
                        self.command_history_index = Some(self.command_history.len() - 1);
                        self.command_buffer =
                            self.command_history[self.command_history.len() - 1].clone();
                        self.command_cursor_pos = self.command_buffer.len();
                    }
                    Some(idx) if idx > 0 => {
                        // Go back in history
                        self.command_history_index = Some(idx - 1);
                        self.command_buffer = self.command_history[idx - 1].clone();
                        self.command_cursor_pos = self.command_buffer.len();
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                if let Some(idx) = self.command_history_index {
                    if idx + 1 < self.command_history.len() {
                        // Go forward in history
                        self.command_history_index = Some(idx + 1);
                        self.command_buffer = self.command_history[idx + 1].clone();
                        self.command_cursor_pos = self.command_buffer.len();
                    } else {
                        // At the end, restore scratch
                        self.command_history_index = None;
                        self.command_buffer = mem::take(&mut self.command_history_scratch);
                        self.command_cursor_pos = self.command_buffer.len();
                    }
                }
            }
            KeyCode::Backspace => {
                if self.command_cursor_pos > 0 && !self.command_buffer.is_empty() {
                    self.command_buffer.remove(self.command_cursor_pos - 1);
                    self.command_cursor_pos -= 1;
                }
                self.command_history_index = None;
            }
            KeyCode::Delete => {
                if self.command_cursor_pos < self.command_buffer.len() {
                    self.command_buffer.remove(self.command_cursor_pos);
                }
                self.command_history_index = None;
            }
            KeyCode::Left => self.command_cursor_pos = self.command_cursor_pos.saturating_sub(1),
            KeyCode::Right => {
                if self.command_cursor_pos < self.command_buffer.len() {
                    self.command_cursor_pos += 1;
                }
            }
            KeyCode::Home => self.command_cursor_pos = 0,
            KeyCode::End => self.command_cursor_pos = self.command_buffer.len(),
            KeyCode::Char(c) => {
                self.command_buffer.insert(self.command_cursor_pos, c);
                self.command_cursor_pos += 1;
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
        let view_mode = self.view_mode;
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
                    eq_state.max_filters,
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
            Self::draw_idx_table(f, chunks[1], eq_state, view_mode, sample_rate);

            // Frequency response chart
            Self::draw_frequency_response(f, chunks[2], eq_state, sample_rate);

            // Footer: Status message, Command line, or Help
            let footer = match &self.input_mode {
                InputMode::Command  => {
                    Paragraph::new(format!(":{}", self.command_buffer))
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
                        "j/k: select | STab: type | m: mute | b: bypass | x: expert | f/F: freq | g/G: gain | q/Q: Q | +/-: preamp | a: add | d: delete | 0: zero | :: command | ?: hide help"
                    )
                    .style(Style::default().fg(Color::DarkGray))
                }
                InputMode::Normal => {
                    Paragraph::new("Press ? for help")
                        .style(Style::default().fg(Color::DarkGray))
                }
            };
            f.render_widget(footer, chunks[3]);

            if let InputMode::Command = &self.input_mode {
                f.set_cursor_position((chunks[3].x + 1 + self.command_cursor_pos as u16, chunks[3].y));
            }
        })?;
        Ok(())
    }

    fn draw_idx_table(
        f: &mut ratatui::Frame,
        area: Rect,
        eq_state: &Eq,
        view_mode: ViewMode,
        sample_rate: u32,
    ) {
        let rows: Vec<Row> = eq_state
            .filters
            .iter()
            .enumerate()
            .map(|(idx, band)| {
                let freq_str = format!("{:.0}", band.frequency);

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

                let is_selected = idx == eq_state.selected_idx;
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
                if matches!(view_mode, ViewMode::Expert) {
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

        let (constraints, header_cells, title) = match view_mode {
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
                            .add_modifier(Modifier::BOLD),
                    )
                    .bottom_margin(0),
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

    fn draw_frequency_response(f: &mut ratatui::Frame, area: Rect, eq: &Eq, sample_rate: u32) {
        const NUM_POINTS: usize = 200;

        // Generate frequency response curve data
        let curve_data = eq.frequency_response_curve(NUM_POINTS, sample_rate as f64);

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
            .block(Block::default().borders(Borders::ALL))
            .x_axis(x_axis)
            .y_axis(y_axis);

        f.render_widget(chart, area);
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

#[cfg(test)]
#[test]
fn test_default_config_parses() {
    let _config = Config::default();
}

mod action;
mod autoeq;
mod draw;
mod eq;
mod theme;

use crate::{FilterId, UpdateFilter, filter::Filter, update_filters};
use pw_util::module::{FilterType, TargetObject};
use std::collections::HashMap;
use std::thread;
use std::{
    collections::BTreeMap,
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
use pw_util::{NodeInfo, pipewire};
use ratatui::{Terminal, prelude::Backend};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::pw::{self, pw_thread};

use self::{eq::Eq, theme::Theme};

pub enum Format {
    PwParamEq,
    Apo,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Rotation {
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
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
enum InputMode {
    #[default]
    Eq,
    AutoEq,
    Command,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Tab {
    Eq,
    AutoEq,
}

pub enum Notif {
    PwModuleLoaded {
        id: u32,
        name: String,
        media_name: String,
        reused: bool,
    },
    AutoEqDbLoaded {
        entries: autoeq_api::Entries,
        targets: Vec<autoeq_api::Target>,
    },
    AutoEqLoaded {
        name: String,
        response: autoeq_api::ParametricEq,
    },
    Error(anyhow::Error),
}

pub type TaskResult = Result<Option<String>, String>;
pub type Task = BoxFuture<'static, TaskResult>;

pub struct App<B: Backend + io::Write> {
    term: Terminal<B>,
    notifs: mpsc::Receiver<Notif>,
    notifs_tx: mpsc::Sender<Notif>,
    tasks: Pin<Box<dyn FusedStream<Item = TaskResult> + Send>>,
    task_tx: mpsc::Sender<Task>,
    pw_tx: pipewire::channel::Sender<pw::Message>,
    eq: Eq,
    active_node_id: Option<u32>,
    original_default_sink: Option<NodeInfo>,
    pw_handle: Option<thread::JoinHandle<anyhow::Result<()>>>,
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
    tab: Tab,
    autoeq_browser: autoeq::AutoEqBrowser,
    http_client: reqwest::Client,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    keymap: KeyMaps,
    pub(super) theme: Theme,
}

#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct KeyMaps {
    normal: BTreeMap<zi_input::KeyEvent, action::NormalAction>,
    autoeq: BTreeMap<zi_input::KeyEvent, action::AutoEqAction>,
    command: BTreeMap<zi_input::KeyEvent, action::CommandAction>,
}

impl Config {
    /// Right-biased in-place merge of two configs
    pub fn merge(mut self, config: Config) -> Self {
        self.keymap.normal.extend(config.keymap.normal);
        self.keymap.autoeq.extend(config.keymap.autoeq);
        self.keymap.command.extend(config.keymap.command);

        // Written in this way to make sure we don't forget to merge new fields later
        Self {
            keymap: self.keymap,
            theme: config.theme,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keymap: serde_json::from_value(serde_json::json!({
                "normal": {
                    "<C-c>":     "quit",
                    "?":         "toggle-help",
                    "j":         "select-next",
                    "k":         "select-previous",
                    "m":         "toggle-mute",
                    "b":         "toggle-bypass",
                    "a":         "add-filter",
                    "x":         "remove-filter",
                    "<S-A>":     "open-auto-eq",
                    ":":         "enter-command-mode",
                    "1":       { "select-index": 0 },
                    "2":       { "select-index": 1 },
                    "3":       { "select-index": 2 },
                    "4":       { "select-index": 3 },
                    "5":       { "select-index": 4 },
                    "6":       { "select-index": 5 },
                    "7":       { "select-index": 6 },
                    "8":       { "select-index": 7 },
                    "9":       { "select-index": 8 },
                    "s":       { "adjust-frequency": { "multiplier": 0.9875 } },
                    "<S-s>":   { "adjust-frequency": { "multiplier": 0.9 } },
                    "f":       { "adjust-frequency": { "multiplier": 1.0125 } },
                    "<S-f>":   { "adjust-frequency": { "multiplier": 1.1 } },
                    "l":       { "adjust-frequency": { "multiplier": 1.0125 } },
                    "<S-l>":   { "adjust-frequency": { "multiplier": 1.1 } },
                    "h":       { "adjust-frequency": { "multiplier": 0.9875 } },
                    "<S-h>":   { "adjust-frequency": { "multiplier": 0.9 } },
                    "e":       { "adjust-gain": { "delta": 0.1 } },
                    "d":       { "adjust-gain": { "delta": -0.1 } },
                    "r":       { "adjust-q": { "delta": 0.01 } },
                    "<S-R>":   { "adjust-q": { "delta": 0.1 } },
                    "w":       { "adjust-q": { "delta": -0.01 } },
                    "<S-W>":   { "adjust-q": { "delta": -0.1 } },
                    "p":       { "adjust-preamp": { "delta": 0.1 } },
                    "+":       { "adjust-preamp": { "delta": 0.1 } },
                    "<S-P>":   { "adjust-preamp": { "delta": -0.1 } },
                    "-":       { "adjust-preamp": { "delta": -0.1 } },
                    "<Tab>":   { "cycle-filter-type": "clockwise" },
                    "<S-Tab>": { "cycle-filter-type": "counter-clockwise" },
                    "v":       { "cycle-view-mode": "clockwise" },
                    "0":       { "adjust-gain": { "set": 0.0 } },
                },
                "autoeq": {
                    "<C-c>":    "quit",
                    "?":        "toggle-help",
                    "j":        "select-next",
                    "k":        "select-previous",
                    "<Down>":   "select-next",
                    "<Up>":     "select-previous",
                    "<CR>":     "apply-auto-eq",
                    "t":        { "cycle-target": "clockwise" },
                    "<S-T>":    { "cycle-target": "counter-clockwise" },
                    "/":        "enter-filter-mode",
                    "<Esc>":    "close-auto-eq",
                    ":":        "enter-command-mode",
                },
                "command": {
                    "<Esc>":       "exit-command-mode",
                    "<C-c>":       "exit-command-mode",
                    "<CR>":        "execute-command",
                    "<Up>":        "command-history-previous",
                    "<Down>":      "command-history-next",
                    "<BS>":        "delete-char-backward",
                    "<Del>":       "delete-char-forward",
                    "<Left>":      "move-cursor-left",
                    "<Right>":     "move-cursor-right",
                    "<Home>":      "move-cursor-home",
                    "<End>":       "move-cursor-end",
                },
            }))
            .unwrap(),
            theme: Theme::default(),
        }
    }
}

impl<B> App<B>
where
    B: Backend + io::Write,
    B::Error: Send + Sync + 'static,
{
    pub async fn new(
        term: Terminal<B>,
        config: Config,
        preamp: f64,
        filters: impl IntoIterator<Item = Filter>,
    ) -> io::Result<Self> {
        let default_audio_sink = match pw_util::get_default_audio_sink().await {
            Ok(node) => {
                tracing::info!(?node, "detected default audio sink");
                Some(node)
            }
            Err(err) => {
                tracing::error!(error = &*err, "failed to get default audio sink");
                None
            }
        };

        let (pw_tx, rx) = pipewire::channel::channel();
        let (notifs_tx, notifs) = mpsc::channel(100);
        let pw_notifs_tx = notifs_tx.clone();
        let pw_handle = thread::spawn(|| pw_thread(pw_notifs_tx, rx, default_audio_sink));

        let (task_tx, task_rx) = mpsc::channel::<BoxFuture<'static, TaskResult>>(100);
        let tasks = Box::pin(ReceiverStream::new(task_rx).buffered(8));

        let filters = filters.into_iter().collect::<Vec<_>>();
        let name = "pw-eq";
        let mut eq = if !filters.is_empty() {
            Eq::new(name, filters)
        } else {
            Eq::new(
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
        };

        eq.adjust_preamp(|_p| preamp);

        Ok(Self {
            term,
            pw_tx,
            notifs,
            notifs_tx,
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
            tab: Tab::Eq,
            autoeq_browser: autoeq::AutoEqBrowser::default(),
            http_client: reqwest::Client::new(),
        })
    }

    fn schedule(&self, fut: impl std::future::Future<Output = TaskResult> + Send + 'static) {
        match self.task_tx.try_send(Box::pin(fut)) {
            Ok(()) => {}
            Err(err) => {
                tracing::error!(
                    error = &err as &dyn std::error::Error,
                    "failed to schedule task"
                );
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

        if !self.eq.is_noop() {
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

        if let Some(handle) = self.pw_handle.take() {
            match handle.join() {
                Ok(Ok(())) => tracing::info!("PipeWire thread exited cleanly"),
                Ok(Err(err)) => tracing::error!(error = &*err, "PipeWire thread exited with error"),
                Err(err) => tracing::error!(error = ?err, "PipeWire thread panicked"),
            }
        }

        Ok(())
    }

    async fn on_notif(&mut self, notif: Notif) {
        match notif {
            Notif::PwModuleLoaded {
                id,
                name,
                media_name,
                reused,
            } => {
                tracing::info!(id, name, media_name, "module loaded");

                // Find the filter's output node (capture side) by media.name
                let Ok(node) = crate::find_eq_node(&media_name).await.inspect_err(|err| {
                    tracing::error!(error = &**err, "failed to find EQ node");
                }) else {
                    return;
                };

                let node_id = node.id;

                if reused {
                    // If the module was reused, it may have stale filter settings
                    self.sync(node_id, self.sample_rate);
                }

                self.active_node_id = Some(node_id);
                if let Err(err) = self.pw_tx.send(pw::Message::SetActiveNode(NodeInfo {
                    node_id,
                    node_name: media_name,
                    object_serial: node
                        .info
                        .props
                        .get("object.serial")
                        .and_then(|v| v.as_i64())
                        .expect("object.serial missing or malformed"),
                })) {
                    tracing::error!(
                        error = ?err,
                        "failed to set active node"
                    );
                }
            }
            Notif::AutoEqDbLoaded { entries, targets } => {
                self.autoeq_browser.on_data_loaded(entries, targets);
                self.status = Some(Ok(format!(
                    "Loaded {} headphones",
                    self.autoeq_browser.filtered_results.len()
                )));
            }
            Notif::AutoEqLoaded { name, response } => {
                tracing::info!(
                    headphone = name,
                    num_filters = response.filters.len(),
                    "AutoEQ applied"
                );
                self.eq.preamp = response.preamp;
                self.eq.filters = autoeq::param_eq_to_filters(response);
                self.status = Some(Ok(format!("Applied EQ for {}", name)));
                self.enter_eq_mode();
            }
            Notif::Error(err) => {
                tracing::error!(error = &*err, "error from notification");
                self.status = Some(Err(err.to_string()));
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

        match self.input_mode {
            InputMode::Eq => {
                if let Some(action) = self.config.keymap.normal.get(&key) {
                    self.perform_normal_action(*action)
                } else {
                    Ok(ControlFlow::Continue(()))
                }
            }
            InputMode::AutoEq => {
                if let Some(action) = self.config.keymap.autoeq.get(&key) {
                    self.perform_autoeq_action(*action)
                } else {
                    Ok(ControlFlow::Continue(()))
                }
            }
            InputMode::Command => {
                if let Some(action) = self.config.keymap.command.get(&key) {
                    self.perform_command_action(*action)
                } else if let KeyCode::Char(c) = key.code()
                    && !key.modifiers().contains(KeyModifiers::CONTROL)
                    && !key.modifiers().contains(KeyModifiers::ALT)
                {
                    // Handle unmapped character input
                    self.command_buffer.insert(self.command_cursor_pos, c);
                    self.command_cursor_pos += 1;
                    self.command_history_index = None;

                    // Update filter in real-time if we're in filter mode
                    if self.command_buffer.starts_with('/') {
                        self.autoeq_browser.filter_query = self.command_buffer[1..].to_string();
                        self.autoeq_browser.update_filtered_results();
                    }

                    Ok(ControlFlow::Continue(()))
                } else {
                    Ok(ControlFlow::Continue(()))
                }
            }
        }
    }

    fn cycle_view_mode(&mut self, _rotation: Rotation) {
        self.view_mode = match self.view_mode {
            ViewMode::Normal => ViewMode::Expert,
            ViewMode::Expert => ViewMode::Normal,
        };
    }

    fn perform_normal_action(
        &mut self,
        action: action::NormalAction,
    ) -> io::Result<ControlFlow<()>> {
        use action::NormalAction;

        let before_idx = self.eq.selected_idx;
        let before_filter = self.eq.filters[self.eq.selected_idx];
        let before_preamp = self.eq.preamp;
        let before_bypass = self.eq.bypassed;
        let before_filter_count = self.eq.filters.len();

        match action {
            NormalAction::Quit => return Ok(ControlFlow::Break(())),
            NormalAction::ToggleHelp => self.show_help = !self.show_help,
            NormalAction::SelectNext => self.eq.select_next_filter(),
            NormalAction::SelectPrevious => self.eq.select_prev_filter(),
            NormalAction::AddFilter => self.eq.add_filter(),
            NormalAction::RemoveFilter => self.eq.delete_selected_filter(),
            NormalAction::ToggleBypass => self.eq.toggle_bypass(),
            NormalAction::ToggleMute => self.eq.toggle_mute(),
            NormalAction::SelectIndex(idx) => {
                if idx < self.eq.filters.len() {
                    self.eq.selected_idx = idx;
                }
            }
            NormalAction::AdjustFrequency(adj) => self.eq.adjust_freq(|f| adj.apply(f)),
            NormalAction::AdjustGain(adj) => self.eq.adjust_gain(|g| adj.apply(g)),
            NormalAction::AdjustQ(adj) => self.eq.adjust_q(|q| adj.apply(q)),
            NormalAction::AdjustPreamp(adj) => self.eq.adjust_preamp(|p| adj.apply(p)),
            NormalAction::CycleFilterType(rotation) => self.eq.cycle_filter_type(rotation),
            NormalAction::CycleViewMode(rotation) => self.cycle_view_mode(rotation),
            NormalAction::OpenAutoEq => self.open_autoeq(),
            NormalAction::EnterCommandMode => self.enter_command_mode(),
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
                self.sync(node_id, self.sample_rate);
            }
        }

        if !self.eq.is_noop()
            && (before_filter_count != self.eq.filters.len() || self.active_node_id.is_none())
        {
            tracing::debug!(
                old_filter_count = before_filter_count,
                new_filter_count = self.eq.filters.len(),
                "Reloading pipewire module"
            );
            self.load_module();
        }

        Ok(ControlFlow::Continue(()))
    }

    fn perform_autoeq_action(
        &mut self,
        action: action::AutoEqAction,
    ) -> io::Result<ControlFlow<()>> {
        use action::AutoEqAction;

        match action {
            AutoEqAction::Quit => return Ok(ControlFlow::Break(())),
            AutoEqAction::ToggleHelp => self.show_help = !self.show_help,
            AutoEqAction::SelectNext => {
                if self.autoeq_browser.selected_index + 1
                    < self.autoeq_browser.filtered_results.len()
                {
                    self.autoeq_browser.selected_index += 1;
                }
            }
            AutoEqAction::SelectPrevious => {
                self.autoeq_browser.selected_index =
                    self.autoeq_browser.selected_index.saturating_sub(1);
            }
            AutoEqAction::ApplyAutoEq => self.apply_selected_autoeq(),
            AutoEqAction::CycleTarget(rotation) => {
                if let Some(targets) = &self.autoeq_browser.targets {
                    let len = targets.len();
                    match rotation {
                        Rotation::Clockwise => {
                            self.autoeq_browser.selected_target_index =
                                (self.autoeq_browser.selected_target_index + 1) % len;
                        }
                        Rotation::CounterClockwise => {
                            self.autoeq_browser.selected_target_index = self
                                .autoeq_browser
                                .selected_target_index
                                .checked_sub(1)
                                .unwrap_or(len - 1);
                        }
                    }
                }
            }
            AutoEqAction::EnterFilterMode => {
                self.command_buffer.clear();
                self.command_buffer.push('/');
                self.command_cursor_pos = 1;
                self.input_mode = InputMode::Command;
                self.command_history_index = None;
                self.command_history_scratch.clear();
                self.status = None;
            }
            AutoEqAction::CloseAutoEq => self.enter_eq_mode(),
            AutoEqAction::EnterCommandMode => self.enter_command_mode(),
        }

        Ok(ControlFlow::Continue(()))
    }

    fn enter_eq_mode(&mut self) {
        self.tab = Tab::Eq;
        self.input_mode = InputMode::Eq;
    }

    fn perform_command_action(
        &mut self,
        action: action::CommandAction,
    ) -> io::Result<ControlFlow<()>> {
        use action::CommandAction;

        match action {
            CommandAction::ExecuteCommand => {
                let buffer = mem::take(&mut self.command_buffer);
                // Check if it's a filter (starts with /) or command (starts with :)
                if let Some(query) = buffer.strip_prefix('/') {
                    // Filter mode - update autoeq filter and return to AutoEq mode
                    self.autoeq_browser.filter_query = query.to_string();
                    self.autoeq_browser.update_filtered_results();
                    self.input_mode = InputMode::AutoEq;
                    self.command_cursor_pos = 0;
                } else if let Some(cmd) = buffer.strip_prefix(':') {
                    // Command mode - strip : prefix, execute and return to Eq mode
                    self.input_mode = InputMode::Eq;
                    self.command_cursor_pos = 0;
                    return self.execute_command(cmd);
                } else {
                    // Shouldn't happen, but handle gracefully
                    self.input_mode = InputMode::Eq;
                    self.command_cursor_pos = 0;
                }
            }
            CommandAction::ExitCommandMode => {
                // Return to previous mode based on what command we were entering
                if self.command_buffer.starts_with('/') {
                    self.input_mode = InputMode::AutoEq;
                } else {
                    self.input_mode = InputMode::Eq;
                }
                self.command_buffer.clear();
                self.command_cursor_pos = 0;
            }
            CommandAction::CommandHistoryPrevious => self.command_history_previous(),
            CommandAction::CommandHistoryNext => self.command_history_next(),
            CommandAction::DeleteCharBackward => {
                // Don't allow deleting the prefix character (: or /)
                if self.command_cursor_pos > 1 && !self.command_buffer.is_empty() {
                    self.command_buffer.remove(self.command_cursor_pos - 1);
                    self.command_cursor_pos -= 1;
                }
                self.command_history_index = None;

                // Update filter in real-time if we're in filter mode
                if self.command_buffer.starts_with('/') {
                    self.autoeq_browser.filter_query = self.command_buffer[1..].to_string();
                    self.autoeq_browser.update_filtered_results();
                }
            }
            CommandAction::DeleteCharForward => {
                if self.command_cursor_pos < self.command_buffer.len() {
                    self.command_buffer.remove(self.command_cursor_pos);
                }
                self.command_history_index = None;

                // Update filter in real-time if we're in filter mode
                if self.command_buffer.starts_with('/') {
                    self.autoeq_browser.filter_query = self.command_buffer[1..].to_string();
                    self.autoeq_browser.update_filtered_results();
                }
            }
            CommandAction::MoveCursorLeft => {
                // Don't move cursor before the prefix character
                self.command_cursor_pos = self.command_cursor_pos.saturating_sub(1).max(1)
            }
            CommandAction::MoveCursorRight => {
                if self.command_cursor_pos < self.command_buffer.len() {
                    self.command_cursor_pos += 1;
                }
            }
            CommandAction::MoveCursorHome => self.command_cursor_pos = 1, // After prefix
            CommandAction::MoveCursorEnd => self.command_cursor_pos = self.command_buffer.len(),
        }

        Ok(ControlFlow::Continue(()))
    }

    fn open_autoeq(&mut self) {
        self.tab = Tab::AutoEq;
        self.input_mode = InputMode::AutoEq;
        self.load_autoeq_data();
    }

    fn load_module(&mut self) {
        let pw_tx = self.pw_tx.clone();
        let mut args = self.eq.to_module_args(self.sample_rate);
        if let Some(sink) = &self.original_default_sink {
            args.playback_props.target_object = Some(TargetObject::Serial(sink.object_serial));
        }

        let _ = pw_tx.send(pw::Message::LoadModule {
            name: "libpipewire-module-filter-chain".into(),
            args: Box::new(args),
        });
    }

    fn load_autoeq_data(&mut self) {
        self.autoeq_browser
            .load_data(self.http_client.clone(), self.notifs_tx.clone());
    }

    fn apply_selected_autoeq(&mut self) {
        if let Some(result) = self
            .autoeq_browser
            .apply_selected(self.http_client.clone(), self.notifs_tx.clone())
        {
            self.status = Some(result);
        } else {
            self.status = Some(Err("No headphone or target selected".to_string()));
        }
    }

    fn enter_command_mode(&mut self) {
        self.command_buffer.clear();
        self.command_buffer.push(':');
        self.command_cursor_pos = 1;
        self.input_mode = InputMode::Command;
        self.command_history_index = None;
        self.command_history_scratch.clear();
        self.status = None;
    }

    fn command_history_previous(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.command_history_index {
            None => {
                // Save current buffer and start at the end of history
                self.command_history_scratch = mem::take(&mut self.command_buffer);
                self.command_history_index = Some(self.command_history.len() - 1);
                self.command_buffer =
                    format!(":{}", self.command_history[self.command_history.len() - 1]);
                self.command_cursor_pos = self.command_buffer.len();
            }
            Some(idx) if idx > 0 => {
                // Go back in history
                self.command_history_index = Some(idx - 1);
                self.command_buffer = format!(":{}", self.command_history[idx - 1]);
                self.command_cursor_pos = self.command_buffer.len();
            }
            _ => {}
        }
    }

    fn command_history_next(&mut self) {
        if let Some(idx) = self.command_history_index {
            if idx + 1 < self.command_history.len() {
                // Go forward in history
                self.command_history_index = Some(idx + 1);
                self.command_buffer = format!(":{}", self.command_history[idx + 1]);
                self.command_cursor_pos = self.command_buffer.len();
            } else {
                // At the end, restore scratch
                self.command_history_index = None;
                self.command_buffer = mem::take(&mut self.command_history_scratch);
                self.command_cursor_pos = self.command_buffer.len();
            }
        }
    }

    pub(super) fn generate_help_text(&self) -> String {
        // Group keys by action description
        let mut action_keys: HashMap<String, Vec<String>> = HashMap::new();

        match self.input_mode {
            InputMode::Eq => {
                for (key, action) in &self.config.keymap.normal {
                    if let Some(desc) = action.description() {
                        action_keys
                            .entry(desc.to_string())
                            .or_default()
                            .push(format!("{key}"));
                    }
                }
            }
            InputMode::AutoEq => {
                for (key, action) in &self.config.keymap.autoeq {
                    if let Some(desc) = action.description() {
                        action_keys
                            .entry(desc.to_string())
                            .or_default()
                            .push(format!("{key}"));
                    }
                }
            }
            InputMode::Command => {
                // Command mode doesn't show help text in the same way
                return String::new();
            }
        }

        let mut help_items: Vec<String> = action_keys
            .into_iter()
            .map(|(desc, mut keys)| {
                keys.sort();
                format!("{} {desc}", keys.join("/"))
            })
            .collect();

        help_items.sort();
        help_items.join(" | ")
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
            ["autoeq"] => self.open_autoeq(),
            [cmd @ ("w" | "write" | "w!" | "write!"), args @ ..] => {
                let force = cmd.ends_with('!');
                let mut path = match args {
                    [path] => PathBuf::from(path),
                    _ => {
                        self.status = Some(Err("usage: write <path>".to_string()));
                        return Ok(ControlFlow::Continue(()));
                    }
                };

                let format = match path.extension() {
                    Some(ext)
                        if ext.eq_ignore_ascii_case("apo") || ext.eq_ignore_ascii_case("txt") =>
                    {
                        Format::Apo
                    }
                    Some(ext) if ext.eq_ignore_ascii_case("conf") => Format::PwParamEq,
                    _ => {
                        self.status = Some(Err(
                            "output file must have an extension of .conf (PipeWire) or .apo/.txt (APO)".to_string(),
                        ));
                        return Ok(ControlFlow::Continue(()));
                    }
                };

                if path.is_relative() && matches!(format, Format::PwParamEq) {
                    path = dirs::config_dir()
                        .unwrap()
                        .join("pipewire/pipewire.conf.d")
                        .join(path);
                }

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
                            Err(err) => {
                                Err(format!("failed to save file to {}: {err}", path.display()))
                            }
                        }
                    }
                });
            }
            _ => self.status = Some(Err(format!("unknown command: {cmd}"))),
        }

        Ok(ControlFlow::Continue(()))
    }
}

impl<B: Backend + io::Write> Drop for App<B> {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), cursor::SetCursorStyle::DefaultUserShape);
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn test_default_config_parses() {
        let _config = Config::default();
    }

    #[test]
    fn test_config_compat() {
        // Ensure that the stable config file can still be parsed in the current version
        let stable_config = include_str!("../pw-eq.conf");
        let _config: Config = spa_json::from_str::<Config>(stable_config).unwrap();
    }

    #[test]
    fn test_config_serdes() {
        let config = Config::default();
        let s = spa_json::to_string_pretty(&config).unwrap();
        let config2: Config = spa_json::from_str(&s).unwrap();
        assert_eq!(config, config2);
    }
}

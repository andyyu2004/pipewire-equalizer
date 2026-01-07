mod action;
mod draw;
mod eq;
mod theme;

use crate::{FilterId, UpdateFilter, filter::Filter, update_filters, use_eq};
use std::collections::HashMap;
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
use pw_util::pipewire;
use ratatui::{Terminal, prelude::Backend};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::pw::{self, pw_thread};

use self::{action::Action, eq::Eq, theme::Theme};

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

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    keymap: KeyMap<InputMode, zi_input::KeyEvent, Action>,
    pub(super) theme: Theme,
}

impl Config {
    /// Right-biased in-place merge of two configs
    pub fn merge(mut self, config: Config) -> Self {
        self.keymap.merge(config.keymap);

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
            theme: Theme::default(),
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
                    "<Esc>":   { "enter-mode": { "mode": "normal" } },
                    ":":       { "enter-mode": { "mode": "command" } },
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
                    "<Tab>":   { "cycle-filter-type": { "rotation": "clockwise" } },
                    "<S-Tab>": { "cycle-filter-type": { "rotation": "counter-clockwise" } },
                    "v":       { "cycle-view-mode": { "rotation": "clockwise" } },
                    "0":       { "adjust-gain": { "set": 0.0 } },
                },
                "command": {
                    "<Esc>":       { "enter-mode": { "mode": "normal" } },
                    "<C-c>":       { "enter-mode": { "mode": "normal" } },
                    "<CR>":        "execute-command",
                    "<Up>":        "command-history-previous",
                    "<Down>":      "command-history-next",
                    "<BS>":        "delete-char-backward",
                    "<Del>":       "delete-char-forward",
                    "<Left>":      "move-cursor-left",
                    "<Right>":     "move-cursor-right",
                    "<Home>":      "move-cursor-home",
                    "<End>":       "move-cursor-end",
                }
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
        if let Some(action) = self.config.keymap.get(&self.input_mode, &key) {
            match self.perform(*action)? {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => return Ok(ControlFlow::Break(())),
            }
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
            Action::ExecuteCommand => {
                let InputMode::Command = mem::replace(&mut self.input_mode, InputMode::Normal)
                else {
                    return Ok(ControlFlow::Continue(()));
                };
                let buffer = mem::take(&mut self.command_buffer);
                return self.execute_command(&buffer);
            }
            Action::CommandHistoryPrevious => self.command_history_previous(),
            Action::CommandHistoryNext => self.command_history_next(),
            Action::DeleteCharBackward => {
                if self.command_cursor_pos > 0 && !self.command_buffer.is_empty() {
                    self.command_buffer.remove(self.command_cursor_pos - 1);
                    self.command_cursor_pos -= 1;
                }
                self.command_history_index = None;
            }
            Action::DeleteCharForward => {
                if self.command_cursor_pos < self.command_buffer.len() {
                    self.command_buffer.remove(self.command_cursor_pos);
                }
                self.command_history_index = None;
            }
            Action::MoveCursorLeft => {
                self.command_cursor_pos = self.command_cursor_pos.saturating_sub(1)
            }
            Action::MoveCursorRight => {
                if self.command_cursor_pos < self.command_buffer.len() {
                    self.command_cursor_pos += 1;
                }
            }
            Action::MoveCursorHome => self.command_cursor_pos = 0,
            Action::MoveCursorEnd => self.command_cursor_pos = self.command_buffer.len(),
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

    fn command_history_previous(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.command_history_index {
            None => {
                // Save current buffer and start at the end of history
                self.command_history_scratch = mem::take(&mut self.command_buffer);
                self.command_history_index = Some(self.command_history.len() - 1);
                self.command_buffer = self.command_history[self.command_history.len() - 1].clone();
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

    fn command_history_next(&mut self) {
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

    pub(super) fn generate_help_text(&self) -> String {
        // Group keys by action description
        let mut action_keys: HashMap<String, Vec<String>> = HashMap::new();

        // Iterate through all normal mode bindings
        for (key, action) in self.config.keymap.iter_mode(&InputMode::Normal) {
            // Special handling for mode switching
            if let Action::EnterMode {
                mode: InputMode::Command,
            } = action
            {
                action_keys
                    .entry("command".to_string())
                    .or_default()
                    .push(format!("{}", key));
                continue;
            }

            // Get description for other actions
            if let Some(desc) = action.description() {
                action_keys
                    .entry(desc.to_string())
                    .or_default()
                    .push(format!("{key}"));
            }
        }

        // Build help text, sorted by action
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

    fn handle_command_key(&mut self, key: KeyEvent) -> io::Result<ControlFlow<()>> {
        assert!(matches!(self.input_mode, InputMode::Command));

        // Try to find a matching action in the keymap
        if let Some(action) = self.config.keymap.get(&self.input_mode, &key) {
            return self.perform(*action);
        }

        if let KeyCode::Char(c) = key.code()
            && !key.modifiers().contains(KeyModifiers::CONTROL)
            && !key.modifiers().contains(KeyModifiers::ALT)
        {
            self.command_buffer.insert(self.command_cursor_pos, c);
            self.command_cursor_pos += 1;
            self.command_history_index = None;
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
                let mut path = match args {
                    [path] => PathBuf::from(path),
                    _ => {
                        self.status = Some(Err("usage: write <path>".to_string()));
                        return Ok(ControlFlow::Continue(()));
                    }
                };

                if path.is_relative() {
                    path = dirs::config_dir()
                        .unwrap()
                        .join("pipewire/pipewire.conf.d")
                        .join(path);
                }

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
    fn test_config_serdes() {
        let config = Config::default();
        let s = spa_json::to_string_pretty(&config).unwrap();
        let config2: Config = spa_json::from_str(&s).unwrap();
        assert_eq!(config, config2);
    }
}

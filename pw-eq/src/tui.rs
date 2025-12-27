use std::{
    backtrace::Backtrace, io, num::NonZero, ops::ControlFlow, pin::pin, sync::mpsc::Receiver,
};

use crossterm::{
    event::{DisableMouseCapture, Event, EventStream, KeyCode, KeyEvent},
    execute,
    terminal::{self, EnterAlternateScreen},
};
use futures_util::{Stream, StreamExt as _};
use pw_util::{config::ModuleArgs, pipewire};
use ratatui::{
    Terminal,
    prelude::{Backend, Constraint, CrosstermBackend, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Row, Table},
};
use tokio::sync::mpsc;

use crate::pw::{self, pw_thread};

// EQ Band state
#[derive(Debug, Clone)]
struct Band {
    frequency: f64,
    gain: f64,
    q: f64,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            frequency: 1000.0,
            gain: 0.0,
            q: 1.0,
        }
    }
}

// EQ state
struct EqState {
    name: String,
    bands: Vec<Band>,
    selected_band: usize,
    max_bands: usize,
}

impl EqState {
    fn new(name: String) -> Self {
        Self {
            name,
            bands: vec![
                Band {
                    frequency: 50.0,
                    gain: 0.0,
                    q: 1.0,
                },
                Band {
                    frequency: 100.0,
                    gain: 0.0,
                    q: 1.0,
                },
                Band {
                    frequency: 200.0,
                    gain: 0.0,
                    q: 1.0,
                },
                Band {
                    frequency: 500.0,
                    gain: 0.0,
                    q: 1.0,
                },
                Band {
                    frequency: 2000.0,
                    gain: 0.0,
                    q: 1.0,
                },
                Band {
                    frequency: 5000.0,
                    gain: 0.0,
                    q: 1.0,
                },
                Band {
                    frequency: 10000.0,
                    gain: 0.0,
                    q: 1.0,
                },
            ],
            selected_band: 0,
            max_bands: 20,
        }
    }

    fn add_band(&mut self) {
        if self.bands.len() >= self.max_bands {
            return;
        }

        let current_band = &self.bands[self.selected_band];

        // Calculate new frequency between current and next band
        let new_freq = if self.selected_band + 1 < self.bands.len() {
            let next_band = &self.bands[self.selected_band + 1];
            // Geometric mean (better for logarithmic frequency scale)
            (current_band.frequency * next_band.frequency).sqrt()
        } else {
            // If at the end, go halfway to 20kHz in log space
            (current_band.frequency * 20000.0).sqrt().min(20000.0)
        };

        let new_band = Band {
            frequency: new_freq,
            gain: 0.0,
            q: current_band.q, // Copy Q from current band
        };

        self.bands.insert(self.selected_band + 1, new_band);
        self.selected_band += 1;
    }

    fn delete_selected_band(&mut self) {
        if self.bands.len() > 1 {
            self.bands.remove(self.selected_band);
            if self.selected_band >= self.bands.len() {
                self.selected_band = self.bands.len().saturating_sub(1);
            }
        }
    }

    fn select_next_band(&mut self) {
        if self.selected_band < self.bands.len().saturating_sub(1) {
            self.selected_band += 1;
        }
    }

    fn select_prev_band(&mut self) {
        self.selected_band = self.selected_band.saturating_sub(1);
    }

    fn adjust_freq(&mut self, delta: f64) {
        if let Some(band) = self.bands.get_mut(self.selected_band) {
            band.frequency = (band.frequency + delta).clamp(20.0, 20000.0);
        }
    }

    fn adjust_gain(&mut self, delta: f64) {
        if let Some(band) = self.bands.get_mut(self.selected_band) {
            band.gain = (band.gain + delta).clamp(-12.0, 12.0);
        }
    }

    fn adjust_q(&mut self, delta: f64) {
        if let Some(band) = self.bands.get_mut(self.selected_band) {
            band.q = (band.q + delta).clamp(0.1, 10.0);
        }
    }

    fn to_apo_config(&self) -> pw_util::apo::Config {
        let filters = self
            .bands
            .iter()
            .enumerate()
            .map(|(idx, band)| pw_util::apo::Filter {
                number: (idx + 1) as u32,
                enabled: true,
                filter_type: pw_util::apo::FilterType::Peaking,
                freq: band.frequency as f32,
                gain: band.gain as f32,
                q: band.q as f32,
            })
            .collect();

        pw_util::apo::Config {
            preamp: None,
            filters,
        }
    }

    fn to_module_args(&self) -> ModuleArgs {
        let apo_config = self.to_apo_config();
        pw_util::config::Module::from_apo(
            &format!("{}-{}", self.name, self.bands.len()),
            &apo_config,
        )
        .args
    }
}

pub async fn run() -> anyhow::Result<()> {
    let (panic_tx, panic_rx) = std::sync::mpsc::sync_channel(1);
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = Backtrace::capture();
        let _ = panic_tx.send((info.to_string(), backtrace));
    }));

    let term = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut app = App::new(term, panic_rx)?;
    app.enter()?;

    let events = EventStream::new();

    app.run(events).await
}

pub enum Notif {
    ModuleLoaded {
        id: u32,
        name: String,
        media_name: String,
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
}

impl<B> App<B>
where
    B: Backend + io::Write,
    B::Error: Send + Sync + 'static,
{
    pub fn new(term: Terminal<B>, panic_rx: Receiver<(String, Backtrace)>) -> io::Result<Self> {
        let (pw_tx, rx) = pipewire::channel::channel();
        let (notifs_tx, notifs) = mpsc::channel(100);
        std::thread::spawn(|| pw_thread(notifs_tx, rx));

        Ok(Self {
            term,
            panic_rx,
            pw_tx,
            notifs,
            eq_state: EqState::new("pweq".to_string()),
            active_node_id: None,
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
        &mut self,
        events: impl Stream<Item = io::Result<Event>>,
    ) -> anyhow::Result<()> {
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

        Ok(())
    }

    async fn on_notif(&mut self, notif: Notif) {
        match notif {
            Notif::ModuleLoaded {
                id,
                name,
                media_name,
            } => {
                tracing::info!(id, name, media_name, "module loaded");
                let Ok(node_id) = pw_eq::use_eq(&media_name).await.inspect_err(|err| {
                    tracing::error!(error = %err, "failed to use EQ");
                }) else {
                    return;
                };

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
        tracing::debug!(key = ?key, "key event");
        match key.code {
            // Quit
            KeyCode::Esc | KeyCode::Char('q') => return Ok(ControlFlow::Break(())),

            // Navigation
            KeyCode::Tab | KeyCode::Char('j') => self.eq_state.select_next_band(),
            KeyCode::BackTab | KeyCode::Char('k') => self.eq_state.select_prev_band(),
            KeyCode::Char(c @ '1'..='9') => {
                let idx = c.to_digit(10).unwrap() as usize - 1;
                if idx < self.eq_state.bands.len() {
                    self.eq_state.selected_band = idx;
                }
            }

            // Frequency adjustment
            KeyCode::Char('f') => self.eq_state.adjust_freq(1.0),
            KeyCode::Char('F') => self.eq_state.adjust_freq(-1.0),

            // Gain adjustment
            KeyCode::Char('g') => self.eq_state.adjust_gain(0.1),
            KeyCode::Char('G') => self.eq_state.adjust_gain(-0.1),

            // Q adjustment
            KeyCode::Char('z') => self.eq_state.adjust_q(0.1),
            KeyCode::Char('Z') => self.eq_state.adjust_q(-0.1),

            // Band management
            KeyCode::Char('a') => self.eq_state.add_band(),
            KeyCode::Char('d') => self.eq_state.delete_selected_band(),
            KeyCode::Char('0') => {
                // Zero the gain on current band
                if let Some(band) = self.eq_state.bands.get_mut(self.eq_state.selected_band) {
                    band.gain = 0.0;
                }
            }

            KeyCode::Char('l') => {
                tracing::info!("Loading PipeWire EQ module");
                let _ = self.pw_tx.send(pw::Message::LoadModule {
                    name: "libpipewire-module-filter-chain".into(),
                    args: Box::new(self.eq_state.to_module_args()),
                });
            }

            _ => {}
        }

        // TODO only run this if there are state changes
        // Also debounce a bit
        if let Some(node_id) = self.active_node_id {
            let band_idx = NonZero::new(self.eq_state.selected_band + 1).unwrap();
            let band = &self.eq_state.bands[self.eq_state.selected_band];
            let update = pw_eq::UpdateBand {
                frequency: Some(band.frequency),
                gain: Some(band.gain),
                q: Some(band.q),
            };

            tokio::spawn(async move {
                if let Err(err) = pw_eq::update_band(node_id, band_idx, update).await {
                    tracing::error!(error = %err, "failed to update band");
                }
            });
        }

        Ok(ControlFlow::Continue(()))
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let eq_state = &self.eq_state;
        self.term.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Min(0),    // Main content
                    Constraint::Length(3), // Footer
                ])
                .split(f.area());

            // Header
            let header = Paragraph::new(format!(
                "PipeWire EQ: {} | Bands: {}/{}",
                eq_state.name,
                eq_state.bands.len(),
                eq_state.max_bands
            ))
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Band table
            Self::draw_band_table(f, chunks[1], eq_state);

            // Footer/Help
            let help = Paragraph::new(
                "Tab/Shift-Tab/j/k: select | f/F: freq | g/G: gain | z/Z: Q | a: add | d: delete | 0: zero gain | Esc/q: quit"
            )
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(help, chunks[2]);
        })?;
        Ok(())
    }

    fn draw_band_table(f: &mut ratatui::Frame, area: Rect, eq_state: &EqState) {
        let rows: Vec<Row> = eq_state
            .bands
            .iter()
            .enumerate()
            .map(|(idx, band)| {
                let freq_str = if band.frequency >= 1000.0 {
                    format!("{:.1}k", band.frequency / 1000.0)
                } else {
                    format!("{:.0}", band.frequency)
                };
                let style = if idx == eq_state.selected_band {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                Row::new(vec![
                    format!("{}", idx + 1),
                    freq_str,
                    format!("{:+.1}", band.gain),
                    format!("{:.2}", band.q),
                ])
                .style(style)
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(5),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(vec!["#", "Freq (Hz)", "Gain (dB)", "Q"])
                .style(Style::default().add_modifier(Modifier::BOLD))
                .bottom_margin(1),
        )
        .block(Block::default().borders(Borders::ALL).title("Bands"));

        f.render_widget(table, area);
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

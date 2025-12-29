use crate::{UpdateFilter, filter::Filter, update_filter, use_eq};
use std::{
    backtrace::Backtrace, error::Error, io, num::NonZero, ops::ControlFlow, pin::pin,
    sync::mpsc::Receiver,
};

use crossterm::{
    event::{DisableMouseCapture, Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen},
};
use futures_util::{Stream, StreamExt as _};
use pw_util::{
    config::{FilterType, Module, ModuleArgs, NodeKind, RateAndBiquadCoefficients, RawNodeConfig},
    pipewire,
};
use ratatui::{
    Terminal,
    layout::Direction,
    prelude::{Backend, Constraint, CrosstermBackend, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table},
};
use tokio::sync::mpsc;

use crate::pw::{self, pw_thread};

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

// EQ state
struct EqState {
    name: String,
    filters: Vec<Filter>,
    selected_band: usize,
    max_bands: usize,
    view_mode: ViewMode,
}

impl EqState {
    fn new(name: String) -> Self {
        Self {
            name,
            filters: vec![
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
            selected_band: 0,
            max_bands: 20,
            view_mode: ViewMode::Normal,
        }
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
            band.q = f(band.q).clamp(0.1, 10.0);
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

    fn to_module_args(&self, rate: u32) -> ModuleArgs {
        Module::from_kinds(
            &format!("{}-{}", self.name, self.filters.len()),
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
    original_default_sink: Option<u32>,
    pw_handle: Option<std::thread::JoinHandle<io::Result<()>>>,
    sample_rate: u32,
}

impl<B> App<B>
where
    B: Backend + io::Write,
    B::Error: Send + Sync + 'static,
{
    pub fn new(term: Terminal<B>, panic_rx: Receiver<(String, Backtrace)>) -> io::Result<Self> {
        let (pw_tx, rx) = pipewire::channel::channel();
        let (notifs_tx, notifs) = mpsc::channel(100);
        let pw_handle = std::thread::spawn(|| pw_thread(notifs_tx, rx));

        Ok(Self {
            term,
            panic_rx,
            pw_tx,
            notifs,
            eq_state: EqState::new("pweq".to_string()),
            active_node_id: None,
            original_default_sink: None,
            pw_handle: Some(pw_handle),
            // TODO query
            sample_rate: 48000,
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
            } => {
                tracing::info!(id, name, media_name, "module loaded");
                let Ok(node_id) = use_eq(&media_name).await.inspect_err(|err| {
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

        let before_idx = self.eq_state.selected_band;
        let before_band = self.eq_state.filters[self.eq_state.selected_band];

        match key.code {
            KeyCode::Esc => return Ok(ControlFlow::Break(())),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(ControlFlow::Break(()));
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

            // Frequency adjustment
            KeyCode::Char('f') => self.eq_state.adjust_freq(|f| f * 1.025),
            KeyCode::Char('F') => self.eq_state.adjust_freq(|f| f / 1.025),

            // Gain adjustment
            KeyCode::Char('g') => self.eq_state.adjust_gain(|g| g + 0.1),
            KeyCode::Char('G') => self.eq_state.adjust_gain(|g| g - 0.1),

            // Q adjustment
            KeyCode::Char('q') => self.eq_state.adjust_q(|q| q + 0.01),
            KeyCode::Char('Q') => self.eq_state.adjust_q(|q| q - 0.01),

            // Filter type
            KeyCode::Char('t') => self.eq_state.cycle_filter_type(Rotation::Clockwise),
            KeyCode::Char('T') => self.eq_state.cycle_filter_type(Rotation::CounterClockwise),

            // Mute
            KeyCode::Char('m') => self.eq_state.toggle_mute(),

            // View mode
            KeyCode::Char('e') => self.eq_state.toggle_view_mode(),

            // Band management
            KeyCode::Char('a') => self.eq_state.add_band(),
            KeyCode::Char('d') => self.eq_state.delete_selected_band(),
            KeyCode::Char('0') => {
                // Zero the gain on current band
                if let Some(band) = self.eq_state.filters.get_mut(self.eq_state.selected_band) {
                    band.gain = 0.0;
                }
            }

            KeyCode::Char('l') => {
                tracing::info!("Loading PipeWire EQ module");
                let _ = self.pw_tx.send(pw::Message::LoadModule {
                    name: "libpipewire-module-filter-chain".into(),
                    args: Box::new(self.eq_state.to_module_args(self.sample_rate)),
                });
            }

            _ => {}
        }

        if let Some(node_id) = self.active_node_id
            && self.eq_state.selected_band == before_idx
            && self.eq_state.filters[self.eq_state.selected_band] != before_band
        {
            let band_idx = NonZero::new(self.eq_state.selected_band + 1).unwrap();
            let band = &self.eq_state.filters[self.eq_state.selected_band];

            // Always send both params and coefficients. This is a bit weird but seems to be
            // necessary to get the changes to apply correctly in all cases.
            let coeffs = band.biquad_coeffs(self.sample_rate as f64);
            let update = UpdateFilter {
                frequency: Some(band.frequency),
                gain: Some(if band.muted { 0.0 } else { band.gain }),
                q: Some(band.q),
                coeffs: Some(coeffs),
            };

            tokio::spawn(async move {
                if let Err(err) = update_filter(node_id, band_idx, update).await {
                    tracing::error!(error = %err, "failed to update band");
                }
            });
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
                    Constraint::Length(3),      // Footer
                ])
                .split(f.area());

            // Header
            let header = Paragraph::new(format!(
                "PipeWire EQ: {} | Bands: {}/{} | Sample Rate: {:.0} Hz",
                eq_state.name,
                eq_state.filters.len(),
                eq_state.max_bands,
                sample_rate
            ))
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Band table
            Self::draw_band_table(f, chunks[1], eq_state, sample_rate);

            // Frequency response chart
            Self::draw_frequency_response(f, chunks[2], eq_state, sample_rate);

            // Footer/Help
            let help = Paragraph::new(
                "Tab/j/k: select | t: type | m: mute | e: expert | f/F: freq | g/G: gain | q/Q: Q | a: add | d: delete | 0: zero | Esc/C-c: quit"
            )
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(help, chunks[3]);
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

                // Dim muted filters
                let (num_color, type_color, freq_color, q_color) = if band.muted {
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

                let final_gain_color = if band.muted {
                    Color::DarkGray
                } else {
                    gain_color
                };

                let coeff_color = if band.muted {
                    Color::DarkGray
                } else if is_selected {
                    Color::Green
                } else {
                    Color::Gray
                };

                // Create base cells
                let mut cells = vec![
                    Cell::from(format!("{}", idx + 1)).style(
                        Style::default().fg(num_color).add_modifier(
                            if is_selected && !band.muted {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            },
                        ),
                    ),
                    Cell::from(type_str).style(Style::default().fg(type_color).add_modifier(
                        if is_selected && !band.muted {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        },
                    )),
                    Cell::from(freq_str).style(Style::default().fg(freq_color).add_modifier(
                        if is_selected && !band.muted {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        },
                    )),
                    Cell::from(format!("{:+.1}", band.gain)).style(
                        Style::default().fg(final_gain_color).add_modifier(
                            if is_selected && !band.muted {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            },
                        ),
                    ),
                    Cell::from(format!("{:.2}", band.q)).style(
                        Style::default()
                            .fg(q_color)
                            .add_modifier(if is_selected && !band.muted {
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

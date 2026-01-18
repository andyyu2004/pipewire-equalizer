use super::{App, Eq, InputMode, Tab, ViewMode, theme::Theme};
use pw_util::module::FilterType;
use ratatui::{
    layout::Direction,
    prelude::{Backend, Constraint, Layout, Rect},
    style::{Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Padding, Paragraph, Row, Table, Wrap,
    },
};
use std::io;

impl<B> App<B>
where
    B: Backend + io::Write,
    B::Error: Send + Sync + 'static,
{
    fn footer_height(help_len: usize, show_help: bool, terminal_width: u16) -> u16 {
        if show_help {
            let lines_needed =
                (help_len + terminal_width as usize - 1) / terminal_width.max(1) as usize;
            lines_needed.clamp(1, 5) as u16
        } else {
            1
        }
    }

    fn render_footer(&self, help_text: String) -> Paragraph<'static> {
        let theme = &self.config.theme;

        match &self.input_mode {
            InputMode::Command => {
                // Buffer always contains the prefix (: or /)
                Paragraph::new(self.command_buffer.clone()).style(Style::default().fg(theme.footer))
            }
            InputMode::Eq | InputMode::AutoEq if self.status.is_some() => {
                let (msg, color) = match self.status.as_ref().unwrap() {
                    Ok(msg) => (msg.to_owned(), theme.status_ok),
                    Err(msg) => (msg.to_owned(), theme.status_error),
                };
                Paragraph::new(msg).style(Style::default().fg(color))
            }
            InputMode::Eq | InputMode::AutoEq if self.show_help => Paragraph::new(help_text)
                .style(Style::default().fg(theme.help))
                .wrap(Wrap { trim: true }),
            InputMode::Eq | InputMode::AutoEq => {
                Paragraph::new("Press ? for help").style(Style::default().fg(theme.footer))
            }
        }
    }

    pub(super) fn draw(&mut self) -> anyhow::Result<()> {
        match self.tab {
            Tab::Eq => self.draw_eq_tab(),
            Tab::AutoEq => self.draw_autoeq_tab(),
        }
    }

    fn draw_eq_tab(&mut self) -> anyhow::Result<()> {
        let eq = &self.eq;
        let sample_rate = self.sample_rate;
        let view_mode = self.view_mode;
        let theme = &self.config.theme;

        let help_text = if self.show_help {
            self.generate_help_text()
        } else {
            String::new()
        };

        let help_len = help_text.len();
        let footer = self.render_footer(help_text);

        self.term.draw(|f| {
            // Set background color for the entire frame
            f.render_widget(
                Block::default().style(Style::default().bg(theme.background)),
                f.area(),
            );
            let footer_height = Self::footer_height(help_len, self.show_help, f.area().width);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),             // Header
                    Constraint::Min(10),               // Band table
                    Constraint::Percentage(40),        // Frequency response chart
                    Constraint::Length(footer_height), // Footer
                ])
                .split(f.area());

            let preamp_color = if eq.preamp > 0.05 {
                theme.gain_positive
            } else if eq.preamp < -0.05 {
                theme.gain_negative
            } else {
                theme.gain_neutral
            };

            let mut header_spans = vec![
                Span::styled(
                    format!(
                        "PipeWire EQ: {} | Bands: {}/{} | Sample Rate: {:.0} Hz | Preamp: ",
                        eq.name,
                        eq.filters.len(),
                        eq.max_filters,
                        sample_rate
                    ),
                    Style::default().fg(theme.header),
                ),
                Span::styled(
                    format!("{} dB", Gain(eq.preamp)),
                    Style::default().fg(preamp_color),
                ),
            ];

            if eq.bypassed {
                header_spans.push(Span::styled(
                    " | BYPASSED",
                    Style::default()
                        .fg(theme.bypassed)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            let header = Paragraph::new(Line::from(header_spans)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .padding(Padding::horizontal(1)),
            );
            f.render_widget(header, chunks[0]);

            draw_filters_table(f, chunks[1], eq, view_mode, sample_rate, theme);

            draw_frequency_response(f, chunks[2], eq, sample_rate, theme);

            f.render_widget(footer.clone(), chunks[3]);

            if let InputMode::Command = &self.input_mode {
                f.set_cursor_position((chunks[3].x + self.command_cursor_pos as u16, chunks[3].y));
            }
        })?;
        Ok(())
    }

    fn draw_autoeq_tab(&mut self) -> anyhow::Result<()> {
        let theme = &self.config.theme;
        let browser = &self.autoeq_browser;
        let help_text = self.generate_help_text();

        let help_len = help_text.len();
        let footer = self.render_footer(help_text);

        self.term.draw(|f| {
            // Set background color
            f.render_widget(
                Block::default().style(Style::default().bg(theme.background)),
                f.area(),
            );

            let footer_height = Self::footer_height(help_len, self.show_help, f.area().width);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),             // Header with target
                    Constraint::Min(10),               // Results table
                    Constraint::Length(footer_height), // Footer
                ])
                .split(f.area());

            // Header showing current target
            let target_text = if let Some(targets) = &browser.targets {
                if let Some(target) = targets.get(browser.selected_target_index) {
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
            if browser.loading {
                let loading = Paragraph::new("Loading headphone database...").block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border))
                        .padding(Padding::horizontal(1)),
                );
                f.render_widget(loading, chunks[1]);
            } else if browser.filtered_results.is_empty() {
                let empty = Paragraph::new("No results found. Press / to filter.").block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border))
                        .padding(Padding::horizontal(1)),
                );
                f.render_widget(empty, chunks[1]);
            } else {
                let rows: Vec<Row> = browser
                    .filtered_results
                    .iter()
                    .enumerate()
                    .map(|(idx, (name, entry))| {
                        let is_selected = idx == browser.selected_index;
                        let style = if is_selected {
                            Style::default().bg(theme.selected_row).fg(theme.background)
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
                        .title(format!(" {} results ", browser.filtered_results.len()))
                        .padding(Padding::horizontal(1)),
                );
                f.render_widget(results_table, chunks[1]);
            }

            f.render_widget(footer.clone(), chunks[2]);
        })?;

        Ok(())
    }
}

fn draw_filters_table(
    f: &mut ratatui::Frame,
    area: Rect,
    eq_state: &Eq,
    view_mode: ViewMode,
    sample_rate: u32,
    theme: &Theme,
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

            // Use theme colors for gain
            let gain_color = if band.gain > 0.05 {
                theme.gain_positive
            } else if band.gain < -0.05 {
                theme.gain_negative
            } else {
                theme.gain_neutral
            };

            let is_selected = idx == eq_state.selected_idx;
            let is_dimmed = band.muted || eq_state.bypassed;

            // Use theme color scheme
            let (num_color, type_color, freq_color, q_color) = if is_dimmed {
                let dimmed = theme.dimmed;
                (dimmed, dimmed, dimmed, dimmed)
            } else {
                (
                    theme.index,
                    theme.filter_type,
                    theme.frequency,
                    theme.q_value,
                )
            };

            let final_gain_color = if is_dimmed { theme.dimmed } else { gain_color };

            let coeff_color = if is_dimmed {
                theme.dimmed
            } else {
                theme.coefficients
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

            // Add expert mode columns
            if matches!(view_mode, ViewMode::Expert) {
                // Calculate biquad coefficients
                let coeff = band.biquad_coeffs(sample_rate as f64);

                cells.push(
                    Cell::from(format!("{:.6}", coeff.b0)).style(Style::default().fg(coeff_color)),
                );
                cells.push(
                    Cell::from(format!("{:.6}", coeff.b1)).style(Style::default().fg(coeff_color)),
                );
                cells.push(
                    Cell::from(format!("{:.6}", coeff.b2)).style(Style::default().fg(coeff_color)),
                );
                cells.push(
                    Cell::from(format!("{:.6}", coeff.a1)).style(Style::default().fg(coeff_color)),
                );
                cells.push(
                    Cell::from(format!("{:.6}", coeff.a2)).style(Style::default().fg(coeff_color)),
                );
            }

            let row = Row::new(cells);
            if is_selected {
                row.style(Style::default().bg(theme.selected_row))
            } else {
                row
            }
        })
        .collect();

    let header = if matches!(view_mode, ViewMode::Expert) {
        Row::new(vec![
            Cell::from("#").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Type").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Freq").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Gain").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Q").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("b0").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("b1").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("b2").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("a1").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("a2").style(Style::default().add_modifier(Modifier::BOLD)),
        ])
    } else {
        Row::new(vec![
            Cell::from("#").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Type").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Freq").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Gain").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Q").style(Style::default().add_modifier(Modifier::BOLD)),
        ])
    };

    let widths = if matches!(view_mode, ViewMode::Expert) {
        vec![
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(6),
        ]
    };

    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .padding(Padding::horizontal(1)),
    );

    f.render_widget(table, area);
}

fn draw_frequency_response(
    f: &mut ratatui::Frame,
    area: Rect,
    eq: &Eq,
    sample_rate: u32,
    theme: &Theme,
) {
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
        .style(Style::default().fg(theme.chart))
        .data(&data);

    // X-axis: log scale from 20 Hz to 20 kHz
    let log_min = 20_f64.log10();
    let log_max = 20000_f64.log10();

    let x_axis = Axis::default()
        .title("Frequency")
        .style(Style::default().fg(theme.border))
        .bounds([log_min, log_max])
        .labels(vec!["20Hz".to_string(), "20kHz".to_string()]);

    // Y-axis: dB scale
    let y_axis = Axis::default()
        .title("Gain (dB)")
        .style(Style::default().fg(theme.border))
        .bounds([min_db - 1.0, max_db + 1.0])
        .labels(vec![
            format!("{:.1}", min_db),
            "0".into(),
            format!("{:.1}", max_db),
        ]);

    let chart = Chart::new(vec![dataset])
        .style(Style::default().bg(theme.background))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .padding(Padding::horizontal(1)),
        )
        .x_axis(x_axis)
        .y_axis(y_axis);

    f.render_widget(chart, area);
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

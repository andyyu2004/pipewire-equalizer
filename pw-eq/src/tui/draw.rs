use super::{App, Eq, InputMode, ViewMode, theme::Theme};
use pw_util::module::FilterType;
use ratatui::{
    layout::Direction,
    prelude::{Backend, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table, Wrap},
};
use std::io;

impl<B> App<B>
where
    B: Backend + io::Write,
    B::Error: Send + Sync + 'static,
{
    pub(super) fn draw(&mut self) -> anyhow::Result<()> {
        let eq = &self.eq;
        let sample_rate = self.sample_rate;
        let view_mode = self.view_mode;
        let theme = &self.config.theme;

        let help_text = if self.show_help {
            self.generate_help_text()
        } else {
            String::new()
        };

        self.term.draw(|f| {
            // Set background color for the entire frame
            f.render_widget(
                Block::default().style(Style::default().bg(theme.background)),
                f.area(),
            );
            // Calculate footer height dynamically based on help text length
            let footer_height = if self.show_help {
                let terminal_width = f.area().width as usize;
                let help_len = help_text.len();
                // Calculate how many lines are needed for the help text
                let lines_needed = (help_len + terminal_width - 1) / terminal_width.max(1);
                // Clamp to a reasonable range (min 1, max 5)
                lines_needed.clamp(1, 5) as u16
            } else {
                1
            };

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
                Color::Green
            } else if eq.preamp < -0.05 {
                Color::Red
            } else {
                Color::Gray
            };

            let mut header_spans = vec![
                Span::raw(format!(
                    "PipeWire EQ: {} | Bands: {}/{} | Sample Rate: {:.0} Hz | Preamp: ",
                    eq.name,
                    eq.filters.len(),
                    eq.max_filters,
                    sample_rate
                )),
                Span::styled(
                    format!("{} dB", Gain(eq.preamp)),
                    Style::default().fg(preamp_color),
                ),
            ];

            if eq.bypassed {
                header_spans.push(Span::styled(
                    " | BYPASSED",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            let header = Paragraph::new(Line::from(header_spans))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            draw_filters_table(f, chunks[1], eq, view_mode, sample_rate, theme);

            draw_frequency_response(f, chunks[2], eq, sample_rate, theme);

            let footer = match &self.input_mode {
                InputMode::Command => Paragraph::new(format!(":{}", self.command_buffer)),
                InputMode::Normal if self.status.is_some() => {
                    let (msg, color) = match self.status.as_ref().unwrap() {
                        Ok(msg) => (msg.as_str(), Color::White),
                        Err(msg) => (msg.as_str(), Color::Red),
                    };
                    Paragraph::new(msg).style(Style::default().fg(color))
                }
                InputMode::Normal if self.show_help => Paragraph::new(help_text)
                    .style(Style::default().fg(Color::DarkGray))
                    .wrap(Wrap { trim: true }),
                InputMode::Normal => {
                    Paragraph::new("Press ? for help").style(Style::default().fg(Color::DarkGray))
                }
            };
            f.render_widget(footer, chunks[3]);

            if let InputMode::Command = &self.input_mode {
                f.set_cursor_position((
                    chunks[3].x + 1 + self.command_cursor_pos as u16,
                    chunks[3].y,
                ));
            }
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

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));

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
        .style(Style::default().bg(theme.background))
        .block(Block::default().borders(Borders::ALL))
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

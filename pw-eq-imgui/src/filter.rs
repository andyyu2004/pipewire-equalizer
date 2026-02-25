use std::ops::Range;

use dear_imgui_rs::{Condition, TableColumnSetup, TableFlags, Ui, WindowFlags};
use dear_implot::{AxisFlags, PlotCond, PlotUi, XAxis};
use futures_executor::block_on;
use pw_eq::{FilterId, filter::Filter, tui::{
    autoeq::{self, param_eq_to_filters},
    eq::Eq,
}};
use pw_util::module::FilterType;
use strum::IntoEnumIterator;

pub struct FilterWindowState {
    pub show_window: bool,
    pub eq: Eq,
    bypass: bool,
    preamp: f64,
    preamp_enable: bool,
    should_sync_all: bool,
    should_sync_preamp: bool,
    prev_bands: Option<usize>,
    sample_rate: u32,
    curve_x: Vec<f64>,
    curve_y: Vec<f64>,
    range_y: Range<f64>,
    filter_types: Vec<String>,
}

fn truncate_string(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        None => s.to_string(),
        Some((idx, _)) => format!("{}...", &s[..idx]),
    }
}

fn right_aligned_checkbox(ui: &Ui, label: impl AsRef<str>, value: &mut bool) -> bool {
    let element_width = ui.current_font().calc_text_size(13.0, 1000.0, 1000.0, label.as_ref())[0] + 32.0;
    ui.set_cursor_pos_x(ui.cursor_pos_x() + f32::max(0.0, ui.get_content_region_avail()[0] - element_width));
    ui.checkbox(label, value)
}

impl FilterWindowState {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            show_window: true,
            eq: Eq::new("empty", []),
            bypass: false,
            preamp: 0.0,
            preamp_enable: true,
            should_sync_all: false,
            should_sync_preamp: false,
            prev_bands: None,
            sample_rate,
            curve_x: vec![],
            curve_y: vec![],
            range_y: -1.0..1.0,
            filter_types: FilterType::iter().map(|ft| ft.to_string()).collect(),
        }
    }

    pub fn apply_all_to_pipewire(&mut self, node_id: u32) {
        if self.should_sync_all {
            self.should_sync_all = false;
            let updates = self.eq.build_all_updates(self.sample_rate);
            block_on(pw_eq::update_filters(node_id, updates)).expect("@mitkus todo error handling");
        }
    }

    pub fn apply_preamp_to_pipewire(&mut self, node_id: u32) {
        if self.should_sync_preamp {
            self.should_sync_preamp = false;
            let update = self.eq.build_preamp_update();
            block_on(pw_eq::update_filters(node_id, [(FilterId::Preamp, update)])).expect("@mitkus todo error handling");
        }
    }

    pub fn set_eq(&mut self, name: impl Into<String>, parametric_eq: autoeq::ParametricEq) {
        let filters = param_eq_to_filters(parametric_eq);
        self.eq = Eq::new(name, filters);
        self.preamp = self.eq.preamp;
        self.recalc_curve();
    }

    pub fn set_eq_apo(&mut self, name: impl Into<String>, apo: pw_util::apo::Config) {
        let filters: Vec<Filter> = apo.filters.into_iter().map(Into::<Filter>::into).collect();
        self.eq.name = name.into();
        self.eq.filters = filters;
        self.eq.preamp = apo.preamp;
        self.preamp = apo.preamp;
        self.recalc_curve();
        self.should_sync_all = true;
    }

    pub fn need_module_load(&mut self) -> bool {
        if self.prev_bands != Some(self.eq.filters.len()) {
            self.prev_bands = Some(self.eq.filters.len());
            return true;
        }
        return false;
    }

    fn recalc_curve(&mut self) {
        let curve = self
            .eq
            .frequency_response_curve(200, self.sample_rate as f64);
        self.range_y = -1.0..1.0;

        self.curve_x.clear();
        self.curve_y.clear();

        for (x, y) in curve {
            self.curve_x.push(x);
            self.curve_y.push(y);
            self.range_y.start = f64::min(self.range_y.start, y);
            self.range_y.end = f64::max(self.range_y.end, y);
        }
    }

    fn draw_filters(&mut self, ui: &Ui) -> bool {
        let columns = [
            TableColumnSetup::new("#").init_width_or_weight(12.0),
            TableColumnSetup::new("Type").init_width_or_weight(12.0),
            TableColumnSetup::new("Freq").init_width_or_weight(30.0),
            TableColumnSetup::new("Gain").init_width_or_weight(30.0),
            TableColumnSetup::new("Q").init_width_or_weight(25.0),
        ];

        let flags =
            TableFlags::BORDERS | TableFlags::BORDERS_OUTER | TableFlags::SIZING_STRETCH_PROP;

        let mut needs_recalc = false;
        let mut table_hovered = false;

        ui.table("##filters")
            .flags(flags)
            .columns(columns)
            .headers(true)
            .build(|ui| {
                let mut delete_filter = false;
                let mut add_filter = false;

                let hovered_row = ui.table_get_hovered_row();
                if hovered_row > 0 {
                    table_hovered = true;
                    self.eq.selected_idx = (hovered_row - 1) as usize;
                }

                for (i, filter) in self.eq.filters.iter_mut().enumerate() {
                    ui.table_next_row();

                    {
                        let row_id = format!("{}", i);
                        let _id_tok = ui.push_id(&row_id);

                        // #, add/remove buttons
                        ui.table_next_column();
                        ui.text(i.to_string());
                        ui.same_line();
                        if ui.button("-") {
                            delete_filter = true;
                            needs_recalc = true;
                        }
                        ui.same_line_with_spacing(0.0, 1.0);
                        if ui.button("+") {
                            add_filter = true;
                            needs_recalc = true;
                        }

                        // Type
                        ui.table_next_column();
                        let _width_tok = ui.push_item_width(-1.0);
                        let mut filter_type = filter.filter_type as usize;
                        if ui.combo_simple_string("##type", &mut filter_type, &self.filter_types) {
                            filter.filter_type = FilterType::iter().nth(filter_type).unwrap();
                            needs_recalc = true;
                        }

                        // Freq
                        ui.table_next_column();
                        let _width_tok = ui.push_item_width(-1.0);
                        needs_recalc |= ui
                            .input_double_config("##freq")
                            .step(10.0)
                            .step_fast(100.0)
                            .build(&mut filter.frequency);

                        // Gain
                        ui.table_next_column();
                        let _width_tok = ui.push_item_width(-1.0);
                        needs_recalc |= ui
                            .slider_config("##gain", -12.0, 12.0)
                            .build(&mut filter.gain);

                        // Q
                        ui.table_next_column();
                        let _width_tok = ui.push_item_width(-1.0);
                        needs_recalc |= ui
                            .input_double_config("##q")
                            .step(0.1)
                            .step(1.0)
                            .build(&mut filter.q);
                        filter.q = f64::max(filter.q, 0.1);
                    }
                }

                if delete_filter {
                    self.eq.delete_selected_filter();
                }

                if add_filter {
                    self.eq.add_filter();
                }
            });

        if ui.button("+") {
            self.eq.selected_idx = self.eq.filters.len();
            self.eq.add_filter();
            needs_recalc = true;
        }

        if needs_recalc {
            self.recalc_curve();
            self.should_sync_all = true;
        }

        table_hovered
    }

    fn draw_curve(&mut self, _ui: &Ui, plot_ui: &PlotUi, table_hovered: bool) {
        if self.curve_y.is_empty() {
            return;
        }

        if let Some(_tok) = plot_ui.begin_plot("Frequency response") {
            let axis_flags = AxisFlags::LOCK_MIN | AxisFlags::LOCK_MAX | AxisFlags::NO_MENUS;

            plot_ui.setup_axes(Some("Hz"), Some("dB"), axis_flags, axis_flags);
            plot_ui.setup_x_axis_scale(XAxis::X1, 2); // ImPlotScale_Log10

            let y_pad = (self.range_y.end - self.range_y.start) * 0.05;
            plot_ui.setup_axes_limits(
                self.curve_x[0],
                *self.curve_x.last().unwrap(),
                self.range_y.start - y_pad,
                self.range_y.end + y_pad,
                PlotCond::Always,
            );

            let _ = plot_ui.line_plot("", &self.curve_x, &self.curve_y);

            if table_hovered && self.eq.selected_idx < self.eq.filters.len() {
                let freq = self.eq.filters[self.eq.selected_idx].frequency;
                let lines = [freq];
                let _ = plot_ui.inf_lines_vertical("##hovered", &lines);
            }
        }
    }

    pub fn draw_window(&mut self, ui: &Ui, plot_ui: &PlotUi, sample_rate: u32) {
        self.sample_rate = sample_rate;
        ui.window("Filter")
            .size([600.0, 700.0], Condition::FirstUseEver)
            .flags(WindowFlags::NO_RESIZE)
            .build(|| {
                // Status text
                ui.text(format!(
                    "PipeWire EQ: {} | Bands: {}/{} | Sample Rate: {} Hz",
                    truncate_string(self.eq.name.as_str(), 16),
                    self.eq.filters.len(),
                    self.eq.max_filters,
                    self.sample_rate,
                ));
                ui.same_line();
                ui.separator_vertical();
                ui.same_line();
                right_aligned_checkbox(ui, "Bypass", &mut self.bypass);
                if ui.io().key_ctrl() && ui.is_key_pressed(dear_imgui_rs::Key::B) {
                    self.bypass = !self.bypass;
                }
                if self.eq.bypassed != self.bypass {
                    self.eq.bypassed = self.bypass;
                    self.should_sync_all = true;
                }
                ui.separator_horizontal();

                {
                    let _tok_bypass = ui.begin_disabled_with_cond(self.bypass); 

                    // Preamp
                    {
                        let _tok_preamp = ui.begin_disabled_with_cond(!self.preamp_enable);
                        ui.text("Preamp (dB):");
                        ui.same_line();
                        if ui.slider_config("##preamp", -10.0_f64, 10.0_f64)
                            .build(&mut self.preamp) {
                                self.eq.preamp = self.preamp;
                                self.should_sync_preamp = true;
                            }
                        ui.same_line();
                    }
                    if right_aligned_checkbox(ui, "Enable", &mut self.preamp_enable) {
                        if self.preamp_enable {
                            self.eq.preamp = self.preamp;
                        }
                        else {
                            self.eq.preamp = 0.0;
                        }
                        self.should_sync_preamp = true;
                    }

                    // Filter table
                    let mut table_hovered = false;
                    ui.child_window("Filters")
                        .border(false)
                        .size([-1.0, 300.0])
                        .build(ui, || {
                            table_hovered = self.draw_filters(ui);
                        });

                    // Freq response curve
                    ui.child_window("Curve")
                        .border(false)
                        .size([-1.0, 300.0])
                        .build(ui, || {
                            self.draw_curve(ui, plot_ui, table_hovered);
                        });
                }
            });
    }
}

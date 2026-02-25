use dear_imgui_rs::{Condition, ListClipper, TableColumnSetup, TableFlags, Ui, WindowFlags};
use pw_eq::tui::{
    Notif,
    autoeq::{AutoEqBrowser, ParametricEq},
};
use tokio::sync::mpsc;

pub struct AutoEqWindowState {
    #[allow(dead_code)]
    pub show_window: bool,
    search_text: String,
    autoeq_browser: AutoEqBrowser,
    selected: Option<i32>,
    status_text: String,
    http_client: reqwest::Client,
    notifs_tx: mpsc::Sender<Notif>,
    eq_to_set: Option<(String, ParametricEq)>,
}

impl AutoEqWindowState {
    pub fn new(notifs_tx: mpsc::Sender<Notif>) -> Self {
        Self {
            show_window: false,
            search_text: String::new(),
            autoeq_browser: AutoEqBrowser::default(),
            status_text: String::default(),
            selected: None,
            http_client: reqwest::Client::new(),
            notifs_tx,
            eq_to_set: None,
        }
    }

    pub fn auto_eq_db_loaded(
        &mut self,
        entries: autoeq_api::Entries,
        targets: Vec<autoeq_api::Target>,
    ) {
        self.autoeq_browser.on_data_loaded(entries, targets);
        self.status_text = String::default();
    }

    pub fn auto_eq_loaded(&mut self, name: String, response: ParametricEq) {
        self.eq_to_set = Some((name, response));
        self.status_text = String::default();
    }

    pub fn get_eq_to_set(&mut self) -> Option<(String, ParametricEq)> {
        self.eq_to_set.take()
    }

    // Returns name and parametric eq filter if one was applied in the UI
    pub fn draw_window(&mut self, ui: &Ui, sample_rate: u32) {
        if !self.autoeq_browser.loading && self.autoeq_browser.entries.is_none() {
            self.autoeq_browser
                .load_data(self.http_client.clone(), self.notifs_tx.clone());
            self.status_text = String::from("Loading AutoEQ DB ...");
        }

        let columns = [
            TableColumnSetup::new("Headphone"),
            TableColumnSetup::new("Profile"),
            TableColumnSetup::new("Rig"),
        ];

        ui.window("AutoEQ")
            .size([500.0, 364.0], Condition::FirstUseEver)
            .flags(WindowFlags::NO_RESIZE)
            .build(|| {
                {
                    let _tok = ui.push_item_width(-1.0);
                    ui.text("Search:");
                    ui.same_line();
                    if ui
                        .input_text("##search", &mut self.search_text)
                        .hint("Headphone model")
                        .build()
                    {
                        self.autoeq_browser.filter_query = self.search_text.clone();
                        self.autoeq_browser.update_filtered_results();
                        self.selected = None;
                    }
                }

                let entries = &self.autoeq_browser.filtered_results;
                let clipper = ListClipper::new(entries.len() as _).begin(ui).iter();
                ui.table("##profiles")
                    .outer_size([-1.0, 280.0])
                    .flags(TableFlags::BORDERS | TableFlags::BORDERS_OUTER | TableFlags::SCROLL_Y)
                    .columns(columns)
                    .headers(true)
                    .build(|ui| {
                        for i in clipper {
                            let (name, entry) = &entries[i as usize];
                            let row_id = format!("{} -> {} -> {:?}", name, entry.source, entry.rig);

                            {
                                ui.table_next_row();
                                ui.table_next_column();
                                let _id = ui.push_id(&row_id);
                                let selected = ui
                                    .selectable_config(&name)
                                    .selected(self.selected == Some(i))
                                    .allow_double_click(true)
                                    .span_all_columns(true)
                                    .build();
                                if selected {
                                    self.selected = Some(i);
                                    self.autoeq_browser.selected_index = i as usize;
                                }
                                ui.table_next_column();
                                ui.text(entry.source.as_str());
                                ui.table_next_column();
                                ui.text(entry.rig.as_deref().unwrap_or(""));
                            }
                        }
                    });

                {
                    let _tok = ui.begin_disabled_with_cond(self.selected.is_none());
                    if ui.button("Apply") {
                        let i = self.selected.unwrap() as usize;
                        self.autoeq_browser.apply_selected(
                            self.http_client.clone(),
                            self.notifs_tx.clone(),
                            sample_rate,
                        );
                        self.status_text = format!(
                            "Downloading filter: {} ...",
                            self.autoeq_browser.filtered_results[i].0
                        );
                    }
                    ui.same_line();
                    ui.text(self.status_text.as_str());
                }
            });
    }
}


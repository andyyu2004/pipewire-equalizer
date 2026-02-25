use std::{fs, path::{Path, PathBuf}};

use futures_executor::block_on;
use pw_eq::tui::{
    eq::Eq,
    Format
};

use dear_imgui_rs::{Condition, Key, Ui, WindowFlags};

use pw_util::apo::Config;
use tracing::instrument::WithSubscriber;

const LAST_SAVED_FILE_PATH: &str = "pw-eq-imgui/last-saved";
const DEFAULT_SAVE_PATH: &str = "pw-eq-imgui/config.apo";

fn path_to_string(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    let home = dirs::home_dir();
    let result = home.as_deref()
        .and_then(|p| p.to_str())
        .and_then(|prefix| s.strip_prefix(prefix))
        .map(|stripped| format!("~/{}", stripped.trim_start_matches('/')))
        .unwrap_or_else(|| s.to_string());
    Some(result)
}

fn str_to_path(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .unwrap_or_default()
            .join(rest),
        None => PathBuf::from(s),
    }
}

pub struct SaveLoadWindowState {
    pub show_window: bool,
    conf_to_load: Option<Config>,
    path: PathBuf,
    last_saved_path: PathBuf,
    result: anyhow::Result<()>,
}

impl SaveLoadWindowState {
    pub fn new() -> Self {
        let mut save_path = dirs::config_dir().unwrap();
        save_path.push(DEFAULT_SAVE_PATH);

        // Preload last saved path if possible
        let mut last_saved_path = dirs::config_dir().unwrap();
        last_saved_path.push(LAST_SAVED_FILE_PATH);
        if last_saved_path.exists() {
            match fs::read_to_string(last_saved_path.clone()) {
                Ok(content) => {
                    if Path::new(&content).exists() {
                        save_path = PathBuf::from(&content);
                    }
                },
                _ => (),
            }
        }

        Self {
            show_window: false,
            conf_to_load: None,
            path: save_path,
            last_saved_path: last_saved_path,
            result: Ok(()),
        }
    }

    pub fn loaded_conf(&mut self) -> Option<Config> {
        self.conf_to_load.take()
    }

    pub fn path_filename(&self) -> Option<&str> {
        self.path.file_name().map(|s| s.to_str()).flatten()
    }

    pub fn draw_window(&mut self, ui: &Ui, eq: &Eq) {
        let mut show_window = self.show_window;
        ui.window("Save/Load")
            .opened(&mut show_window)
            .size([500.0, 100.0], Condition::FirstUseEver)
            .flags(WindowFlags::NO_RESIZE)
            .build(|| {
                ui.text("File path:");

                if let Some(mut path_string) = path_to_string(&self.path) {
                    let _width_tok = ui.push_item_width(-1.0);
                    if ui.input_text("##path", &mut path_string).build() {
                        self.path = str_to_path(&path_string);
                        if !self.result.is_ok() {
                            self.result = anyhow::Result::Ok(());
                        }
                    }
                }
                else {
                    ui.text("Invalid Path!");
                }

                let ext = self.path.extension().and_then(|e| e.to_str());
                let valid_ext = ext == Some("apo");
                let format = ext.map(|e| {
                    match e {
                        "apo" => Format::Apo,
                        _ => Format::PwParamEq,
                    }
                });

                // Save button
                {
                    let _enable_tok = ui.begin_disabled_with_cond(!valid_ext);
                    let key_shortcut = ui.io().key_ctrl() && ui.is_key_pressed(Key::S);
                    if ui.button("Save") || key_shortcut {
                        let eq_clone = eq.clone();
                        self.result = block_on(eq_clone.save_config(self.path.clone(), format.unwrap()));
                        if self.result.is_ok() {
                            // Not a big deal if this fails, just convience to load last saved file next time
                            let _ = std::fs::write(&self.last_saved_path, self.path.to_str().unwrap());
                            self.show_window = false;
                        }
                    }
                    ui.same_line();
                }

                // Load button
                {
                    let _enable_tok = ui.begin_disabled_with_cond(!valid_ext || !self.path.exists());
                    let key_shortcut = ui.io().key_ctrl() && ui.is_key_pressed(Key::L);
                    if ui.button("Load") || key_shortcut {
                        match block_on(Config::parse_file(&self.path)) {
                            Err(e) => {
                                self.result = Err(anyhow::anyhow!("unable to parse apo file: {}", e));
                            },
                            Ok(apo) => {
                                self.conf_to_load = Some(apo);
                                self.result = Ok(());
                                self.show_window = false;
                            }
                        }
                    }
                    ui.same_line();
                }

                let status_text = match (&self.result, valid_ext, self.path.exists()) {
                    (Err(e), _, _) => format!("File save error: {}", e),
                    (Ok(_), false, _) => "Invalid extension - must be .apo".to_string(),
                    (Ok(_), true, true) => "File already exists".to_string(),
                    (Ok(_), true, false) => "File doesn't exist yet".to_string(),
                };

                ui.text(status_text);
            });
        
        if !show_window {
            self.show_window = false;
        }
    }
}
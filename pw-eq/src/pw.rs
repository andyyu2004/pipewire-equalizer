use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io;
use std::sync::Mutex;

use pw_util::api;
use pw_util::config::ModuleArgs;
use pw_util::pipewire::{self, context::ContextRc, main_loop::MainLoopRc};
use tokio::sync::mpsc;

use crate::tui::Notif;

pub enum Message {
    Terminate,
    LoadModule { name: String, args: Box<ModuleArgs> },
}

pub fn pw_thread(
    notifs: mpsc::Sender<Notif>,
    pw_receiver: pipewire::channel::Receiver<Message>,
) -> io::Result<()> {
    let mainloop = MainLoopRc::new(None).map_err(io::Error::other)?;
    let context = ContextRc::new(&mainloop, None).map_err(io::Error::other)?;

    // Lazy-load modules per band count (1-20 bands)
    // Each band count gets its own module that stays loaded
    // Dropping modules causes playback to pause, so we keep them around
    let modules: Mutex<HashMap<usize, api::ImplModule>> = Mutex::new(HashMap::new());

    let _receiver = pw_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let context = context.clone();
        move |msg| match msg {
            Message::Terminate => mainloop.quit(),
            Message::LoadModule { name, args } => {
                // FIXME this count isn't necessary accurate if we use the param_eq config
                let band_count = args.filter_graph.nodes.len();
                let spa_json_args = pw_util::to_spa_json(&args);
                tracing::error!(spa_json_args = spa_json_args, "Loading module with args");

                let mut modules = modules.lock().unwrap();

                let module = match modules.entry(band_count) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        tracing::info!(band_count, "Loading new module for band count");
                        let module = match api::load_module(&context, &name, &spa_json_args) {
                            Ok(module) => module,
                            Err(err) => {
                                let _ = notifs.blocking_send(Notif::Error(err));
                                return;
                            }
                        };

                        entry.insert(module)
                    }
                };

                let info = module.info();

                let _ = notifs.blocking_send(Notif::ModuleLoaded {
                    id: info.id(),
                    name: info.name().to_string(),
                    media_name: args.media_name.clone(),
                });
            }
        }
    });

    mainloop.run();
    Ok(())
}

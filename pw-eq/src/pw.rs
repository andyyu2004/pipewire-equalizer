use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io;
use std::sync::Mutex;

use pw_util::api;
use pw_util::pipewire::{self, context::ContextRc, main_loop::MainLoopRc};
use tokio::sync::mpsc;

use crate::tui::Notif;

pub enum Message {
    Terminate,
    LoadModule {
        name: String,
        args: String,
        band_count: usize,
    },
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
            Message::LoadModule {
                name,
                args,
                band_count,
            } => {
                let mut modules = modules.lock().unwrap();

                // Check if we already have a module for this band count
                if let Entry::Vacant(e) = modules.entry(band_count) {
                    tracing::info!(band_count, "Loading new module for band count");
                    let module = match api::load_module(&context, &name, &args) {
                        Ok(module) => module,
                        Err(err) => {
                            let _ = notifs.blocking_send(Notif::Error(err));
                            return;
                        }
                    };

                    let info = module.info();
                    let _ = notifs.blocking_send(Notif::ModuleLoaded {
                        id: info.id(),
                        name: info.name().to_string(),
                    });

                    e.insert(module);
                } else {
                    tracing::info!(band_count, "Reusing existing module for band count");
                }

                // TODO: Set this module's sink as default and update params
            }
        }
    });

    mainloop.run();
    Ok(())
}

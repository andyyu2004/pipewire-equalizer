use std::io;
use std::sync::Mutex;

use pw_util::api;
use pw_util::pipewire::{self, context::ContextRc, main_loop::MainLoopRc};
use tokio::sync::mpsc;

use crate::tui::Notif;

pub enum Message {
    Terminate,
    LoadModule { name: String, args: String },
}

pub fn pw_thread(
    notifs: mpsc::Sender<Notif>,
    pw_receiver: pipewire::channel::Receiver<Message>,
) -> io::Result<()> {
    let mainloop = MainLoopRc::new(None).map_err(io::Error::other)?;
    let context = ContextRc::new(&mainloop, None).map_err(io::Error::other)?;

    // Store the active module to keep it alive, dropping previous one on load of a new module.
    let active_module: Mutex<Option<api::ImplModule>> = Mutex::new(None);

    let _receiver = pw_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let context = context.clone();
        move |msg| match msg {
            Message::Terminate => mainloop.quit(),
            Message::LoadModule { name, args } => {
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
                *active_module.lock().unwrap() = Some(module);
            }
        }
    });

    mainloop.run();
    Ok(())
}

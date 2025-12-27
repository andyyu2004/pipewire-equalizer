use std::io;

use pw_util::api;
use pw_util::pipewire::{self, context::ContextRc, main_loop::MainLoopRc};
use tokio::sync::mpsc;

use crate::tui::Notification;

pub enum Message {
    Terminate,
    LoadModule { name: String },
}

pub fn pw_thread(
    notifs: mpsc::Sender<Notification>,
    pw_receiver: pipewire::channel::Receiver<Message>,
) -> io::Result<()> {
    let mainloop = MainLoopRc::new(None).map_err(io::Error::other)?;
    let context = ContextRc::new(&mainloop, None).map_err(io::Error::other)?;

    let _receiver = pw_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let context = context.clone();
        move |msg| match msg {
            Message::Terminate => mainloop.quit(),
            Message::LoadModule { name } => {
                let module = match api::load_module(&context, &name, "") {
                    Ok(module) => module,
                    Err(err) => {
                        let _ = notifs.blocking_send(Notification::Error(err));
                        return;
                    }
                };

                dbg!(module.info());
            }
        }
    });

    mainloop.run();
    Ok(())
}

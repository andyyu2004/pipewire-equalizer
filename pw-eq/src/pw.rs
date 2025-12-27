use pw_util::pipewire::{self, main_loop::MainLoopRc};

pub enum Message {
    Terminate,
}

pub fn pw_thread(pw_receiver: pipewire::channel::Receiver<Message>) {
    let mainloop = MainLoopRc::new(None).expect("Failed to create main loop");

    let _receiver = pw_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |msg| match msg {
            Message::Terminate => mainloop.quit(),
        }
    });

    mainloop.run();
}

use std::thread::{self, JoinHandle};

use pw_eq::{pw, tui::Notif};
use pw_util::NodeInfo;
use tokio::sync::mpsc;

use crate::autoeq::AutoEqWindowState;

pub struct PipewireState {
    pub notifs_tx: mpsc::Sender<Notif>,
    pub pw_tx: pipewire::channel::Sender<pw::Message>,
    pub sample_rate: u32,
    notifs_rx: mpsc::Receiver<Notif>,
    pw_handle: Option<JoinHandle<anyhow::Result<()>>>,
}

impl PipewireState {
    pub fn new(default_audio_sink: Option<NodeInfo>) -> Self {
        let (pw_tx, rx) = pipewire::channel::channel();
        let (notifs_tx, notifs_rx) = mpsc::channel(100);
        let pw_notifs_tx = notifs_tx.clone();
        let pw_handle =
            thread::spawn(|| pw_eq::pw::pw_thread(pw_notifs_tx, rx, default_audio_sink));

        Self {
            notifs_tx,
            pw_tx,
            sample_rate: 48000,
            notifs_rx,
            pw_handle: Some(pw_handle),
        }
    }

    pub fn close(&mut self) {
        let _ = self.pw_tx.send(pw::Message::Terminate);

        if let Some(handle) = self.pw_handle.take() {
            match handle.join() {
                Ok(Ok(())) => tracing::info!("PipeWire thread exited cleanly"),
                Ok(Err(err)) => tracing::error!(error = &*err, "PipeWire thread exited with error"),
                Err(err) => tracing::error!(error = ?err, "PipeWire thread panicked"),
            }
        }
    }

    pub fn update(&mut self, autoeq_window: &mut AutoEqWindowState) {
        if let Ok(notif) = self.notifs_rx.try_recv() {
            match notif {
                Notif::AutoEqDbLoaded { entries, targets } => {
                    autoeq_window.auto_eq_db_loaded(entries, targets);
                },
                Notif::AutoEqLoaded { name, response } => {
                    autoeq_window.auto_eq_loaded(name, response);
                },
                Notif::PwModuleLoaded { id, name, media_name: _ } => {
                    println!("Module loaded id {}, name {}", id, name);
                },
                _ => (),
            }
        }
    }
}
use std::thread::{self, JoinHandle};

use futures_executor::block_on;
use pw_eq::{pw, tui::Notif};
use pw_util::NodeInfo;
use tokio::sync::mpsc;

use crate::{autoeq::AutoEqWindowState, filter::FilterWindowState};

pub struct PipewireState {
    pub notifs_tx: mpsc::Sender<Notif>,
    pub pw_tx: pipewire::channel::Sender<pw::Message>,
    pub sample_rate: u32,
    notifs_rx: mpsc::Receiver<Notif>,
    pw_handle: Option<JoinHandle<anyhow::Result<()>>>,
    active_node_id: Option<u32>,
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
            notifs_rx,
            sample_rate: 48000,
            active_node_id: None,
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

    // Needs to be called in more places as appropriate. See tui.rs for when.
    fn load_module(&mut self, filter_window: &mut FilterWindowState) {
        let pw_tx = self.pw_tx.clone();
        let args = filter_window.eq.to_module_args(self.sample_rate);

        let _ = pw_tx.send(pw::Message::LoadModule {
            name: "libpipewire-module-filter-chain".into(),
            args: Box::new(args),
        });
    }

    pub fn update(
        &mut self,
        filter_window: &mut FilterWindowState,
        autoeq_window: &mut AutoEqWindowState,
    ) {
        if let Some(node_id) = self.active_node_id {
            filter_window.sync(node_id);
        }

        if let Ok(notif) = self.notifs_rx.try_recv() {
            match notif {
                Notif::AutoEqDbLoaded { entries, targets } => {
                    autoeq_window.auto_eq_db_loaded(entries, targets);
                }
                Notif::AutoEqLoaded { name, response } => {
                    autoeq_window.auto_eq_loaded(name, response);
                    self.load_module(filter_window);
                }
                Notif::PwModuleLoaded {
                    id,
                    name,
                    media_name,
                } => {
                    println!("Module loaded id {}, name {}", id, name);
                    // Find the filter's output node (capture side) by media.name
                    let Ok(node) = block_on(pw_eq::find_eq_node(&media_name)).inspect_err(|err| {
                        tracing::error!(error = &**err, "failed to find EQ node");
                    }) else {
                        return;
                    };

                    let node_id = node.id;

                    filter_window.sync(node_id);

                    self.active_node_id = Some(node_id);
                    if let Err(err) = self.pw_tx.send(pw::Message::SetActiveNode(NodeInfo {
                        node_id,
                        node_name: media_name,
                        object_serial: node
                            .info
                            .props
                            .get("object.serial")
                            .and_then(|v| v.as_i64())
                            .expect("object.serial missing or malformed"),
                    })) {
                        tracing::error!(
                            error = ?err,
                            "failed to set active node"
                        );
                    }
                }
                _ => (),
            }
        }
    }
}

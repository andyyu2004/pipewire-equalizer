use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::rc::Rc;
use std::sync::Mutex;

use dashmap::DashMap;
use pipewire::metadata::Metadata;
use pipewire::types::ObjectType;
use pipewire::{self, context::ContextRc, main_loop::MainLoopRc};
use pw_util::module::ModuleArgs;
use pw_util::{NodeInfo, api};
use tokio::sync::mpsc;

use crate::tui::Notif;

#[derive(Debug, Clone)]
pub enum Message {
    Terminate,
    SetActiveNode(NodeInfo),
    LoadModule { name: String, args: Box<ModuleArgs> },
}

#[derive(Clone)]
struct AudioStreamInfo {
    node_id: u32,
    node_name: String,
    original_target_object: Option<String>,
}

#[derive(Clone)]
struct State {
    default_audio_sink: Option<NodeInfo>,
    metadata: Rc<Mutex<Option<Metadata>>>,
    active_node: Rc<Mutex<Option<NodeInfo>>>,
    audio_stream_nodes: Rc<DashMap<u32, AudioStreamInfo>>,
}

impl State {
    fn new(default_audio_sink: Option<NodeInfo>) -> Self {
        Self {
            default_audio_sink,
            metadata: Rc::new(Mutex::new(None)),
            active_node: Rc::new(Mutex::new(None)),
            audio_stream_nodes: Rc::new(DashMap::new()),
        }
    }

    fn route_stream_to_active_node(&self, stream_node: &AudioStreamInfo) {
        let metadata_opt = self.metadata.lock().unwrap();
        let active_node_opt = self.active_node.lock().unwrap();

        if let (Some(metadata), Some(node)) = (metadata_opt.as_ref(), active_node_opt.as_ref()) {
            do_route_stream(metadata, stream_node, &node.object_serial.to_string());
        }
    }

    fn route_all_streams_to_active_node(&self) {
        let metadata_opt = self.metadata.lock().unwrap();
        let active_node_opt = self.active_node.lock().unwrap();

        if let (Some(metadata), Some(node)) = (metadata_opt.as_ref(), active_node_opt.as_ref()) {
            for entry in self.audio_stream_nodes.iter() {
                do_route_stream(metadata, entry.value(), &node.object_serial.to_string());
            }
        }
    }

    fn cleanup(&self) {
        if let Some(metadata) = self.metadata.lock().unwrap().as_ref() {
            for entry in self.audio_stream_nodes.iter() {
                let stream_node = entry.value();
                let target = if stream_node.original_target_object.is_some() {
                    stream_node.original_target_object.clone()
                } else {
                    self.default_audio_sink
                        .as_ref()
                        .map(|sink| sink.object_serial.to_string())
                };

                if let Some(target) = target {
                    do_route_stream(metadata, stream_node, &target);
                }
            }
        }
    }
}

fn do_route_stream(metadata: &Metadata, stream_node: &AudioStreamInfo, target: &str) {
    metadata.set_property(
        stream_node.node_id,
        "target.object",
        Some("Spa:Id"),
        Some(target),
    );

    tracing::info!(
        stream_node_id = stream_node.node_id,
        stream = %stream_node.node_name,
        %target,
        "Routed stream to target"
    );
}

pub fn pw_thread(
    notifs: mpsc::Sender<Notif>,
    pw_receiver: pipewire::channel::Receiver<Message>,
    default_audio_sink: Option<NodeInfo>,
) -> anyhow::Result<()> {
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    let st = State::new(default_audio_sink);

    // Listen for any `Stream/Output/Audio` nodes and attach them to our sink by
    // setting `target.object` using the default metadata object.
    let metadata_registry = registry.clone();

    let _node_listener = registry
        .add_listener_local()
        .global({
            let st = st.clone();
            move |obj| match obj.type_ {
                ObjectType::Metadata => {
                    // Find the "default" metadata object
                    if obj
                        .props
                        .is_none_or(|props| props.get("metadata.name") != Some("default"))
                    {
                        return;
                    }

                    match metadata_registry.bind::<Metadata, _>(obj) {
                        Ok(metadata) => *st.metadata.lock().unwrap() = Some(metadata),
                        Err(err) => {
                            tracing::error!(?err, "Failed to bind to metadata object");
                            return;
                        }
                    };

                    tracing::info!(id = obj.id, "Bound to default metadata object");
                }
                ObjectType::Node => {
                    let Some(stream_info) = obj.props.as_ref().and_then(|props| {
                        let node_id = obj.id;
                        let node_name = props.get("node.name")?;
                        let media_class = props.get("media.class")?;
                        let original_target_object =
                            props.get("target.object").map(|s| s.to_string());

                        (media_class == "Stream/Output/Audio" && !node_name.contains("pw-eq")).then(
                            || {
                                tracing::info!(
                                    node_id = node_id,
                                    %node_name,
                                    ?props,
                                    "Detected audio stream node"
                                );
                                AudioStreamInfo {
                                    node_id,
                                    node_name: node_name.to_string(),
                                    original_target_object,
                                }
                            },
                        )
                    }) else {
                        return;
                    };

                    st.audio_stream_nodes
                        .insert(stream_info.node_id, stream_info.clone());

                    st.route_stream_to_active_node(&stream_info);
                }
                _ => {}
            }
        })
        .register();

    // Lazy-load modules per filter count as there is no way to dynamically change the number of
    // filters in an existing module.
    let modules: Mutex<HashMap<usize, api::ImplModule>> = Mutex::new(HashMap::new());

    let _receiver = pw_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let context = context.clone();
        let state = st.clone();
        move |msg| match msg {
            Message::Terminate => {
                state.cleanup();
                mainloop.quit();
            }
            Message::SetActiveNode(node_info) => {
                *state.active_node.lock().unwrap() = Some(node_info.clone());
                state.route_all_streams_to_active_node();
            }
            Message::LoadModule { name, args } => {
                // FIXME this count isn't necessary accurate if we use the param_eq config
                let band_count = args.filter_graph.nodes.len();
                let spa_json_args = pw_util::to_spa_json(&args);

                let mut modules = modules.lock().unwrap();

                let (module, reused) = match modules.entry(band_count) {
                    Entry::Occupied(entry) => (entry.into_mut(), true),
                    Entry::Vacant(entry) => {
                        tracing::info!(band_count, "Loading new module for band count");
                        let module = match api::load_module(&context, &name, &spa_json_args) {
                            Ok(module) => module,
                            Err(err) => {
                                let _ = notifs.blocking_send(Notif::Error(err));
                                return;
                            }
                        };

                        (entry.insert(module), false)
                    }
                };

                let info = module.info();
                let _ = notifs.blocking_send(Notif::ModuleLoaded {
                    id: info.id(),
                    name: info.name().to_string(),
                    media_name: args.media_name.clone(),
                    reused,
                });
            }
        }
    });

    mainloop.run();
    Ok(())
}

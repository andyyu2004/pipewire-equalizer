use crate::apo;
use std::fmt;

// Property to mark nodes as managed by pw-eq
// Ensure this matches the field name in CaptureProps
pub const MANAGED_PROP: &str = "pweq.managed";
pub const BAND_PREFIX: &str = "pweq.band";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    #[serde(rename = "context.modules")]
    context_modules: Vec<Module>,
}

impl Config {
    pub fn from_apo(name: &str, apo: &apo::Config) -> Self {
        let nodes: Vec<Node> = apo
            .filters
            .iter()
            .map(|filter| Node {
                node_type: NodeType::Builtin,
                name: format!("{BAND_PREFIX}{}", filter.number),
                filter: filter.filter_type.into(),
                control: Control {
                    freq: filter.freq,
                    q: filter.q,
                    gain: filter.gain,
                },
            })
            .collect();

        let links: Vec<Link> = (0..apo.filters.len().saturating_sub(1))
            .map(|i| {
                let curr = &apo.filters[i];
                let next = &apo.filters[i + 1];
                Link {
                    output: format!("{BAND_PREFIX}{}:Out", curr.number),
                    input: format!("{BAND_PREFIX}{}:In", next.number),
                }
            })
            .collect();

        let audio_position = vec![AudioPosition::FrontLeft, AudioPosition::FrontRight];
        let module = Module {
            name: "libpipewire-module-filter-chain".to_string(),
            args: ModuleArgs {
                node_description: format!("{name} equalizer"),
                media_name: name.to_string(),
                audio_channels: audio_position.len(),
                audio_position,
                filter_graph: FilterGraph {
                    nodes: nodes.into_boxed_slice(),
                    links,
                },
                playback_props: PlaybackProps {
                    node_name: format!("effect_input.pweq.{name}"),
                    node_passive: false,
                },
                capture_props: CaptureProps {
                    node_name: format!("effect_output.pweq.{name}"),
                    media_class: "Audio/Sink".to_string(),
                    pweq_managed: true,
                },
            },
        };

        Config {
            context_modules: vec![module],
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Module {
    name: String,
    args: ModuleArgs,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModuleArgs {
    #[serde(rename = "node.description")]
    node_description: String,
    #[serde(rename = "media.name")]
    media_name: String,
    #[serde(rename = "filter.graph")]
    filter_graph: FilterGraph,
    #[serde(rename = "audio.channels")]
    audio_channels: usize,
    audio_position: Vec<AudioPosition>,
    #[serde(rename = "playback.props")]
    playback_props: PlaybackProps,
    #[serde(rename = "capture.props")]
    capture_props: CaptureProps,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlaybackProps {
    #[serde(rename = "node.name")]
    node_name: String,
    #[serde(rename = "node.passive")]
    node_passive: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CaptureProps {
    #[serde(rename = "node.name")]
    node_name: String,
    #[serde(rename = "media.class")]
    media_class: String,
    // Ensure this rename matches the constant MANAGED_PROP
    #[serde(rename = "pweq.managed")]
    pweq_managed: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AudioPosition {
    #[serde(rename = "FL")]
    FrontLeft,
    #[serde(rename = "FR")]
    FrontRight,
    #[serde(rename = "FC")]
    FrontCenter,
    #[serde(rename = "LFE")]
    LowFrequency,
    #[serde(rename = "SL")]
    SideLeft,
    #[serde(rename = "SR")]
    SideRight,
    #[serde(rename = "BL")]
    BackLeft,
    #[serde(rename = "BR")]
    BackRight,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilterGraph {
    nodes: Box<[Node]>,
    links: Vec<Link>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Node {
    #[serde(rename = "type")]
    node_type: NodeType,
    name: String,
    #[serde(rename = "label")]
    filter: FilterType,
    control: Control,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NodeType {
    #[serde(rename = "builtin")]
    Builtin,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum FilterType {
    #[serde(rename = "bq_peaking")]
    Peaking,
    #[serde(rename = "bq_lowshelf")]
    LowShelf,
    #[serde(rename = "bq_highshelf")]
    HighShelf,
}

impl From<apo::FilterType> for FilterType {
    fn from(ft: apo::FilterType) -> Self {
        match ft {
            apo::FilterType::Peaking => FilterType::Peaking,
            apo::FilterType::LowShelf => FilterType::LowShelf,
            apo::FilterType::HighShelf => FilterType::HighShelf,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Control {
    freq: f32,
    q: f32,
    gain: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Link {
    output: String,
    input: String,
}

/// Wrapper around serde_json::Value that formats as SPA JSON (PipeWire config format)
pub struct SpaJson<'a> {
    value: &'a serde_json::Value,
    indent: usize,
}

impl<'a> SpaJson<'a> {
    pub fn new(value: &'a serde_json::Value) -> Self {
        Self { value, indent: 0 }
    }

    fn with_indent(&self, indent: usize) -> Self {
        Self {
            value: self.value,
            indent,
        }
    }
}

impl fmt::Display for SpaJson<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            serde_json::Value::Object(map) => {
                writeln!(f, "{{")?;
                for (key, value) in map {
                    write!(f, "{:indent$}{} = ", "", key, indent = self.indent + 4)?;
                    write!(f, "{}", SpaJson::new(value).with_indent(self.indent + 4))?;
                    writeln!(f)?;
                }
                write!(f, "{:indent$}}}", "", indent = self.indent)?;
                Ok(())
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    write!(f, "[]")?;
                } else {
                    writeln!(f, "[")?;
                    for item in arr {
                        write!(f, "{:indent$}", "", indent = self.indent + 4)?;
                        write!(f, "{}", SpaJson::new(item).with_indent(self.indent + 4))?;
                        writeln!(f)?;
                    }
                    write!(f, "{:indent$}]", "", indent = self.indent)?;
                }
                Ok(())
            }
            serde_json::Value::String(s) => write!(f, "\"{}\"", s),
            serde_json::Value::Number(n) => write!(f, "{}", n),
            serde_json::Value::Bool(b) => write!(f, "{}", b),
            serde_json::Value::Null => write!(f, "null"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        apo::{self, FilterType},
        config::SpaJson,
    };
    use expect_test::expect;

    use super::Config;

    #[test]
    fn test_generate_config() {
        let config = apo::Config {
            preamp: Some(-1.9),
            filters: vec![
                apo::Filter {
                    number: 1,
                    enabled: true,
                    filter_type: FilterType::Peaking,
                    freq: 46.0,
                    gain: 0.8,
                    q: 2.9,
                },
                apo::Filter {
                    number: 2,
                    enabled: true,
                    filter_type: FilterType::LowShelf,
                    freq: 105.0,
                    gain: -0.3,
                    q: 0.667,
                },
            ],
        };

        let cfg = Config::from_apo("test-eq", &config);
        let out = SpaJson::new(&serde_json::to_value(&cfg).unwrap()).to_string();

        expect![[r#"
            {
                context.modules = [
                    {
                        name = "libpipewire-module-filter-chain"
                        args = {
                            node.description = "test-eq equalizer"
                            media.name = "test-eq"
                            filter.graph = {
                                nodes = [
                                    {
                                        type = "builtin"
                                        name = "pweq.band1"
                                        label = "bq_peaking"
                                        control = {
                                            Freq = 46.0
                                            Q = 2.9000000953674316
                                            Gain = 0.800000011920929
                                        }
                                    }
                                    {
                                        type = "builtin"
                                        name = "pweq.band2"
                                        label = "bq_lowshelf"
                                        control = {
                                            Freq = 105.0
                                            Q = 0.6669999957084656
                                            Gain = -0.30000001192092896
                                        }
                                    }
                                ]
                                links = [
                                    {
                                        output = "pweq.band1:Out"
                                        input = "pweq.band2:In"
                                    }
                                ]
                            }
                            audio.channels = 2
                            audio_position = [
                                "FL"
                                "FR"
                            ]
                            playback.props = {
                                node.name = "effect_input.pweq.test-eq"
                                node.passive = false
                            }
                            capture.props = {
                                node.name = "effect_output.pweq.test-eq"
                                media.class = "Audio/Sink"
                                pweq.managed = true
                            }
                        }
                    }
                ]
            }"#]]
        .assert_eq(&out);
    }
}

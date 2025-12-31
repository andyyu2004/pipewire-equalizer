use crate::apo;
use std::fmt;

// Property to mark nodes as managed by pw-eq
// Ensure this matches the field name in CaptureProps
pub const MANAGED_PROP: &str = "pweq.managed";
pub const FILTER_PREFIX: &str = "pweq.filter_";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    #[serde(rename = "context.modules")]
    pub context_modules: Vec<Module>,
}

impl Config {
    pub fn from_kinds(name: &str, preamp: f64, kinds: impl IntoIterator<Item = NodeKind>) -> Self {
        Config {
            context_modules: vec![Module::from_kinds(name, preamp, kinds)],
        }
    }

    pub fn from_apo(name: &str, apo: &apo::Config) -> Self {
        Config {
            context_modules: vec![Module::from_apo(name, apo)],
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Module {
    pub name: String,
    pub args: ModuleArgs,
}

impl Module {
    pub fn from_kinds(name: &str, preamp: f64, kinds: impl IntoIterator<Item = NodeKind>) -> Self {
        let mut kinds = kinds.into_iter().peekable();

        let preamp_node = Node {
            node_type: NodeType::Builtin,
            name: format!("{FILTER_PREFIX}preamp"),
            kind: NodeKind::HighShelf {
                control: Control {
                    // pipewire biquad high-shelf has a special case for freq=0 that applies gain uniformly
                    freq: 0.0,
                    q: 0.0,
                    gain: preamp,
                },
            },
        };

        let nodes: Vec<Node> = if let Some(NodeKind::ParamEq { config }) = kinds.peek() {
            // If using param_eq, integrate preamp into that node
            let mut filters = config.filters.clone();
            filters.insert(
                0,
                ParamEqFilter {
                    ty: FilterType::HighShelf,
                    control: Control {
                        freq: 0.0,
                        q: 0.0,
                        gain: preamp,
                    },
                },
            );
            let param_eq_node = Node {
                node_type: NodeType::Builtin,
                name: format!("{FILTER_PREFIX}1"),
                kind: NodeKind::ParamEq {
                    config: ParamEqConfig { filters },
                },
            };
            vec![param_eq_node]
        } else {
            std::iter::once(preamp_node)
                .chain(kinds.enumerate().map(|(i, kind)| Node {
                    node_type: NodeType::Builtin,
                    name: format!("{FILTER_PREFIX}{}", i + 1),
                    kind,
                }))
                .collect()
        };

        let links: Vec<Link> = (0..nodes.len().saturating_sub(1))
            .map(|i| Link {
                output: format!("{}:Out", nodes[i].name),
                input: format!("{}:In", nodes[i + 1].name),
            })
            .collect();

        let audio_position = vec![AudioPosition::FrontLeft, AudioPosition::FrontRight];
        Module {
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
        }
    }

    pub fn from_apo(name: &str, apo: &apo::Config) -> Self {
        let kinds = apo.filters.iter().map(|filter| {
            let control = Control {
                freq: filter.frequency,
                q: filter.q,
                gain: filter.gain,
            };
            match filter.filter_type {
                FilterType::Peaking => NodeKind::Peaking { control },
                FilterType::LowShelf => NodeKind::LowShelf { control },
                FilterType::HighShelf => NodeKind::HighShelf { control },
            }
        });

        Self::from_kinds(name, apo.preamp, kinds)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModuleArgs {
    #[serde(rename = "node.description")]
    pub node_description: String,
    #[serde(rename = "media.name")]
    pub media_name: String,
    #[serde(rename = "filter.graph")]
    pub filter_graph: FilterGraph,
    #[serde(rename = "audio.channels")]
    pub audio_channels: usize,
    pub audio_position: Vec<AudioPosition>,
    #[serde(rename = "playback.props")]
    pub playback_props: PlaybackProps,
    #[serde(rename = "capture.props")]
    pub capture_props: CaptureProps,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaybackProps {
    #[serde(rename = "node.name")]
    pub node_name: String,
    #[serde(rename = "node.passive")]
    pub node_passive: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaptureProps {
    #[serde(rename = "node.name")]
    pub node_name: String,
    #[serde(rename = "media.class")]
    pub media_class: String,
    // Ensure this rename matches the constant MANAGED_PROP
    #[serde(rename = "pweq.managed")]
    pub pweq_managed: bool,
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
    pub nodes: Box<[Node]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
}

// Make this an enum of bq_raw and param_eq
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Node {
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub name: String,
    #[serde(flatten)]
    pub kind: NodeKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "label")]
pub enum NodeKind {
    #[serde(rename = "bq_peaking")]
    Peaking { control: Control },
    #[serde(rename = "bq_lowshelf")]
    LowShelf { control: Control },
    #[serde(rename = "bq_highshelf")]
    HighShelf { control: Control },
    #[serde(rename = "bq_raw")]
    Raw { config: RawNodeConfig },
    #[serde(rename = "param_eq")]
    ParamEq { config: ParamEqConfig },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamEqConfig {
    pub filters: Vec<ParamEqFilter>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamEqFilter {
    #[serde(rename = "type")]
    pub ty: FilterType,
    #[serde(flatten)]
    pub control: Control,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawNodeConfig {
    pub coefficients: Vec<RateAndBiquadCoefficients>,
}

/// Sample rate mapped to biquad coefficients
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RateAndBiquadCoefficients {
    pub rate: u32,
    #[serde(flatten)]
    pub coefficients: BiquadCoefficients,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
/// Normalized biquad coefficients, with a0 = 1.0
pub struct BiquadCoefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NodeType {
    #[serde(rename = "builtin")]
    Builtin,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FilterType {
    #[serde(rename = "bq_lowshelf")]
    LowShelf,
    #[serde(rename = "bq_peaking")]
    Peaking,
    #[serde(rename = "bq_highshelf")]
    HighShelf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Control {
    pub freq: f64,
    pub q: f64,
    pub gain: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Link {
    pub output: String,
    pub input: String,
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
        apo::{self},
        module::{
            BiquadCoefficients, Control, FilterType, NodeKind, ParamEqConfig, ParamEqFilter,
            RateAndBiquadCoefficients, RawNodeConfig,
        },
        to_spa_json,
    };
    use expect_test::expect;

    use super::Config;

    #[test]
    fn test_generate_config_from_raw() {
        let out = to_spa_json(&Config::from_kinds(
            "test-eq",
            0.0,
            [NodeKind::Raw {
                config: RawNodeConfig {
                    coefficients: vec![RateAndBiquadCoefficients {
                        rate: 48000,
                        coefficients: BiquadCoefficients {
                            b0: 0.0,
                            b1: 0.1,
                            b2: 0.2,
                            a1: 0.3,
                            a2: 0.4,
                        },
                    }],
                },
            }],
        ));

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
                                        name = "pweq.filter_preamp"
                                        label = "bq_highshelf"
                                        control = {
                                            Freq = 0.0
                                            Q = 0.0
                                            Gain = 0.0
                                        }
                                    }
                                    {
                                        type = "builtin"
                                        name = "pweq.filter_1"
                                        label = "bq_raw"
                                        config = {
                                            coefficients = [
                                                {
                                                    rate = 48000
                                                    b0 = 0.0
                                                    b1 = 0.1
                                                    b2 = 0.2
                                                    a1 = 0.3
                                                    a2 = 0.4
                                                }
                                            ]
                                        }
                                    }
                                ]
                                links = [
                                    {
                                        output = "pweq.filter_preamp:Out"
                                        input = "pweq.filter_1:In"
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

    #[test]
    fn test_generate_config_from_param_eq() {
        let out = to_spa_json(&Config::from_kinds(
            "param-eq",
            -4.2,
            [NodeKind::ParamEq {
                config: ParamEqConfig {
                    filters: vec![
                        ParamEqFilter {
                            ty: FilterType::LowShelf,
                            control: Control {
                                freq: 200.0,
                                q: 0.707,
                                gain: -6.0,
                            },
                        },
                        ParamEqFilter {
                            ty: FilterType::Peaking,
                            control: Control {
                                freq: 1000.0,
                                q: 1.0,
                                gain: 3.0,
                            },
                        },
                    ],
                },
            }],
        ));

        // The preamp should be added into the param-eq node rather than using a separate node
        expect![[r#"
            {
                context.modules = [
                    {
                        name = "libpipewire-module-filter-chain"
                        args = {
                            node.description = "param-eq equalizer"
                            media.name = "param-eq"
                            filter.graph = {
                                nodes = [
                                    {
                                        type = "builtin"
                                        name = "pweq.filter_1"
                                        label = "param_eq"
                                        config = {
                                            filters = [
                                                {
                                                    type = "bq_highshelf"
                                                    Freq = 0.0
                                                    Q = 0.0
                                                    Gain = -4.2
                                                }
                                                {
                                                    type = "bq_lowshelf"
                                                    Freq = 200.0
                                                    Q = 0.707
                                                    Gain = -6.0
                                                }
                                                {
                                                    type = "bq_peaking"
                                                    Freq = 1000.0
                                                    Q = 1.0
                                                    Gain = 3.0
                                                }
                                            ]
                                        }
                                    }
                                ]
                            }
                            audio.channels = 2
                            audio_position = [
                                "FL"
                                "FR"
                            ]
                            playback.props = {
                                node.name = "effect_input.pweq.param-eq"
                                node.passive = false
                            }
                            capture.props = {
                                node.name = "effect_output.pweq.param-eq"
                                media.class = "Audio/Sink"
                                pweq.managed = true
                            }
                        }
                    }
                ]
            }"#]]
        .assert_eq(&out);
    }

    #[test]
    fn test_generate_config_from_apo() {
        let config = apo::Config {
            preamp: -1.9,
            filters: vec![
                apo::Filter {
                    number: 1,
                    enabled: true,
                    filter_type: FilterType::Peaking,
                    frequency: 46.0,
                    gain: 0.8,
                    q: 2.9,
                },
                apo::Filter {
                    number: 2,
                    enabled: true,
                    filter_type: FilterType::LowShelf,
                    frequency: 105.0,
                    gain: -0.3,
                    q: 0.667,
                },
            ],
        };

        let out = to_spa_json(&Config::from_apo("test-eq", &config));

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
                                        name = "pweq.filter_preamp"
                                        label = "bq_highshelf"
                                        control = {
                                            Freq = 0.0
                                            Q = 0.0
                                            Gain = -1.9
                                        }
                                    }
                                    {
                                        type = "builtin"
                                        name = "pweq.filter_1"
                                        label = "bq_peaking"
                                        control = {
                                            Freq = 46.0
                                            Q = 2.9
                                            Gain = 0.8
                                        }
                                    }
                                    {
                                        type = "builtin"
                                        name = "pweq.filter_2"
                                        label = "bq_lowshelf"
                                        control = {
                                            Freq = 105.0
                                            Q = 0.667
                                            Gain = -0.3
                                        }
                                    }
                                ]
                                links = [
                                    {
                                        output = "pweq.filter_preamp:Out"
                                        input = "pweq.filter_1:In"
                                    }
                                    {
                                        output = "pweq.filter_1:Out"
                                        input = "pweq.filter_2:In"
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

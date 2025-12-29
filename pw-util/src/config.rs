use crate::apo;
use std::fmt;

// Property to mark nodes as managed by pw-eq
// Ensure this matches the field name in CaptureProps
pub const MANAGED_PROP: &str = "pweq.managed";
pub const FILTER_PREFIX: &str = "pweq.filter";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    #[serde(rename = "context.modules")]
    pub context_modules: Vec<Module>,
}

impl Config {
    pub fn from_kinds(name: &str, kinds: impl IntoIterator<Item = NodeKind>) -> Self {
        Config {
            context_modules: vec![Module::from_kinds(name, kinds)],
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
    pub fn from_kinds(name: &str, kinds: impl IntoIterator<Item = NodeKind>) -> Self {
        let nodes: Vec<Node> = kinds
            .into_iter()
            .enumerate()
            .map(|(i, kind)| Node {
                node_type: NodeType::Builtin,
                name: format!("{FILTER_PREFIX}{}", i + 1),
                kind,
            })
            .collect();
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
                freq: filter.freq,
                q: filter.q,
                gain: filter.gain,
            };
            match filter.filter_type {
                apo::FilterType::Peaking => NodeKind::Peaking { control },
                apo::FilterType::LowShelf => NodeKind::LowShelf { control },
                apo::FilterType::HighShelf => NodeKind::HighShelf { control },
            }
        });

        Self::from_kinds(name, kinds)
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
    filters: Vec<ParamEqFilter>,
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
        config::{BiquadCoefficients, NodeKind, RateAndBiquadCoefficients, RawNodeConfig},
        to_spa_json,
    };
    use expect_test::expect;

    use super::Config;

    #[test]
    fn test_generate_config_from_raw() {
        let out = to_spa_json(&Config::from_kinds(
            "test-eq",
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
                                        name = "pweq.filter1"
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
    fn test_generate_config_from_apo() {
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
                                        name = "pweq.filter1"
                                        label = "bq_peaking"
                                        control = {
                                            Freq = 46.0
                                            Q = 2.9000000953674316
                                            Gain = 0.800000011920929
                                        }
                                    }
                                    {
                                        type = "builtin"
                                        name = "pweq.filter2"
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
                                        output = "pweq.filter1:Out"
                                        input = "pweq.filter2:In"
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

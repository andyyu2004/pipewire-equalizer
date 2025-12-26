use crate::apo;
use std::fmt;

pub const MANAGED_PROP: &str = "pweq.managed";

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
                name: format!("eq_band_{}", filter.number),
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
                    output: format!("eq_band_{}:Out", curr.number),
                    input: format!("eq_band_{}:In", next.number),
                }
            })
            .collect();

        let module = Module {
            name: "libpipewire-module-filter-chain".to_string(),
            args: ModuleArgs {
                node_description: format!("{name} equalizer"),
                media_name: name.to_string(),
                filter_graph: FilterGraph {
                    nodes: nodes.into_boxed_slice(),
                    links,
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
                                        name = "eq_band_1"
                                        label = "bq_peaking"
                                        control = {
                                            Freq = 46.0
                                            Q = 2.9000000953674316
                                            Gain = 0.800000011920929
                                        }
                                    }
                                    {
                                        type = "builtin"
                                        name = "eq_band_2"
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
                                        output = "eq_band_1:Out"
                                        input = "eq_band_2:In"
                                    }
                                ]
                            }
                        }
                    }
                ]
            }"#]]
        .assert_eq(&out);
    }
}

use crate::apo;
use serde_json::json;
use std::fmt;

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

/// Generate a PipeWire filter-chain config from an AutoEQ .apo file
pub fn generate_filter_chain_config(name: &str, apo: &apo::Config) -> String {
    // Build nodes
    let nodes: Vec<_> = apo
        .filters
        .iter()
        .map(|filter| {
            json!({
                "type": "builtin",
                "name": format!("eq_band_{}", filter.number),
                "label": filter.filter_type.to_pipewire_label(),
                "control": {
                    "Freq": filter.freq,
                    "Q": filter.q,
                    "Gain": filter.gain
                }
            })
        })
        .collect();

    // Build links between bands
    let links: Vec<_> = (0..apo.filters.len().saturating_sub(1))
        .map(|i| {
            let curr = &apo.filters[i];
            let next = &apo.filters[i + 1];
            json!({
                "output": format!("eq_band_{}:Out", curr.number),
                "input": format!("eq_band_{}:In", next.number)
            })
        })
        .collect();

    let config_json = json!({
        "context.modules": [{
            "name": "libpipewire-module-filter-chain",
            "args": {
                "node.description": format!("{name} equalizer"),
                "media.name": name,
                "filter.graph": {
                    "nodes": nodes,
                    "links": links
                },
                "audio.channels": 2,
                "audio.position": ["FL", "FR"],
                "capture.props": {
                    "node.name": format!("effect_input.pweq_{name}"),
                    "media.class": "Audio/Sink",
                    "pweq.managed": true
                },
                "playback.props": {
                    "node.name": format!("effect_output.pweq_{name}"),
                    "node.passive": true
                }
            }
        }]
    });

    SpaJson::new(&config_json).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo::{self, FilterType};
    use expect_test::expect;

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

        let out = generate_filter_chain_config("test-eq", &config);

        expect![[r#"
            {
                context.modules = [
                    {
                        args = {
                            audio.channels = 2
                            audio.position = [
                                "FL"
                                "FR"
                            ]
                            capture.props = {
                                media.class = "Audio/Sink"
                                node.name = "effect_input.pweq_test-eq"
                                pweq.managed = true
                            }
                            filter.graph = {
                                links = [
                                    {
                                        input = "eq_band_2:In"
                                        output = "eq_band_1:Out"
                                    }
                                ]
                                nodes = [
                                    {
                                        control = {
                                            Freq = 46.0
                                            Gain = 0.800000011920929
                                            Q = 2.9000000953674316
                                        }
                                        label = "bq_peaking"
                                        name = "eq_band_1"
                                        type = "builtin"
                                    }
                                    {
                                        control = {
                                            Freq = 105.0
                                            Gain = -0.30000001192092896
                                            Q = 0.6669999957084656
                                        }
                                        label = "bq_lowshelf"
                                        name = "eq_band_2"
                                        type = "builtin"
                                    }
                                ]
                            }
                            media.name = "test-eq"
                            node.description = "test-eq"
                            playback.props = {
                                node.name = "effect_output.pweq_test-eq"
                                node.passive = true
                            }
                        }
                        name = "libpipewire-module-filter-chain"
                    }
                ]
            }"#]]
        .assert_eq(&out);
    }
}

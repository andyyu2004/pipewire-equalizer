use crate::apo::ApoConfig;
use anyhow::Result;

/// Generate a PipeWire filter-chain config from an AutoEQ .apo file
pub fn generate_filter_chain_config(name: &str, apo: &ApoConfig) -> Result<String> {
    let mut config = String::new();

    config.push_str("context.modules = [\n");
    config.push_str("    {\n");
    config.push_str("        name = libpipewire-module-filter-chain\n");
    config.push_str("        args = {\n");
    config.push_str(&format!("            node.description = \"{}\"\n", name));
    config.push_str(&format!("            media.name       = \"{}\"\n", name));
    config.push_str("            filter.graph = {\n");
    config.push_str("                nodes = [\n");

    // Generate nodes for each filter band
    for filter in &apo.filters {
        config.push_str("                    {\n");
        config.push_str("                        type  = builtin\n");
        config.push_str(&format!("                        name  = eq_band_{}\n", filter.number));
        config.push_str(&format!("                        label = {}\n", filter.filter_type.to_pipewire_label()));
        config.push_str(&format!(
            "                        control = {{ \"Freq\" = {} \"Q\" = {} \"Gain\" = {} }}\n",
            filter.freq, filter.q, filter.gain
        ));
        config.push_str("                    }\n");
    }

    config.push_str("                ]\n");

    // Generate links between bands
    if apo.filters.len() > 1 {
        config.push_str("                links = [\n");
        for i in 0..(apo.filters.len() - 1) {
            let curr = &apo.filters[i];
            let next = &apo.filters[i + 1];
            config.push_str(&format!(
                "                    {{ output = \"eq_band_{}:Out\" input = \"eq_band_{}:In\" }}\n",
                curr.number, next.number
            ));
        }
        config.push_str("                ]\n");
    }

    config.push_str("            }\n");
    config.push_str("            audio.channels = 2\n");
    config.push_str("            audio.position = [ FL FR ]\n");
    config.push_str("            capture.props = {\n");
    config.push_str(&format!("                node.name   = \"effect_input.pweq_{}\"\n", name));
    config.push_str("                media.class = Audio/Sink\n");
    config.push_str("                pweq.managed = true\n");
    config.push_str("            }\n");
    config.push_str("            playback.props = {\n");
    config.push_str(&format!("                node.name   = \"effect_output.pweq_{}\"\n", name));
    config.push_str("                node.passive = true\n");
    config.push_str("            }\n");
    config.push_str("        }\n");
    config.push_str("    }\n");
    config.push_str("]\n");

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo::{ApoFilter, FilterType};

    #[test]
    fn test_generate_config() {
        let apo = ApoConfig {
            preamp: Some(-1.9),
            filters: vec![
                ApoFilter {
                    number: 1,
                    enabled: true,
                    filter_type: FilterType::Peaking,
                    freq: 46.0,
                    gain: 0.8,
                    q: 2.9,
                },
                ApoFilter {
                    number: 2,
                    enabled: true,
                    filter_type: FilterType::LowShelf,
                    freq: 105.0,
                    gain: -0.3,
                    q: 0.667,
                },
            ],
        };

        let config = generate_filter_chain_config("test-eq", &apo).unwrap();

        assert!(config.contains("effect_input.pweq_test-eq"));
        assert!(config.contains("eq_band_1"));
        assert!(config.contains("eq_band_2"));
        assert!(config.contains("bq_peaking"));
        assert!(config.contains("bq_lowshelf"));
        assert!(config.contains("pweq.managed = true"));
        assert!(config.contains("eq_band_1:Out"));
    }
}

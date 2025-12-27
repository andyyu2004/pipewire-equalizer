use std::num::NonZero;

use anyhow::Context;
use pw_util::config::{BAND_PREFIX, MANAGED_PROP};
use tabled::Tabled;
use tokio::process::Command;

#[derive(Tabled)]
pub struct EqMeta {
    id: u32,
    name: String,
}

pub async fn list_eqs() -> anyhow::Result<Vec<EqMeta>> {
    let objects = pw_util::dump().await?;

    let eqs = objects
        .into_iter()
        .filter(is_managed_eq)
        .filter(|obj| matches!(obj.object_type, pw_util::PwObjectType::Node))
        .map(|obj| {
            let props = &obj.info.props;
            let name = props
                .get("media.name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            EqMeta {
                id: obj.id,
                name: name.to_string(),
            }
        })
        .collect();

    Ok(eqs)
}

pub fn is_managed_eq(props: &pw_util::PwDumpObject) -> bool {
    props
        .info
        .props
        .get(MANAGED_PROP)
        .is_some_and(|managed| managed == true)
}

/// Find an EQ node by profile name or ID
pub async fn find_eq_node(profile: &str) -> anyhow::Result<pw_util::PwDumpObject> {
    let objects = pw_util::dump().await?;

    // Try to parse as ID first
    let target_id: Option<u32> = profile.parse().ok();

    objects
        .into_iter()
        .filter(|obj| matches!(obj.object_type, pw_util::PwObjectType::Node))
        .filter(is_managed_eq)
        .find(|obj| {
            if let Some(target_id) = target_id {
                obj.id == target_id
            } else {
                let props = &obj.info.props;
                if let Some(name) = props.get("media.name") {
                    name == profile
                } else {
                    false
                }
            }
        })
        .ok_or_else(|| anyhow::anyhow!("EQ '{profile}' not found"))
}

pub async fn use_eq(profile: &str) -> anyhow::Result<u32> {
    let node = find_eq_node(profile).await?;
    pw_util::set_default(node.id).await?;
    Ok(node.id)
}

#[derive(Debug, Clone)]
pub struct UpdateBand {
    pub frequency: Option<f64>,
    pub gain: Option<f64>,
    pub q: Option<f64>,
}

pub async fn update_band(
    node_id: u32,
    band_idx: NonZero<usize>,
    UpdateBand { frequency, gain, q }: UpdateBand,
) -> anyhow::Result<()> {
    // Build the params array for pw-cli
    let mut params = Vec::new();

    if let Some(freq) = frequency {
        params.push(format!(r#""{BAND_PREFIX}{band_idx}:Freq""#));
        params.push(freq.to_string());
    }

    if let Some(gain_val) = gain {
        params.push(format!(r#""{BAND_PREFIX}{band_idx}:Gain""#));
        params.push(gain_val.to_string());
    }

    if let Some(q_val) = q {
        params.push(format!(r#""{BAND_PREFIX}{band_idx}:Q""#));
        params.push(q_val.to_string());
    }

    let output = Command::new("pw-cli")
        .arg("set-param")
        .arg(node_id.to_string())
        .arg("Props")
        .arg(format!("{{ params = [ {} ] }}", params.join(", ")))
        .output()
        .await
        .context("Failed to execute pw-cli")?;

    if !output.status.success() {
        anyhow::bail!("pw-cli failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}

#![recursion_limit = "256"]

pub mod filter;
pub mod pw;
pub mod tui;

use std::num::NonZero;

use anyhow::Context;
use pw_util::module::{BiquadCoefficients, FILTER_PREFIX, MANAGED_PROP};
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

#[derive(Debug, Clone)]
pub struct UpdateFilter {
    pub frequency: Option<f64>,
    pub gain: Option<f64>,
    pub q: Option<f64>,
    pub coeffs: Option<BiquadCoefficients>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FilterId {
    Preamp,
    Index(NonZero<usize>),
}

impl std::fmt::Display for FilterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterId::Preamp => write!(f, "preamp"),
            FilterId::Index(idx) => write!(f, "{idx}"),
        }
    }
}

impl std::str::FromStr for FilterId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("preamp") {
            Ok(FilterId::Preamp)
        } else {
            let idx: usize = s.parse().context("Invalid filter index")?;
            let nz_idx = NonZero::new(idx).context("Filter index must be non-zero")?;
            Ok(FilterId::Index(nz_idx))
        }
    }
}

/// Update multiple filter bands in a single pw-cli call
#[tracing::instrument(skip(updates))]
pub async fn update_filters(
    node_id: u32,
    updates: impl IntoIterator<Item = (FilterId, UpdateFilter)>,
) -> anyhow::Result<()> {
    let mut updates = updates.into_iter().peekable();
    if updates.peek().is_none() {
        tracing::warn!("no filter updates provided");
        return Ok(());
    }

    // Build the params array for pw-cli
    let mut params = Vec::new();

    for (filter_id, update) in updates {
        if let Some(freq) = update.frequency {
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:Freq""#));
            params.push(freq.to_string());
        }

        if let Some(gain_val) = update.gain {
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:Gain""#));
            params.push(gain_val.to_string());
        }

        if let Some(q_val) = update.q {
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:Q""#));
            params.push(q_val.to_string());
        }

        if let Some(BiquadCoefficients { b0, b1, b2, a1, a2 }) = update.coeffs {
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:b0""#));
            params.push(b0.to_string());
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:b1""#));
            params.push(b1.to_string());
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:b2""#));
            params.push(b2.to_string());
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:a1""#));
            params.push(a1.to_string());
            params.push(format!(r#""{FILTER_PREFIX}{filter_id}:a2""#));
            params.push(a2.to_string());
        }
    }

    tracing::trace!(?params, "updating filter parameters");

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

/// Update a single filter (convenience wrapper)
#[tracing::instrument(skip(update))]
pub async fn update_filter(
    node_id: u32,
    filter_id: FilterId,
    update: UpdateFilter,
) -> anyhow::Result<()> {
    update_filters(node_id, [(filter_id, update)]).await
}

mod tui;

use anyhow::Context as _;
use clap::Parser;
use pw_util::config::{BAND_PREFIX, MANAGED_PROP, SpaJson};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tabled::{Table, Tabled};
use tokio::fs;
use tokio::process::Command;

#[derive(Parser)]
#[command(name = "pw-eq")]
#[command(about = "PipeWire Parametric Equalizer Control")]
struct Args {
    #[clap(subcommand)]
    command: Cmd,
}

#[derive(Parser)]
/// Create a new EQ from an AutoEQ .apo file
struct Create {
    /// Name for the EQ (e.g., focal-celestee)
    name: String,
    /// Path to the .apo file
    #[arg(short, long)]
    apo_path: PathBuf,
    /// Set as default sink after creating
    #[arg(short, long)]
    r#use: bool,
    /// Overwrite existing EQ configuration if it exists
    #[arg(short, long)]
    force: bool,
}

#[derive(Parser)]
/// Describe an EQ filter in detail
struct Describe {
    /// EQ name or ID
    profile: String,
}

#[derive(Parser)]
/// Set EQ band parameters (can only modify existing bands, not add new ones)
#[command(group(clap::ArgGroup::new("params").required(true).multiple(true)))]
struct Set {
    /// EQ name or ID
    profile: String,
    /// Band number (depends on preset, use 'describe' to see available bands)
    band: u32,
    /// Set frequency in Hz
    #[arg(short, long, group = "params")]
    freq: Option<f64>,
    /// Set gain in dB
    #[arg(
        short,
        long,
        group = "params",
        allow_hyphen_values = true,
        number_of_values = 1
    )]
    gain: Option<f64>,
    /// Set Q factor
    #[arg(short, long, group = "params")]
    q: Option<f64>,
    /// Persist changes to config file
    #[arg(short, long)]
    persist: bool,
}

#[derive(Parser)]
/// Set an EQ as the default sink
struct Use {
    /// EQ name or ID
    profile: String,
}

#[derive(Parser)]
enum Cmd {
    Create(Create),
    /// List available EQ filters
    #[clap(alias = "ls")]
    List,
    #[clap(alias = "desc")]
    Describe(Describe),
    Set(Set),
    Use(Use),
    /// Interactive TUI mode
    Tui,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Cmd::Create(create) => create_eq(create).await?,
        Cmd::List => list_eqs().await?,
        Cmd::Describe(describe) => describe_eq(&describe.profile).await?,
        Cmd::Set(set) => set_band(set).await?,
        Cmd::Use(use_cmd) => use_eq(&use_cmd.profile).await?,
        Cmd::Tui => tui::run().await?,
    }

    Ok(())
}

fn is_managed_eq(props: &pw_util::PwDumpObject) -> bool {
    props
        .info
        .props
        .get(MANAGED_PROP)
        .is_some_and(|managed| managed == true)
}

/// Find an EQ node by profile name or ID
async fn find_eq_node(profile: &str) -> anyhow::Result<pw_util::PwDumpObject> {
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

async fn create_eq(
    Create {
        name,
        apo_path: apo,
        r#use: use_after,
        force,
    }: Create,
) -> anyhow::Result<()> {
    // Parse the .apo file
    let apo_config = pw_util::apo::parse_file(apo).await?;

    // Generate the filter-chain config
    let config_content = pw_util::config::Config::from_apo(&name, &apo_config);
    let json = serde_json::to_value(&config_content)
        .context("failed to serialize PipeWire config to JSON")?;
    let content = SpaJson::new(&json).to_string();

    // Get the config directory path
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
        .join("pipewire/pipewire.conf.d");

    // Create the directory if it doesn't exist
    fs::create_dir_all(&config_dir).await?;

    // Write the config file
    let config_file = config_dir.join(format!("pweq-{name}.conf"));
    if !force && config_file.exists() {
        return Err(anyhow::anyhow!(
            "EQ configuration '{}' already exists",
            config_file.display()
        ));
    }

    fs::write(&config_file, content).await?;

    if use_after {
        use_eq(&name).await?;
    }

    Ok(())
}

async fn use_eq(profile: &str) -> anyhow::Result<()> {
    // This is a placeholder implementation
    // In a real implementation, you would set the default sink to the EQ node
    println!(
        "Setting EQ '{}' as the default sink (not yet implemented)",
        profile
    );
    Ok(())
}

async fn set_band(
    Set {
        profile,
        band,
        freq,
        gain,
        q,
        persist,
    }: Set,
) -> anyhow::Result<()> {
    if persist {
        anyhow::bail!("Persisting changes is not yet implemented");
    }

    let node = find_eq_node(&profile).await?;

    // Build the params array for pw-cli
    let mut params = Vec::new();

    if let Some(freq_val) = freq {
        params.push(format!(r#""{BAND_PREFIX}{band}:Freq""#));
        params.push(freq_val.to_string());
    }

    if let Some(gain_val) = gain {
        params.push(format!(r#""{BAND_PREFIX}{band}:Gain""#));
        params.push(gain_val.to_string());
    }

    if let Some(q_val) = q {
        params.push(format!(r#""{BAND_PREFIX}{band}:Q""#));
        params.push(q_val.to_string());
    }

    let output = Command::new("pw-cli")
        .arg("set-param")
        .arg(node.id.to_string())
        .arg("Props")
        .arg(format!("{{ params = [ {} ] }}", params.join(", ")))
        .output()
        .await
        .context("Failed to execute pw-cli")?;

    if !output.status.success() {
        anyhow::bail!("pw-cli failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    println!(
        "Updated band {} on EQ '{}' (node {})",
        band, profile, node.id
    );

    Ok(())
}

async fn describe_eq(profile: &str) -> anyhow::Result<()> {
    let node = find_eq_node(profile).await?;
    let info = node.info;

    #[derive(Debug, Default)]
    struct BandInfo {
        freq: Option<f64>,
        gain: Option<f64>,
        q: Option<f64>,
    }

    let mut band_info = BTreeMap::<usize, BandInfo>::new();
    // Dodgy parsing, weird structures. See `pw-dump <id>`
    for prop in info.params.props {
        for (key, value) in &prop.params.0 {
            let Some((idx, param_name)) = key
                .strip_prefix(BAND_PREFIX)
                .and_then(|s| s.split_once(':'))
            else {
                continue;
            };

            let idx = idx
                .parse::<usize>()
                .with_context(|| format!("invalid band index in parameter name: {key}"))?;
            let value = value
                .as_f64()
                .with_context(|| format!("invalid value for parameter {key}"))?;

            let band_info = band_info.entry(idx).or_default();
            match param_name {
                "Freq" => band_info.freq = Some(value),
                "Gain" => band_info.gain = Some(value),
                "Q" => band_info.q = Some(value),
                "a0" | "a1" | "a2" | "b0" | "b1" | "b2" => {}
                _ => anyhow::bail!("Unknown EQ band parameter: {param_name}"),
            }
        }

        if !band_info.is_empty() {
            break;
        }
    }

    println!("EQ Profile: {profile}");
    println!("Node ID: {}", node.id);
    println!("Bands:");
    for (idx, band) in band_info {
        let freq = band
            .freq
            .ok_or_else(|| anyhow::anyhow!("Missing frequency for band {idx}"))?;
        let gain = band
            .gain
            .ok_or_else(|| anyhow::anyhow!("Missing gain for band {idx}"))?;
        let q = band
            .q
            .ok_or_else(|| anyhow::anyhow!("Missing Q for band {idx}"))?;

        println!(
            "  Band {:>2}: Freq {:>8.2} Hz  Gain {:+5.2} dB  Q {:.2}",
            idx, freq, gain, q
        );
    }

    Ok(())
}

async fn list_eqs() -> anyhow::Result<()> {
    let objects = pw_util::dump().await?;

    #[derive(Tabled)]
    struct Row {
        id: u32,
        name: String,
    }

    let rows = objects
        .into_iter()
        .filter(is_managed_eq)
        .filter(|obj| matches!(obj.object_type, pw_util::PwObjectType::Node))
        .map(|obj| {
            let props = &obj.info.props;
            let name = props
                .get("media.name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let id = obj.id;
            Row {
                id,
                name: name.to_string(),
            }
        });

    let table = Table::new(rows);
    println!("{table}");

    Ok(())
}

mod pw;
mod tui;

use anyhow::Context as _;
use clap::Parser;
use pw_eq::{find_eq_node, use_eq};
use pw_util::config::BAND_PREFIX;
use std::collections::BTreeMap;
use std::fs::File;
use std::num::NonZero;
use std::path::PathBuf;
use tabled::Table;
use tokio::fs;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt as _;

#[derive(Parser)]
#[command(name = "pw-eq")]
#[command(about = "PipeWire Parametric Equalizer Control")]
struct Args {
    #[clap(long)]
    pub log_file: Option<PathBuf>,
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
    band: NonZero<usize>,
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

    // Set up tracing subscriber with file logging
    let _guard = if let Some(log_file_path) = args.log_file {
        let file = File::create(log_file_path)?;
        let (writer, guard) = tracing_appender::non_blocking(file);

        tracing_subscriber::registry()
            .with(EnvFilter::try_from_env("PWEQ_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false),
            )
            .init();
        Some(guard)
    } else {
        None
    };

    match args.command {
        Cmd::Create(create) => create_eq(create).await?,
        Cmd::List => {
            let eqs = pw_eq::list_eqs().await?;
            let table = Table::new(eqs);
            println!("{table}");
        }
        Cmd::Describe(describe) => describe_eq(&describe.profile).await?,
        Cmd::Set(set) => set_band(set).await?,
        Cmd::Use(use_cmd) => {
            use_eq(&use_cmd.profile).await?;
        }
        Cmd::Tui => tui::run().await?,
    }

    Ok(())
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
    let content = pw_util::to_spa_json(&config_content);

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

async fn set_band(
    Set {
        profile,
        band,
        freq: frequency,
        gain,
        q,
        persist,
    }: Set,
) -> anyhow::Result<()> {
    if persist {
        anyhow::bail!("Persisting changes is not yet implemented");
    }

    let node = find_eq_node(&profile).await?;

    pw_eq::update_band(node.id, band, pw_eq::UpdateBand { frequency, gain, q }).await?;

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

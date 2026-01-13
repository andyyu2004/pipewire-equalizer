use anyhow::Context as _;
use clap::Parser;
use crossterm::event::EventStream;
use futures_util::StreamExt as _;
use pw_eq::filter::Filter;
use pw_eq::tui;
use pw_eq::{FilterId, find_eq_node};
use pw_util::apo::{self, FilterType};
use pw_util::module::{self, FILTER_PREFIX};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::str::FromStr;
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
    command: Option<Cmd>,
}

#[derive(Parser)]
/// Create a new Pipewire EQ from an AutoEQ .apo file
struct CreateArgs {
    /// Name for the EQ (e.g., focal-celestee)
    name: String,
    /// Path to the file (APO)
    #[arg(short, long)]
    file: PathBuf,
    /// Overwrite existing EQ configuration if it exists
    #[arg(short, long)]
    force: bool,
}

#[derive(Parser)]
/// Describe an EQ filter in detail
struct DescribeArgs {
    /// EQ name or ID
    profile: String,
    #[arg(short, long)]
    all: bool,
}

#[derive(Parser)]
/// Set EQ filter parameters (can only modify existing filters, not add new ones)
#[command(group(clap::ArgGroup::new("params").required(true).multiple(true)))]
struct SetArgs {
    /// EQ name or ID
    profile: String,
    /// Filter ID (depends on preset, use 'describe' to see available filters)
    filter: FilterId,
    /// Set frequency in Hz
    #[arg(short, long = "freq", group = "params")]
    frequency: Option<f64>,
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

#[derive(Debug, Parser)]
/// Set an EQ as the default sink
struct UseArgs {
    /// EQ name or ID
    profile: String,
}

#[derive(Debug, Default, Parser)]
struct TuiArgs {
    /// Load a specific EQ profile on startup
    /// Currently supports .apo and .conf pipewire module files
    #[arg(short, long, conflicts_with = "preset")]
    file: Option<PathBuf>,
    /// Apply a pre-existing preset filter configuration on startup
    #[arg(short, long)]
    preset: Option<Preset>,
}

#[derive(Debug, Clone)]
enum Preset {
    Flat { bands: usize },
}

impl FromStr for Preset {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();
        if let Some(num_str) = s.strip_prefix("flat") {
            let bands = num_str
                .trim_start_matches('-')
                .parse::<usize>()
                .with_context(|| format!("invalid flat preset number: {num_str}"))?;
            if bands == 0 || bands > 31 {
                return Err(anyhow::anyhow!(
                    "flat preset bands must be between 1 and 31, got {bands}",
                ));
            }
            Ok(Preset::Flat { bands })
        } else {
            Err(anyhow::anyhow!("unknown preset: {s}"))
        }
    }
}

impl Preset {
    fn make_filters(&self) -> Vec<Filter> {
        match self {
            Preset::Flat { bands } => {
                let n = *bands as f64;
                let f_min = 50.0f64;
                let f_max = 10000.0;

                (0..*bands)
                    .map(|i| {
                        // Calculate frequency logarithmically
                        let frequency = if *bands > 1 {
                            f_min * (f_max / f_min).powf(i as f64 / (n - 1.0))
                        } else {
                            1000.0
                        };

                        let q = if *bands == 1 {
                            1.0
                        } else {
                            // Calculate the octave distance between each band
                            // log2(f_max / f_min) gives total octaves (~9.96 for 20-20k)
                            let total_octaves = (f_max / f_min).log2();
                            let bandwidth = total_octaves / (n - 1.0);

                            2f64.powf(bandwidth).sqrt() / (2f64.powf(bandwidth) - 1.0)
                        };

                        Filter {
                            frequency,
                            q,
                            filter_type: FilterType::Peaking,
                            gain: 0.0,
                            muted: false,
                        }
                    })
                    .collect()
            }
        }
    }
}

#[derive(Parser)]
pub enum ConfigArgs {
    /// Create default configuration file
    Init {
        /// Overwrite existing configuration file
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Parser)]
enum Cmd {
    /// Configuration commands
    #[clap(subcommand)]
    Config(ConfigArgs),
    Create(CreateArgs),
    /// List available EQ filters
    #[clap(alias = "ls")]
    List,
    #[clap(alias = "desc")]
    Describe(DescribeArgs),
    Set(SetArgs),
    /// Interactive TUI mode
    Tui(TuiArgs),
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
        None => run_tui(Default::default()).await?,
        Some(cmd) => match cmd {
            Cmd::Config(config) => configure(config).await?,
            Cmd::Create(create) => create_eq(create).await?,
            Cmd::List => {
                let eqs = pw_eq::list_eqs().await?;
                let table = Table::new(eqs);
                println!("{table}");
            }
            Cmd::Describe(describe) => describe_eq(&describe).await?,
            Cmd::Set(set) => set_filter(set).await?,
            Cmd::Tui(tui) => run_tui(tui).await?,
        },
    }

    Ok(())
}

async fn configure(args: ConfigArgs) -> anyhow::Result<()> {
    match args {
        ConfigArgs::Init { force } => {
            let config_dir = dirs::config_dir()
                .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
                .join("pw-eq");

            fs::create_dir_all(&config_dir).await?;

            let config_path = config_dir.join("pw-eq.conf");
            if !force && config_path.exists() {
                return Err(anyhow::anyhow!(
                    "Configuration file at `{}` already exists",
                    config_path.display()
                ));
            }

            let file = fs::File::create(&config_path).await?;
            spa_json::to_writer_pretty(file.try_into_std().unwrap(), &tui::Config::default())?;

            println!(
                "Created default configuration file at `{}`",
                config_path.display()
            );

            Ok(())
        }
    }
}

fn extract_pw_module_filters(conf: &module::Config) -> anyhow::Result<(f64, Vec<Filter>)> {
    let mut fs = vec![];
    let mut preamp = 0.0;

    fn mk(control: &module::Control, filter_type: FilterType) -> Filter {
        Filter {
            frequency: control.freq,
            q: control.q,
            gain: control.gain,
            filter_type,
            muted: false,
        }
    }

    use FilterType::*;
    for node in conf.context_modules[0].args.filter_graph.nodes.iter() {
        match &node.kind {
            module::NodeKind::Peaking { control } => {
                fs.push(mk(control, Peaking));
            }
            module::NodeKind::LowShelf { control } => {
                fs.push(mk(control, LowShelf));
            }
            module::NodeKind::HighShelf { control } => {
                if control.freq == 0.0 {
                    preamp = control.gain;
                    continue;
                }
                fs.push(mk(control, HighShelf));
            }
            module::NodeKind::LowPass { control } => {
                fs.push(mk(control, LowPass));
            }
            module::NodeKind::BandPass { control } => {
                fs.push(mk(control, BandPass));
            }
            module::NodeKind::Notch { control } => {
                fs.push(mk(control, Notch));
            }
            module::NodeKind::HighPass { control } => {
                fs.push(mk(control, HighPass));
            }
            module::NodeKind::Raw { config: _ } => {
                anyhow::bail!("cannot load filters from 'raw' node kind in pipewire configuration")
            }
            module::NodeKind::ParamEq { config } => {
                fs.extend(config.filters.iter().filter_map(|f| match f.ty {
                    HighShelf if f.control.freq == 0.0 => {
                        preamp = f.control.gain;
                        None
                    }
                    ty => Some(mk(&f.control, ty)),
                }))
            }
        }
    }

    Ok((preamp, fs))
}

async fn run_tui(args: TuiArgs) -> anyhow::Result<()> {
    let (preamp, filters) = match (args.file, args.preset) {
        (Some(_), Some(_)) => unreachable!("clap should prevent this case"),
        (Some(path), None) => match path.extension() {
            Some(ext) if ext == "conf" => {
                let conf = module::Config::parse_file(&path)?;
                if conf.context_modules.len() != 1 {
                    anyhow::bail!(
                        "cannot load .conf file with {} context modules, expected 1",
                        conf.context_modules.len()
                    );
                }

                extract_pw_module_filters(&conf)?
            }
            Some(ext) if ext.eq_ignore_ascii_case("apo") || ext.eq_ignore_ascii_case("txt") => {
                let c = apo::Config::parse_file(path).await?;
                (c.preamp, c.filters.into_iter().map(Into::into).collect())
            }
            _ => anyhow::bail!("file must have an extension of .apo, .txt or .conf"),
        },
        (None, Some(preset)) => (0.0, preset.make_filters()),
        _ => Default::default(),
    };

    let term = ratatui::init();

    let base_config = tui::Config::default();
    let user_config_path = dirs::config_dir().unwrap().join("pw-eq/pw-eq.conf");
    let config = if user_config_path.exists() {
        tracing::info!(
            path = %user_config_path.display(),
            "loading user configuration",
        );
        let file = fs::File::open(user_config_path).await?;
        let config =
            spa_json::from_reader::<_, tui::Config>(BufReader::new(file.try_into_std().unwrap()))?;
        base_config.merge(config)
    } else {
        base_config
    };

    let mut app = tui::App::new(term, config, preamp, filters).await?;
    app.enter()?;

    let events = EventStream::new()
        .filter_map(|event| async { event.ok() })
        .filter_map(|event| async { event.try_into().ok() });

    app.run(events).await?;
    ratatui::restore();
    Ok(())
}

async fn create_eq(CreateArgs { name, file, force }: CreateArgs) -> anyhow::Result<()> {
    // Parse the .apo file
    let apo_config = apo::Config::parse_file(file).await?;

    // Generate the filter-chain config
    let config_content = pw_util::module::Config::from_apo(&name, &apo_config);
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

    Ok(())
}

async fn set_filter(
    SetArgs {
        profile,
        filter,
        frequency,
        gain,
        q,
        persist,
    }: SetArgs,
) -> anyhow::Result<()> {
    if persist {
        anyhow::bail!("Persisting changes is not yet implemented");
    }

    let node = find_eq_node(&profile).await?;

    pw_eq::update_filter(
        node.id,
        filter,
        pw_eq::UpdateFilter {
            frequency,
            gain,
            q,
            coeffs: None,
        },
    )
    .await?;

    println!(
        "Updated filter {filter} on EQ '{profile}' (node {})",
        node.id
    );

    Ok(())
}

async fn describe_eq(DescribeArgs { all, profile }: &DescribeArgs) -> anyhow::Result<()> {
    let node = find_eq_node(profile).await?;
    let info = node.info;

    #[derive(Debug, Default)]
    struct FilterInfo {
        freq: Option<f64>,
        gain: Option<f64>,
        q: Option<f64>,
        a0: Option<f64>,
        a1: Option<f64>,
        a2: Option<f64>,
        b0: Option<f64>,
        b1: Option<f64>,
        b2: Option<f64>,
    }

    let mut filter_infos = BTreeMap::<FilterId, FilterInfo>::new();
    // Dodgy parsing, weird structures. See `pw-dump <id>`
    for prop in info.params.props {
        for (key, value) in &prop.params.0 {
            let Some((id, param_name)) = key
                .strip_prefix(FILTER_PREFIX)
                .and_then(|s| s.split_once(':'))
            else {
                continue;
            };

            let id = id
                .parse::<FilterId>()
                .with_context(|| format!("invalid filter id in parameter name: {key}"))?;
            let value = value
                .as_f64()
                .with_context(|| format!("invalid value for parameter {key}"))?;

            let filter_info = filter_infos.entry(id).or_default();
            match param_name {
                "Freq" => filter_info.freq = Some(value),
                "Gain" => filter_info.gain = Some(value),
                "Q" => filter_info.q = Some(value),
                "a0" => filter_info.a0 = Some(value),
                "a1" => filter_info.a1 = Some(value),
                "a2" => filter_info.a2 = Some(value),
                "b0" => filter_info.b0 = Some(value),
                "b1" => filter_info.b1 = Some(value),
                "b2" => filter_info.b2 = Some(value),
                _ => anyhow::bail!("Unknown EQ filter parameter: {param_name}"),
            }
        }

        if !filter_infos.is_empty() {
            break;
        }
    }

    println!("EQ Profile: {profile}");
    println!("Node ID: {}", node.id);
    println!("Filters:");
    for (id, filter) in filter_infos {
        let freq = filter
            .freq
            .ok_or_else(|| anyhow::anyhow!("Missing frequency for filter {id}"))?;
        let gain = filter
            .gain
            .ok_or_else(|| anyhow::anyhow!("Missing gain for filter {id}"))?;
        let q = filter
            .q
            .ok_or_else(|| anyhow::anyhow!("Missing Q for filter {id}"))?;

        if *all {
            println!(
                "  Filter {id:>2}: Freq {freq:>8.2} Hz  Gain {gain:+5.2} dB  Q {q:.2} --> ({:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6})",
                filter.b0.unwrap_or(0.0),
                filter.b1.unwrap_or(0.0),
                filter.b2.unwrap_or(0.0),
                filter.a0.unwrap_or(0.0),
                filter.a1.unwrap_or(0.0),
                filter.a2.unwrap_or(0.0),
            );
        } else {
            println!("  Filter {id:>2}: Freq {freq:>8.2} Hz  Gain {gain:+5.2} dB  Q {q:.2}",);
        }
    }

    Ok(())
}

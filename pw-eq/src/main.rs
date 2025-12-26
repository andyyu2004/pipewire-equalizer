use clap::Parser;
use std::path::PathBuf;
use tokio::fs;

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
enum Cmd {
    Create(Create),
    /// List available EQ filters
    List,
    /// Set an EQ as the default sink
    Use {
        /// EQ name or ID
        profile: String,
    },
    /// Interactive TUI mode
    Tui,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Cmd::Create(create) => create_eq(create).await?,
        Cmd::List => list_eqs().await?,
        Cmd::Use { profile: _ } => todo!(),
        Cmd::Tui => {
            println!("TUI not yet implemented");
        }
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
    let config_content = pw_util::config::generate_filter_chain_config(&name, &apo_config);

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

    fs::write(&config_file, config_content).await?;

    println!("Created EQ configuration: {}", config_file.display());
    println!("\nTo activate the EQ, restart PipeWire:");
    println!("  systemctl --user restart pipewire");

    if use_after {
        println!("\nAfter restart, run:");
        println!("  pw-eq use {}", name);
    }

    Ok(())
}

async fn list_eqs() -> anyhow::Result<()> {
    let objects = pw_util::dump().await?;

    println!("Available EQ Filters:");

    for obj in objects {
        // Only look at Node types
        if obj.object_type != pw_util::PwObjectType::Node {
            continue;
        }

        println!("{}", obj.id)
    }

    Ok(())
}

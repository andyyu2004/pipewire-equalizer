use anyhow::Context as _;
use clap::Parser;
use pw_util::config::MANAGED_PROP;
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
        Cmd::Use { profile } => use_eq(&profile).await?,
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

    // Ideally find a way to not require a restart if possible
    Command::new("systemctl")
        .args(["--user", "restart", "pipewire"])
        .output()
        .await
        .context("failed to restart PipeWire")?;

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

async fn list_eqs() -> anyhow::Result<()> {
    let objects = pw_util::dump().await?;

    #[derive(Tabled)]
    struct Row {
        id: u32,
        name: String,
    }

    let rows = objects
        .into_iter()
        .filter(|obj| matches!(obj.object_type, pw_util::PwObjectType::Node))
        .filter_map(|obj| {
            let props = &obj.info.as_ref()?.props;
            let managed = props.get(MANAGED_PROP)?;
            (managed == true).then_some(obj)
        })
        .map(|obj| {
            let props = &obj.info.as_ref().unwrap().props;
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

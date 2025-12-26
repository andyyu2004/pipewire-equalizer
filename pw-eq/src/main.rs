use clap::Parser;

#[derive(Parser)]
#[command(name = "pw-eq")]
#[command(about = "PipeWire Parametric Equalizer Control")]
struct Args {
    #[clap(subcommand)]
    command: Cmd,
}

#[derive(Parser)]
enum Cmd {
    /// List available EQ filters
    List,
    /// Interactive TUI mode
    Tui,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Cmd::List => list_eqs().await?,
        Cmd::Tui => {
            println!("TUI not yet implemented");
        }
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

        dbg!(&obj);
    }

    Ok(())
}

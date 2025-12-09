mod immichctl;

use clap::{Parser, Subcommand};
use immichctl::ImmichCtl;
use anyhow::Result;

/// A command line interface for Immich.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose output for detailed error messages
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Prints version information
    Version,
    /// Login to an Immich instance
    Login {
        /// The server URL as positional argument
        server: Option<String>,
        /// The API key
        #[arg(long)]
        apikey: Option<String>,
    },
    /// Logout from the current Immich instance
    Logout,
    /// Clear data
    Clear {
        #[command(subcommand)]
        command: ClearCommands,
    },
    /// Count data
    Count {
        #[command(subcommand)]
        command: CountCommands,
    },
    /// List data
    List {
        #[command(subcommand)]
        command: ListCommands,
    },
    /// Add data
    Add {
        #[command(subcommand)]
        command: AddCommands,
    },
}

#[derive(Subcommand, Debug)]
enum ClearCommands {
    /// Clear the local selection store
    Selection,
}

#[derive(Subcommand, Debug)]
enum CountCommands {
    /// Count items in the local selection store
    Selection,
}

#[derive(Subcommand, Debug)]
enum ListCommands {
    /// List asset ids in the local selection store
    Selection,
}

#[derive(Subcommand, Debug)]
enum AddCommands {
    /// Add an asset to the local selection by searching metadata
    Selection {
        /// Asset id to add (UUID)
        #[arg(long, value_name = "asset id")]
        id: Option<String>,
        /// Tag name to search and add by tag id
        #[arg(long, value_name = "tag name")]
        tag: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(err) = _main(&cli).await  {
        if cli.verbose {
            eprintln!("Error: {:?}", err);
        } else {
            eprintln!("Error: {}", err);
        }
        std::process::exit(1);
    }
}

async fn _main(cli: &Cli) -> Result<()> {
    let mut immichctl = ImmichCtl::new();

    match &cli.command {
        Commands::Version => {
            immichctl.version().await?;
        }
        Commands::Login { server, apikey } => match (server, apikey) {
            (Some(server), Some(apikey)) => {
                immichctl.login(server, apikey).await?;
            }
            (None, None) => {
                immichctl.show_login()?;
            }
            _ => {
                println!(
                    "Please provide both server URL and --apikey to login, or no arguments to see the current server."
                );
            }
        },
        Commands::Logout => {
            immichctl.logout()?;
        }
        Commands::Clear { command } => match command {
            ClearCommands::Selection => {
                immichctl.selection_clear()?;
            }
        },
        Commands::Count { command } => match command {
            CountCommands::Selection => {
                immichctl.selection_count();
            }
        },
        Commands::List { command } => match command {
            ListCommands::Selection => {
                immichctl.selection_list();
            }
        },
        Commands::Add { command } => match command {
            AddCommands::Selection { id, tag } => {
                immichctl.selection_add(id, tag).await?;
            }
        },
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

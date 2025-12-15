mod immichctl;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use immichctl::ImmichCtl;

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
    /// Manage the selection
    Selection {
        #[command(subcommand)]
        command: SelectionCommands,
    },
    /// Tag selected assets
    Tag {
        #[command(subcommand)]
        command: TagCommands,
    },
}

#[derive(Subcommand, Debug)]
enum SelectionCommands {
    /// Add an asset to the local selection by searching metadata
    Add {
        /// Asset id to add (UUID)
        #[arg(long, value_name = "asset id")]
        id: Option<String>,
        /// Tag name to search and add by tag id
        #[arg(long, value_name = "tag name")]
        tag: Option<String>,
        /// Album name to search
        #[arg(long, value_name = "album name")]
        album: Option<String>,
    },
    /// Clear the local selection store
    Clear,
    /// Count items in the local selection store
    Count,
    /// List asset ids in the local selection store
    List,
    /// Remove an asset from the local selection by searching metadata
    Remove {
        /// Asset id to remove (UUID)
        #[arg(long, value_name = "asset id")]
        id: Option<String>,
        /// Tag name to search and remove by tag id
        #[arg(long, value_name = "tag name")]
        tag: Option<String>,
        /// Album name to search
        #[arg(long, value_name = "album name")]
        album: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TagCommands {
    /// Add a tag to selected assets
    Add {
        /// Tag name to add
        name: String,
    },
    /// Remove a tag from selected assets
    Remove {
        /// Tag name to remove
        name: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(err) = _main(&cli).await {
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
            (Some(server), Some(apikey)) => immichctl.login(server, apikey).await?,
            (None, None) => immichctl.show_login()?,
            _ => bail!(
                "Please provide both server URL and --apikey to login, or no arguments to see the current server."
            ),
        },
        Commands::Logout => {
            immichctl.logout()?;
        }
        Commands::Selection { command } => match command {
            SelectionCommands::Add { id, tag, album } => {
                immichctl.selection_add(id, tag, album).await?;
            }
            SelectionCommands::Clear => {
                immichctl.selection_clear()?;
            }
            SelectionCommands::Count => {
                immichctl.selection_count();
            }
            SelectionCommands::List => {
                immichctl.selection_list();
            }
            SelectionCommands::Remove { id, tag, album } => {
                immichctl.selection_remove(id, tag, album).await?;
            }
        },
        Commands::Tag { command } => match command {
            TagCommands::Add { name } => {
                immichctl.tag_add(name).await?;
            }
            TagCommands::Remove { name } => {
                immichctl.tag_remove(name).await?;
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

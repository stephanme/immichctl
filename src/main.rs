mod immichctl;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use immichctl::{AssetColumns, ImmichCtl};

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
    /// Manage the asset selection
    #[command(visible_aliases = ["asset", "a"])]
    Assets {
        #[command(subcommand)]
        command: AssetCommands,
    },
    /// Tag selected assets
    #[command(visible_aliases = ["tag", "t"])]
    Tags {
        #[command(subcommand)]
        command: TagCommands,
    },
}

#[derive(Subcommand, Debug)]
enum AssetCommands {
    /// Search for assets and add/remove them to/from the local asset selection.
    Search {
        /// Remove assets from selection instead of adding
        #[arg(long)]
        remove: bool,
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
    List {
        /// Output format
        #[arg(long, default_value = "csv", value_enum)]
        format: ListFormat,
        /// Columns to display
        #[arg(
            short,
            long = "column",
            default_value = "original-file-name",
            value_enum
        )]
        columns: Vec<AssetColumns>,
    },
}

/// Columns for CSV listing of selected assets
#[derive(clap::ValueEnum, Clone, Debug)]
enum ListFormat {
    /// CSV format
    Csv,
    /// Json format
    Json,
    /// Json format, pretty printed
    JsonPretty,
}

#[derive(Subcommand, Debug)]
enum TagCommands {
    /// Assign a tag to selected assets
    Assign {
        /// Tag name to add
        name: String,
    },
    /// Unassign a tag from selected assets
    Unassign {
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
        Commands::Assets { command } => match command {
            AssetCommands::Search {
                remove,
                id,
                tag,
                album,
            } => {
                if *remove {
                    immichctl.assets_search_remove(id, tag, album).await?;
                } else {
                    immichctl.assets_search_add(id, tag, album).await?;
                }
            }
            AssetCommands::Clear => {
                immichctl.assets_clear()?;
            }
            AssetCommands::Count => {
                immichctl.assets_count();
            }
            AssetCommands::List { format, columns } => match format {
                ListFormat::Csv => immichctl.assets_list_csv(columns),
                ListFormat::Json => immichctl.assets_list_json(false)?,
                ListFormat::JsonPretty => immichctl.assets_list_json(true)?,
            },
        },
        Commands::Tags { command } => match command {
            TagCommands::Assign { name } => {
                immichctl.tag_assign(name).await?;
            }
            TagCommands::Unassign { name } => {
                immichctl.tag_unassign(name).await?;
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

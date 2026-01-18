mod immichctl;
mod timedelta;

use anyhow::{Result, bail};
use chrono::{FixedOffset, TimeDelta};
use clap::{Parser, Subcommand};
use immichctl::{AssetColumns, AssetSearchArgs, CurlMethod, ImmichCtl};
use timedelta::TimeDeltaValue;

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
    /// Manage tags
    #[command(visible_aliases = ["tag", "t"])]
    Tags {
        #[command(subcommand)]
        command: TagCommands,
    },
    /// Manage albums
    #[command(visible_aliases = ["album"])]
    Albums {
        #[command(subcommand)]
        command: AlbumCommands,
    },
    /// Execute an Immich API request
    Curl {
        /// API endpoint path
        path: String,
        /// HTTP method
        #[arg(short = 'X', long, default_value = "get")]
        method: CurlMethod,
        /// HTTP data to include in the request body
        #[arg(short = 'd', long)]
        data: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum AssetCommands {
    /// Clear the local selection store
    Clear,
    /// Search for assets and add/remove them to/from the local asset selection.
    Search(AssetSearchArgs),
    /// Refresh asset metadata including exif data (slow)
    Refresh,
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
    /// Adjust dateTimeOriginal and timezone of selected assets
    Datetime {
        /// dateTimeOriginal offset, e.g. 1d1h1m or -2h30m
        #[arg(long, value_name = "offset")]
        offset: Option<TimeDeltaValue>,
        /// New timezone in format ±HH:MM
        #[arg(long, value_name = "timezone")]
        timezone: Option<FixedOffset>,
        #[arg(long)]
        dry_run: bool,
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

#[derive(Subcommand, Debug)]
enum AlbumCommands {
    /// Assign selected assets to an album
    Assign {
        /// Album name to assign
        name: String,
    },
    /// Unassign selected assets from an album
    Unassign {
        /// Album name to remove
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
    tracing_subscriber::fmt::init();

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
        Commands::Curl { path, method, data } => {
            immichctl.curl(path, *method, data).await?;
        }
        Commands::Assets { command } => match command {
            AssetCommands::Search(args) => match args.remove {
                true => {
                    immichctl.assets_search_remove(args).await?;
                }
                false => {
                    if let Some(_tz) = &args.timezone {
                        bail!(
                            "The --timezone option can only be used when removing assets from the selection."
                        );
                    }
                    immichctl.assets_search_add(args).await?;
                }
            },
            AssetCommands::Clear => {
                immichctl.assets_clear()?;
            }
            AssetCommands::Count => {
                immichctl.assets_count();
            }
            AssetCommands::Refresh => {
                immichctl.assets_refresh().await?;
            }
            AssetCommands::List { format, columns } => match format {
                ListFormat::Csv => immichctl.assets_list_csv(columns),
                ListFormat::Json => immichctl.assets_list_json(false)?,
                ListFormat::JsonPretty => immichctl.assets_list_json(true)?,
            },
            AssetCommands::Datetime {
                offset,
                timezone,
                dry_run,
            } => {
                let o = match offset {
                    Some(v) => **v,
                    None => TimeDelta::zero(),
                };
                immichctl
                    .assets_datetime_adjust(&o, timezone, *dry_run)
                    .await?;
            }
        },
        Commands::Tags { command } => match command {
            TagCommands::Assign { name } => {
                immichctl.tag_assign(name).await?;
            }
            TagCommands::Unassign { name } => {
                immichctl.tag_unassign(name).await?;
            }
        },
        Commands::Albums { command } => match command {
            AlbumCommands::Assign { name } => {
                immichctl.album_assign(name).await?;
            }
            AlbumCommands::Unassign { name } => {
                immichctl.album_unassign(name).await?;
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

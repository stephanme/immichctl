use clap::{Parser, Subcommand};
use reqwest::{self};

pub mod config;
use config::Config;

pub mod selection;

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

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

    let config_file =
        Config::get_default_config_file().expect("Could not determine default config file path");
    let mut config = Config::load(&config_file);
    let selection_file = selection::Selection::get_default_selection_file()
        .expect("Could not determine default selection file path");

    // immich client gets rebuild when config changes, i.e. for login command
    let mut immich = build_client(&config);

    match &cli.command {
        Commands::Version => {
            let version = env!("CARGO_PKG_VERSION");
            println!("immichctl version: {}", version);
            if config.logged_in() {
                match immich.get_server_version().await {
                    Ok(response) => {
                        println!(
                            "Immich server version: {}.{}.{}",
                            response.major, response.minor, response.patch
                        );
                    }
                    Err(e) => {
                        print_error(
                            &e,
                            cli.verbose,
                            "Could not connect to the server to get the version",
                        );
                    }
                }
            } else {
                println!("Not logged in. Cannot determine server version.");
            }
        }
        Commands::Login { server, apikey } => match (server, apikey) {
            (Some(server), Some(apikey)) => {
                config.server = server.clone();
                config.apikey = apikey.clone();
                immich = build_client(&config);
                match immich.validate_access_token().await {
                    Ok(_response) => {
                        println!("Login successful to server: {}", server);
                        config.save().expect("Could not save configuration");
                    }
                    Err(e) => {
                        print_error(
                            &e,
                            cli.verbose,
                            "Login failed. Could not connect to the server",
                        );
                    }
                }
            }
            (None, None) => {
                if config.logged_in() {
                    println!("Currently logged in to: {}", config.server);
                } else {
                    println!("Not logged in. Use 'immichctl login <URL> --apikey <KEY>' to login.");
                }
            }
            _ => {
                println!(
                    "Please provide both server URL and --apikey to login, or no arguments to see the current server."
                );
            }
        },
        Commands::Logout => {
            config.logout();
            config.save().expect("Could not save configuration");
        }
        Commands::Clear { command } => match command {
            ClearCommands::Selection => {
                let mut sel = selection::Selection::load(&selection_file);
                sel.clear();
                sel.save().expect("Could not save selection");
                println!("Selection cleared.");
            }
        },
        Commands::Count { command } => match command {
            CountCommands::Selection => {
                let sel = selection::Selection::load(&selection_file);
                println!("{}", sel.len());
            }
        },
        Commands::List { command } => match command {
            ListCommands::Selection => {
                let sel = selection::Selection::load(&selection_file);
                for asset in sel.list_assets() {
                    println!("{}", asset.id);
                }
            }
        },
        Commands::Add { command } => match command {
            AddCommands::Selection { id, tag } => {
                // TODO: check if logged-in
                use crate::types::MetadataSearchDto;
                let mut body = MetadataSearchDto::default();
                if let Some(id) = id {
                    let uuid = match uuid::Uuid::parse_str(id) {
                        Ok(u) => u,
                        Err(_) => {
                            eprintln!("Invalid asset id, expected uuid: {}", id);
                            return;
                        }
                    };
                    body.id = Some(uuid);
                }
                if let Some(tag_name) = tag {
                    match immich.get_all_tags().await {
                        Ok(tags_resp) => {
                            // TODO: handle hierarchical tags
                            let maybe_tag = tags_resp.iter().find(|t| t.name == *tag_name);
                            match maybe_tag {
                                Some(t) => {
                                    let tag_uuid =
                                        uuid::Uuid::parse_str(&t.id).unwrap_or_else(|_| {
                                            panic!("Could not parse tag id {}", &t.id)
                                        });
                                    body.tag_ids = Some(vec![tag_uuid]);
                                }
                                None => {
                                    eprintln!("Tag not found: '{}'", tag_name);
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            print_error(&e, cli.verbose, "Could not retrieve tags");
                            return;
                        }
                    }
                }
                // check that at least one search flag is provided
                if body.id.is_none() && body.tag_ids.as_ref().map(|v| v.is_empty()).unwrap_or(true)
                {
                    eprintln!("Please provide at least one search flag.");
                    return;
                }
                // TODO: handle pagination
                match immich.search_assets(&body).await {
                    Ok(mut resp) => {
                        let mut sel = selection::Selection::load(&selection_file);
                        let old_len = sel.len();
                        for asset in resp.assets.items.drain(..) {
                            sel.add_asset(asset);
                        }
                        sel.save().expect("Could not save selection");
                        let new_len = sel.len();
                        println!(
                            "Added {} asset(s) to selection.",
                            new_len.saturating_sub(old_len)
                        );
                    }
                    Err(e) => {
                        print_error(&e, cli.verbose, "Search failed");
                    }
                }
            }
        },
    }
}

fn build_client(config: &Config) -> Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-api-key",
        reqwest::header::HeaderValue::from_str(&config.apikey).unwrap(),
    );
    let client_with_custom_defaults = reqwest::ClientBuilder::new()
        .default_headers(headers)
        .build()
        .unwrap();
    let immich_api_url = config.server.clone() + "/api";
    Client::new_with_client(&immich_api_url, client_with_custom_defaults)
}

fn print_error(e: &Error, verbose: bool, context: &str) {
    if verbose {
        eprintln!("{}: {:?}", context, e);
    } else {
        let status = match e.status() {
            Some(s) => s.canonical_reason().unwrap_or("Unknown status code"),
            None => "Unknown error",
        };
        eprintln!("{}: {}", context, status);
    }
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

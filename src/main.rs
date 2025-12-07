use clap::{Parser, Subcommand};
use reqwest::{self};

pub mod config;
use config::Config;

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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config_file =
        Config::get_default_config_file().expect("Could not determine default config file path");
    let mut config = Config::load(&config_file);

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

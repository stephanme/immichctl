use clap::{Parser, Subcommand};
use reqwest;
use serde::{Deserialize, Serialize};

pub mod config;
use config::Config;

/// A command line interface for Immich.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
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

#[derive(Serialize, Deserialize, Debug)]
struct ServerVersionResponse {
    major: i32,
    minor: i32,
    patch: i32,
}

#[derive(Serialize, Deserialize, Debug)]
struct ValidateAccessTokenResponse {
    #[serde(rename = "authStatus")]
    auth_status: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    
    let config_file = Config::get_default_config_file().expect("Could not determine default config file path");
    let mut config = Config::load(&config_file);

    match &cli.command {
        Commands::Version => {
            let version = env!("CARGO_PKG_VERSION");
            println!("immichctl version: {}", version);
            if config.logged_in() {
                let client = reqwest::Client::new();
                let server_version_url = format!("{}/api/server/version", config.server);
                match client.get(&server_version_url).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            match response.json::<ServerVersionResponse>().await {
                                Ok(server_version) => {
                                    println!(
                                        "Immich server version: {}.{}.{}",
                                        server_version.major,
                                        server_version.minor,
                                        server_version.patch
                                    );
                                }
                                Err(_) => {
                                    println!("Could not parse server version response.");
                                }
                            }
                        } else {
                            println!(
                                "Could not get server version. Status: {}",
                                response.status()
                            );
                        }
                    }
                    Err(_) => {
                        println!("Could not connect to the server to get the version.");
                    }
                }
            } else {
                println!("Not logged in. Cannot determine server version.");
            }
        }
        Commands::Login { server, apikey } => match (server, apikey) {
            (Some(server), Some(apikey)) => {
                let client = reqwest::Client::new();
                let validate_url = format!("{}/api/auth/validateToken", server);
                match client
                    .post(&validate_url)
                    .header("x-api-key", apikey.clone())
                    .send()
                    .await
                {
                    Ok(response) => {
                        if response.status().is_success() {
                            match response.json::<ValidateAccessTokenResponse>().await {
                                Ok(validate_response) => {
                                    if validate_response.auth_status {
                                        println!("Successfully logged in to {:?}", server);

                                        config.server = server.clone();
                                        config.apikey = apikey.clone();
                                        // Save config
                                        config.save().expect("Could not save configuration");
                                    } else {
                                        println!("Login failed. API key is not valid.");
                                    }
                                }
                                Err(_) => {
                                    println!("Could not parse validation response. Login failed.");
                                }
                            }
                        } else {
                            println!(
                                "Login failed. Could not connect to server. Status: {}",
                                response.status()
                            );
                        }
                    }
                    Err(_) => {
                        println!("Login failed. Could not connect to the server.");
                    }
                }
            }
            (None, None) => {
                if config.logged_in() {
                    println!("Currently logged in to: {}", config.server);
                } else {
                    println!(
                        "Not logged in. Use 'immichctl login <URL> --apikey <KEY>' to login."
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

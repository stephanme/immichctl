mod config;
mod selection;
mod selection_cmd;
mod server_cmd;

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

use config::Config;
use std::path::PathBuf;
use anyhow::{Result, bail};

use selection::Selection;

pub struct ImmichCtl {
    config: Config,
    immich: Client,
    selection_file: PathBuf,
}

impl ImmichCtl {
    pub fn new() -> Self {
        let config_file = Config::get_default_config_file()
            .expect("Could not determine default config file path");
        let config = Config::load(&config_file);
        let selection_file = Selection::get_default_selection_file()
            .expect("Could not determine default selection file path");

        // immich client gets rebuild when config changes, i.e. for login command
        let immich = Self::build_client(&config);

        ImmichCtl {
            config,
            immich,
            selection_file,
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

    pub fn assert_logged_in(&self) -> Result<()> {
        if !self.config.logged_in() {
            bail!("Not logged in. Use 'immichctl login <URL> --apikey <KEY>' to login.")
        }
        Ok(())
    }
}

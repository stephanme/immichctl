mod album_cmd;
mod asset_cmd;
mod assets;
mod config;
mod curl_cmd;
mod server_cmd;
mod tag_cmd;

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

pub use asset_cmd::{AssetColumns, AssetSearchArgs};
pub use curl_cmd::CurlMethod;

use anyhow::{Result, anyhow, bail};
use config::Config;
use std::path::{Path, PathBuf};

pub struct ImmichCtl {
    config: Config,
    immich: Result<Client>,
    assets_file: PathBuf,
}

impl ImmichCtl {
    pub fn new() -> Self {
        let config_dir =
            Self::get_default_config_dir().expect("Could not determine config directory");
        Self::with_config_dir(&config_dir)
    }

    pub fn with_config_dir(config_dir: &Path) -> Self {
        let config_file = config_dir.join("config.json");
        let config = Config::load(&config_file);
        let assets_file = config_dir.join("assets.json");

        // immich client gets rebuild when config changes, i.e. for login command
        let immich = Self::build_client(&config);

        ImmichCtl {
            config,
            immich,
            assets_file,
        }
    }

    pub fn get_default_config_dir() -> Result<PathBuf> {
        let Some(mut path) = dirs::home_dir() else {
            bail!("Could not determine home directory")
        };
        path.push(".immichctl");
        Ok(path)
    }

    fn build_client(config: &Config) -> Result<Client> {
        if !config.logged_in() {
            bail!("Not logged in. Use 'immichctl login <URL> --apikey <KEY>' to login.")
        }

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-api-key",
            reqwest::header::HeaderValue::from_str(&config.apikey).unwrap(),
        );
        let client_with_custom_defaults = reqwest::ClientBuilder::new()
            .default_headers(headers)
            .connection_verbose(true)
            .build()?;
        let immich_api_url = config.server.clone() + "/api";
        Ok(Client::new_with_client(
            &immich_api_url,
            client_with_custom_defaults,
        ))
    }

    /// Get immich api client if logged in.
    ///
    /// # Errors
    ///
    /// This function will return an error if not logged in.
    ///
    /// # Example
    ///
    /// ```
    /// # within an fn implementation of ImmichCtl
    /// let version = self.immich()?.get_server_version().await.context("Could not get server version")?;
    /// ```
    pub fn immich(&self) -> Result<&Client> {
        match &self.immich {
            Ok(client) => Ok(client),
            Err(err) => Err(anyhow!("{}", err)),
        }
    }

    pub fn assert_logged_in(&self) -> Result<()> {
        if !self.config.logged_in() {
            bail!("Not logged in. Use 'immichctl login <URL> --apikey <KEY>' to login.")
        }
        Ok(())
    }

    pub fn eprint_progress_indicator(&self, current: usize, total: usize, delta: usize) {
        if current == 0 || current == total - 1 || current.is_multiple_of(delta) {
            let percentage = current as f32 / total as f32 * 100.0;
            eprint!("\r{:.0}%", percentage);
        }
        if current == total - 1 {
            eprintln!();
        }
    }
}

#[cfg(test)]
mod tests {
    use mockito::{Server, ServerGuard};

    use super::*;

    pub async fn create_immichctl_with_server() -> (ImmichCtl, ServerGuard) {
        let server = Server::new_async().await;
        let config_dir = tempfile::tempdir().unwrap();
        let mut config = Config::load(&config_dir.path().join("config.json"));
        config.server = server.url();
        config.apikey = "apikey".to_string();
        config.save().expect("could not save config");
        let ctl = ImmichCtl::with_config_dir(config_dir.path());
        (ctl, server)
    }

    #[test]
    fn test_get_default_config_dir() {
        let path = ImmichCtl::get_default_config_dir().expect("no home path");
        assert!(path.ends_with(".immichctl"));
    }

    #[test]
    fn test_with_config_dir() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());
        assert!(ctl.config.server.is_empty());
        assert!(ctl.assert_logged_in().is_err());
        assert!(ctl.immich().is_err());
        assert_eq!(
            ctl.immich().err().unwrap().to_string(),
            "Not logged in. Use 'immichctl login <URL> --apikey <KEY>' to login."
        );
    }
}

use anyhow::{Context, Result};

use super::ImmichCtl;

impl ImmichCtl {
    pub async fn version(&self) -> Result<()> {
        let version = env!("CARGO_PKG_VERSION");
        println!("immichctl version: {}", version);
        if self.config.logged_in() {
            let response = self
                .immich
                .get_server_version()
                .await
                .context("Could not connect to the server to get the version")?;
            println!(
                "Immich server version: {}.{}.{}",
                response.major, response.minor, response.patch
            );
        } else {
            println!("Not logged in. Cannot determine server version.");
        }
        Ok(())
    }

    pub async fn login(&mut self, server: &str, apikey: &str) -> Result<()> {
        self.config.server = server.to_string();
        self.config.apikey = apikey.to_string();
        self.immich = Self::build_client(&self.config);

        self.immich
            .validate_access_token()
            .await
            .context("Login failed. Could not connect to the server.")?;
        println!("Login successful to server: {}", server);
        self.config.save()?;
        Ok(())
    }

    pub fn show_login(&self) -> Result<()> {
        self.assert_logged_in()?;
        println!("Currently logged in to: {}", self.config.server);
        Ok(())
    }

    pub fn logout(&mut self) -> Result<()> {
        self.config.logout();
        self.config.save()?;
        println!("Logged out.");
        Ok(())
    }
}

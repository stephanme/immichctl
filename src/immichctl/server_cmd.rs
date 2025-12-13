use anyhow::{Context, Result};

use super::ImmichCtl;

impl ImmichCtl {
    pub async fn version(&self) -> Result<()> {
        let version = env!("CARGO_PKG_VERSION");
        println!("immichctl version: {}", version);
        if self.config.logged_in() {
            let response = self
                .immich()?
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
        let mut temp_config = self.config.clone();
        temp_config.server = server.to_string();
        temp_config.apikey = apikey.to_string();
        let immich = Self::build_client(&temp_config)?;

        immich
            .validate_access_token()
            .await
            .context("Login failed. Could not connect to the server.")?;

        self.config = temp_config;
        self.immich = Ok(immich);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immichctl::{ImmichCtl, config::Config};
    use mockito::Server;

    #[tokio::test]
    async fn test_login_logout() -> Result<()> {
        let config_dir = tempfile::tempdir().unwrap();
        let mut ctl = ImmichCtl::with_config_dir(config_dir.path());
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/api/auth/validateToken")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"authStatus":true}"#)
            .create_async()
            .await;

        ctl.login(&server.url(), "apikey").await?;
        ctl.immich()?;

        mock.assert_async().await;

        assert!(ctl.config.logged_in());

        ctl.logout()?;
        assert!(!ctl.config.logged_in());

        Ok(())
    }

    #[tokio::test]
    async fn test_login_failed() {
        let config_dir = tempfile::tempdir().unwrap();
        let mut ctl = ImmichCtl::with_config_dir(config_dir.path());
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/api/auth/validateToken")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error":"Unauthorized"}"#)
            .create_async()
            .await;

        let result = ctl.login(&server.url(), "invalid-key").await;

        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Login failed. Could not connect to the server."
        );
        mock.assert_async().await;
        assert!(!ctl.config.logged_in());
    }

    #[tokio::test]
    async fn test_version_not_logged_in() -> Result<()> {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        ctl.version().await?;
        Ok(())
    }
    #[tokio::test]
    async fn test_version_logged_in() -> Result<()> {
        let mut server = Server::new_async().await;
        let config_dir = tempfile::tempdir().unwrap();
        let mut config = Config::load(&config_dir.path().join("config.json"));
        config.server = server.url();
        config.apikey = "apikey".to_string();
        config.save()?;
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let version_mock = server
            .mock("GET", "/api/server/version")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"major":1,"minor":100,"patch":0,"release":""}"#)
            .create_async()
            .await;

        ctl.version().await?;
        version_mock.assert_async().await;

        Ok(())
    }
}

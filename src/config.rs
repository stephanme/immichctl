use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Config {
    #[serde(skip)]
    config_file: PathBuf,
    pub server: String,
    pub apikey: String,
}

impl Config {
    pub fn load(config_file: &Path) -> Config {
        match Self::load_config(config_file) {
            Some(mut cfg) => {
                cfg.config_file = config_file.to_path_buf();
                cfg
            }
            None => Config {
                config_file: config_file.to_path_buf(),
                server: String::new(),
                apikey: String::new(),
            },
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        fs::create_dir_all(self.config_file.parent().unwrap())?;
        let contents = serde_json::to_string_pretty(&self)
            .map_err(|e| std::io::Error::other(format!("Serialization error: {}", e)))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            options.mode(0o600); // User read/write only
            let mut file = options.open(&self.config_file)?;
            file.write_all(contents.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            // On non-Unix platforms, default permissions are used.
            let mut file = fs::File::create(&self.config_file)?;
            file.write_all(contents.as_bytes())?;
        }
        Ok(())
    }

    pub fn logged_in(&self) -> bool {
        !self.server.is_empty() && !self.apikey.is_empty()
    }

    pub fn logout(&mut self) {
        self.server.clear();
        self.apikey.clear();
    }

    pub fn get_default_config_file() -> Option<PathBuf> {
        dirs::home_dir().map(|mut path| {
            path.push(".immichctl");
            path.push("config.json");
            path
        })
    }

    fn load_config(config_file: &Path) -> Option<Config> {
        if !config_file.exists() {
            return None;
        }
        let mut file = fs::File::open(config_file).ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;
        serde_json::from_str(&contents).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_config_path() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("immichctl_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.push("config.json");
        dir
    }

    #[test]
    fn test_save_and_load() {
        let config_path = temp_config_path();
        let config = Config {
            config_file: config_path.clone(),
            server: "http://localhost".to_string(),
            apikey: "testkey".to_string(),
        };
        config.save().unwrap();
        let loaded = Config::load(&config_path);
        assert_eq!(config, loaded);
        // Clean up
        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_dir_all(config_path.parent().unwrap());
    }

    #[test]
    fn test_logged_in() {
        let config = Config {
            config_file: PathBuf::new(),
            server: "http://localhost".to_string(),
            apikey: "testkey".to_string(),
        };
        assert!(config.logged_in());
        let config = Config {
            config_file: PathBuf::new(),
            server: String::new(),
            apikey: String::new(),
        };
        assert!(!config.logged_in());
    }

    #[test]
    fn test_logout() {
        let mut config = Config {
            config_file: PathBuf::new(),
            server: "http://localhost".to_string(),
            apikey: "testkey".to_string(),
        };
        config.logout();
        assert!(config.server.is_empty());
        assert!(config.apikey.is_empty());
    }

    #[test]
    fn test_get_default_config_file() {
        let path = Config::get_default_config_file();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with(".immichctl/config.json"));
    }
}

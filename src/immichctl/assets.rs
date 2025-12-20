use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::immichctl::types::AssetResponseDto;

// could keep asset data on disk only to avoid large memory usage
#[derive(Serialize, Deserialize, Debug)]
pub struct Assets {
    #[serde(skip)]
    file: PathBuf,

    assets: HashMap<String, AssetResponseDto>,
}

impl Assets {
    pub fn load(file: &Path) -> Assets {
        match Self::load_selection(file) {
            Some(mut s) => {
                s.file = file.to_path_buf();
                s
            }
            None => Assets {
                file: file.to_path_buf(),
                assets: HashMap::new(),
            },
        }
    }

    fn load_selection(file: &Path) -> Option<Assets> {
        if !file.exists() {
            return None;
        }
        let mut file = fs::File::open(file).ok()?;
        // TODO check if loading into string can be avoided
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;
        serde_json::from_str(&contents).ok()
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(self.file.parent().unwrap())?;
        let contents = serde_json::to_string_pretty(&self)
            .context("Could not save asset selection, serialization error")?;
        let mut file = fs::File::create(&self.file).context("Could not save asset selection.")?;
        file.write_all(contents.as_bytes())
            .context("Could not save asset selection.")?;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.assets.clear();
    }

    #[allow(dead_code)]
    pub fn contains(&self, asset_id: &str) -> bool {
        self.assets.contains_key(asset_id)
    }

    pub fn add_asset(&mut self, asset: AssetResponseDto) {
        self.assets.insert(asset.id.clone(), asset);
    }

    pub fn remove_asset(&mut self, asset_id: &str) {
        self.assets.remove(asset_id);
    }

    pub fn iter_assets(&self) -> impl Iterator<Item = &AssetResponseDto> {
        self.assets.values()
    }

    pub fn iter_mut_assets(&mut self) -> impl Iterator<Item = &mut AssetResponseDto> {
        self.assets.values_mut()
    }

    pub fn asset_uuids(&self) -> Vec<Uuid> {
        self.assets
            .keys()
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::immichctl::types::{AssetTypeEnum, AssetVisibility};
    use chrono::{DateTime, Utc};

    use super::*;

    fn default_asset() -> AssetResponseDto {
        // Fill all required fields of AssetResponseDto based on the openapi schema.
        AssetResponseDto {
            // required
            id: String::from("5460dc82-2353-47d1-878c-2f15a1084001"),
            checksum: String::new(),
            created_at: DateTime::<Utc>::from_timestamp_nanos(0),
            device_asset_id: String::from("device_asset_id"),
            device_id: String::from("device_id"),
            duration: String::from("0"),
            file_created_at: DateTime::<Utc>::from_timestamp_nanos(0),
            file_modified_at: DateTime::<Utc>::from_timestamp_nanos(0),
            has_metadata: false,
            is_archived: false,
            is_favorite: false,
            is_offline: false,
            is_trashed: false,
            local_date_time: DateTime::<Utc>::from_timestamp_nanos(0),
            original_file_name: String::from("file.jpg"),
            original_path: String::from("/tmp/file.jpg"),
            owner_id: String::from("owner_id"),
            thumbhash: None, // required but can be null
            type_: AssetTypeEnum::Image,
            updated_at: DateTime::<Utc>::from_timestamp_nanos(0),
            visibility: AssetVisibility::Timeline,

            // optional (not required by schema) - omit or set None
            duplicate_id: None,
            exif_info: Default::default(),
            library_id: None,
            live_photo_video_id: None,
            original_mime_type: Some(String::from("image/jpeg")),
            owner: None,
            people: vec![],
            resized: Some(false),
            stack: None,
            tags: vec![],
            unassigned_faces: vec![],
        }
    }

    #[test]
    fn add_remove_list_assets() {
        let mut sel = Assets {
            file: PathBuf::from("test_selection.json"),
            assets: HashMap::new(),
        };
        let asset = default_asset();
        let asset_id = asset.id.clone();

        sel.add_asset(asset);
        assert_eq!(sel.len(), 1);
        assert!(sel.contains(&asset_id));

        let assets: Vec<&AssetResponseDto> = sel.iter_assets().collect();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].id, asset_id);

        sel.remove_asset(&asset_id);
        assert_eq!(sel.len(), 0);
        assert!(!sel.contains(&asset_id));
        assert!(sel.is_empty())
    }

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("immichctl_test_{}", name));
        // ensure parent exists
        let _ = fs::create_dir_all(&p);
        p.push("selection.json");
        p
    }

    #[test]
    fn load_nonexistent_creates_empty_asset_selection() {
        let path = tmp_path("load_nonexistent");
        // ensure file does not exist
        let _ = fs::remove_file(&path);
        let sel = Assets::load(&path);
        assert_eq!(sel.assets.len(), 0);
        assert_eq!(sel.file, path);
    }

    #[test]
    fn save_and_load_roundtrip_without_assets() {
        let path = tmp_path("roundtrip_no_assets");
        let _ = fs::remove_file(&path);

        let sel = Assets::load(&path);
        sel.save().expect("save failed");

        let loaded = Assets::load(&path);
        assert_eq!(loaded.assets.len(), 0);
        // file path is set on load
        assert_eq!(loaded.file, path);
    }

    #[test]
    fn save_and_load_roundtrip_with_assets() {
        let path = tmp_path("roundtrip_with_assets");
        let _ = fs::remove_file(&path);

        let mut sel = Assets::load(&path);
        let asset = default_asset();
        sel.add_asset(asset);
        sel.save().expect("save failed");

        let loaded = Assets::load(&path);
        assert_eq!(loaded.len(), 1);
        // file path is set on load
        assert_eq!(loaded.file, path);
    }

    #[test]
    fn serialization_skips_file_field() {
        let path = tmp_path("serialize_skip");
        let _ = fs::remove_file(&path);
        let sel = Assets::load(&path);

        let json = serde_json::to_string(&sel).expect("serialize");
        // The JSON should contain assets, but no "file" key
        assert!(json.contains("assets"));
        assert!(!json.contains("\"file\""));
    }

    #[test]
    fn asset_uuids() {
        let mut sel = Assets {
            file: PathBuf::from("test_selection.json"),
            assets: HashMap::new(),
        };
        let asset = default_asset();
        let asset_id = asset.id.clone();
        sel.add_asset(asset);

        let uuids = sel.asset_uuids();
        assert_eq!(uuids.len(), 1);
        assert_eq!(uuids[0], Uuid::parse_str(&asset_id).unwrap());
    }
}

use super::ImmichCtl;
use super::assets::Assets;
use super::types::BulkIdsDto;
use anyhow::{Context, Result, bail};
use uuid::Uuid;

impl ImmichCtl {
    pub async fn album_assign(&mut self, name: &str) -> Result<()> {
        let sel = Assets::load(&self.assets_file);
        if sel.is_empty() {
            eprintln!("Selection is empty, nothing to assign to album.");
            return Ok(());
        }

        let album_id = self.find_album_by_name(name).await?;
        let dto = BulkIdsDto {
            ids: sel.asset_uuids(),
        };
        // TODO: find out meaning of key and slug parameters
        let resp = self
            .immich()?
            .add_assets_to_album(&album_id, None, None, &dto)
            .await
            .context("Could not assign assets to album")?;
        let cnt = resp.iter().filter(|r| r.success).count();
        eprintln!("Assigned {} assets to album '{}'.", cnt, name);
        Ok(())
    }

    pub async fn album_unassign(&mut self, name: &str) -> Result<()> {
        let sel = Assets::load(&self.assets_file);
        if sel.is_empty() {
            eprintln!("Selection is empty, nothing to unassign.");
            return Ok(());
        }

        let album_id = self.find_album_by_name(name).await?;
        let dto = BulkIdsDto {
            ids: sel.asset_uuids(),
        };
        let resp = self
            .immich()?
            .remove_asset_from_album(&album_id, &dto)
            .await
            .context("Could not unassign assets from album")?;
        let cnt = resp.iter().filter(|r| r.success).count();
        eprintln!("Unassigned {} assets from album '{}'.", cnt, name);
        Ok(())
    }

    pub async fn find_album_by_name(&self, name: &str) -> Result<Uuid> {
        let albums_resp = self
            .immich()?
            .get_all_albums(None, None)
            .await
            .context("Could not retrieve albums")?;

        let mut matching_albums: Vec<Result<Uuid>> = albums_resp
            .iter()
            .filter(|a| a.album_name == name)
            .map(|found_album| Uuid::parse_str(&found_album.id).map_err(anyhow::Error::from))
            .collect();

        match matching_albums.len() {
            0 => bail!("Album not found: '{}'", name),
            1 => matching_albums.pop().unwrap(),
            _ => bail!("Album name is not unique: '{}'", name),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::immichctl::tests::create_immichctl_with_server;
    use crate::immichctl::types::{AlbumResponseDto, UserAvatarColor, UserResponseDto};
    use anyhow::Result;
    use chrono::DateTime;
    use uuid::Uuid;

    pub fn create_album(id: &str, name: &str) -> AlbumResponseDto {
        let timestamp = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        AlbumResponseDto {
            id: id.to_string(),
            album_name: name.to_string(),
            owner_id: Uuid::new_v4().to_string(),
            created_at: timestamp,
            updated_at: timestamp,
            asset_count: 1,
            album_thumbnail_asset_id: None,
            shared: false,
            assets: vec![],
            owner: UserResponseDto {
                id: Uuid::new_v4().to_string(),
                email: "test@test.com".to_string(),
                name: "Test User".to_string(),
                avatar_color: UserAvatarColor::Blue,
                profile_image_path: "".to_string(),
                profile_changed_at: timestamp,
            },
            start_date: None,
            end_date: None,
            has_shared_link: false,
            album_users: vec![],
            contributor_counts: vec![],
            description: "".to_string(),
            is_activity_enabled: false,
            last_modified_asset_timestamp: None,
            order: None,
        }
    }

    #[tokio::test]
    async fn test_find_album_by_name() -> Result<()> {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let albums = vec![
            create_album("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1", "Album 1"),
            create_album("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab2", "Album 2"),
            create_album("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab3", "Another Album"),
            create_album("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab4", "Album 2"), // Duplicate name
        ];

        let mock = server
            .mock("GET", "/api/albums")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&albums)?)
            .expect(3)
            .create_async()
            .await;

        // Find an existing album with a unique name
        let result = ctl.find_album_by_name("Album 1").await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1").unwrap()
        );

        // Album not found
        let result = ctl.find_album_by_name("Nonexistent Album").await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Album not found: 'Nonexistent Album'"
        );

        // Album name is not unique
        let result = ctl.find_album_by_name("Album 2").await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Album name is not unique: 'Album 2'"
        );

        mock.assert_async().await;
        Ok(())
    }
}

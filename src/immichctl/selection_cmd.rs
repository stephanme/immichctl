use super::ImmichCtl;
use super::selection::Selection;
use super::types::{AlbumResponseDto, MetadataSearchDto, TagResponseDto};
use anyhow::{Context, Result, bail};
use uuid::Uuid;

impl ImmichCtl {
    pub fn selection_clear(&mut self) -> Result<()> {
        let mut sel = Selection::load(&self.selection_file);
        sel.clear();
        sel.save().context("Could not save selection")?;
        println!("Selection cleared.");
        Ok(())
    }

    pub fn selection_count(&self) {
        let sel = Selection::load(&self.selection_file);
        println!("{}", sel.len());
    }

    pub fn selection_list(&self) {
        let sel = Selection::load(&self.selection_file);
        for asset in sel.list_assets() {
            println!("{}, {}", asset.id, asset.original_file_name);
        }
    }

    pub async fn selection_add(
        &mut self,
        id: &Option<String>,
        tag: &Option<String>,
        album: &Option<String>,
    ) -> Result<()> {
        let mut body = self.build_search_dto(id, tag, album).await?;

        let mut sel = Selection::load(&self.selection_file);
        let old_len = sel.len();
        // TODO map OpenAPI number to i32 (instead of f64)
        let mut page = 1f64;
        while page > 0f64 {
            body.page = Some(page);
            let mut resp = self
                .immich()?
                .search_assets(&body)
                .await
                .context("Search failed")?;
            for asset in resp.assets.items.drain(..) {
                sel.add_asset(asset);
            }
            match &resp.assets.next_page {
                Some(next_page) => {
                    page = next_page
                        .parse::<f64>()
                        .context("Invalid next_page value")?;
                }
                None => page = 0f64,
            }
        }
        sel.save()?;
        let new_len = sel.len();
        println!(
            "Added {} asset(s) to selection.",
            new_len.saturating_sub(old_len)
        );
        Ok(())
    }

    pub async fn selection_remove(
        &mut self,
        id: &Option<String>,
        tag: &Option<String>,
        album: &Option<String>,
    ) -> Result<()> {
        let mut body = self.build_search_dto(id, tag, album).await?;

        let mut sel = Selection::load(&self.selection_file);
        let old_len = sel.len();
        // TODO map OpenAPI number to i32 (instead of f64)
        let mut page = 1f64;
        while page > 0f64 {
            body.page = Some(page);
            let resp = self
                .immich()?
                .search_assets(&body)
                .await
                .context("Search failed")?;
            for asset in resp.assets.items.iter() {
                sel.remove_asset(&asset.id);
            }
            match &resp.assets.next_page {
                Some(next_page) => {
                    page = next_page
                        .parse::<f64>()
                        .context("Invalid next_page value")?;
                }
                None => page = 0f64,
            }
        }
        sel.save()?;
        let new_len = sel.len();
        println!(
            "Removed {} asset(s) from selection.",
            old_len.saturating_sub(new_len)
        );
        Ok(())
    }

    async fn build_search_dto(
        &self,
        id: &Option<String>,
        tag: &Option<String>,
        album: &Option<String>,
    ) -> Result<MetadataSearchDto> {
        let mut search_dto = MetadataSearchDto::default();
        if let Some(id) = id {
            let uuid = uuid::Uuid::parse_str(id).context("Invalid asset id, expected uuid")?;
            search_dto.id = Some(uuid);
        }
        if let Some(tag_name) = tag {
            search_dto.tag_ids = Some(vec![self.find_tag_by_name(tag_name).await?]);
        }
        if let Some(album_name) = album {
            let albums_resp = self
                .immich()?
                .get_all_albums(None, None)
                .await
                .context("Could not retrieve albums")?;
            let album_id = Self::find_album_by_name(album_name, &albums_resp);
            match album_id {
                Some(uuid) => search_dto.album_ids.push(uuid),
                None => {
                    bail!("Album not found: '{}'", album_name);
                }
            }
        }
        // check that at least one search flag is provided
        if search_dto == MetadataSearchDto::default() {
            bail!("Please provide at least one search flag.");
        }
        Ok(search_dto)
    }

    pub async fn find_tag_by_name(&self, name: &str) -> Result<Uuid> {
        let tags_resp = self
            .immich()?
            .get_all_tags()
            .await
            .context("Could not retrieve tags")?;
        let tag_id = Self::_find_tag_by_name(name, &tags_resp);
        match tag_id {
            Some(uuid) => Ok(uuid),
            None => {
                bail!("Tag not found or not unique: '{}'", name);
            }
        }
    }
    /// Find a tag by its full or simple name (full name = including parent tags separated by '/').
    /// Returns the UUID of the tag if found and unambiguous.
    fn _find_tag_by_name(name: &str, tags: &[TagResponseDto]) -> Option<Uuid> {
        let matching_tags: Vec<_> = tags
            .iter()
            .filter(|t| t.name == name || t.value == name)
            .collect();

        if matching_tags.len() == 1 {
            return Uuid::parse_str(&matching_tags[0].id).ok();
        }

        None
    }

    pub fn find_album_by_name(name: &str, albums: &[AlbumResponseDto]) -> Option<Uuid> {
        albums
            .iter()
            .find(|a| a.album_name == name)
            .and_then(|found_album| Uuid::parse_str(&found_album.id).ok())
    }
}

#[cfg(test)]
mod tests {
    use crate::immichctl::tests::create_immichctl_with_server;
    use crate::immichctl::types::{UserAvatarColor, UserResponseDto};

    use super::*;
    use chrono::DateTime;

    fn create_tag(id: &str, value: &str, parent_id: Option<&str>) -> TagResponseDto {
        let timestamp = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let name = value.split('/').last().unwrap_or(value);
        TagResponseDto {
            id: id.to_string(),
            name: name.to_string(),
            value: value.to_string(),
            parent_id: parent_id.map(|s| s.to_string()),
            created_at: timestamp,
            updated_at: timestamp,
            color: None,
        }
    }

    fn create_album(id: &str, name: &str) -> AlbumResponseDto {
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

    #[test]
    fn test_find_album_by_name() {
        let albums = vec![
            create_album("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1", "Album 1"),
            create_album("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab2", "Album 2"),
            create_album("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab3", "Another Album"),
        ];

        // Find an existing album
        assert_eq!(
            ImmichCtl::find_album_by_name("Album 2", &albums),
            Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab2").ok()
        );

        // Album not found
        assert_eq!(
            ImmichCtl::find_album_by_name("Nonexistent Album", &albums),
            None
        );

        // Find another existing album
        assert_eq!(
            ImmichCtl::find_album_by_name("Album 1", &albums),
            Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1").ok()
        );
    }

    #[test]
    fn test_find_tag_by_name() {
        let tags = vec![
            create_tag("5460dc82-2353-47d1-878c-2f15a1084001", "root1", None),
            create_tag("5460dc82-2353-47d1-878c-2f15a1084002", "root2", None),
            create_tag(
                "5460dc82-2353-47d1-878c-2f15a1084003",
                "root1/child1",
                Some("5460dc82-2353-47d1-878c-2f15a1084001"),
            ),
            create_tag(
                "5460dc82-2353-47d1-878c-2f15a1084004",
                "root2/child2",
                Some("5460dc82-2353-47d1-878c-2f15a1084002"),
            ),
            create_tag(
                "5460dc82-2353-47d1-878c-2f15a1084005",
                "root1/child1/grandchild1",
                Some("5460dc82-2353-47d1-878c-2f15a1084003"),
            ),
            create_tag(
                "5460dc82-2353-47d1-878c-2f15a1084006",
                "root2/otherchild",
                Some("5460dc82-2353-47d1-878c-2f15a1084002"),
            ),
            create_tag(
                "5460dc82-2353-47d1-878c-2f15a1084007",
                "root1/non-unique-child",
                Some("5460dc82-2353-47d1-878c-2f15a1084001"),
            ),
            create_tag(
                "5460dc82-2353-47d1-878c-2f15a1084008",
                "root2/non-unique-child",
                Some("5460dc82-2353-47d1-878c-2f15a1084002"),
            ),
        ];

        // Find a root tag
        assert_eq!(
            ImmichCtl::_find_tag_by_name("root1", &tags),
            Uuid::parse_str("5460dc82-2353-47d1-878c-2f15a1084001").ok()
        );

        // Find a nested tag (1 level)
        assert_eq!(
            ImmichCtl::_find_tag_by_name("root1/child1", &tags),
            Uuid::parse_str("5460dc82-2353-47d1-878c-2f15a1084003").ok()
        );

        // Find a deeply nested tag (2 levels)
        assert_eq!(
            ImmichCtl::_find_tag_by_name("root1/child1/grandchild1", &tags),
            Uuid::parse_str("5460dc82-2353-47d1-878c-2f15a1084005").ok()
        );

        // Tag not found (root)
        assert_eq!(ImmichCtl::_find_tag_by_name("nonexistent", &tags), None);

        // Tag not found (child)
        assert_eq!(
            ImmichCtl::_find_tag_by_name("root1/nonexistent", &tags),
            None
        );

        // Tag not found (grandchild)
        assert_eq!(
            ImmichCtl::_find_tag_by_name("root1/child1/nonexistent", &tags),
            None
        );

        // Correct child, wrong parent
        assert_eq!(ImmichCtl::_find_tag_by_name("root2/child1", &tags), None);

        // find by simple name when full name not found
        assert_eq!(
            ImmichCtl::_find_tag_by_name("otherchild", &tags),
            Uuid::parse_str("5460dc82-2353-47d1-878c-2f15a1084006").ok()
        );
        assert_eq!(
            ImmichCtl::_find_tag_by_name("child1", &tags),
            Uuid::parse_str("5460dc82-2353-47d1-878c-2f15a1084003").ok()
        );

        // find non-uniquie-child by full path but not by simple name
        assert_eq!(
            ImmichCtl::_find_tag_by_name("root1/non-unique-child", &tags),
            Uuid::parse_str("5460dc82-2353-47d1-878c-2f15a1084007").ok()
        );
        assert_eq!(
            ImmichCtl::_find_tag_by_name("root2/non-unique-child", &tags),
            Uuid::parse_str("5460dc82-2353-47d1-878c-2f15a1084008").ok()
        );
        assert_eq!(
            ImmichCtl::_find_tag_by_name("non-unique-child", &tags),
            None
        );
    }

    #[tokio::test]
    async fn test_build_search_dto_no_flags() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let result = ctl.build_search_dto(&None, &None, &None).await;

        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Please provide at least one search flag."
        );
    }

    #[tokio::test]
    async fn test_build_search_dto_with_id() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let mut result = ctl
            .build_search_dto(
                &Some("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1".to_string()),
                &None,
                &None,
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            MetadataSearchDto {
                id: Some(Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1").unwrap()),
                ..Default::default()
            }
        );

        result = ctl
            .build_search_dto(&Some("no-uuid".to_string()), &None, &None)
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Invalid asset id, expected uuid"
        );
    }

    #[tokio::test]
    async fn test_build_search_dto_with_tag() -> Result<()> {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let tags = vec![create_tag(
            "a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1",
            "tag1",
            None,
        )];
        let tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&tags).unwrap())
            .create_async()
            .await;

        let mut result = ctl
            .build_search_dto(&None, &Some("tag1".to_string()), &None)
            .await;
        tags_mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            MetadataSearchDto {
                tag_ids: Some(vec!(
                    Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1").unwrap()
                )),
                ..Default::default()
            }
        );

        result = ctl
            .build_search_dto(&None, &Some("no-tag".to_string()), &None)
            .await;
        tags_mock.expect(2).assert_async().await;
        assert!(result.is_err());
        assert_eq!(result.err().unwrap().to_string(), "Tag not found or not unique: 'no-tag'");
        Ok(())
    }

    #[tokio::test]
    async fn test_build_search_dto_with_album() -> Result<()> {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let albums = vec![create_album(
            "a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1",
            "album1",
        )];
        let albums_mock = server
            .mock("GET", "/api/albums")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&albums).unwrap())
            .create_async()
            .await;

        let mut result = ctl
            .build_search_dto(&None, &None, &Some("album1".to_string()))
            .await;
        albums_mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            MetadataSearchDto {
                album_ids: vec!(Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1").unwrap()),
                ..Default::default()
            }
        );

        result = ctl
            .build_search_dto(&None, &None, &Some("no-album".to_string()))
            .await;
        albums_mock.expect(2).assert_async().await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Album not found: 'no-album'"
        );
        Ok(())
    }
}

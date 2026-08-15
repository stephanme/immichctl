use super::ImmichCtl;
use super::assets::Assets;
use super::types::{BulkIdsDto, TagCreateDto, TagCreateDtoColor, TagResponseDto};
use anyhow::{Context, Result, bail};
use uuid::Uuid;

impl ImmichCtl {
    /// Create a new tag.
    ///
    /// The name may be hierarchical (e.g. `parent/child`): the parent path is
    /// resolved to a UUID via [`find_tag_by_name`] and passed as `parentId` to
    /// the Immich API, so the parent tag must already exist.  An optional hex
    /// color (e.g. `#aabbcc` or `aabbcc`) is validated by the generated client
    /// type [`TagCreateDtoColor`].
    pub async fn tag_create(&mut self, name: &str, color: Option<&str>) -> Result<()> {
        // Split the hierarchical tag name into parent path and simple name.
        let (parent_path, name_part) = Self::split_tag_name(name);

        if name_part.is_empty() {
            bail!("Invalid tag name: '{}'", name);
        }

        let parent_id = match parent_path {
            Some(p) if !p.is_empty() => Some(self.find_tag_by_name(p).await?),
            _ => None,
        };

        let color: Option<TagCreateDtoColor> = color
            .map(|c| {
                c.try_into()
                    .context("Invalid color format, use hex like #aabbcc or aabbcc")
            })
            .transpose()?;

        let dto = TagCreateDto {
            name: name_part.to_string(),
            parent_id,
            color,
        };

        let resp = self
            .immich()?
            .create_tag(&dto)
            .await
            .context("Could not create tag")?
            .into_inner();

        eprintln!("Created tag '{}' (id: {}).", name, resp.id);
        Ok(())
    }

    /// Delete a tag by its name (full hierarchical name or simple name).
    ///
    /// This operation is **idempotent**: if the tag does not exist, a warning is
    /// printed and the method returns `Ok(())`.  If the name matches multiple
    /// tags, an error is returned so that ambiguous deletions are avoided.
    pub async fn tag_delete(&mut self, name: &str) -> Result<()> {
        let client = self.immich()?;
        let tags_resp = client
            .get_all_tags()
            .await
            .context("Could not retrieve tags")?;
        let matches = Self::_matching_tags(name, &tags_resp);

        match matches.len() {
            0 => {
                eprintln!("Warning: tag '{}' not found, nothing to delete.", name);
                Ok(())
            }
            1 => {
                let tag_id = matches[0].id;
                client
                    .delete_tag(&tag_id)
                    .await
                    .context("Could not delete tag")?;
                eprintln!("Deleted tag '{}'.", name);
                Ok(())
            }
            _ => {
                bail!(
                    "Tag name '{}' matches {} tags. Use the full hierarchical name for unambiguous matching.",
                    name,
                    matches.len()
                );
            }
        }
    }

    pub async fn tag_assign(&mut self, name: &str) -> Result<()> {
        let sel = Assets::load(&self.assets_file);
        if sel.is_empty() {
            eprintln!("Selection is empty, nothing to tag.");
            return Ok(());
        }

        let tag_id = self.find_tag_by_name(name).await?;
        let dto = BulkIdsDto {
            ids: sel.asset_uuids(),
        };
        let tag_resp = self
            .immich()?
            .tag_assets(&tag_id, &dto)
            .await
            .context("Could not tag assets")?;
        let cnt = tag_resp.iter().filter(|r| r.success).count();
        eprintln!("Tagged {} assets with '{}'.", cnt, name);
        Ok(())
    }

    pub async fn tag_unassign(&mut self, name: &str) -> Result<()> {
        let sel = Assets::load(&self.assets_file);
        if sel.is_empty() {
            eprintln!("Selection is empty, nothing to untag.");
            return Ok(());
        }

        let tag_id = self.find_tag_by_name(name).await?;
        let dto = BulkIdsDto {
            ids: sel.asset_uuids(),
        };
        let untag_resp = self
            .immich()?
            .untag_assets(&tag_id, &dto)
            .await
            .context("Could not untag assets")?;
        let cnt = untag_resp.iter().filter(|r| r.success).count();
        eprintln!("Untagged {} assets from '{}'.", cnt, name);
        Ok(())
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
    /// Split a hierarchical tag name (e.g. `parent/child`) into the parent
    /// path (if any) and the simple name (last component).
    fn split_tag_name(name: &str) -> (Option<&str>, &str) {
        match name.rfind('/') {
            Some(idx) => (Some(&name[..idx]), &name[idx + 1..]),
            None => (None, name),
        }
    }

    /// Find all tags matching a name or value (full or simple name).
    fn _matching_tags<'a>(name: &str, tags: &'a [TagResponseDto]) -> Vec<&'a TagResponseDto> {
        tags.iter()
            .filter(|t| t.name == name || t.value == name)
            .collect()
    }

    /// Find a tag by its full or simple name (full name = including parent tags separated by '/').
    /// Returns the UUID of the tag if found and unambiguous.
    fn _find_tag_by_name(name: &str, tags: &[TagResponseDto]) -> Option<Uuid> {
        let matching_tags = Self::_matching_tags(name, tags);
        if matching_tags.len() == 1 {
            return Some(matching_tags[0].id);
        }

        None
    }

    pub async fn tag_list(&self) -> Result<()> {
        let tags_resp = self
            .immich()?
            .get_all_tags()
            .await
            .context("Could not retrieve tags")?;
        let mut tags: Vec<&TagResponseDto> = tags_resp.iter().collect();
        tags.sort_by(|a, b| a.value.cmp(&b.value));
        for tag in tags {
            println!("{}", tag.value);
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::immichctl::tests::create_immichctl_with_server;
    use chrono::DateTime;

    pub fn create_tag(id: &str, value: &str, parent_id: Option<&str>) -> TagResponseDto {
        let timestamp = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (_, name) = ImmichCtl::split_tag_name(value);
        TagResponseDto {
            id: Uuid::parse_str(id).unwrap(),
            name: name.to_string(),
            value: value.to_string(),
            parent_id: parent_id.map(|s| s.to_string()),
            created_at: timestamp,
            updated_at: timestamp,
            color: None,
        }
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
    async fn test_tag_create_no_parent() -> Result<()> {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        let created_tag = create_tag("11111111-1111-4111-8111-111111111111", "test_tag", None);

        let create_mock = server
            .mock("POST", "/api/tags")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "name": "test_tag"
            })))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&created_tag)?)
            .create_async()
            .await;

        let result = ctl.tag_create("test_tag", None).await;
        assert!(result.is_ok(), "{:?}", result.err());

        create_mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_tag_create_with_color() -> Result<()> {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        let created_tag = create_tag("11111111-1111-4111-8111-111111111111", "color_tag", None);

        let create_mock = server
            .mock("POST", "/api/tags")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "name": "color_tag",
                "color": "#ff0000"
            })))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&created_tag)?)
            .create_async()
            .await;

        let result = ctl.tag_create("color_tag", Some("#ff0000")).await;
        assert!(result.is_ok(), "{:?}", result.err());

        create_mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_tag_create_with_parent() -> Result<()> {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        let parent_tag = create_tag("22222222-2222-4222-8222-222222222222", "parent", None);
        let created_tag = create_tag(
            "33333333-3333-4333-8333-333333333333",
            "parent/child",
            Some("22222222-2222-4222-8222-222222222222"),
        );

        // Mock GET /api/tags to return the parent tag
        let tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![&parent_tag])?)
            .create_async()
            .await;

        // Mock POST /api/tags — body must include parentId
        let create_mock = server
            .mock("POST", "/api/tags")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "name": "child",
                "parentId": "22222222-2222-4222-8222-222222222222"
            })))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&created_tag)?)
            .create_async()
            .await;

        let result = ctl.tag_create("parent/child", None).await;
        assert!(result.is_ok(), "{:?}", result.err());

        tags_mock.assert_async().await;
        create_mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_tag_create_parent_not_found() -> Result<()> {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        let tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let result = ctl.tag_create("nonexistent/child", None).await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Tag not found or not unique: 'nonexistent'")
        );

        tags_mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_tag_delete() -> Result<()> {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        let tag = create_tag("11111111-1111-4111-8111-111111111111", "test_tag", None);

        let tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![&tag])?)
            .create_async()
            .await;

        let delete_mock = server
            .mock("DELETE", "/api/tags/11111111-1111-4111-8111-111111111111")
            .with_status(204)
            .create_async()
            .await;

        let result = ctl.tag_delete("test_tag").await;
        assert!(result.is_ok(), "{:?}", result.err());

        tags_mock.assert_async().await;
        delete_mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_tag_delete_not_found() -> Result<()> {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        let tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        // Tag not found is idempotent — should succeed with a warning
        let result = ctl.tag_delete("nonexistent").await;
        assert!(result.is_ok(), "{:?}", result.err());

        tags_mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_tag_delete_not_unique() -> Result<()> {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        // Two tags with the same simple name under different parents
        let tag1 = create_tag(
            "11111111-1111-4111-8111-111111111111",
            "parent1/child",
            Some("11111111"),
        );
        let tag2 = create_tag(
            "22222222-2222-4222-8222-222222222222",
            "parent2/child",
            Some("22222222"),
        );

        let tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![&tag1, &tag2])?)
            .create_async()
            .await;

        // Deleting by simple name with multiple matches should fail
        let result = ctl.tag_delete("child").await;
        assert!(result.is_err());
        assert!(
            result.err().unwrap().to_string().contains("matches 2 tags"),
            "Expected error about ambiguous tag name"
        );

        tags_mock.assert_async().await;
        Ok(())
    }
}

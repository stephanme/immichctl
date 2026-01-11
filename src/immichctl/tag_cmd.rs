use super::ImmichCtl;
use super::assets::Assets;
use super::types::{BulkIdsDto, TagBulkAssetsDto, TagResponseDto};
use anyhow::{Context, Result, bail};
use uuid::Uuid;

impl ImmichCtl {
    pub async fn tag_assign(&mut self, name: &str) -> Result<()> {
        let sel = Assets::load(&self.assets_file);
        if sel.is_empty() {
            eprintln!("Selection is empty, nothing to tag.");
            return Ok(());
        }

        let tag_id = self.find_tag_by_name(name).await?;
        let dto = TagBulkAssetsDto {
            asset_ids: sel.asset_uuids(),
            tag_ids: vec![tag_id],
        };
        let tagged_assets = self
            .immich()?
            .bulk_tag_assets(&dto)
            .await
            .context("Could not tag assets")?;
        eprintln!("Tagged {} assets with '{}'.", tagged_assets.count, name);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    pub fn create_tag(id: &str, value: &str, parent_id: Option<&str>) -> TagResponseDto {
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
}

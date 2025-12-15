use crate::immichctl::selection::Selection;
use crate::immichctl::types::TagBulkAssetsDto;
use crate::immichctl::{ImmichCtl, types::BulkIdsDto};
use anyhow::{Context, Result};

impl ImmichCtl {
    pub async fn tag_add(&mut self, name: &str) -> Result<()> {
        let sel = Selection::load(&self.selection_file);
        if sel.is_empty() {
            println!("Selection is empty, nothing to tag.");
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
        println!("Tagged {} assets with '{}'.", tagged_assets.count, name);
        Ok(())
    }

    pub async fn tag_remove(&mut self, name: &str) -> Result<()> {
        let sel = Selection::load(&self.selection_file);
        if sel.is_empty() {
            println!("Selection is empty, nothing to untag.");
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
        println!("Untagged {} assets from '{}'.", cnt, name);
        Ok(())
    }
}

use super::ImmichCtl;
use super::selection::Selection;
use super::types::{MetadataSearchDto};
use anyhow::{Context, Result, bail};

impl ImmichCtl {
    pub fn selection_clear(&mut self) -> Result<()>{
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
            println!("{}", asset.id);
        }
    }

    pub async fn selection_add(&mut self, id: &Option<String>, tag: &Option<String>) -> Result<()> {
        self.assert_logged_in()?;
        let mut body = MetadataSearchDto::default();
        if let Some(id) = id {
            let uuid = uuid::Uuid::parse_str(id).context("Invalid asset id, expected uuid")?;
            body.id = Some(uuid);
        }
        if let Some(tag_name) = tag {
            let tags_resp =  self.immich.get_all_tags().await.context("Could not retrieve tags")?;
            let maybe_tag = tags_resp.iter().find(|t| t.name == *tag_name);
            match maybe_tag {
                Some(t) => {
                    let tag_uuid = uuid::Uuid::parse_str(&t.id)
                        .unwrap_or_else(|_| panic!("Could not parse tag id {}", &t.id));
                    body.tag_ids = Some(vec![tag_uuid]);
                }
                None => {
                    bail!("Tag not found: '{}'", tag_name);
                }
            }
        }
        // check that at least one search flag is provided
        if body.id.is_none() && body.tag_ids.as_ref().map(|v| v.is_empty()).unwrap_or(true) {
            bail!("Please provide at least one search flag.");
        }
        // TODO: handle pagination
        let mut resp = self.immich.search_assets(&body).await.context("Search failed")?;
        let mut sel = Selection::load(&self.selection_file);
        let old_len = sel.len();
        for asset in resp.assets.items.drain(..) {
            sel.add_asset(asset);
        }
        sel.save()?;
        let new_len = sel.len();
        println!(
            "Added {} asset(s) to selection.",
            new_len.saturating_sub(old_len)
        );
        Ok(())
    }
}

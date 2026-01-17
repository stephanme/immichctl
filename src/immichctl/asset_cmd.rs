use std::borrow::Cow;

use super::ImmichCtl;
use super::assets::Assets;
use super::types::{AlbumResponseDto, AssetResponseDto, MetadataSearchDto, UpdateAssetDto};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, TimeDelta, Utc};
use uuid::Uuid;

/// Columns for CSV listing of selected assets
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum AssetColumns {
    /// Asset UUID
    Id,
    /// Original file name (alias: file)
    #[value(alias("file"))]
    OriginalFileName,
    /// File creation timestamp [UTC] (alias: created)
    #[value(alias("created"))]
    FileCreatedAt,
    /// Timezone (= DateTimeOriginal - created)
    Timezone,
    /// DateTimeOriginal from asset metadata with timezone (alias: datetime)
    #[value(alias("datetime"))]
    DateTimeOriginal,

    /// Timezone from EXIF metadata
    ExifTimezone,
    /// DateTimeOriginal from EXIF metadata with timezone (alias: exif-datetime)
    #[value(alias("exif-datetime"))]
    ExifDateTimeOriginal,
}

impl ImmichCtl {
    pub fn assets_clear(&mut self) -> Result<()> {
        let mut sel = Assets::load(&self.assets_file);
        sel.clear();
        sel.save().context("Could not save asset selection")?;
        eprintln!("Asset selection cleared.");
        Ok(())
    }

    pub fn assets_count(&self) {
        let sel = Assets::load(&self.assets_file);
        println!("{}", sel.len());
    }

    pub async fn assets_refresh(&mut self) -> Result<()> {
        let mut sel = Assets::load(&self.assets_file);
        let total = sel.len();
        if total == 0 {
            eprintln!("No assets to refresh.");
            return Ok(());
        }
        for (i, asset) in sel.iter_mut_assets().enumerate() {
            let uuid = Uuid::parse_str(&asset.id)
                .with_context(|| format!("Invalid asset id '{}', expected uuid", asset.id))?;
            let asset_res = self
                .immich()?
                .get_asset_info(&uuid, None, None)
                .await
                .with_context(|| format!("Could not retrieve asset '{}'", asset.id))?;
            *asset = asset_res.into_inner();
            self.eprint_progress_indicator(i, total, 50);
        }
        sel.save()?;
        eprintln!("Refreshed metadata for {} assets.", sel.len());
        Ok(())
    }

    pub fn assets_list_json(&self, pretty: bool) -> Result<()> {
        let sel = Assets::load(&self.assets_file);
        let assets: Vec<_> = sel.iter_assets().collect();
        let stdout = std::io::stdout();
        let writer = stdout.lock();
        if pretty {
            serde_json::to_writer_pretty(writer, &assets)?;
        } else {
            serde_json::to_writer(writer, &assets)?;
        }
        Ok(())
    }

    pub fn assets_list_csv(&self, columns: &[AssetColumns]) {
        let sel = Assets::load(&self.assets_file);
        for asset in sel.iter_assets() {
            for (i, col) in columns.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!("{}", Self::asset_column(asset, *col));
            }
            println!();
        }
    }

    fn asset_column(asset: &AssetResponseDto, col: AssetColumns) -> Cow<'_, str> {
        match col {
            AssetColumns::Id => Cow::Borrowed(&asset.id),
            AssetColumns::OriginalFileName => Cow::Borrowed(&asset.original_file_name),
            AssetColumns::FileCreatedAt => Cow::Owned(asset.file_created_at.to_rfc3339()),
            AssetColumns::Timezone => Cow::Owned(Self::asset_timezone_offset(asset).to_string()),
            AssetColumns::DateTimeOriginal => {
                Cow::Owned(Self::get_assert_date_time_original(asset).to_rfc3339())
            }
            AssetColumns::ExifTimezone => {
                if let Some(exif_info) = &asset.exif_info {
                    if let Some(tz_str) = &exif_info.time_zone {
                        Cow::Borrowed(tz_str)
                    } else {
                        Cow::Borrowed("")
                    }
                } else {
                    Cow::Borrowed("")
                }
            }
            AssetColumns::ExifDateTimeOriginal => {
                if let Some(date_time_original) = Self::get_exif_date_time_original(asset) {
                    Cow::Owned(date_time_original.to_rfc3339())
                } else {
                    Cow::Borrowed("")
                }
            }
        }
    }

    pub async fn assets_search_add(
        &mut self,
        id: &Option<String>,
        tag: &Option<String>,
        album: &Option<String>,
        taken_after: &Option<DateTime<FixedOffset>>,
        taken_before: &Option<DateTime<FixedOffset>>,
    ) -> Result<()> {
        let mut search_dto = self
            .build_search_dto(id, tag, album, taken_after,taken_before)
            .await?;
        search_dto.with_exif = Some(true);

        let mut sel = Assets::load(&self.assets_file);
        let old_len = sel.len();
        // TODO map OpenAPI number to i32 (instead of f64)
        let mut page = 1f64;
        while page > 0f64 {
            search_dto.page = Some(page);
            let mut resp = self
                .immich()?
                .search_assets(&search_dto)
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
        eprintln!(
            "Added {} asset(s) to selection.",
            new_len.saturating_sub(old_len)
        );
        Ok(())
    }

    pub async fn assets_search_remove(
        &mut self,
        id: &Option<String>,
        tag: &Option<String>,
        album: &Option<String>,
        timezone: &Option<FixedOffset>,
        taken_after: &Option<DateTime<FixedOffset>>,
        taken_before: &Option<DateTime<FixedOffset>>,
    ) -> Result<()> {
        let mut assets = Assets::load(&self.assets_file);
        let old_len = assets.len();

        // check for remove operations that can be handled locally
        match (id, tag, album, timezone, taken_after, taken_before) {
            // remove by id
            (Some(id), None, None, None, None, None) => {
                let uuid = Uuid::parse_str(id)
                    .with_context(|| format!("Invalid asset id '{}', expected uuid", id))?;
                assets.remove_asset(&uuid.to_string());
            }
            // remove by timezone
            (None, None, None, Some(tz), None, None) => {
                assets.retain(|asset| {
                    let asset_tz = match ImmichCtl::exif_timezone_offset(asset) {
                        Some(tz) => tz,
                        None => ImmichCtl::asset_timezone_offset(asset),
                    };
                    asset_tz != *tz
                });
            }
            (None, None, None, None, Some(taken_after), None) => {
                assets.retain(|asset| {
                    let dto = ImmichCtl::get_date_time_original(asset);
                    dto <= *taken_after
                });
            }
            (None, None, None, None, None, Some(taken_before)) => {
                assets.retain(|asset| {
                    let dto = ImmichCtl::get_date_time_original(asset);
                    dto >= *taken_before
                });
            }
            (None, None, None, None, Some(taken_after), Some(taken_before)) => {
                assets.retain(|asset| {
                    let dto = ImmichCtl::get_date_time_original(asset);
                    !(dto > *taken_after && dto < *taken_before)
                });
            }
            _ => {
                if let Some(_tz) = timezone {
                    bail!(
                        "The --timezone option cannot be used together with other search options."
                    );
                }
                // remove by searching on the server
                let search_dto = self.build_search_dto(id, tag, album, taken_after, taken_before).await?;
                self.assets_search_remove_by_immich_query(search_dto, &mut assets)
                    .await?;
            }
        }

        assets.save()?;
        let new_len = assets.len();
        eprintln!(
            "Removed {} asset(s) from selection.",
            old_len.saturating_sub(new_len)
        );
        Ok(())
    }

    async fn assets_search_remove_by_immich_query(
        &mut self,
        mut search_dto: MetadataSearchDto,
        assets: &mut Assets,
    ) -> Result<()> {
        // TODO map OpenAPI number to i32 (instead of f64)
        let mut page = 1f64;
        while page > 0f64 {
            search_dto.page = Some(page);
            let resp = self
                .immich()?
                .search_assets(&search_dto)
                .await
                .context("Search failed")?;
            for asset in resp.assets.items.iter() {
                assets.remove_asset(&asset.id);
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
        Ok(())
    }

    async fn build_search_dto(
        &self,
        id: &Option<String>,
        tag: &Option<String>,
        album: &Option<String>,
        taken_after: &Option<DateTime<FixedOffset>>,
        taken_before: &Option<DateTime<FixedOffset>>,
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
        if let Some(taken_after) = taken_after {
            search_dto.taken_after = Some(taken_after.with_timezone(&Utc));
        }
        if let Some(taken_before) = taken_before {
            search_dto.taken_before = Some(taken_before.with_timezone(&Utc));
        }
        // check that at least one search flag is provided
        if search_dto == MetadataSearchDto::default() {
            bail!("Please provide at least one search flag.");
        }
        Ok(search_dto)
    }

    fn find_album_by_name(name: &str, albums: &[AlbumResponseDto]) -> Option<Uuid> {
        albums
            .iter()
            .find(|a| a.album_name == name)
            .and_then(|found_album| Uuid::parse_str(&found_album.id).ok())
    }

    pub async fn assets_datetime_adjust(
        &mut self,
        offset: &TimeDelta,
        timezone: &Option<FixedOffset>,
        dry_run: bool,
    ) -> Result<()> {
        let mut assets = Assets::load(&self.assets_file);
        let total = assets.len();
        for (i, asset) in assets.iter_mut_assets().enumerate() {
            let (old_date_time_original, new_date_time_original) =
                Self::adjust_date_time_original(asset, offset, timezone);
            if dry_run {
                println!(
                    "{}: {} -> {}",
                    asset.original_file_name, old_date_time_original, new_date_time_original
                );
                continue;
            }

            let uuid = Uuid::parse_str(&asset.id)
                .with_context(|| format!("Invalid asset id '{}', expected uuid", asset.id))?;

            let asset_res = self
                .immich()?
                .update_asset(
                    &uuid,
                    &UpdateAssetDto {
                        date_time_original: Some(new_date_time_original.to_rfc3339()),
                        ..Default::default()
                    },
                )
                .await
                .with_context(|| format!("Could not update asset '{}'", asset.id))?;
            // !!! response: file_created_at and local_date_time are not updated, only exif data is updated !!!
            *asset = asset_res.into_inner();
            self.eprint_progress_indicator(i, total, 50);
        }
        if !dry_run {
            eprintln!("Updated date/time for {} assets.", total);
            assets.save()?;
        }
        Ok(())
    }

    fn adjust_date_time_original(
        asset: &AssetResponseDto,
        offset: &TimeDelta,
        new_timezone: &Option<FixedOffset>,
    ) -> (chrono::DateTime<FixedOffset>, chrono::DateTime<FixedOffset>) {
        let date_time_original = Self::get_date_time_original(asset);

        let asset_tz = date_time_original.timezone();
        let tz = if let Some(tz) = new_timezone {
            tz
        } else {
            &asset_tz
        };
        // let timezone_offset = tz.utc_minus_local() - asset_tz.utc_minus_local();
        let new_date_time_original = date_time_original + *offset;
        // date_time_original + chrono::Duration::seconds(timezone_offset as i64) + *offset;
        (date_time_original, new_date_time_original.with_timezone(tz))
    }

    fn get_date_time_original(asset: &AssetResponseDto) -> chrono::DateTime<FixedOffset> {
        if let Some(date_time_original) = Self::get_exif_date_time_original(asset) {
            return date_time_original;
        }
        Self::get_assert_date_time_original(asset)
    }

    fn get_exif_date_time_original(
        asset: &AssetResponseDto,
    ) -> Option<chrono::DateTime<FixedOffset>> {
        if let Some(exif_info) = &asset.exif_info
            && let Some(date_time_original) = &exif_info.date_time_original
            && let Some(tz_str) = &exif_info.time_zone
            && let Ok(tz) = Self::parse_exif_timezone(tz_str)
        {
            return Some(date_time_original.with_timezone(&tz));
        }
        None
    }

    fn exif_timezone_offset(asset: &AssetResponseDto) -> Option<FixedOffset> {
        if let Some(exif_info) = &asset.exif_info
            && let Some(tz_str) = &exif_info.time_zone
            && let Ok(tz) = Self::parse_exif_timezone(tz_str)
        {
            return Some(tz);
        }
        None
    }

    fn get_assert_date_time_original(asset: &AssetResponseDto) -> chrono::DateTime<FixedOffset> {
        let tz = Self::asset_timezone_offset(asset);
        asset.file_created_at.with_timezone(&tz)
    }

    fn asset_timezone_offset(asset: &AssetResponseDto) -> FixedOffset {
        let delta = asset
            .local_date_time
            .signed_duration_since(asset.file_created_at);
        let delta_sec = delta.num_seconds() as i32;
        FixedOffset::east_opt(delta_sec).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap())
    }

    fn parse_exif_timezone(tz_str: &str) -> Result<FixedOffset> {
        let tz_str = tz_str.trim();
        if tz_str.is_empty() {
            bail!("Timezone string cannot be empty");
        }
        if tz_str == "UTC" {
            return FixedOffset::east_opt(0)
                .ok_or_else(|| anyhow::anyhow!("Invalid timezone offset value: {}", tz_str));
        }

        // Handle "UTC" prefix
        let tz_str = if let Some(stripped) = tz_str.strip_prefix("UTC") {
            stripped
        } else {
            tz_str
        };

        let sign_char = tz_str
            .chars()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid timezone format: missing sign"))?;
        let sign = match sign_char {
            '+' => 1,
            '-' => -1,
            _ => bail!("Timezone must start with '+' or '-'"),
        };

        let mut parts = tz_str[1..].split(':');
        let hours_str = parts.next().unwrap_or("");
        let minutes_str = parts.next().unwrap_or("0");

        let (hours, minutes) = if !hours_str.contains(':') && hours_str.len() > 2 {
            // Handle "HHMM" format
            if hours_str.len() != 4 {
                bail!(
                    "Invalid timezone format: expected HHMM, found '{}'",
                    hours_str
                );
            }
            let h = hours_str[0..2].parse::<i32>()?;
            let m = hours_str[2..4].parse::<i32>()?;
            (h, m)
        } else {
            // Handle "H", "HH", or "H:MM", "HH:MM"
            let h = hours_str.parse::<i32>()?;
            let m = minutes_str.parse::<i32>()?;
            (h, m)
        };

        if hours > 14 || minutes > 59 {
            bail!("Invalid timezone offset: hours must be <= 14 and minutes <= 59");
        }

        let total_seconds = (hours * 3600 + minutes * 60) * sign;
        FixedOffset::east_opt(total_seconds)
            .ok_or_else(|| anyhow::anyhow!("Invalid timezone offset value: {}", tz_str))
    }
}

#[cfg(test)]
mod tests {
    use crate::immichctl::tests::create_immichctl_with_server;
    use crate::immichctl::types::{
        AssetTypeEnum, AssetVisibility, ExifResponseDto, TagResponseDto, UserAvatarColor,
        UserResponseDto,
    };

    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

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

    fn create_asset_with_timestamps(
        file_created_at: DateTime<Utc>,
        local_date_time: DateTime<Utc>,
    ) -> AssetResponseDto {
        let timestamp = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        AssetResponseDto {
            id: Uuid::new_v4().to_string(),
            original_file_name: "test.jpg".to_string(),
            file_created_at,
            local_date_time,
            checksum: "checksum".to_string(),
            created_at: timestamp,
            device_asset_id: "device_asset_id".to_string(),
            device_id: "device_id".to_string(),
            duplicate_id: None,
            duration: "0:00".to_string(),
            exif_info: None,
            file_modified_at: timestamp,
            has_metadata: true,
            is_archived: false,
            is_favorite: false,
            is_offline: false,
            is_trashed: false,
            library_id: None,
            live_photo_video_id: None,
            original_mime_type: None,
            original_path: "original_path".to_string(),
            owner: None,
            owner_id: "owner_id".to_string(),
            people: vec![],
            tags: vec![],
            type_: AssetTypeEnum::Image,
            updated_at: timestamp,
            resized: None,
            stack: None,
            thumbhash: None,
            unassigned_faces: vec![],
            visibility: AssetVisibility::Timeline,
        }
    }

    fn create_asset_with_exif(
        file_created_at: DateTime<Utc>,
        local_date_time: DateTime<Utc>,
        exif_date_time: Option<DateTime<Utc>>,
        exif_time_zone: Option<String>,
    ) -> AssetResponseDto {
        let mut asset = create_asset_with_timestamps(file_created_at, local_date_time);
        asset.exif_info = Some(ExifResponseDto {
            date_time_original: exif_date_time,
            time_zone: exif_time_zone,
            ..Default::default()
        });
        asset
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

    #[tokio::test]
    async fn test_assets_refresh_retrieval_error_includes_id() {
        let (mut ctl, mut server) = create_immichctl_with_server().await;

        // Prepare selection with a valid UUID that will trigger a 404/500
        let file_created_at = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
        let mut asset = create_asset_with_timestamps(file_created_at, local_date_time);
        let asset_id = Uuid::new_v4().to_string();
        asset.id = asset_id.clone();

        let mut sel = Assets::load(&ctl.assets_file);
        sel.add_asset(asset);
        sel.save().expect("failed to save selection");

        // Mock GET /api/assets/{id} to fail
        let _m = server
            .mock("GET", format!("/api/assets/{}", asset_id).as_str())
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body("{\"error\":\"not found\"}")
            .create_async()
            .await;

        let result = ctl.assets_refresh().await;
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains(&format!("Could not retrieve asset '{}'", asset_id)));
    }

    #[test]
    fn test_asset_timezone_offset() {
        // Case 1: Positive offset (+2 hours)
        let file_created_at = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
        let asset = create_asset_with_timestamps(file_created_at, local_date_time);
        assert_eq!(
            ImmichCtl::asset_timezone_offset(&asset),
            FixedOffset::east_opt(2 * 3600).unwrap()
        );

        // Case 2: Negative offset (-3 hours)
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 7, 0, 0).unwrap();
        let asset = create_asset_with_timestamps(file_created_at, local_date_time);
        assert_eq!(
            ImmichCtl::asset_timezone_offset(&asset),
            FixedOffset::east_opt(-3 * 3600).unwrap()
        );

        // Case 3: Zero offset (UTC)
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
        let asset = create_asset_with_timestamps(file_created_at, local_date_time);
        assert_eq!(
            ImmichCtl::asset_timezone_offset(&asset),
            FixedOffset::east_opt(0).unwrap()
        );

        // Case 4: Out-of-range offset (> 24 hours), should default to UTC
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap(); // 26 hours difference
        let asset = create_asset_with_timestamps(file_created_at, local_date_time);
        assert_eq!(
            ImmichCtl::asset_timezone_offset(&asset),
            FixedOffset::east_opt(0).unwrap()
        );
    }

    #[test]
    fn test_asset_column() {
        let file_created_at = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(); // +2h offset
        let asset = create_asset_with_timestamps(file_created_at, local_date_time);

        // Test basic columns
        assert_eq!(ImmichCtl::asset_column(&asset, AssetColumns::Id), asset.id);
        assert_eq!(
            ImmichCtl::asset_column(&asset, AssetColumns::OriginalFileName),
            "test.jpg"
        );
        assert_eq!(
            ImmichCtl::asset_column(&asset, AssetColumns::FileCreatedAt),
            "2024-01-01T10:00:00+00:00"
        );
        assert_eq!(
            ImmichCtl::asset_column(&asset, AssetColumns::Timezone),
            "+02:00"
        );
        assert_eq!(
            ImmichCtl::asset_column(&asset, AssetColumns::DateTimeOriginal),
            "2024-01-01T12:00:00+02:00"
        );

        // Test EXIF columns with full data (with changed month to verify correctness)
        let exif_dt = Utc.with_ymd_and_hms(2024, 2, 1, 10, 0, 0).unwrap();
        let asset_with_exif = create_asset_with_exif(
            file_created_at,
            local_date_time,
            Some(exif_dt),
            Some("+02:00".to_string()),
        );

        assert_eq!(
            ImmichCtl::asset_column(&asset_with_exif, AssetColumns::ExifTimezone),
            "+02:00"
        );
        assert_eq!(
            ImmichCtl::asset_column(&asset_with_exif, AssetColumns::ExifDateTimeOriginal),
            "2024-02-01T12:00:00+02:00"
        );

        // Test EXIF columns with missing timezone in EXIF -> no exif datetime output
        let asset_with_partial_exif =
            create_asset_with_exif(file_created_at, local_date_time, Some(exif_dt), None);
        assert_eq!(
            ImmichCtl::asset_column(&asset_with_partial_exif, AssetColumns::ExifTimezone),
            ""
        );
        assert_eq!(
            ImmichCtl::asset_column(&asset_with_partial_exif, AssetColumns::ExifDateTimeOriginal),
            ""
        );

        // Test EXIF columns with no EXIF data at all
        assert_eq!(
            ImmichCtl::asset_column(&asset, AssetColumns::ExifTimezone),
            ""
        );
        assert_eq!(
            ImmichCtl::asset_column(&asset, AssetColumns::ExifDateTimeOriginal),
            ""
        );
    }

    #[test]
    fn test_parse_exif_timezone() {
        assert_eq!(
            ImmichCtl::parse_exif_timezone("+02:00").unwrap(),
            FixedOffset::east_opt(2 * 3600).unwrap()
        );
        assert_eq!(
            ImmichCtl::parse_exif_timezone("UTC+2").unwrap(),
            FixedOffset::east_opt(2 * 3600).unwrap()
        );
        for tz_str in &[
            "UTC",
            "UTC+0",
            "UTC-0",
            "UTC+00:00",
            "+00:00",
            "-00:00",
            "+0",
            "-0",
        ] {
            assert_eq!(
                ImmichCtl::parse_exif_timezone(tz_str).unwrap(),
                FixedOffset::east_opt(0).unwrap()
            );
        }
        assert_eq!(
            ImmichCtl::parse_exif_timezone("-0530").unwrap(),
            FixedOffset::east_opt(-5 * 3600 - 30 * 60).unwrap()
        );
        assert_eq!(
            ImmichCtl::parse_exif_timezone("+1").unwrap(),
            FixedOffset::east_opt(3600).unwrap()
        );
        assert!(ImmichCtl::parse_exif_timezone("invalid").is_err());
        assert!(ImmichCtl::parse_exif_timezone("").is_err());
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

    #[tokio::test]
    async fn test_build_search_dto_no_flags() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let result = ctl
            .build_search_dto(&None, &None, &None, &None, &None)
            .await;

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
            .build_search_dto(&Some("no-uuid".to_string()), &None, &None, &None, &None)
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
            .build_search_dto(&None, &Some("tag1".to_string()), &None, &None, &None)
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
            .build_search_dto(&None, &Some("no-tag".to_string()), &None, &None, &None)
            .await;
        tags_mock.expect(2).assert_async().await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Tag not found or not unique: 'no-tag'"
        );
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
            .build_search_dto(&None, &None, &Some("album1".to_string()), &None, &None)
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
            .build_search_dto(&None, &None, &Some("no-album".to_string()), &None, &None)
            .await;
        albums_mock.expect(2).assert_async().await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Album not found: 'no-album'"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_build_search_dto_with_taken_before_after() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let taken_after_str = "2024-07-18T00:00:00+00:00";
        let taken_before_str = "2024-07-18T23:59:59+00:00";
        let taken_after = DateTime::parse_from_rfc3339(taken_after_str).ok();
        let taken_before = DateTime::parse_from_rfc3339(taken_before_str).ok();

        let result = ctl
            .build_search_dto(&None, &None, &None, &taken_after, &taken_before)
            .await;

        assert!(result.is_ok());
        let search_dto = result.unwrap();
        assert_eq!(
            search_dto.taken_after,
            Some(taken_after.unwrap().with_timezone(&Utc))
        );
        assert_eq!(
            search_dto.taken_before,
            Some(taken_before.unwrap().with_timezone(&Utc))
        );
    }

    #[test]
    fn test_adjust_date_time_original_no_exif() {
        let file_created_at = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(); // +2h offset
        let asset = create_asset_with_timestamps(file_created_at, local_date_time);

        // No offset, no timezone change
        let offset = TimeDelta::zero();
        let new_timezone = None;
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.0.to_rfc3339(), "2024-01-01T12:00:00+02:00");
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T12:00:00+02:00");

        // Positive offset, no timezone change
        let offset = TimeDelta::hours(1);
        let new_timezone = None;
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T13:00:00+02:00");

        // Negative offset, no timezone change
        let offset = TimeDelta::hours(-3);
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T09:00:00+02:00");

        // Timezone change, no offset
        let offset = TimeDelta::zero();
        let new_timezone = Some(FixedOffset::east_opt(0).unwrap()); // UTC
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T10:00:00+00:00");
        let new_timezone = Some(FixedOffset::east_opt(5 * 3600).unwrap()); // +5h
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T15:00:00+05:00");

        // Both offset and timezone change
        let offset = TimeDelta::minutes(30);
        let new_timezone = Some(FixedOffset::east_opt(-4 * 3600).unwrap()); // -4h
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T06:30:00-04:00");
    }

    #[test]
    fn test_adjust_date_time_original_with_exif() {
        let file_created_at = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 1).unwrap(); // modified seconds
        let local_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 1).unwrap(); // +2h offset
        let exif_date_time = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
        let asset = create_asset_with_exif(
            file_created_at,
            local_date_time,
            Some(exif_date_time),
            Some("+02:00".to_string()),
        );

        // No offset, no timezone change
        let offset = TimeDelta::zero();
        let new_timezone = None;
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.0.to_rfc3339(), "2024-01-01T12:00:00+02:00");
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T12:00:00+02:00");

        // Positive offset, no timezone change
        let offset = TimeDelta::hours(1);
        let new_timezone = None;
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T13:00:00+02:00");

        // Negative offset, no timezone change
        let offset = TimeDelta::hours(-3);
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T09:00:00+02:00");

        // Timezone change, no offset
        let offset = TimeDelta::zero();
        let new_timezone = Some(FixedOffset::east_opt(0).unwrap()); // UTC
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T10:00:00+00:00");
        let new_timezone = Some(FixedOffset::east_opt(5 * 3600).unwrap()); // +5h
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T15:00:00+05:00");

        // Both offset and timezone change
        let offset = TimeDelta::minutes(30);
        let new_timezone = Some(FixedOffset::east_opt(-4 * 3600).unwrap()); // -4h
        let result = ImmichCtl::adjust_date_time_original(&asset, &offset, &new_timezone);
        assert_eq!(result.1.to_rfc3339(), "2024-01-01T06:30:00-04:00");
    }
}

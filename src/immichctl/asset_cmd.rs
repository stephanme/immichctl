use std::borrow::Cow;

use super::ImmichCtl;
use super::assets::Assets;
use super::types::{AssetResponseDto, MetadataSearchDto, UpdateAssetDto};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, TimeDelta, Utc};
use uuid::Uuid;

#[derive(clap::Args, Debug, Default)]
pub struct AssetSearchArgs {
    /// Remove assets from selection instead of adding
    #[arg(long)]
    pub remove: bool,
    /// Asset id to add (UUID)
    #[arg(long, value_name = "asset id")]
    pub id: Option<String>,
    /// Tag name to search and add by tag id
    #[arg(long, value_name = "tag name")]
    pub tag: Option<String>,
    /// Album name to search
    #[arg(long, value_name = "album name")]
    pub album: Option<String>,
    /// Assets (not) marked as favorite. If used without a value, it's equivalent to `--favorite=true`.
    #[arg(long, value_name = "true|false", num_args = 0..=1, default_missing_value = "true", action = clap::ArgAction::Set)]
    pub favorite: Option<bool>,
    /// Assets taken after this date/time
    #[arg(long, value_name = "YYYY-MM-DDTHH:MM:SS±00:00")]
    pub taken_after: Option<DateTime<FixedOffset>>,
    /// Assets taken before this date/time
    #[arg(long, value_name = "YYYY-MM-DDTHH:MM:SS±00:00")]
    pub taken_before: Option<DateTime<FixedOffset>>,
    /// Timezone (remove only)
    #[arg(long)]
    pub timezone: Option<FixedOffset>,
}

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

    pub async fn assets_search_add(&mut self, args: &AssetSearchArgs) -> Result<()> {
        let mut search_dto = self.build_search_dto(args).await?;
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

    pub async fn assets_search_remove(&mut self, args: &AssetSearchArgs) -> Result<()> {
        let mut assets = Assets::load(&self.assets_file);
        let old_len = assets.len();

        if args.tag.is_some() || args.album.is_some() {
            // remote search needed if tag or album is specified
            if args.timezone.is_some() {
                bail!(
                    "The --timezone option cannot be used together with other search options when multiple filters are applied."
                );
            }
            let search_dto = self.build_search_dto(args).await?;
            self.assets_search_remove_by_immich_query(search_dto, &mut assets)
                .await?;
        } else {
            // other args can be handled locally
            assets.retain(|asset| {
                let mut retain = false;
                if let Some(id) = &args.id
                    && asset.id != *id
                {
                    retain = true;
                }
                if let Some(favorite) = &args.favorite
                    && asset.is_favorite != *favorite
                {
                    retain = true;
                }
                if let Some(taken_after) = &args.taken_after
                    && ImmichCtl::get_date_time_original(asset) <= *taken_after
                {
                    retain = true;
                }
                if let Some(taken_before) = &args.taken_before
                    && ImmichCtl::get_date_time_original(asset) >= *taken_before
                {
                    retain = true;
                }
                if let Some(tz) = &args.timezone {
                    let asset_tz = match ImmichCtl::exif_timezone_offset(asset) {
                        Some(tz) => tz,
                        None => ImmichCtl::asset_timezone_offset(asset),
                    };
                    if asset_tz != *tz {
                        retain = true;
                    }
                }

                retain
            });
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

    async fn build_search_dto(&self, args: &AssetSearchArgs) -> Result<MetadataSearchDto> {
        let mut search_dto = MetadataSearchDto::default();
        if let Some(id) = &args.id {
            let uuid = uuid::Uuid::parse_str(id).context("Invalid asset id, expected uuid")?;
            search_dto.id = Some(uuid);
        }
        if let Some(tag_name) = &args.tag {
            search_dto.tag_ids = Some(vec![self.find_tag_by_name(tag_name).await?]);
        }
        if let Some(album_name) = &args.album {
            let album_id = self.find_album_by_name(album_name).await?;
            search_dto.album_ids.push(album_id);
        }
        if let Some(favorite) = args.favorite {
            search_dto.is_favorite = Some(favorite);
        }
        if let Some(taken_after) = args.taken_after {
            search_dto.taken_after = Some(taken_after.with_timezone(&Utc));
        }
        if let Some(taken_before) = args.taken_before {
            search_dto.taken_before = Some(taken_before.with_timezone(&Utc));
        }
        // check that at least one search flag is provided
        if search_dto == MetadataSearchDto::default() {
            bail!("Please provide at least one search flag.");
        }
        Ok(search_dto)
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
pub mod tests {
    use crate::immichctl::album_cmd::tests::create_album;
    use crate::immichctl::tag_cmd::tests::create_tag;
    use crate::immichctl::tests::create_immichctl_with_server;
    use crate::immichctl::types::{AssetTypeEnum, AssetVisibility, ExifResponseDto};

    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

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
            height: None,
            width: None,
            is_edited: false,
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

    #[tokio::test]
    async fn test_build_search_dto_no_flags() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let result = ctl.build_search_dto(&AssetSearchArgs::default()).await;

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

        let args = AssetSearchArgs {
            id: Some("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1".to_string()),
            ..Default::default()
        };
        let mut result = ctl.build_search_dto(&args).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            MetadataSearchDto {
                id: Some(Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1").unwrap()),
                ..Default::default()
            }
        );

        let args = AssetSearchArgs {
            id: Some("no-uuid".to_string()),
            ..Default::default()
        };
        result = ctl.build_search_dto(&args).await;
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

        let args = AssetSearchArgs {
            tag: Some("tag1".to_string()),
            ..Default::default()
        };
        let mut result = ctl.build_search_dto(&args).await;
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

        let args = AssetSearchArgs {
            tag: Some("no-tag".to_string()),
            ..Default::default()
        };
        result = ctl.build_search_dto(&args).await;
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

        let args = AssetSearchArgs {
            album: Some("album1".to_string()),
            ..Default::default()
        };
        let mut result = ctl.build_search_dto(&args).await;
        albums_mock.assert_async().await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            MetadataSearchDto {
                album_ids: vec!(Uuid::parse_str("a1a7f1a9-7394-49f7-a5a3-e876a7e16ab1").unwrap()),
                ..Default::default()
            }
        );

        let args = AssetSearchArgs {
            album: Some("no-album".to_string()),
            ..Default::default()
        };
        result = ctl.build_search_dto(&args).await;
        albums_mock.expect(2).assert_async().await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Album not found: 'no-album'"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_build_search_dto_with_favorite() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let args = AssetSearchArgs {
            favorite: Some(true),
            ..Default::default()
        };
        let result = ctl.build_search_dto(&args).await;

        assert!(result.is_ok());
        let search_dto = result.unwrap();
        assert_eq!(search_dto.is_favorite, Some(true));
    }

    #[tokio::test]
    async fn test_build_search_dto_with_taken_before_after() {
        let config_dir = tempfile::tempdir().unwrap();
        let ctl = ImmichCtl::with_config_dir(config_dir.path());

        let taken_after_str = "2024-07-18T00:00:00+00:00";
        let taken_before_str = "2024-07-18T23:59:59+00:00";
        let taken_after = DateTime::parse_from_rfc3339(taken_after_str).ok();
        let taken_before = DateTime::parse_from_rfc3339(taken_before_str).ok();

        let args = AssetSearchArgs {
            taken_after,
            taken_before,
            ..Default::default()
        };
        let result = ctl.build_search_dto(&args).await;

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

    #[tokio::test]
    async fn test_assets_search_remove_by_id() {
        let config_dir = tempfile::tempdir().unwrap();
        let mut ctl = ImmichCtl::with_config_dir(config_dir.path());

        let asset1 = create_asset_with_timestamps(
            Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(),
        );
        let asset2 = create_asset_with_timestamps(
            Utc.with_ymd_and_hms(2024, 1, 2, 10, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap(),
        );
        let asset_to_remove_id = asset1.id.clone();

        let mut assets = Assets::load(&ctl.assets_file);
        assets.add_asset(asset1);
        assets.add_asset(asset2);
        assets.save().unwrap();

        let args = AssetSearchArgs {
            id: Some(asset_to_remove_id.clone()),
            ..Default::default()
        };

        let result = ctl.assets_search_remove(&args).await;
        assert!(result.is_ok());

        let assets_after_remove = Assets::load(&ctl.assets_file);
        assert_eq!(assets_after_remove.len(), 1);
        assert!(
            assets_after_remove
                .iter_assets()
                .all(|a| a.id != asset_to_remove_id)
        );
    }

    #[tokio::test]
    async fn test_assets_search_remove_by_taken_after_and_before() {
        let config_dir = tempfile::tempdir().unwrap();
        let mut ctl = ImmichCtl::with_config_dir(config_dir.path());

        let asset1 = create_asset_with_timestamps(
            Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
        );

        let asset2_ts = Utc.with_ymd_and_hms(2024, 1, 2, 10, 0, 0).unwrap();
        let asset2 = create_asset_with_timestamps(asset2_ts, asset2_ts);

        let asset3 = create_asset_with_timestamps(
            Utc.with_ymd_and_hms(2024, 1, 3, 10, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 1, 3, 10, 0, 0).unwrap(),
        );

        let mut assets = Assets::load(&ctl.assets_file);
        assets.add_asset(asset1.clone());
        assets.add_asset(asset2.clone());
        assets.add_asset(asset3.clone());
        assets.save().unwrap();

        let args = AssetSearchArgs {
            taken_after: Some(Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().into()),
            taken_before: Some(Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap().into()),
            ..Default::default()
        };

        let result = ctl.assets_search_remove(&args).await;
        assert!(result.is_ok());

        let assets_after_remove = Assets::load(&ctl.assets_file);
        assert_eq!(assets_after_remove.len(), 2);
        let remaining_ids: Vec<_> = assets_after_remove.iter_assets().map(|a| &a.id).collect();
        assert!(remaining_ids.contains(&&asset1.id));
        assert!(remaining_ids.contains(&&asset3.id));
    }

    #[tokio::test]
    async fn test_assets_search_remove_bad_params() {
        let config_dir = tempfile::tempdir().unwrap();
        let mut ctl = ImmichCtl::with_config_dir(config_dir.path());

        let args = AssetSearchArgs {
            tag: Some("tag1".to_string()),
            timezone: Some(FixedOffset::east_opt(2 * 3600).unwrap()),
            ..Default::default()
        };

        let result = ctl.assets_search_remove(&args);
        assert!(result.await.is_err());
    }
}

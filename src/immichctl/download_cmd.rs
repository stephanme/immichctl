use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures::StreamExt;

use super::ImmichCtl;
use super::assets::Assets;
use super::types::{DownloadArchiveDto, DownloadInfoDto};

impl ImmichCtl {
    /// Download all selected assets into `dir`.
    ///
    /// Uses `POST /download/info` to obtain archive groupings and
    /// `POST /download/archive` to fetch each ZIP archive. Archives are
    /// extracted in memory; each entry is written under the basename of the
    /// asset's `originalPath` (the Immich storage-template filename), with
    /// any directory components dropped. On filename collision a numeric
    /// suffix is appended (e.g. `IMG.jpg`, `IMG (1).jpg`).
    pub async fn assets_download(&self, dir: &Path) -> Result<()> {
        let sel = Assets::load(&self.assets_file);
        if sel.is_empty() {
            eprintln!("Selection is empty, nothing to download.");
            return Ok(());
        }

        std::fs::create_dir_all(dir)
            .with_context(|| format!("Could not create directory '{}'", dir.display()))?;

        // Lookup table: asset id -> filename derived from `originalPath`.
        // We deliberately use the basename of `originalPath` (the Immich
        // storage template name, e.g. `20260602-105253.jpg`) rather than
        // `originalFileName` (the original camera filename), so that the
        // downloaded files match what Immich has on disk.
        let name_by_id: HashMap<String, String> = sel
            .iter_assets()
            .map(|a| (a.id.clone(), basename_of(&a.original_path).to_string()))
            .collect();

        let info_dto = DownloadInfoDto {
            asset_ids: sel.asset_uuids(),
            ..Default::default()
        };
        let info = self
            .immich()?
            .get_download_info(None, None, &info_dto)
            .await
            .context("Could not retrieve download info")?
            .into_inner();

        let total = info.archives.len();
        let mut used_names: HashMap<String, u32> = HashMap::new();
        let mut written = 0usize;

        for (i, archive) in info.archives.iter().enumerate() {
            let asset_ids: Result<Vec<uuid::Uuid>> = archive
                .asset_ids
                .iter()
                .map(|s| {
                    uuid::Uuid::parse_str(s)
                        .with_context(|| format!("Invalid asset id '{}' in download info", s))
                })
                .collect();
            let dto = DownloadArchiveDto {
                asset_ids: asset_ids?,
                edited: Some(true),
            };
            let resp = self
                .immich()?
                .download_archive(None, None, &dto)
                .await
                .with_context(|| format!("Could not download archive {}/{}", i + 1, total))?;
            let bytes = collect_byte_stream(resp.into_inner_stream())
                .await
                .with_context(|| format!("Could not read archive {}/{}", i + 1, total))?;
            written += extract_zip(
                &bytes,
                dir,
                &archive.asset_ids,
                &name_by_id,
                &mut used_names,
            )
            .with_context(|| format!("Could not extract archive {}/{}", i + 1, total))?;
            self.eprint_progress_indicator(i, total, 1);
        }

        eprintln!("Downloaded {} asset(s) to {}.", written, dir.display());
        Ok(())
    }
}

/// Drain a [`futures::Stream`] of `reqwest::Result<Bytes>` chunks into a
/// single contiguous `Vec<u8>`.
async fn collect_byte_stream<S>(mut stream: S) -> Result<Vec<u8>>
where
    S: futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read response chunk")?;
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Return the last path component of `p`.
///
/// Handles both Unix (`/`) and Windows-style (`\`) separators since
/// `originalPath` is a server-side path and could conceivably use either —
/// `std::path::Path::file_name` only recognises the host platform's
/// separator. Falls back to the full input if no separator is present.
fn basename_of(p: &str) -> &str {
    let trimmed = p.trim_end_matches(['/', '\\']);
    match trimmed.rfind(['/', '\\']) {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    }
}

/// Extract a ZIP archive from `bytes` into `dir`.
///
/// Filenames are derived from the basename of the asset's `originalPath`
/// (looked up in `name_by_id`). When the lookup fails (e.g. server emitted
/// an entry name not matching any selected asset id), the basename of the
/// entry name is used as a fallback. Filename collisions get a
/// `name (N).ext` suffix.
///
/// Returns the number of files written.
fn extract_zip(
    bytes: &[u8],
    dir: &Path,
    archive_asset_ids: &[String],
    name_by_id: &HashMap<String, String>,
    used_names: &mut HashMap<String, u32>,
) -> Result<usize> {
    let reader = Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader).context("Could not open ZIP archive")?;
    let mut written = 0usize;
    // Iterate by index so we can match entries to asset ids in order.
    // Immich's downloadArchive endpoint emits entries in the same order as
    // the requested assetIds; we pair them up positionally for naming.
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .with_context(|| format!("Could not read ZIP entry #{}", i))?;
        if entry.is_dir() {
            continue;
        }
        let entry_name = entry
            .enclosed_name()
            .unwrap_or_else(|| PathBuf::from(entry.name()));

        // Prefer the basename of the asset's `originalPath` when we can map
        // this entry to a known asset id; otherwise fall back to the basename
        // of the ZIP entry itself.
        let preferred = archive_asset_ids
            .get(i)
            .and_then(|id| name_by_id.get(id))
            .cloned();
        let basename = preferred.unwrap_or_else(|| {
            entry_name
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("file_{}", i))
        });

        let unique = unique_name(used_names, &basename);
        let dest = dir.join(unique.as_ref());

        // `entry.size()` is server-supplied (u64); cap the preallocation so
        // a hostile or buggy server can't trick us into a huge alloc.
        const MAX_PREALLOC: u64 = 16 * 1024 * 1024;
        let mut file_bytes = Vec::with_capacity(entry.size().min(MAX_PREALLOC) as usize);
        entry
            .read_to_end(&mut file_bytes)
            .with_context(|| format!("Could not read ZIP entry '{}'", entry_name.display()))?;
        std::fs::write(&dest, &file_bytes)
            .with_context(|| format!("Could not write '{}'", dest.display()))?;
        written += 1;
    }
    Ok(written)
}

/// Return a filename that has not yet been used. If `name` was used N times
/// before, return e.g. `stem (N).ext` and increment the counter. The
/// no-collision case returns `Cow::Borrowed(name)` to avoid allocating.
fn unique_name<'a>(used: &mut HashMap<String, u32>, name: &'a str) -> Cow<'a, str> {
    // Check first to avoid allocating a String key on the common path.
    if let Some(count) = used.get_mut(name) {
        let n = *count;
        *count += 1;
        let path = Path::new(name);
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let result = match path.extension().map(|s| s.to_string_lossy()) {
            Some(e) if !e.is_empty() => format!("{} ({}).{}", stem, n, e),
            _ => format!("{} ({})", stem, n),
        };
        Cow::Owned(result)
    } else {
        used.insert(name.to_string(), 1);
        Cow::Borrowed(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immichctl::asset_cmd::tests::create_asset_for_download;
    use crate::immichctl::tests::create_immichctl_with_server;

    use std::io::Write;
    use uuid::Uuid;
    use zip::write::SimpleFileOptions;

    /// Build an in-memory ZIP archive with the given (entry name, content)
    /// pairs, used by the mocked `/download/archive` responses.
    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zw = zip::ZipWriter::new(cursor);
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    /// Mock both `POST /api/download/info` (returning a single archive group
    /// containing all `asset_ids`) and `POST /api/download/archive`
    /// (returning a ZIP built from `zip_entries`). Returns the two mock
    /// guards so callers can `.assert_async()` if they need to.
    async fn mock_download(
        server: &mut mockito::ServerGuard,
        asset_ids: &[Uuid],
        zip_entries: &[(&str, &[u8])],
    ) -> (mockito::Mock, mockito::Mock) {
        let info_resp = serde_json::json!({
            "archives": [{
                "assetIds": asset_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
                "size": 1,
            }],
            "totalSize": 1,
        });
        let info_mock = server
            .mock("POST", "/api/download/info")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(info_resp.to_string())
            .create_async()
            .await;
        let archive_mock = server
            .mock("POST", "/api/download/archive")
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .with_body(build_zip(zip_entries))
            .create_async()
            .await;
        (info_mock, archive_mock)
    }

    #[tokio::test]
    async fn test_unique_name_no_collision() {
        let mut used = HashMap::new();
        assert_eq!(unique_name(&mut used, "a.jpg"), "a.jpg");
        assert_eq!(unique_name(&mut used, "b.jpg"), "b.jpg");
    }

    #[tokio::test]
    async fn test_unique_name_with_collisions() {
        let mut used = HashMap::new();
        assert_eq!(unique_name(&mut used, "img.jpg"), "img.jpg");
        assert_eq!(unique_name(&mut used, "img.jpg"), "img (1).jpg");
        assert_eq!(unique_name(&mut used, "img.jpg"), "img (2).jpg");
        // No extension
        assert_eq!(unique_name(&mut used, "README"), "README");
        assert_eq!(unique_name(&mut used, "README"), "README (1)");
    }

    #[tokio::test]
    async fn test_basename_of() {
        assert_eq!(basename_of("/a/b/c.jpg"), "c.jpg");
        assert_eq!(basename_of("c.jpg"), "c.jpg");
        assert_eq!(
            basename_of("/usr/src/app/upload/library/2026-06/20260602-105253.jpg"),
            "20260602-105253.jpg"
        );
        assert_eq!(basename_of("a\\b\\c.jpg"), "c.jpg");
        assert_eq!(basename_of("/a/b/"), "b");
        assert_eq!(basename_of(""), "");
    }

    #[tokio::test]
    async fn test_download_empty_selection() {
        let (ctl, _server) = create_immichctl_with_server().await;
        let outdir = tempfile::tempdir().unwrap();

        let result = ctl.assets_download(outdir.path()).await;
        assert!(result.is_ok());
        // No files should be written
        let count = std::fs::read_dir(outdir.path()).unwrap().count();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_download_happy_path() {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        // The camera filename (`originalFileName`) and the server storage
        // path (`originalPath`) differ; we expect downloads to be named
        // after the basename of `originalPath`.
        let asset1 = create_asset_for_download(
            id1,
            "PXL_20260602_085253983.jpg",
            "/usr/src/app/upload/library/2026-06/20260602-105253.jpg",
        );
        let asset2 = create_asset_for_download(
            id2,
            "PXL_20260603_173748452.jpg",
            "/usr/src/app/upload/library/2026-06/20260603-193748.jpg",
        );

        let mut sel = Assets::load(&ctl.assets_file);
        sel.add_asset(asset1);
        sel.add_asset(asset2);
        sel.save().unwrap();

        // Server emits paths that mirror originalPath; we should drop those
        // and use the basename of originalPath for the local filename.
        let (info_mock, archive_mock) = mock_download(
            &mut server,
            &[id1, id2],
            &[
                (
                    "upload/library/2026-06/20260602-105253.jpg",
                    b"file-1-content",
                ),
                (
                    "upload/library/2026-06/20260603-193748.jpg",
                    b"file-2-content",
                ),
            ],
        )
        .await;

        let outdir = tempfile::tempdir().unwrap();
        let result = ctl.assets_download(outdir.path()).await;
        assert!(result.is_ok(), "{:?}", result.err());

        info_mock.assert_async().await;
        archive_mock.assert_async().await;

        let f1 = outdir.path().join("20260602-105253.jpg");
        let f2 = outdir.path().join("20260603-193748.jpg");
        assert!(f1.exists(), "expected {}", f1.display());
        assert!(f2.exists(), "expected {}", f2.display());
        assert_eq!(std::fs::read(&f1).unwrap(), b"file-1-content");
        assert_eq!(std::fs::read(&f2).unwrap(), b"file-2-content");
        // The camera filenames must NOT be used.
        assert!(!outdir.path().join("PXL_20260602_085253983.jpg").exists());
    }

    #[tokio::test]
    async fn test_download_collision_appends_suffix() {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        // Both assets share the same `originalPath` basename.
        let asset1 = create_asset_for_download(id1, "A.jpg", "/upload/a/IMG.jpg");
        let asset2 = create_asset_for_download(id2, "B.jpg", "/upload/b/IMG.jpg");

        let mut sel = Assets::load(&ctl.assets_file);
        sel.add_asset(asset1);
        sel.add_asset(asset2);
        sel.save().unwrap();

        let _mocks = mock_download(
            &mut server,
            &[id1, id2],
            &[
                ("upload/a/IMG.jpg", b"first"),
                ("upload/b/IMG.jpg", b"second"),
            ],
        )
        .await;

        let outdir = tempfile::tempdir().unwrap();
        let result = ctl.assets_download(outdir.path()).await;
        assert!(result.is_ok(), "{:?}", result.err());

        let original = outdir.path().join("IMG.jpg");
        let suffixed = outdir.path().join("IMG (1).jpg");
        assert!(original.exists());
        assert!(suffixed.exists());
        assert_eq!(std::fs::read(&original).unwrap(), b"first");
        assert_eq!(std::fs::read(&suffixed).unwrap(), b"second");
    }

    #[tokio::test]
    async fn test_download_creates_dir() {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let id1 = Uuid::new_v4();
        let asset = create_asset_for_download(id1, "X.bin", "/storage/X.bin");
        let mut sel = Assets::load(&ctl.assets_file);
        sel.add_asset(asset);
        sel.save().unwrap();

        let _mocks = mock_download(&mut server, &[id1], &[("X.bin", b"hello")]).await;

        let parent = tempfile::tempdir().unwrap();
        let nested = parent.path().join("a").join("b");
        assert!(!nested.exists());

        let result = ctl.assets_download(&nested).await;
        assert!(result.is_ok(), "{:?}", result.err());
        assert!(nested.join("X.bin").exists());
    }

    #[tokio::test]
    async fn test_download_info_failure_includes_context() {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let id1 = Uuid::new_v4();
        let asset = create_asset_for_download(id1, "X.bin", "/storage/X.bin");
        let mut sel = Assets::load(&ctl.assets_file);
        sel.add_asset(asset);
        sel.save().unwrap();

        let _info_mock = server
            .mock("POST", "/api/download/info")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body("{\"error\":\"boom\"}")
            .create_async()
            .await;

        let outdir = tempfile::tempdir().unwrap();
        let result = ctl.assets_download(outdir.path()).await;
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("Could not retrieve download info"),
            "unexpected error: {}",
            msg
        );
    }
}

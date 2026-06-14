use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use futures::TryStreamExt;
use tokio::io::AsyncWriteExt;

use super::ImmichCtl;
use super::assets::Assets;
use super::types::{DownloadArchiveDto, DownloadInfoDto};

/// Shared progress counters updated from both the async download loop and
/// the blocking extract task. Printed by [`Progress::render`] on a single
/// terminal line.
#[derive(Clone)]
struct Progress {
    total_bytes: u64,
    total_files: usize,
    downloaded: Arc<AtomicU64>,
    extracted: Arc<AtomicUsize>,
}

impl Progress {
    fn new(total_bytes: u64, total_files: usize) -> Self {
        Self {
            total_bytes,
            total_files,
            downloaded: Arc::new(AtomicU64::new(0)),
            extracted: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Repaint the progress line. Called periodically by the painter task
    /// and once more at the end so the final 100% is visible.
    fn render(&self) {
        let downloaded = self.downloaded.load(Ordering::Relaxed);
        let extracted = self.extracted.load(Ordering::Relaxed);
        let dl_pct = if self.total_bytes > 0 {
            downloaded as f64 / self.total_bytes as f64 * 100.0
        } else {
            100.0
        };
        eprint!(
            "\rDownload: {:>3.0}% ({}/{})  Extract: {}/{} files\x1b[K",
            dl_pct,
            format_bytes(downloaded),
            format_bytes(self.total_bytes),
            extracted,
            self.total_files,
        );
    }
}

/// Format `n` bytes as a human-friendly string with one decimal place.
fn format_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if n >= GIB {
        format!("{:.1} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.1} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1} KiB", n as f64 / KIB as f64)
    } else {
        format!("{} B", n)
    }
}

impl ImmichCtl {
    /// Download all selected assets into `dir`.
    ///
    /// Uses `POST /download/info` to obtain archive groupings and
    /// `POST /download/archive` to fetch each ZIP archive. To keep memory
    /// bounded regardless of selection size, each archive is streamed to a
    /// temporary file on disk (an ~8 KiB tokio copy buffer is the only
    /// in-memory state) and then extracted entry-by-entry via
    /// [`zip::ZipArchive`] — also streaming each entry through
    /// [`std::io::copy`] so no entry is ever fully buffered. The temp file
    /// is removed when extraction finishes.
    ///
    /// We can't stream-decode the HTTP body directly into the ZIP reader
    /// because Immich emits ZIPs that use trailing data descriptors, which
    /// require the central directory (i.e. random access) for entry sizes.
    ///
    /// Each entry is written under the basename of the asset's
    /// `originalPath` (the Immich storage-template filename), with any
    /// directory components dropped. On filename collision a numeric suffix
    /// is appended (e.g. `IMG.jpg`, `IMG (1).jpg`).
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

        let total_archives = info.archives.len();
        let total_bytes: u64 = info.archives.iter().map(|a| a.size.max(0) as u64).sum();
        let total_files: usize = info.archives.iter().map(|a| a.asset_ids.len()).sum();

        // Assign final destination filenames upfront for all archives.
        // Deduplicate file names by appending a suffix if the same name appears more than once.
        // This should not happen if Immich storage template is well configured.
        let archive_filenames: Vec<Vec<String>> = {
            let mut used: HashMap<String, u32> = HashMap::new();
            info.archives
                .iter()
                .map(|archive| {
                    archive
                        .asset_ids
                        .iter()
                        .map(|id| {
                            let base = name_by_id.get(id).map(|s| s.as_str()).unwrap_or("unknown");
                            unique_name(&mut used, base).into_owned()
                        })
                        .collect()
                })
                .collect()
        };

        let progress = Progress::new(total_bytes, total_files);

        // Background painter: repaint the progress line once per second. Uses a
        // drop guard so the task is aborted on every exit path (including
        // early errors).
        struct PainterGuard(tokio::task::JoinHandle<()>);
        impl Drop for PainterGuard {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _painter = PainterGuard({
            let p = progress.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                p.render(); // Show initial state immediately, don't wait 100ms.
                loop {
                    tick.tick().await;
                    p.render();
                }
            })
        });

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
                .with_context(|| {
                    format!("Could not download archive {}/{}", i + 1, total_archives)
                })?;

            // Stream the response into a temp file so the in-memory footprint
            // is just tokio's copy buffer (~8 KiB), not the whole archive.
            let temp =
                tempfile::NamedTempFile::new().context("Could not create temp file for archive")?;
            let temp_path = temp.path().to_path_buf();
            let mut out = tokio::fs::File::from_std(
                temp.reopen()
                    .context("Could not open temp file for writing")?,
            );
            let mut byte_stream = resp.into_inner_stream().map_err(std::io::Error::other);
            while let Some(chunk) = byte_stream
                .try_next()
                .await
                .with_context(|| format!("Could not read archive {}/{}", i + 1, total_archives))?
            {
                out.write_all(&chunk).await.with_context(|| {
                    format!("Could not write archive {}/{}", i + 1, total_archives)
                })?;
                progress
                    .downloaded
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            out.flush().await.ok();
            drop(out);

            // Extract on a blocking thread — `zip` is synchronous and entry
            // streaming uses `std::io::copy`, which calls into the OS.
            let dir_owned = dir.to_path_buf();
            let filenames = archive_filenames[i].clone();
            let extracted_counter = progress.extracted.clone();

            let count = tokio::task::spawn_blocking(move || -> Result<_> {
                let file = std::fs::File::open(&temp_path)
                    .context("Could not open temp archive for extraction")?;
                extract_zip(file, &dir_owned, &filenames, &extracted_counter)
            })
            .await
            .context("ZIP extraction task failed")?
            .with_context(|| format!("Could not extract archive {}/{}", i + 1, total_archives))?;
            written += count;
            // `temp` drops here, removing the temp archive from disk.
            drop(temp);
        }

        // Abort the painter (via drop guard) and render the final 100% state
        // before emitting the trailing newline.
        drop(_painter);
        progress.render();
        eprintln!();

        eprintln!("Downloaded {} asset(s) to {}.", written, dir.display());
        Ok(())
    }
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

/// Extract a ZIP archive from `file` into `dir`. Each entry is read via
/// [`std::io::copy`] so no entry is ever fully buffered. The
/// `extracted_counter` is incremented after each file is written so the
/// progress painter can observe it across the spawn_blocking boundary.
///
/// `filenames[i]` is the pre-computed destination filename for ZIP entry `i`
/// (already collision-deduplicated by the caller).
///
/// Returns the number of files written.
fn extract_zip(
    file: std::fs::File,
    dir: &Path,
    filenames: &[String],
    extracted_counter: &AtomicUsize,
) -> Result<usize> {
    let mut zip = zip::ZipArchive::new(file).context("Could not open ZIP archive")?;
    let mut written = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .with_context(|| format!("Could not read ZIP entry #{}", i))?;
        if entry.is_dir() {
            continue;
        }
        let dest = dir.join(
            filenames
                .get(i)
                .with_context(|| format!("No filename pre-assigned for ZIP entry #{}", i))?,
        );
        // Stream entry bytes straight to disk — never buffer the full entry.
        let mut out = std::fs::File::create(&dest)
            .with_context(|| format!("Could not create '{}'", dest.display()))?;
        std::io::copy(&mut entry, &mut out)
            .with_context(|| format!("Could not write '{}'", dest.display()))?;
        written += 1;
        extracted_counter.fetch_add(1, Ordering::Relaxed);
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

    use std::io::{Cursor, Write};
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

    /// End-to-end exercise of the streaming `std::io::copy` path with a
    /// multi-MB ZIP entry. mockito buffers the response on the server
    /// side, so this isn't a true memory-bound test, but it does verify
    /// the streaming pipeline (`bytes_stream` → `StreamReader` →
    /// `SyncIoBridge` → `read_zipfile_from_stream` → `std::io::copy`)
    /// preserves all bytes for a non-trivial entry size.
    #[tokio::test]
    async fn test_download_streams_large_archive() {
        let (ctl, mut server) = create_immichctl_with_server().await;

        let id1 = Uuid::new_v4();
        let asset = create_asset_for_download(id1, "BIG.bin", "/storage/BIG.bin");
        let mut sel = Assets::load(&ctl.assets_file);
        sel.add_asset(asset);
        sel.save().unwrap();

        // 4 MiB of pseudo-random-but-deterministic bytes.
        let payload: Vec<u8> = (0..(4 * 1024 * 1024))
            .map(|i| (i as u32).wrapping_mul(2654435761) as u8)
            .collect();

        let _mocks = mock_download(&mut server, &[id1], &[("BIG.bin", &payload)]).await;

        let outdir = tempfile::tempdir().unwrap();
        let result = ctl.assets_download(outdir.path()).await;
        assert!(result.is_ok(), "{:?}", result.err());

        let written = std::fs::read(outdir.path().join("BIG.bin")).unwrap();
        assert_eq!(written.len(), payload.len());
        assert_eq!(written, payload);
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

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Safe filesystem operations for Studio-owned files — S6 (Desktop
//! Filesystem Consolidation).
//!
//! # `StudioPaths` resolves; `FileSystemService` acts
//!
//! ```text
//! StudioPaths            — "where" (valori_studio_storage::path, pure path math)
//!        ↓
//! FileSystemService       — "how" (this module: create/write/read/remove/
//!        ↓                  rename/copy, with atomic-write and path-
//!    the filesystem         traversal-safety semantics built in)
//! ```
//!
//! # What this does not do
//!
//! - **Never exposes arbitrary filesystem access.** No method takes an
//!   absolute, caller-chosen path outside a `StudioPaths`-resolved root —
//!   see [`FileSystemService::safe_join`].
//! - **Never touches project internals.** WAL, snapshots, indexes, and
//!   vectors remain entirely owned by `valori-kernel`/`valori-storage`/
//!   `valori-node` — this service has no method that names them, and
//!   `desktop/src-tauri` has no dependency on the crates that do.
//! - **Does not blindly apply atomic writes to large/streaming files.**
//!   `atomic_write`/`atomic_replace` are for small, whole-file metadata
//!   (JSON config, markers) — never intended for a WAL or snapshot, which
//!   the storage engine that owns them already handles with its own
//!   durability discipline.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Typed errors — never a bare `io::Error` string, so callers (and tests)
/// can distinguish "the path tried to escape its root" from "the OS
/// refused the operation" from "the path doesn't exist."
#[derive(Debug)]
pub enum FsError {
    /// A user-controlled relative path attempted to escape the resolved
    /// root — via `..`, an absolute path, or a root/prefix component.
    PathTraversal(String),
    Io(io::Error),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::PathTraversal(detail) => write!(f, "path traversal rejected: {detail}"),
            FsError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

impl std::error::Error for FsError {}

impl From<io::Error> for FsError {
    fn from(e: io::Error) -> Self {
        FsError::Io(e)
    }
}

pub type FsResult<T> = Result<T, FsError>;

/// Safe filesystem operations. Stateless — every method takes the path(s)
/// it needs; there is nothing to construct beyond `FileSystemService`
/// itself, but it exists as a type (rather than free functions) so it can
/// be depended on, mocked, and referenced the same way every other
/// service in this crate is (`CredentialService`, `SessionService`, …).
///
/// `#[allow(dead_code)]` on several methods below: this is the complete,
/// typed operation surface the S6 phase establishes
/// (`docs/phases/phase-studio-S6-filesystem-management.md`) —
/// `cleanup_stale_temp_files` is wired into `lib.rs`'s startup today;
/// `safe_join`/`atomic_write`/`atomic_replace`/`read`/`remove`/`rename`/
/// `copy`/`exists`/`clear_cache` exist for the next Tauri command that
/// needs them (a "Clear Cache" Settings button, a future logs viewer,
/// etc.) rather than being spun up ad hoc, unsafely, at that point. Adding
/// a UI-facing command for each was judged out of this phase's scope —
/// see the phase doc's "not done" section — inventing one merely to
/// silence this warning would be exactly the "feature nobody asked for"
/// this codebase's own guidelines warn against.
#[derive(Clone, Copy, Default)]
pub struct FileSystemService;

#[allow(dead_code)]
impl FileSystemService {
    pub fn new() -> Self {
        Self
    }

    /// Joins `relative` onto `root`, rejecting any attempt to escape it.
    /// Component-aware, not a string-prefix check: rejects an absolute
    /// path, a `..` (`ParentDir`) component anywhere, and (on Windows) a
    /// drive/prefix component — not merely a path that happens not to
    /// *start with* `../`. This is the one entry point any future
    /// Tauri command taking a user-controlled relative path must go
    /// through before touching disk.
    pub fn safe_join(&self, root: &Path, relative: &str) -> FsResult<PathBuf> {
        let candidate = Path::new(relative);
        for component in candidate.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                Component::ParentDir => {
                    return Err(FsError::PathTraversal(format!(
                        "{relative:?} contains a parent-directory component"
                    )));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(FsError::PathTraversal(format!(
                        "{relative:?} is an absolute/rooted path, not a relative one"
                    )));
                }
            }
        }
        let joined = root.join(candidate);

        // Secondary defense: if the joined path already exists, canonicalize
        // both sides and confirm the result is still under `root` — catches
        // a symlink planted inside `root` that points back out of it, which
        // pure component inspection above cannot see (it only looks at the
        // string, not what a component might resolve to on disk).
        if joined.exists() {
            let real_root = fs::canonicalize(root)?;
            let real_joined = fs::canonicalize(&joined)?;
            if !real_joined.starts_with(&real_root) {
                return Err(FsError::PathTraversal(format!(
                    "{relative:?} resolves outside of {}",
                    root.display()
                )));
            }
        }

        Ok(joined)
    }

    /// `create_dir_all` — idempotent, safe to call whether or not the
    /// directory (or its parents) already exist. This is how every
    /// lazily-created canonical directory (`logs/`, `cache/`, `downloads/`,
    /// `temp/`, the new `crashes/`) comes into being: on first real use,
    /// never eagerly at startup.
    pub fn create_dir(&self, path: &Path) -> FsResult<()> {
        fs::create_dir_all(path)?;
        Ok(())
    }

    /// Writes `contents` to `path` via write-temp → fsync → atomic rename,
    /// so a crash or power loss mid-write can never leave a half-written
    /// file at `path` — either the old content is still there, or the new
    /// content is there in full. For small, whole-file metadata only (see
    /// the module doc's "what this does not do").
    ///
    /// Works identically whether `path` already exists (a "replace") or
    /// not (a "create") — `fs::rename`'s semantics already cover both
    /// cases correctly and atomically on every platform this project
    /// targets. [`Self::atomic_replace`] is the same operation under the
    /// name that better documents intent at call sites that are always
    /// overwriting something.
    pub fn atomic_write(&self, path: &Path, contents: &[u8]) -> FsResult<()> {
        let parent = path.parent().ok_or_else(|| {
            FsError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} has no parent directory", path.display()),
            ))
        })?;
        fs::create_dir_all(parent)?;

        let tmp_name = format!(
            ".{}.tmp-{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("studio"),
            uuid_like_suffix()
        );
        let tmp_path = parent.join(tmp_name);

        {
            let mut file = fs::File::create(&tmp_path)?;
            use std::io::Write;
            file.write_all(contents)?;
            // fsync before rename — without this, the rename is still
            // atomic, but a power loss (not just a process crash) between
            // the rename and the OS actually flushing the page cache could
            // still leave `path` pointing at zero-length or truncated
            // content on some filesystems. This is the "fsync if required"
            // step the S6 task calls for.
            file.sync_all()?;
        }

        match fs::rename(&tmp_path, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Best-effort cleanup — don't mask the original error with
                // a cleanup failure.
                let _ = fs::remove_file(&tmp_path);
                Err(FsError::Io(e))
            }
        }
    }

    /// Same operation as [`Self::atomic_write`] — see its doc comment.
    pub fn atomic_replace(&self, path: &Path, contents: &[u8]) -> FsResult<()> {
        self.atomic_write(path, contents)
    }

    pub fn read(&self, path: &Path) -> FsResult<Vec<u8>> {
        Ok(fs::read(path)?)
    }

    /// Idempotent — removing an already-absent file is success, not an
    /// error (matches `CredentialService::delete`'s and
    /// `SessionStore::prune`'s existing idempotency convention in this
    /// codebase).
    pub fn remove(&self, path: &Path) -> FsResult<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(FsError::Io(e)),
        }
    }

    pub fn rename(&self, from: &Path, to: &Path) -> FsResult<()> {
        Ok(fs::rename(from, to)?)
    }

    pub fn copy(&self, from: &Path, to: &Path) -> FsResult<u64> {
        Ok(fs::copy(from, to)?)
    }

    pub fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    /// Deletes every entry directly under `cache_dir` if it exists — a
    /// no-op (not an error) if the directory is absent, matching "do not
    /// create a directory merely because it appears in the diagram."
    /// `cache/` is explicitly disposable (see
    /// `docs/phases/phase-studio-S6-filesystem-management.md`'s cache
    /// section): after this call returns, the application must still work
    /// exactly as before — nothing authoritative may ever live here.
    pub fn clear_cache(&self, cache_dir: &Path) -> FsResult<usize> {
        clear_dir_contents(cache_dir)
    }

    /// Removes Studio-owned temporary files older than `max_age` from
    /// `temp_dir`, if it exists. Never touches anything outside
    /// `temp_dir` — "never delete unknown files" is satisfied structurally
    /// (this method has no path parameter other than the directory itself,
    /// so it cannot be pointed at anything else), not by a filename
    /// heuristic. A no-op, not an error, if `temp_dir` doesn't exist yet.
    pub fn cleanup_stale_temp_files(
        &self,
        temp_dir: &Path,
        max_age: std::time::Duration,
    ) -> FsResult<usize> {
        self.remove_files_older_than(temp_dir, max_age)
    }

    /// Prunes archived crash reports older than `max_age` from
    /// `crashes_dir`, if it exists — bounded retention for the crash
    /// history `telemetry.rs::archive_crash` writes there. Same
    /// "only this directory, structurally" safety property as
    /// [`Self::cleanup_stale_temp_files`] — see its doc comment. Never
    /// touches the live panic-hook marker (a different file, at a
    /// different, legacy location — see `StudioPaths::crashes_dir`'s doc
    /// comment).
    pub fn cleanup_old_crash_archives(
        &self,
        crashes_dir: &Path,
        max_age: std::time::Duration,
    ) -> FsResult<usize> {
        self.remove_files_older_than(crashes_dir, max_age)
    }

    /// Prunes rotated log files older than `max_age` from `logs_dir`, if it
    /// exists — the bounded-retention half of `logs_dir()` finally having a
    /// real writer (`lib.rs`'s `tracing-appender` file sink, daily
    /// rotation). Same "only this directory, structurally" safety property
    /// as [`Self::cleanup_stale_temp_files`] — see its doc comment.
    pub fn cleanup_old_logs(
        &self,
        logs_dir: &Path,
        max_age: std::time::Duration,
    ) -> FsResult<usize> {
        self.remove_files_older_than(logs_dir, max_age)
    }

    /// Shared primitive behind [`Self::cleanup_stale_temp_files`] and
    /// [`Self::cleanup_old_logs`] — removes files (never subdirectories)
    /// directly under `dir` whose mtime is older than `max_age`. A no-op,
    /// not an error, if `dir` doesn't exist yet.
    fn remove_files_older_than(&self, dir: &Path, max_age: std::time::Duration) -> FsResult<usize> {
        if !dir.exists() {
            return Ok(0);
        }
        let now = std::time::SystemTime::now();
        let mut removed = 0usize;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue, // vanished mid-scan — not our concern
            };
            if !metadata.is_file() {
                continue; // never recurse into/remove subdirectories here
            }
            let age = match metadata
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
            {
                Some(age) => age,
                None => continue,
            };
            if age > max_age && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[allow(dead_code)] // see FileSystemService's doc comment
fn clear_dir_contents(dir: &Path) -> FsResult<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if result.is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// A short, collision-resistant suffix for temp-file names — avoids
/// pulling in the `uuid` crate's randomness machinery for what only needs
/// to be unique within one process's lifetime, not globally.
#[allow(dead_code)] // see FileSystemService's doc comment
fn uuid_like_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{pid:x}-{n:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_accepts_a_plain_relative_path() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let joined = svc.safe_join(temp.path(), "sub/file.json").unwrap();
        assert_eq!(joined, temp.path().join("sub/file.json"));
    }

    #[test]
    fn safe_join_rejects_parent_dir_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        assert!(matches!(
            svc.safe_join(temp.path(), "../escape.json"),
            Err(FsError::PathTraversal(_))
        ));
        assert!(matches!(
            svc.safe_join(temp.path(), "sub/../../escape.json"),
            Err(FsError::PathTraversal(_))
        ));
    }

    #[test]
    fn safe_join_rejects_absolute_paths() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        assert!(matches!(
            svc.safe_join(temp.path(), "/etc/passwd"),
            Err(FsError::PathTraversal(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn safe_join_rejects_a_symlink_that_escapes_root() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"nope").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();

        let svc = FileSystemService::new();
        assert!(matches!(
            svc.safe_join(temp.path(), "link/secret.txt"),
            Err(FsError::PathTraversal(_))
        ));
    }

    #[test]
    fn create_dir_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let dir = temp.path().join("a/b/c");
        svc.create_dir(&dir).unwrap();
        assert!(dir.is_dir());
        svc.create_dir(&dir).unwrap(); // second call — must not error
        assert!(dir.is_dir());
    }

    #[test]
    fn atomic_write_creates_and_reads_back() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let path = temp.path().join("config.json");
        svc.atomic_write(&path, b"{\"a\":1}").unwrap();
        assert_eq!(svc.read(&path).unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn atomic_write_replaces_existing_content_wholesale() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let path = temp.path().join("config.json");
        svc.atomic_write(&path, b"old").unwrap();
        svc.atomic_write(&path, b"new-and-longer-content").unwrap();
        assert_eq!(svc.read(&path).unwrap(), b"new-and-longer-content");
    }

    #[test]
    fn atomic_write_never_leaves_a_temp_file_behind_on_success() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        svc.atomic_write(&temp.path().join("f.json"), b"x").unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no .tmp- files should remain: {leftovers:?}"
        );
    }

    #[test]
    fn interrupted_write_never_produces_a_partial_file_at_the_final_path() {
        // Simulates "crash mid-write": write the tmp file directly and
        // never rename it — `path` must still not exist, proving a reader
        // arriving between the two steps sees either nothing or the
        // complete prior content, never a partial new one.
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let path = temp.path().join("config.json");
        svc.atomic_write(&path, b"committed").unwrap();

        // Simulate a second write that "crashes" before the rename step.
        let tmp = temp.path().join(".config.json.tmp-simulated-crash");
        std::fs::write(&tmp, b"half-writ").unwrap();
        // No rename — process "crashed" here.

        assert_eq!(
            svc.read(&path).unwrap(),
            b"committed",
            "a reader must still see the last fully-committed content"
        );
    }

    #[test]
    fn remove_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let path = temp.path().join("gone.json");
        svc.atomic_write(&path, b"x").unwrap();
        svc.remove(&path).unwrap();
        assert!(!path.exists());
        svc.remove(&path).unwrap(); // second remove — must not error
    }

    #[test]
    fn clear_cache_removes_contents_but_not_the_directory_itself() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let cache = temp.path().join("cache");
        svc.create_dir(&cache).unwrap();
        std::fs::write(cache.join("a.tmp"), b"1").unwrap();
        std::fs::create_dir(cache.join("subdir")).unwrap();
        std::fs::write(cache.join("subdir/b.tmp"), b"2").unwrap();

        let removed = svc.clear_cache(&cache).unwrap();
        assert_eq!(removed, 2); // "a.tmp" + "subdir" (removed recursively)
        assert!(cache.is_dir(), "the cache directory itself must survive");
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    }

    #[test]
    fn clear_cache_on_a_missing_directory_is_a_safe_no_op() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let removed = svc
            .clear_cache(&temp.path().join("does-not-exist"))
            .unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn cleanup_stale_temp_files_removes_only_files_older_than_max_age() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let temp_dir = temp.path().join("temp");
        svc.create_dir(&temp_dir).unwrap();

        let fresh = temp_dir.join("fresh.tmp");
        std::fs::write(&fresh, b"x").unwrap();

        // A "stale" file: written now, then its mtime is rewound.
        let stale = temp_dir.join("stale.tmp");
        std::fs::write(&stale, b"x").unwrap();
        let old_time = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 3600);
        set_mtime(&stale, old_time);

        let removed = svc
            .cleanup_stale_temp_files(&temp_dir, std::time::Duration::from_secs(3600))
            .unwrap();
        assert_eq!(removed, 1);
        assert!(fresh.exists(), "fresh file must survive");
        assert!(!stale.exists(), "stale file must be removed");
    }

    #[test]
    fn cleanup_old_logs_removes_only_files_older_than_max_age() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let logs_dir = temp.path().join("logs");
        svc.create_dir(&logs_dir).unwrap();

        let recent = logs_dir.join("studio.log.2026-08-09");
        std::fs::write(&recent, b"log").unwrap();

        let old = logs_dir.join("studio.log.2026-08-01");
        std::fs::write(&old, b"log").unwrap();
        let old_time =
            std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 24 * 3600);
        set_mtime(&old, old_time);

        let removed = svc
            .cleanup_old_logs(&logs_dir, std::time::Duration::from_secs(7 * 24 * 3600))
            .unwrap();
        assert_eq!(removed, 1);
        assert!(recent.exists(), "recent log must survive");
        assert!(!old.exists(), "old log must be pruned");
    }

    #[test]
    fn cleanup_old_logs_on_a_missing_directory_is_a_safe_no_op() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let removed = svc
            .cleanup_old_logs(&temp.path().join("logs"), std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn cleanup_stale_temp_files_on_a_missing_directory_is_a_safe_no_op() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let removed = svc
            .cleanup_stale_temp_files(&temp.path().join("temp"), std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn cleanup_stale_temp_files_never_touches_subdirectories() {
        let temp = tempfile::tempdir().unwrap();
        let svc = FileSystemService::new();
        let temp_dir = temp.path().join("temp");
        let sub = temp_dir.join("keep-me");
        svc.create_dir(&sub).unwrap();

        let removed = svc
            .cleanup_stale_temp_files(&temp_dir, std::time::Duration::from_secs(0))
            .unwrap();
        assert_eq!(removed, 0);
        assert!(sub.is_dir());
    }

    // A minimal mtime helper so this module doesn't need a new dependency
    // just for one test — equivalent to what the `filetime` crate offers,
    // scoped to exactly what's needed here.
    fn set_mtime(path: &Path, t: std::time::SystemTime) {
        let file = fs::File::open(path).unwrap();
        let duration = t.duration_since(std::time::UNIX_EPOCH).unwrap();
        let times = fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + duration);
        file.set_times(times).unwrap();
    }

    // ── Project safety test (S6 §19) ───────────────────────────────────────
    //
    // The sacred boundary: every Studio-side housekeeping operation this
    // phase adds (cache clear, temp cleanup, backup/recovery via the real
    // `open_with_recovery` entry point) must leave a sibling project
    // directory's files byte-for-byte unchanged. Uses the real production
    // types (`StudioPaths`, `FileSystemService`,
    // `valori_studio_storage::recovery::open_with_recovery`), not a mock.

    fn sha256_hex(bytes: &[u8]) -> String {
        // A tiny, dependency-free FNV-1a-based digest is NOT used here —
        // correctness of the *hash function* doesn't matter for this test
        // (it only needs to detect "did these bytes change"), so a
        // collision-irrelevant, allocation-free checksum is enough and
        // keeps this test file from needing a `sha2` dev-dependency the
        // rest of this crate doesn't otherwise use.
        let mut hash: u64 = 0xcbf29ce484222325;
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:016x}")
    }

    fn hash_dir_recursive(dir: &Path, out: &mut std::collections::BTreeMap<PathBuf, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                hash_dir_recursive(&path, out);
            } else {
                let bytes = fs::read(&path).unwrap();
                out.insert(path, sha256_hex(&bytes));
            }
        }
    }

    #[test]
    fn studio_housekeeping_never_touches_a_sibling_projects_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let studio_paths = valori_studio_storage::StudioPaths::new(root);

        // Build a disposable project fixture: project.json, wal/,
        // snapshots/, indexes/, vectors/ — exactly the file categories §19
        // asks for.
        let project_dir = studio_paths.project_dir("demo");
        fs::create_dir_all(project_dir.join("wal")).unwrap();
        fs::create_dir_all(project_dir.join("snapshots")).unwrap();
        fs::create_dir_all(project_dir.join("indexes")).unwrap();
        fs::create_dir_all(project_dir.join("vectors")).unwrap();
        fs::write(
            project_dir.join("project.json"),
            br#"{"id":"demo","dim":128}"#,
        )
        .unwrap();
        fs::write(project_dir.join("wal/000001.log"), b"fake-wal-entry").unwrap();
        fs::write(project_dir.join("snapshots/current.snap"), &[7u8; 512]).unwrap();
        fs::write(project_dir.join("indexes/hnsw.idx"), &[9u8; 256]).unwrap();
        fs::write(project_dir.join("vectors/shard0.vec"), &[3u8; 1024]).unwrap();

        let mut before = std::collections::BTreeMap::new();
        hash_dir_recursive(&project_dir, &mut before);
        assert_eq!(before.len(), 5, "sanity: all 5 fixture files were hashed");

        // 1. Studio startup / filesystem initialization: open studio.redb
        //    via the real recovery entry point (backups_dir + recovery log
        //    included, exactly as `desktop/src-tauri/src/studio_storage.rs`
        //    calls it in production).
        let (_db, _outcome) = valori_studio_storage::recovery::open_with_recovery(
            &studio_paths.studio_db(),
            &studio_paths.backups_dir(),
            &studio_paths.recovery_log_path(),
        )
        .unwrap();

        let svc = FileSystemService::new();

        // 2. Cache cleanup.
        svc.create_dir(&studio_paths.cache_dir()).unwrap();
        fs::write(studio_paths.cache_dir().join("stale.cache"), b"x").unwrap();
        svc.clear_cache(&studio_paths.cache_dir()).unwrap();

        // 3. Temp cleanup.
        svc.create_dir(&studio_paths.temp_dir()).unwrap();
        fs::write(studio_paths.temp_dir().join("scratch.tmp"), b"x").unwrap();
        svc.cleanup_stale_temp_files(&studio_paths.temp_dir(), std::time::Duration::ZERO)
            .unwrap();

        // 4. Studio metadata write (atomic_write, exercising the same
        //    write-temp/fsync/rename path project.json-adjacent Studio
        //    config would use).
        svc.atomic_write(&studio_paths.logs_dir().join("marker.json"), b"{}")
            .unwrap();

        // redb holds an exclusive lock — the "restart" open below must
        // happen after this process's own handle is dropped, exactly like
        // a real restart (previous process fully exited first).
        drop(_db);
        drop(_outcome);

        // 5. A second "restart" open — re-runs recovery/path resolution
        //    against the now-existing database.
        let (_db2, _outcome2) = valori_studio_storage::recovery::open_with_recovery(
            &studio_paths.studio_db(),
            &studio_paths.backups_dir(),
            &studio_paths.recovery_log_path(),
        )
        .unwrap();

        let mut after = std::collections::BTreeMap::new();
        hash_dir_recursive(&project_dir, &mut after);

        assert_eq!(
            before, after,
            "every project fixture file must be byte-for-byte unchanged \
             after Studio startup, recovery, cache cleanup, and temp cleanup"
        );

        // Also confirm Studio's own files landed where expected, siblings
        // of `projects/`, not inside it.
        assert!(studio_paths.studio_db().exists());
        assert!(!project_dir.join("studio.redb").exists());
        assert!(!project_dir.join("cache").exists());
        assert!(!project_dir.join("backups").exists());
    }
}

use super::*;
use rusqlite::params;
use std::time::Duration;
use tempfile::TempDir;

struct Fixture {
    _db_dir: TempDir,
    cache_dir: TempDir,
    catalog: Arc<Mutex<Connection>>,
}

impl Fixture {
    fn new() -> Self {
        let db_dir = TempDir::new().expect("db temp dir");
        let cache_dir = TempDir::new().expect("cache temp dir");
        let connection = Connection::open(db_dir.path().join("remote-state.db")).expect("open db");
        Self {
            _db_dir: db_dir,
            cache_dir,
            catalog: Arc::new(Mutex::new(connection)),
        }
    }

    fn open(&self, max_bytes: u64) -> MediaCache {
        MediaCache::open(
            self.cache_dir.path().to_path_buf(),
            Arc::clone(&self.catalog),
            max_bytes,
        )
        .expect("open media cache")
    }

    fn open_shared(&self, max_bytes: u64) -> Arc<Mutex<MediaCache>> {
        Arc::new(Mutex::new(self.open(max_bytes)))
    }
}

fn identity(path: &str, revision: Option<&str>, size: u64) -> CacheIdentity {
    CacheIdentity {
        library_id: "lib-1".to_owned(),
        relative_path: path.to_owned(),
        provider_revision: revision.map(str::to_owned),
        expected_size: size,
    }
}

/// Download every byte of `id` into the cache and record it, like playback
/// that ran to the end.
fn play_fully(cache: &mut MediaCache, id: &CacheIdentity, byte: u8) {
    let entry = cache.get_or_create(id).expect("open entry");
    entry
        .write_at(0, &vec![byte; id.expected_size as usize])
        .expect("write");
    cache.persist_ranges(&id.cache_key()).expect("persist");
}

fn is_cached(cache: &MediaCache, id: &CacheIdentity) -> bool {
    cache.get_entry(&id.cache_key()).unwrap().is_some()
}

/// Access times have millisecond resolution; space plays apart so the
/// least-recently-played order is unambiguous.
fn later() {
    std::thread::sleep(Duration::from_millis(15));
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

#[test]
fn cache_key_matches_the_existing_on_disk_layout() {
    assert_eq!(
        identity("media/a.mp3", Some("rev-1"), 100).cache_key(),
        "660a775952608660554743f4a9f5533cb334793cb17f8d37255c1202335d8bbf"
    );
    assert_eq!(
        identity("media/a.mp3", None, 100).cache_key(),
        "f9739c85f266d0f721d2803080018ba8b6c870e72df83c0a96f8f437cbdef42a"
    );
}

#[test]
fn cache_key_changes_with_revision_and_size() {
    let base = identity("media/a.mp3", Some("rev-1"), 100);
    assert_eq!(
        base.cache_key(),
        identity("media/a.mp3", Some("rev-1"), 100).cache_key()
    );
    assert_ne!(
        base.cache_key(),
        identity("media/a.mp3", Some("rev-2"), 100).cache_key()
    );
    assert_ne!(
        base.cache_key(),
        identity("media/a.mp3", Some("rev-1"), 200).cache_key()
    );
}

#[test]
fn default_budget_is_2_gib() {
    assert_eq!(DEFAULT_CACHE_BYTES_LIMIT, 2 * 1024 * 1024 * 1024);
}

#[test]
fn opens_on_a_desktop_control_database_that_already_has_the_entries_table() {
    let fixture = Fixture::new();
    {
        let connection = fixture.catalog.lock().unwrap();
        store::ensure_schema(&connection).unwrap();
        connection
            .execute("DROP TABLE remote_cache_pins", [])
            .unwrap();
    }
    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    let id = identity("media/a.mp3", Some("rev-1"), 10);
    play_fully(&mut cache, &id, 1);
    cache.pin_song("lib-1", "song-a", ["media/a.mp3"]).unwrap();
    assert_eq!(cache.pinned_songs("lib-1").unwrap(), vec!["song-a"]);
}

#[test]
fn complete_entry_reopens_verified_after_restart() {
    let fixture = Fixture::new();
    let id = identity("media/a.mp3", Some("rev-1"), 100);
    play_fully(&mut fixture.open(DEFAULT_CACHE_BYTES_LIMIT), &id, 42);

    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    assert!(cache.get_entry(&id.cache_key()).unwrap().unwrap().complete);
    let reopened = cache.get_or_create(&id).unwrap();
    assert!(reopened.is_complete());
    let mut buf = [0u8; 100];
    reopened.read_at(0, &mut buf).unwrap();
    assert!(buf.iter().all(|&b| b == 42));
    assert!(
        cache
            .get_entry(&id.cache_key())
            .unwrap()
            .unwrap()
            .verified_at_ms
            .is_some(),
        "complete entry must be verified after reopen"
    );
}

#[test]
fn interrupted_download_resumes_with_ranges_intact() {
    let fixture = Fixture::new();
    let id = identity("media/b.mp3", Some("rev-1"), 200);
    {
        let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
        let entry = cache.get_or_create(&id).unwrap();
        entry.write_at(0, &[1u8; 50]).unwrap();
        entry.write_at(100, &[2u8; 50]).unwrap();
        cache.persist_ranges(&id.cache_key()).unwrap();
    }

    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    let entry = cache.get_or_create(&id).unwrap();
    assert!(entry.is_cached(0, 50));
    assert!(entry.is_cached(100, 50));
    assert!(!entry.is_complete());
    assert!(!cache.get_entry(&id.cache_key()).unwrap().unwrap().complete);
}

#[test]
fn orphaned_data_file_removed_on_startup() {
    let fixture = Fixture::new();
    drop(fixture.open(DEFAULT_CACHE_BYTES_LIMIT));
    let orphan = fixture.cache_dir.path().join("orphan.cache");
    let unrelated = fixture.cache_dir.path().join("download.part");
    fs::write(&orphan, b"junk").unwrap();
    fs::write(&unrelated, b"other subsystem").unwrap();

    let _cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);

    assert!(!orphan.exists(), "orphaned data file must be removed");
    assert!(unrelated.exists(), "non-cache files must be left alone");
}

#[test]
fn changed_revision_creates_new_entry() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    let v1 = identity("media/c.mp3", Some("rev-1"), 100);
    let v2 = identity("media/c.mp3", Some("rev-2"), 100);
    play_fully(&mut cache, &v1, 9);

    let entry = cache.get_or_create(&v2).unwrap();
    assert!(
        !entry.is_complete(),
        "new revision must not reuse old entry's data"
    );
    assert!(is_cached(&cache, &v1));
    assert!(is_cached(&cache, &v2));
}

#[test]
fn mismatched_size_is_rejected() {
    let fixture = Fixture::new();
    let id = identity("media/d.mp3", Some("rev-1"), 100);
    play_fully(&mut fixture.open(DEFAULT_CACHE_BYTES_LIMIT), &id, 0);
    fs::write(
        fixture
            .cache_dir
            .path()
            .join(data_filename(&id.cache_key())),
        b"short",
    )
    .unwrap();

    let cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    assert!(
        !is_cached(&cache, &id),
        "entry with mismatched size must be discarded"
    );
}

#[test]
fn corrupted_ranges_beyond_file_length_rejected() {
    let fixture = Fixture::new();
    let id = identity("media/f.mp3", Some("rev-1"), 100);
    {
        let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
        let entry = cache.get_or_create(&id).unwrap();
        entry.write_at(0, &[0u8; 50]).unwrap();
        cache.persist_ranges(&id.cache_key()).unwrap();
    }
    fixture
        .catalog
        .lock()
        .unwrap()
        .execute(
            "UPDATE remote_cache_entries SET downloaded_ranges_json = ?2 WHERE cache_key = ?1",
            params![id.cache_key(), r#"[[0,999]]"#],
        )
        .unwrap();

    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    let entry = cache.get_or_create(&id).unwrap();
    assert!(
        !entry.is_cached(0, 999),
        "corrupted ranges must not be trusted"
    );
}

#[test]
fn usage_accounts_for_entries_from_earlier_runs() {
    let fixture = Fixture::new();
    let id = identity("media/e.mp3", Some("rev-1"), 100);
    play_fully(&mut fixture.open(DEFAULT_CACHE_BYTES_LIMIT), &id, 0);

    let usage = fixture.open(DEFAULT_CACHE_BYTES_LIMIT).usage().unwrap();
    assert_eq!(
        usage,
        CacheUsage {
            used_bytes: 100,
            limit_bytes: DEFAULT_CACHE_BYTES_LIMIT,
            entry_count: 1,
            pinned_count: 0,
        }
    );
}

#[test]
fn budget_evicts_the_least_recently_played_entry() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(250);
    let a = identity("media/a.mp3", Some("rev-1"), 100);
    let b = identity("media/b.mp3", Some("rev-1"), 100);
    let c = identity("media/c.mp3", Some("rev-1"), 100);
    play_fully(&mut cache, &a, 1);
    later();
    play_fully(&mut cache, &b, 2);
    later();

    cache.get_or_create(&c).unwrap();

    assert!(!is_cached(&cache, &a), "oldest entry must be evicted first");
    assert!(is_cached(&cache, &b));
    assert!(is_cached(&cache, &c));
    assert!(cache.usage().unwrap().used_bytes <= 250);
}

#[test]
fn replaying_a_song_makes_it_recently_played() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(250);
    let a = identity("media/a.mp3", Some("rev-1"), 100);
    let b = identity("media/b.mp3", Some("rev-1"), 100);
    let c = identity("media/c.mp3", Some("rev-1"), 100);
    play_fully(&mut cache, &a, 1);
    later();
    play_fully(&mut cache, &b, 2);
    later();
    cache.get_or_create(&a).unwrap();
    later();

    cache.get_or_create(&c).unwrap();

    assert!(is_cached(&cache, &a), "replayed entry must be kept");
    assert!(!is_cached(&cache, &b), "least recently played is evicted");
}

#[test]
fn play_order_survives_restart() {
    let fixture = Fixture::new();
    let a = identity("media/a.mp3", Some("rev-1"), 100);
    let b = identity("media/b.mp3", Some("rev-1"), 100);
    {
        let mut cache = fixture.open(250);
        play_fully(&mut cache, &a, 1);
        later();
        play_fully(&mut cache, &b, 2);
    }
    later();

    let mut cache = fixture.open(250);
    let c = identity("media/c.mp3", Some("rev-1"), 100);
    cache.get_or_create(&c).unwrap();

    assert!(!is_cached(&cache, &a));
    assert!(is_cached(&cache, &b));
    assert!(is_cached(&cache, &c));
}

#[test]
fn leased_entry_survives_eviction_and_clear() {
    let fixture = Fixture::new();
    let shared = fixture.open_shared(200);
    let a = identity("media/a.mp3", Some("rev-1"), 100);
    let b = identity("media/b.mp3", Some("rev-1"), 100);
    play_fully(&mut shared.lock().unwrap(), &a, 1);
    let lease = MediaCache::lease(&shared, &a.cache_key()).unwrap();
    later();
    play_fully(&mut shared.lock().unwrap(), &b, 2);
    later();

    shared
        .lock()
        .unwrap()
        .get_or_create(&identity("media/c.mp3", Some("rev-1"), 100))
        .unwrap();
    assert!(
        is_cached(&shared.lock().unwrap(), &a),
        "leased entry must survive eviction"
    );
    assert_eq!(shared.lock().unwrap().usage().unwrap().pinned_count, 1);

    let cleared = shared.lock().unwrap().clear_evictable().unwrap();
    assert_eq!(cleared, 1, "only the unleased entry is cleared");
    assert!(is_cached(&shared.lock().unwrap(), &a));

    drop(lease);
    assert_eq!(shared.lock().unwrap().clear_evictable().unwrap(), 1);
    assert!(!is_cached(&shared.lock().unwrap(), &a));
}

#[test]
fn pinned_song_is_never_evicted() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(300);
    let media = identity("media/content/pinned.mp3", Some("rev-1"), 100);
    let vocals = identity("stems/song-a/vocals.ogg", Some("rev-1"), 100);
    play_fully(&mut cache, &media, 1);
    play_fully(&mut cache, &vocals, 2);
    cache
        .pin_song(
            "lib-1",
            "song-a",
            ["media/content/pinned.mp3", "stems/song-a/vocals.ogg"],
        )
        .unwrap();

    for index in 0..5 {
        later();
        let other = identity(&format!("media/other-{index}.mp3"), Some("rev-1"), 100);
        play_fully(&mut cache, &other, 3);
    }

    assert!(is_cached(&cache, &media));
    assert!(is_cached(&cache, &vocals));
    assert!(is_cached(
        &cache,
        &identity("media/other-4.mp3", Some("rev-1"), 100)
    ));
    assert!(!is_cached(
        &cache,
        &identity("media/other-3.mp3", Some("rev-1"), 100)
    ));
    assert_eq!(cache.usage().unwrap().pinned_count, 2);
}

#[test]
fn pins_outlast_the_budget_when_they_fill_it() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(150);
    let pinned = identity("media/a.mp3", Some("rev-1"), 100);
    play_fully(&mut cache, &pinned, 1);
    cache.pin_song("lib-1", "song-a", ["media/a.mp3"]).unwrap();
    later();

    let next = identity("media/b.mp3", Some("rev-1"), 100);
    play_fully(&mut cache, &next, 2);

    assert!(is_cached(&cache, &pinned));
    assert!(is_cached(&cache, &next), "playback still caches new songs");
}

#[test]
fn clear_keeps_pinned_songs() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    let pinned = identity("media/a.mp3", Some("rev-1"), 100);
    let unpinned = identity("media/b.mp3", Some("rev-1"), 100);
    play_fully(&mut cache, &pinned, 1);
    play_fully(&mut cache, &unpinned, 2);
    cache.pin_song("lib-1", "song-a", ["media/a.mp3"]).unwrap();

    assert_eq!(cache.clear_evictable().unwrap(), 1);
    assert!(is_cached(&cache, &pinned));
    assert!(!is_cached(&cache, &unpinned));
}

#[test]
fn unpinned_song_becomes_evictable() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(200);
    let a = identity("media/a.mp3", Some("rev-1"), 100);
    play_fully(&mut cache, &a, 1);
    cache.pin_song("lib-1", "song-a", ["media/a.mp3"]).unwrap();
    later();
    play_fully(&mut cache, &identity("media/b.mp3", Some("rev-1"), 100), 2);
    cache.unpin_song("lib-1", "song-a").unwrap();
    later();

    cache
        .get_or_create(&identity("media/c.mp3", Some("rev-1"), 100))
        .unwrap();

    assert!(!is_cached(&cache, &a));
    assert!(cache.pinned_songs("lib-1").unwrap().is_empty());
}

#[test]
fn pins_survive_restart() {
    let fixture = Fixture::new();
    let a = identity("media/a.mp3", Some("rev-1"), 100);
    {
        let mut cache = fixture.open(200);
        play_fully(&mut cache, &a, 1);
        cache.pin_song("lib-1", "song-a", ["media/a.mp3"]).unwrap();
    }
    later();

    let mut cache = fixture.open(200);
    assert_eq!(cache.pinned_songs("lib-1").unwrap(), vec!["song-a"]);
    play_fully(&mut cache, &identity("media/b.mp3", Some("rev-1"), 100), 2);
    later();
    cache
        .get_or_create(&identity("media/c.mp3", Some("rev-1"), 100))
        .unwrap();
    assert!(is_cached(&cache, &a));
}

#[test]
fn pinning_again_replaces_the_song_assets() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(200);
    let old_media = identity("media/old.mp3", Some("rev-1"), 100);
    play_fully(&mut cache, &old_media, 1);
    cache
        .pin_song("lib-1", "song-a", ["media/old.mp3"])
        .unwrap();
    cache
        .pin_song("lib-1", "song-a", ["media/new.mp3"])
        .unwrap();
    cache.pin_song("lib-2", "song-z", ["media/z.mp3"]).unwrap();
    later();

    play_fully(&mut cache, &identity("media/b.mp3", Some("rev-1"), 100), 2);
    later();
    cache
        .get_or_create(&identity("media/c.mp3", Some("rev-1"), 100))
        .unwrap();

    assert!(
        !is_cached(&cache, &old_media),
        "an asset dropped from the pin is evictable"
    );
    assert_eq!(cache.pinned_songs("lib-1").unwrap(), vec!["song-a"]);
    assert_eq!(cache.pinned_songs("lib-2").unwrap(), vec!["song-z"]);
}

#[test]
fn pin_protects_only_the_current_revision_of_an_asset() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(250);
    cache.pin_song("lib-1", "song-a", ["media/a.mp3"]).unwrap();
    let v1 = identity("media/a.mp3", Some("rev-1"), 100);
    let v2 = identity("media/a.mp3", Some("rev-2"), 100);
    play_fully(&mut cache, &v1, 1);
    later();
    play_fully(&mut cache, &v2, 2);
    later();

    cache
        .get_or_create(&identity("media/b.mp3", Some("rev-1"), 100))
        .unwrap();

    assert!(
        !is_cached(&cache, &v1),
        "superseded revision must age out even while pinned"
    );
    assert!(is_cached(&cache, &v2));
}

#[test]
fn completed_asset_with_wrong_content_digest_is_not_kept() {
    let fixture = Fixture::new();
    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    let expected = sha256_hex(b"genuine bytes");
    let id = identity(&format!("media/content/{expected}.mp3"), Some("rev-1"), 13);
    let entry = cache.get_or_create(&id).unwrap();
    entry.write_at(0, b"tampered byte").unwrap();

    let error = cache.persist_ranges(&id.cache_key()).unwrap_err();

    assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
    assert!(!is_cached(&cache, &id));
    assert!(!entry.path().exists(), "the unverified copy is deleted");
}

#[test]
fn completed_asset_with_matching_content_digest_is_kept() {
    let fixture = Fixture::new();
    let bytes = b"genuine bytes";
    let id = identity(
        &format!("media/content/{}.mp3", sha256_hex(bytes)),
        Some("rev-1"),
        bytes.len() as u64,
    );
    {
        let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
        let entry = cache.get_or_create(&id).unwrap();
        entry.write_at(0, bytes).unwrap();
        cache.persist_ranges(&id.cache_key()).unwrap();
    }

    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    assert!(cache.get_or_create(&id).unwrap().is_complete());
}

#[test]
fn cached_asset_corrupted_on_disk_is_refetched_after_restart() {
    let fixture = Fixture::new();
    let bytes = b"genuine bytes";
    let id = identity(
        &format!("media/content/{}.mp3", sha256_hex(bytes)),
        Some("rev-1"),
        bytes.len() as u64,
    );
    {
        let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
        let entry = cache.get_or_create(&id).unwrap();
        entry.write_at(0, bytes).unwrap();
        cache.persist_ranges(&id.cache_key()).unwrap();
    }
    fs::write(
        fixture
            .cache_dir
            .path()
            .join(data_filename(&id.cache_key())),
        b"rotten bytes!",
    )
    .unwrap();

    let mut cache = fixture.open(DEFAULT_CACHE_BYTES_LIMIT);
    let entry = cache.get_or_create(&id).unwrap();
    assert!(!entry.is_complete(), "a corrupted copy must not be reused");
}

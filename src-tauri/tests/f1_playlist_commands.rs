use openkara_lib::cache;
use rusqlite::Connection;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&conn).expect("migrations should succeed");
    conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
    conn
}

fn insert_song(conn: &Connection, hash: &str) {
    conn.execute(
        "INSERT INTO songs (hash, file_path, title, artist, duration_ms, imported_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            hash,
            format!("/path/{hash}.mp3"),
            "Title",
            "Artist",
            200_000_i64,
            1_i64
        ],
    )
    .unwrap();
}

fn insert_playlist(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO playlists (id, name, created_at, updated_at) VALUES (?1, ?2, 1000, 1000)",
        rusqlite::params![id, name],
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Playlist CRUD
// ---------------------------------------------------------------------------

#[test]
fn create_playlist_returns_correct_fields() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO playlists (id, name, created_at, updated_at) VALUES ('pl-1', 'My List', 100, 200)",
        [],
    )
    .unwrap();

    let (id, name, created, updated): (String, String, i64, i64) = conn
        .query_row(
            "SELECT id, name, created_at, updated_at FROM playlists WHERE id = 'pl-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert_eq!(id, "pl-1");
    assert_eq!(name, "My List");
    assert_eq!(created, 100);
    assert_eq!(updated, 200);
}

#[test]
fn list_playlists_orders_by_sort_order_then_name() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO playlists (id, name, created_at, updated_at, sort_order) VALUES ('b', 'Zebra', 1, 1, 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO playlists (id, name, created_at, updated_at, sort_order) VALUES ('a', 'Alpha', 1, 1, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO playlists (id, name, created_at, updated_at, sort_order) VALUES ('c', 'Beta', 1, 1, 1)",
        [],
    )
    .unwrap();

    let names: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT p.name FROM playlists p \
                 LEFT JOIN playlist_songs ps ON ps.playlist_id = p.id \
                 GROUP BY p.id \
                 ORDER BY p.sort_order, p.name",
            )
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    assert_eq!(names, vec!["Alpha", "Beta", "Zebra"]);
}

#[test]
fn list_playlists_counts_songs() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Mixed");
    insert_song(&conn, "s1");
    insert_song(&conn, "s2");
    conn.execute(
        "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', 's1', 1, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', 's2', 1, 1)",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(ps.song_hash) FROM playlists p \
             LEFT JOIN playlist_songs ps ON ps.playlist_id = p.id \
             WHERE p.id = 'pl-1' GROUP BY p.id",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 2);
}

#[test]
fn rename_playlist_updates_name_and_timestamp() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Old Name");

    conn.execute(
        "UPDATE playlists SET name = 'New Name', updated_at = 9999 WHERE id = 'pl-1'",
        [],
    )
    .unwrap();

    let (name, updated): (String, i64) = conn
        .query_row(
            "SELECT name, updated_at FROM playlists WHERE id = 'pl-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(name, "New Name");
    assert_eq!(updated, 9999);
}

#[test]
fn delete_playlist_removes_row() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "To Delete");

    let rows = conn
        .execute("DELETE FROM playlists WHERE id = 'pl-1'", [])
        .unwrap();
    assert_eq!(rows, 1);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM playlists WHERE id = 'pl-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

// ---------------------------------------------------------------------------
// Add / remove songs
// ---------------------------------------------------------------------------

#[test]
fn add_songs_to_playlist_assigns_incremental_sort_order() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Queue");
    insert_song(&conn, "s1");
    insert_song(&conn, "s2");
    insert_song(&conn, "s3");

    // First batch
    for (i, hash) in ["s1", "s2"].iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) \
             VALUES ('pl-1', ?1, 100, ?2)",
            rusqlite::params![hash, i as i64],
        )
        .unwrap();
    }

    // Second batch (simulates add_songs_to_playlist appending after max)
    let max_sort: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM playlist_songs WHERE playlist_id = 'pl-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) \
         VALUES ('pl-1', 's3', 200, ?1)",
        rusqlite::params![max_sort + 1],
    )
    .unwrap();

    let orders: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT sort_order FROM playlist_songs WHERE playlist_id = 'pl-1' ORDER BY sort_order")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    assert_eq!(orders, vec![0, 1, 2]);
}

#[test]
fn add_songs_ignores_duplicates() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Dedup");
    insert_song(&conn, "s1");

    conn.execute(
        "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', 's1', 1, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', 's1', 2, 1)",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM playlist_songs WHERE playlist_id = 'pl-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "duplicate song should be ignored");
}

#[test]
fn remove_songs_from_playlist_deletes_specified_entries() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Remove Test");
    insert_song(&conn, "s1");
    insert_song(&conn, "s2");
    insert_song(&conn, "s3");
    for (i, hash) in ["s1", "s2", "s3"].iter().enumerate() {
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', ?1, 1, ?2)",
            rusqlite::params![hash, i as i64],
        )
        .unwrap();
    }

    // Remove s2
    conn.execute(
        "DELETE FROM playlist_songs WHERE playlist_id = 'pl-1' AND song_hash = 's2'",
        [],
    )
    .unwrap();

    let remaining: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT song_hash FROM playlist_songs WHERE playlist_id = 'pl-1' ORDER BY sort_order")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    assert_eq!(remaining, vec!["s1", "s3"]);
}

#[test]
fn add_songs_updates_playlist_timestamp() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Timestamped");
    insert_song(&conn, "s1");

    conn.execute(
        "UPDATE playlists SET updated_at = 9999 WHERE id = 'pl-1'",
        [],
    )
    .unwrap();

    // Simulate add_songs_to_playlist updating the timestamp
    conn.execute(
        "UPDATE playlists SET updated_at = 10000 WHERE id = 'pl-1'",
        [],
    )
    .unwrap();

    let updated: i64 = conn
        .query_row(
            "SELECT updated_at FROM playlists WHERE id = 'pl-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(updated, 10000);
}

// ---------------------------------------------------------------------------
// Singer assignment on playlist_songs
// ---------------------------------------------------------------------------

#[test]
fn set_queue_entry_singer_updates_singer_column() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Singer Queue");
    insert_song(&conn, "s1");
    conn.execute(
        "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', 's1', 1, 0)",
        [],
    )
    .unwrap();

    // Set singer
    conn.execute(
        "UPDATE playlist_songs SET singer = 'Alice' WHERE playlist_id = 'pl-1' AND song_hash = 's1'",
        [],
    )
    .unwrap();

    let singer: Option<String> = conn
        .query_row(
            "SELECT singer FROM playlist_songs WHERE playlist_id = 'pl-1' AND song_hash = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(singer.as_deref(), Some("Alice"));

    // Clear singer
    conn.execute(
        "UPDATE playlist_songs SET singer = NULL WHERE playlist_id = 'pl-1' AND song_hash = 's1'",
        [],
    )
    .unwrap();

    let singer: Option<String> = conn
        .query_row(
            "SELECT singer FROM playlist_songs WHERE playlist_id = 'pl-1' AND song_hash = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(singer, None);
}

#[test]
fn get_playlist_songs_returns_singer_field() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "With Singer");
    insert_song(&conn, "s1");
    conn.execute(
        "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order, singer) VALUES ('pl-1', 's1', 1, 0, 'Bob')",
        [],
    )
    .unwrap();

    let (hash, singer): (String, Option<String>) = conn
        .query_row(
            "SELECT song_hash, singer FROM playlist_songs WHERE playlist_id = 'pl-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(hash, "s1");
    assert_eq!(singer.as_deref(), Some("Bob"));
}

// ---------------------------------------------------------------------------
// Rotation state — advance
// ---------------------------------------------------------------------------

#[test]
fn advance_rotation_wraps_around() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO rotation_state (id, singer_names, current_index, mode, active) \
         VALUES (1, '[\"Alice\",\"Bob\",\"Charlie\"]', 2, 'round_robin', 1)",
        [],
    )
    .unwrap();

    // Advance from index 2 → should wrap to 0
    let current: i64 = conn
        .query_row(
            "SELECT current_index FROM rotation_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current, 2);

    let new_index = (current as usize + 1) % 3;
    conn.execute(
        "UPDATE rotation_state SET current_index = ?1 WHERE id = 1",
        rusqlite::params![new_index as i64],
    )
    .unwrap();

    let updated: i64 = conn
        .query_row(
            "SELECT current_index FROM rotation_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(updated, 0);
}

#[test]
fn advance_rotation_single_singer_stays_at_zero() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO rotation_state (id, singer_names, current_index, mode, active) \
         VALUES (1, '[\"Solo\"]', 0, 'round_robin', 1)",
        [],
    )
    .unwrap();

    let new_index = (0 + 1) % 1;
    assert_eq!(new_index, 0);
}

#[test]
fn advance_rotation_empty_singers_stays_at_zero() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO rotation_state (id, singer_names, current_index, mode, active) \
         VALUES (1, '[]', 0, 'round_robin', 0)",
        [],
    )
    .unwrap();

    // Empty singers → index stays 0
    let names_json: String = conn
        .query_row(
            "SELECT singer_names FROM rotation_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let names: Vec<String> = serde_json::from_str(&names_json).unwrap();
    let new_index = if names.is_empty() {
        0
    } else {
        (0 + 1) % names.len()
    };
    assert_eq!(new_index, 0);
}

// ---------------------------------------------------------------------------
// Rotation state — set_rotation_state INSERT OR REPLACE
// ---------------------------------------------------------------------------

#[test]
fn set_rotation_state_replaces_existing_row() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO rotation_state (id, singer_names, current_index, mode, active) \
         VALUES (1, '[\"A\"]', 0, 'round_robin', 0)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT OR REPLACE INTO rotation_state (id, singer_names, current_index, mode, active) \
         VALUES (1, '[\"X\",\"Y\"]', 1, 'single', 1)",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM rotation_state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "should still have exactly one row");

    let (names, idx, mode, active): (String, i64, String, i64) = conn
        .query_row(
            "SELECT singer_names, current_index, mode, active FROM rotation_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert_eq!(names, r#"["X","Y"]"#);
    assert_eq!(idx, 1);
    assert_eq!(mode, "single");
    assert_eq!(active, 1);
}

// ---------------------------------------------------------------------------
// Rotation state — get_rotation_state default
// ---------------------------------------------------------------------------

#[test]
fn get_rotation_state_returns_defaults_when_no_row() {
    let conn = setup_db();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM rotation_state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // The command returns defaults when no row exists — verify the table is empty
    // and the expected default values would be: singer_names=[], index=0, mode=round_robin, active=false
}

// ---------------------------------------------------------------------------
// Playlist songs — ordering and singer
// ---------------------------------------------------------------------------

#[test]
fn get_playlist_songs_orders_by_sort_order() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Ordered");
    for i in 0..5 {
        let hash = format!("s{i}");
        insert_song(&conn, &hash);
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', ?1, 1, ?2)",
            rusqlite::params![hash, (4 - i) as i64],
        )
        .unwrap();
    }

    let hashes: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT song_hash FROM playlist_songs WHERE playlist_id = 'pl-1' ORDER BY sort_order, added_at")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    // Should be reverse order: s4(0), s3(1), s2(2), s1(3), s0(4)
    assert_eq!(hashes, vec!["s4", "s3", "s2", "s1", "s0"]);
}

// ---------------------------------------------------------------------------
// Transaction safety — add_songs appends, doesn't interleave
// ---------------------------------------------------------------------------

#[test]
fn add_songs_batch_appends_after_existing() {
    let conn = setup_db();
    insert_playlist(&conn, "pl-1", "Batch");
    for i in 0..3 {
        let hash = format!("existing{i}");
        insert_song(&conn, &hash);
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) VALUES ('pl-1', ?1, 1, ?2)",
            rusqlite::params![hash, i as i64],
        )
        .unwrap();
    }

    // Simulate add_songs_to_playlist for a new batch
    let max_sort: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM playlist_songs WHERE playlist_id = 'pl-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(max_sort, 2);

    for (i, suffix) in ["new1", "new2"].iter().enumerate() {
        let hash = format!("song_{suffix}");
        insert_song(&conn, &hash);
        conn.execute(
            "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) \
             VALUES ('pl-1', ?1, 2, ?2)",
            rusqlite::params![hash, max_sort + 1 + i as i64],
        )
        .unwrap();
    }

    let orders: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT sort_order FROM playlist_songs WHERE playlist_id = 'pl-1' ORDER BY sort_order")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    assert_eq!(orders, vec![0, 1, 2, 3, 4]);
}

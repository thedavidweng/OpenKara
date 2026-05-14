use openkara_lib::cache;
use rusqlite::Connection;

#[test]
fn create_and_list_playlists() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");

    // Insert a playlist directly using SQL INSERT
    connection
        .execute(
            "INSERT INTO playlists (id, name, created_at, updated_at, sort_order) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["pl-001", "Favourites", 1000_i64, 1000_i64, 0_i64],
        )
        .expect("playlist insert should succeed");

    // Query playlists with the same JOIN used by list_playlists
    let mut stmt = connection
        .prepare(
            "SELECT p.id, p.name, p.created_at, p.updated_at, COUNT(ps.song_hash) \
             FROM playlists p \
             LEFT JOIN playlist_songs ps ON ps.playlist_id = p.id \
             GROUP BY p.id \
             ORDER BY p.sort_order, p.name",
        )
        .expect("query should prepare");

    let playlists: Vec<(String, String, i64, i64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .expect("query_map should succeed")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect should succeed");

    assert_eq!(playlists.len(), 1, "should have exactly one playlist");
    assert_eq!(playlists[0].0, "pl-001", "id should match");
    assert_eq!(playlists[0].1, "Favourites", "name should match");
    assert_eq!(playlists[0].2, 1000, "created_at should match");
    assert_eq!(playlists[0].3, 1000, "updated_at should match");
    assert_eq!(playlists[0].4, 0, "song_count should be 0 (no songs yet)");
}

#[test]
fn playlist_songs_cascade_on_delete() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");

    // Enable foreign keys (SQLite needs them enabled per-connection)
    connection
        .execute("PRAGMA foreign_keys = ON", [])
        .expect("foreign keys pragma should succeed");

    // Insert a song into songs
    connection
        .execute(
            "INSERT INTO songs (hash, file_path, title, artist, duration_ms, imported_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "song-abc",
                "/path/to/song.mp3",
                "Test Song",
                "Test Artist",
                200_000_i64,
                1_i64
            ],
        )
        .expect("song insert should succeed");

    // Insert a playlist
    connection
        .execute(
            "INSERT INTO playlists (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["pl-002", "My Playlist", 2000_i64, 2000_i64],
        )
        .expect("playlist insert should succeed");

    // Add song to playlist
    connection
        .execute(
            "INSERT INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["pl-002", "song-abc", 2000_i64, 1_i64],
        )
        .expect("playlist_songs insert should succeed");

    // Verify the entry exists before deletion
    let count_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM playlist_songs WHERE song_hash = ?1",
            rusqlite::params!["song-abc"],
            |row| row.get(0),
        )
        .expect("count query should succeed");
    assert_eq!(
        count_before, 1,
        "playlist_songs entry should exist before song deletion"
    );

    // Delete the song from songs – CASCADE should remove the playlist_songs entry
    connection
        .execute(
            "DELETE FROM songs WHERE hash = ?1",
            rusqlite::params!["song-abc"],
        )
        .expect("song delete should succeed");

    // Verify the playlist_songs entry is gone
    let count_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM playlist_songs WHERE song_hash = ?1",
            rusqlite::params!["song-abc"],
            |row| row.get(0),
        )
        .expect("count query should succeed");
    assert_eq!(
        count_after, 0,
        "playlist_songs entry should be cascaded on song deletion"
    );
}

#[test]
fn rotation_state_insert_and_read() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");

    // Insert rotation_state row with 3 singers, index=1, mode="round_robin", active=true
    connection
        .execute(
            "INSERT INTO rotation_state (id, singer_names, current_index, mode, active) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                1_i64,
                r#"["Alice","Bob","Charlie"]"#,
                1_i64,
                "round_robin",
                1_i64,
            ],
        )
        .expect("rotation_state insert should succeed");

    // Query it back and verify all fields
    let (singer_names, current_index, mode, active): (String, i64, String, i64) = connection
        .query_row(
            "SELECT singer_names, current_index, mode, active FROM rotation_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("query rotation_state should succeed");

    assert_eq!(singer_names, r#"["Alice","Bob","Charlie"]"#);
    assert_eq!(current_index, 1);
    assert_eq!(mode, "round_robin");
    assert_eq!(active, 1);

    // Insert again with updated data (id=1 with CHECK constraint) – should replace
    connection
        .execute(
            "INSERT OR REPLACE INTO rotation_state (id, singer_names, current_index, mode, active) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                1_i64,
                r#"["Dave","Eve"]"#,
                0_i64,
                "single",
                0_i64,
            ],
        )
        .expect("rotation_state replace should succeed");

    // Verify only one row exists and it has the new data
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM rotation_state", [], |row| row.get(0))
        .expect("count query should succeed");
    assert_eq!(count, 1, "should still be exactly one rotation_state row");

    let (new_names, new_index, new_mode, new_active): (String, i64, String, i64) = connection
        .query_row(
            "SELECT singer_names, current_index, mode, active FROM rotation_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("query rotation_state should succeed");

    assert_eq!(new_names, r#"["Dave","Eve"]"#);
    assert_eq!(new_index, 0);
    assert_eq!(new_mode, "single");
    assert_eq!(new_active, 0);
}

#[test]
fn rotation_state_defaults() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");

    // Verify rotation_state table is empty initially
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM rotation_state", [], |row| row.get(0))
        .expect("count query should succeed");
    assert_eq!(count, 0, "rotation_state should be empty initially");

    // Insert a row with only id (use all defaults)
    connection
        .execute("INSERT INTO rotation_state (id) VALUES (1)", [])
        .expect("rotation_state insert with defaults should succeed");

    // Query and verify defaults
    let (singer_names, current_index, mode, active): (String, i64, String, i64) = connection
        .query_row(
            "SELECT singer_names, current_index, mode, active FROM rotation_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("query rotation_state defaults should succeed");

    assert_eq!(singer_names, "[]", "default singer_names should be '[]'");
    assert_eq!(current_index, 0, "default current_index should be 0");
    assert_eq!(mode, "round_robin", "default mode should be 'round_robin'");
    assert_eq!(active, 0, "default active should be 0 (false)");
}

use crate::cache;
use crate::commands::error::{database_error, CommandResult};
use crate::AppState;
use rusqlite::TransactionBehavior;
use tauri::State;

// --- Data types ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub song_count: usize,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaylistSong {
    pub song_hash: String,
    pub added_at: i64,
    pub sort_order: i64,
    pub singer: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RotationState {
    pub singer_names: Vec<String>,
    pub current_index: usize,
    pub mode: String,
    pub active: bool,
}

// --- Helpers ---

fn get_connection(state: &AppState) -> CommandResult<rusqlite::Connection> {
    let library_root = state.library_root()?;
    cache::open_database(&library_root.database_path()).map_err(database_error)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

// --- Playlist commands ---

#[tauri::command]
pub fn list_playlists(state: State<'_, AppState>) -> CommandResult<Vec<Playlist>> {
    let conn = get_connection(&state)?;
    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.name, p.created_at, p.updated_at, COUNT(ps.song_hash) \
             FROM playlists p \
             LEFT JOIN playlist_songs ps ON ps.playlist_id = p.id \
             GROUP BY p.id \
             ORDER BY p.sort_order, p.name",
        )
        .map_err(database_error)?;
    let playlists = stmt
        .query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                song_count: row.get::<_, i64>(4)? as usize,
            })
        })
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    Ok(playlists)
}

#[tauri::command]
pub fn create_playlist(state: State<'_, AppState>, name: String) -> CommandResult<Playlist> {
    let conn = get_connection(&state)?;
    let id = uuid::Uuid::new_v4().to_string();
    let ts = now();
    conn.execute(
        "INSERT INTO playlists (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, name, ts, ts],
    )
    .map_err(database_error)?;
    Ok(Playlist {
        id,
        name,
        song_count: 0,
        created_at: ts,
        updated_at: ts,
    })
}

#[tauri::command]
pub fn rename_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    name: String,
) -> CommandResult<()> {
    let conn = get_connection(&state)?;
    let rows = conn
        .execute(
            "UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![name, now(), playlist_id],
        )
        .map_err(database_error)?;
    if rows == 0 {
        return Err(database_error(format!("playlist {playlist_id} not found")));
    }
    Ok(())
}

#[tauri::command]
pub fn delete_playlist(state: State<'_, AppState>, playlist_id: String) -> CommandResult<()> {
    let conn = get_connection(&state)?;
    let rows = conn
        .execute(
            "DELETE FROM playlists WHERE id = ?1",
            rusqlite::params![playlist_id],
        )
        .map_err(database_error)?;
    if rows == 0 {
        return Err(database_error(format!("playlist {playlist_id} not found")));
    }
    Ok(())
}

#[tauri::command]
pub fn add_songs_to_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    song_hashes: Vec<String>,
) -> CommandResult<()> {
    let conn = get_connection(&state)?;
    // Start sort_order after the highest existing value so multi-batch
    // inserts append rather than interleave.
    let max_sort: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM playlist_songs WHERE playlist_id = ?1",
            rusqlite::params![playlist_id],
            |row| row.get(0),
        )
        .map_err(database_error)?;
    let ts = now();
    for (i, hash) in song_hashes.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_hash, added_at, sort_order) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![playlist_id, hash, ts, max_sort + 1 + i as i64],
        )
        .map_err(database_error)?;
    }
    conn.execute(
        "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![ts, playlist_id],
    )
    .map_err(database_error)?;
    Ok(())
}

#[tauri::command]
pub fn remove_songs_from_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    song_hashes: Vec<String>,
) -> CommandResult<()> {
    let conn = get_connection(&state)?;
    for hash in &song_hashes {
        conn.execute(
            "DELETE FROM playlist_songs WHERE playlist_id = ?1 AND song_hash = ?2",
            rusqlite::params![playlist_id, hash],
        )
        .map_err(database_error)?;
    }
    conn.execute(
        "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now(), playlist_id],
    )
    .map_err(database_error)?;
    Ok(())
}

#[tauri::command]
pub fn get_playlist_songs(
    state: State<'_, AppState>,
    playlist_id: String,
) -> CommandResult<Vec<PlaylistSong>> {
    let conn = get_connection(&state)?;
    let mut stmt = conn
        .prepare(
            "SELECT song_hash, added_at, sort_order, singer \
             FROM playlist_songs \
             WHERE playlist_id = ?1 \
             ORDER BY sort_order, added_at",
        )
        .map_err(database_error)?;
    let songs = stmt
        .query_map(rusqlite::params![playlist_id], |row| {
            Ok(PlaylistSong {
                song_hash: row.get(0)?,
                added_at: row.get(1)?,
                sort_order: row.get(2)?,
                singer: row.get(3)?,
            })
        })
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    Ok(songs)
}

// --- Singer rotation commands ---

#[tauri::command]
pub fn set_rotation_state(
    state: State<'_, AppState>,
    rotation: RotationState,
) -> CommandResult<()> {
    let conn = get_connection(&state)?;
    let singer_names_json = serde_json::to_string(&rotation.singer_names)
        .map_err(|e| database_error(format!("JSON serialization failed: {e}")))?;
    conn.execute(
        "INSERT OR REPLACE INTO rotation_state (id, singer_names, current_index, mode, active) \
         VALUES (1, ?1, ?2, ?3, ?4)",
        rusqlite::params![
            singer_names_json,
            rotation.current_index as i64,
            rotation.mode,
            rotation.active as i64,
        ],
    )
    .map_err(database_error)?;
    Ok(())
}

#[tauri::command]
pub fn get_rotation_state(state: State<'_, AppState>) -> CommandResult<RotationState> {
    let conn = get_connection(&state)?;
    let mut stmt = conn
        .prepare(
            "SELECT singer_names, current_index, mode, active FROM rotation_state WHERE id = 1",
        )
        .map_err(database_error)?;
    let mut rows = stmt.query([]).map_err(database_error)?;
    match rows.next().map_err(database_error)? {
        Some(row) => {
            let singer_names_json: String = row.get(0).map_err(database_error)?;
            let current_index: i64 = row.get(1).map_err(database_error)?;
            let mode: String = row.get(2).map_err(database_error)?;
            let active: i64 = row.get(3).map_err(database_error)?;
            let singer_names: Vec<String> = serde_json::from_str(&singer_names_json)
                .map_err(|e| database_error(format!("JSON deserialization failed: {e}")))?;
            Ok(RotationState {
                singer_names,
                current_index: current_index as usize,
                mode,
                active: active != 0,
            })
        }
        None => Ok(RotationState {
            singer_names: Vec::new(),
            current_index: 0,
            mode: "round_robin".to_string(),
            active: false,
        }),
    }
}

#[tauri::command]
pub fn advance_rotation(state: State<'_, AppState>) -> CommandResult<RotationState> {
    let mut conn = get_connection(&state)?;
    // Use an immediate transaction so two concurrent calls cannot both
    // read the same current_index and produce a lost advance.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)?;
    let (singer_names, current_index, mode, active) = {
        let mut stmt = tx
            .prepare(
                "SELECT singer_names, current_index, mode, active FROM rotation_state WHERE id = 1",
            )
            .map_err(database_error)?;
        let mut rows = stmt.query([]).map_err(database_error)?;
        match rows.next().map_err(database_error)? {
            Some(row) => {
                let singer_names_json: String = row.get(0).map_err(database_error)?;
                let current_index: i64 = row.get(1).map_err(database_error)?;
                let mode: String = row.get(2).map_err(database_error)?;
                let active: i64 = row.get(3).map_err(database_error)?;
                let singer_names: Vec<String> = serde_json::from_str(&singer_names_json)
                    .map_err(|e| database_error(format!("JSON deserialization failed: {e}")))?;
                (singer_names, current_index as usize, mode, active != 0)
            }
            None => {
                return Ok(RotationState {
                    singer_names: Vec::new(),
                    current_index: 0,
                    mode: "round_robin".to_string(),
                    active: false,
                });
            }
        }
    };
    let new_index = if singer_names.is_empty() {
        0
    } else {
        (current_index + 1) % singer_names.len()
    };
    tx.execute(
        "UPDATE rotation_state SET current_index = ?1 WHERE id = 1",
        rusqlite::params![new_index as i64],
    )
    .map_err(database_error)?;
    tx.commit().map_err(database_error)?;
    Ok(RotationState {
        singer_names,
        current_index: new_index,
        mode,
        active,
    })
}

#[tauri::command]
pub fn set_queue_entry_singer(
    state: State<'_, AppState>,
    playlist_id: String,
    song_hash: String,
    singer: Option<String>,
) -> CommandResult<()> {
    let conn = get_connection(&state)?;
    let rows = conn
        .execute(
            "UPDATE playlist_songs SET singer = ?1 WHERE playlist_id = ?2 AND song_hash = ?3",
            rusqlite::params![singer, playlist_id, song_hash],
        )
        .map_err(database_error)?;
    if rows == 0 {
        return Err(database_error(format!(
            "playlist entry ({playlist_id}, {song_hash}) not found",
        )));
    }
    Ok(())
}

//! Song-scoped rebase for Publish Changes (ADR 0040).
//!
//! When the Remote Revision moved after a local edit, the writer compares
//! three databases: the base snapshot the Local Working Copy was built on,
//! the newer remote snapshot, and the local candidate. Rows are grouped per
//! song (the `songs` row plus every row of a table with a `song_hash`
//! column) and per shared table (every other table). A group the local side
//! changed is applied onto the remote snapshot when the remote side left it
//! equal to the base. A group both sides changed is a Pre-Publish Conflict.
//!
//! Which songs the local side changed is read from the rows, not from the
//! outbox: a song the outbox names but left equal to the base never
//! overrides the remote, and a song a mutation under-reported is still
//! carried. The outbox only says whether whole-repository rows changed.
//!
//! The comparison is by row content, never by bytes, so local migrations or
//! row order do not count as changes. The machine-local publish outbox and
//! full-text index tables (derived from `songs`) take no part.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode};
use sha2::{Digest, Sha256};

use crate::download::{integrity_error, io_error};
use crate::error::RemoteResult;
use crate::reader::open_readonly;

const SONGS_TABLE: &str = "songs";
const SONGS_KEY: &str = "hash";
const SONG_KEY_COLUMN: &str = "song_hash";
const OUTBOX_TABLE: &str = "remote_publish_outbox";

/// The local changes one publication carries, as recorded by the outbox.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub song_ids: Vec<String>,
    pub whole_repository: bool,
}

/// What a rebase does with changes both devices made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Stop with a Pre-Publish Conflict that names the songs both sides
    /// changed.
    #[default]
    Stop,
    /// The user chose to keep this device's changes for one Pre-Publish
    /// Conflict. Local rows win only where that conflict covered them, and
    /// remote changes elsewhere are kept. A conflict outside it stops again,
    /// so a stored choice never overwrites remote changes the user never saw.
    KeepLocal(ApprovedConflict),
}

impl ConflictPolicy {
    fn approves_song(&self, song_id: &str) -> bool {
        match self {
            Self::Stop => false,
            Self::KeepLocal(approved) => {
                approved.whole_repository || approved.song_ids.iter().any(|id| id == song_id)
            }
        }
    }

    pub(crate) fn approves_whole_repository(&self) -> bool {
        matches!(self, Self::KeepLocal(approved) if approved.whole_repository)
    }
}

/// The Pre-Publish Conflict a Keep Local choice answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovedConflict {
    /// Songs whose local rows replace the remote rows.
    pub song_ids: Vec<String>,
    /// The user agreed to replace the whole remote library with this
    /// device's database.
    pub whole_repository: bool,
}

impl From<&PrePublishConflict> for ApprovedConflict {
    fn from(conflict: &PrePublishConflict) -> Self {
        Self {
            song_ids: conflict
                .songs
                .iter()
                .map(|song| song.song_id.clone())
                .collect(),
            whole_repository: conflict.whole_repository,
        }
    }
}

/// A song named by a Pre-Publish Conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictingSong {
    pub song_id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
}

/// Publication stopped because both devices changed the same rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrePublishConflict {
    /// The remote generation the local changes could not be rebased onto.
    pub remote_generation: i64,
    /// Songs both devices changed, sorted by song id.
    pub songs: Vec<ConflictingSong>,
    /// The local changes include whole-repository rows, or the two
    /// snapshots cannot be compared row by row.
    pub whole_repository: bool,
}

impl PrePublishConflict {
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.songs.is_empty() {
            let names: Vec<&str> = self
                .songs
                .iter()
                .map(|song| song.title.as_deref().unwrap_or(&song.song_id))
                .collect();
            parts.push(format!(
                "songs changed on both devices: {}",
                names.join(", ")
            ));
        }
        if self.whole_repository {
            parts.push("whole-repository changes cannot be rebased".to_owned());
        }
        format!(
            "remote generation {} conflicts with local changes ({})",
            self.remote_generation,
            parts.join("; ")
        )
    }
}

/// Remove the remote snapshot and rebased candidate an unfinished rebase of
/// `operation_id` kept, once the user discarded that operation.
pub fn discard_rebase_state(working_copy_root: &Path, operation_id: &str) {
    let _ = fs::remove_file(remote_snapshot_path(working_copy_root, operation_id));
    let _ = fs::remove_file(rebased_candidate_path(working_copy_root, operation_id));
}

pub(crate) fn remote_snapshot_path(working_copy_root: &Path, operation_id: &str) -> PathBuf {
    working_copy_root
        .join(".openkara/rebase")
        .join(format!("{operation_id}.remote.sqlite"))
}

pub(crate) fn rebased_candidate_path(working_copy_root: &Path, operation_id: &str) -> PathBuf {
    working_copy_root
        .join(".openkara/candidates")
        .join(format!("{operation_id}.rebased.sqlite"))
}

/// The result of [`merge_onto_remote`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MergeResult {
    /// `output` holds the remote snapshot with the local changes applied.
    Merged,
    /// Both sides changed these songs, or whole-repository rows.
    Conflict {
        song_ids: Vec<String>,
        whole_repository: bool,
    },
    /// The snapshots cannot be compared row by row (different schemas), or
    /// the local rows break a constraint on the remote snapshot after the
    /// user approved replacing the whole remote library.
    Unmergeable,
}

/// Rebase the local changes onto the remote snapshot into `output`.
pub(crate) fn merge_onto_remote(
    base: &Path,
    remote: &Path,
    local: &Path,
    changes: &ChangeSet,
    policy: &ConflictPolicy,
    output: &Path,
) -> RemoteResult<MergeResult> {
    let Some(schema) = common_schema(&[base, remote, local])? else {
        return Ok(MergeResult::Unmergeable);
    };
    let base_rows = Fingerprint::read(base, &schema)?;
    let remote_rows = Fingerprint::read(remote, &schema)?;
    let local_rows = Fingerprint::read(local, &schema)?;

    let local_songs = changed_keys(&base_rows.songs, &local_rows.songs);
    let remote_songs = changed_keys(&base_rows.songs, &remote_rows.songs);
    let local_tables = changed_keys(&base_rows.tables, &local_rows.tables);
    let remote_tables = changed_keys(&base_rows.tables, &remote_rows.tables);

    // Both sides arriving at identical rows is no conflict: nothing is lost.
    let conflicting_songs: Vec<String> = local_songs
        .intersection(&remote_songs)
        .filter(|song| local_rows.songs.get(*song) != remote_rows.songs.get(*song))
        .cloned()
        .collect();
    let whole_repository = changes.whole_repository
        || local_tables
            .intersection(&remote_tables)
            .any(|table| local_rows.tables.get(table) != remote_rows.tables.get(table));
    let unapproved = (whole_repository && !policy.approves_whole_repository())
        || conflicting_songs
            .iter()
            .any(|song| !policy.approves_song(song));
    if unapproved {
        return Ok(MergeResult::Conflict {
            song_ids: conflicting_songs,
            whole_repository,
        });
    }

    let songs_to_apply: Vec<&String> = local_songs
        .iter()
        .filter(|song| local_rows.songs.get(*song) != remote_rows.songs.get(*song))
        .collect();
    let tables_to_apply: Vec<&TableShape> = schema
        .shared_tables()
        .filter(|table| {
            local_tables.contains(&table.name)
                && local_rows.tables.get(&table.name) != remote_rows.tables.get(&table.name)
        })
        .collect();

    let _ = fs::remove_file(output);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                format!("failed to create directory {}", parent.display()),
                error,
            )
        })?;
    }
    fs::copy(remote, output).map_err(|error| {
        io_error(
            format!(
                "failed to copy remote snapshot {} to {}",
                remote.display(),
                output.display()
            ),
            error,
        )
    })?;
    let result = apply_local_rows(output, local, &schema, &songs_to_apply, &tables_to_apply);
    match result {
        Ok(Applied::Clean) => Ok(MergeResult::Merged),
        Ok(Applied::Broken { song_ids }) => {
            let _ = fs::remove_file(output);
            // Rows that break a constraint on the remote snapshot cannot be
            // rebased at all: only replacing the whole remote library keeps them.
            Ok(if policy.approves_whole_repository() {
                MergeResult::Unmergeable
            } else {
                MergeResult::Conflict {
                    song_ids,
                    whole_repository: true,
                }
            })
        }
        Err(error) => {
            let _ = fs::remove_file(output);
            Err(error)
        }
    }
}

/// Whether `committed` already holds every change `local` made relative to
/// `base`: the accepted-commit test for a rebased publication whose commit
/// landed before the publisher learned about it.
pub(crate) fn carries_local_changes(
    base: &Path,
    committed: &Path,
    local: &Path,
) -> RemoteResult<bool> {
    let Some(schema) = common_schema(&[base, committed, local])? else {
        return Ok(false);
    };
    let base_rows = Fingerprint::read(base, &schema)?;
    let committed_rows = Fingerprint::read(committed, &schema)?;
    let local_rows = Fingerprint::read(local, &schema)?;
    let songs_carried = changed_keys(&base_rows.songs, &local_rows.songs)
        .iter()
        .all(|song| committed_rows.songs.get(song) == local_rows.songs.get(song));
    let tables_carried = changed_keys(&base_rows.tables, &local_rows.tables)
        .iter()
        .all(|table| committed_rows.tables.get(table) == local_rows.tables.get(table));
    Ok(songs_carried && tables_carried)
}

/// Copy the remaining publish outbox rows of the working database into the
/// replacement, so a crash-window row that was never projected survives.
pub(crate) fn carry_outbox_rows(replacement: &Path, working_database: &Path) -> RemoteResult<()> {
    if !working_database.is_file() {
        return Ok(());
    }
    let pending: i64 = {
        let working = open_readonly(working_database)?;
        if !table_exists(&working, OUTBOX_TABLE)? {
            return Ok(());
        }
        working
            .query_row(&format!("SELECT COUNT(*) FROM {OUTBOX_TABLE}"), [], |row| {
                row.get(0)
            })
            .map_err(|error| integrity_error(format!("failed to count outbox rows: {error}")))?
    };
    if pending == 0 {
        return Ok(());
    }
    let connection = open_writable(replacement)?;
    let result = (|| {
        connection
            .execute(
                "ATTACH DATABASE ?1 AS live",
                [working_database.to_string_lossy().as_ref()],
            )
            .map_err(sql_error)?;
        if !table_exists(&connection, OUTBOX_TABLE)? {
            let create_sql: String = connection
                .query_row(
                    "SELECT sql FROM live.sqlite_master WHERE type = 'table' AND name = ?1",
                    [OUTBOX_TABLE],
                    |row| row.get(0),
                )
                .map_err(sql_error)?;
            connection.execute_batch(&create_sql).map_err(sql_error)?;
        }
        connection
            .execute(
                &format!(
                    "INSERT OR REPLACE INTO main.{OUTBOX_TABLE} SELECT * FROM live.{OUTBOX_TABLE}"
                ),
                [],
            )
            .map_err(sql_error)?;
        connection
            .execute_batch("DETACH DATABASE live")
            .map_err(sql_error)
    })();
    close(connection, result)
}

fn changed_keys(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect()
}

enum Applied {
    Clean,
    /// Applying the local rows violated a constraint, so they cannot be
    /// rebased. `song_ids` names the songs whose rows broke it.
    Broken {
        song_ids: Vec<String>,
    },
}

fn apply_local_rows(
    output: &Path,
    local: &Path,
    schema: &Schema,
    songs: &[&String],
    tables: &[&TableShape],
) -> RemoteResult<Applied> {
    let connection = open_writable(output)?;
    let result = (|| {
        // Foreign keys are checked once after every row moved; enforcing them
        // per statement would reject a song whose stems arrive after it.
        connection
            .execute_batch("PRAGMA foreign_keys = OFF")
            .map_err(sql_error)?;
        connection
            .execute(
                "ATTACH DATABASE ?1 AS src",
                [local.to_string_lossy().as_ref()],
            )
            .map_err(sql_error)?;
        connection.execute_batch("BEGIN").map_err(sql_error)?;
        let mut broken_songs = BTreeSet::new();
        for song in songs {
            connection
                .execute_batch("SAVEPOINT song_rows")
                .map_err(sql_error)?;
            match replace_song_rows(&connection, schema, song) {
                Ok(()) => connection
                    .execute_batch("RELEASE song_rows")
                    .map_err(sql_error)?,
                Err(error) if is_constraint_violation(&error) => {
                    connection
                        .execute_batch("ROLLBACK TO song_rows; RELEASE song_rows")
                        .map_err(sql_error)?;
                    broken_songs.insert((*song).clone());
                }
                Err(error) => return Err(sql_error(error)),
            }
        }
        let mut broken_tables = false;
        for table in tables {
            let name = quote(&table.name);
            let replaced = connection.execute_batch(&format!(
                "SAVEPOINT shared_rows;
                 DELETE FROM main.{name};
                 INSERT INTO main.{name} SELECT * FROM src.{name};
                 RELEASE shared_rows;"
            ));
            match replaced {
                Ok(()) => {}
                Err(error) if is_constraint_violation(&error) => {
                    connection
                        .execute_batch("ROLLBACK TO shared_rows; RELEASE shared_rows")
                        .map_err(sql_error)?;
                    broken_tables = true;
                }
                Err(error) => return Err(sql_error(error)),
            }
        }
        connection.execute_batch("COMMIT").map_err(sql_error)?;
        connection
            .execute_batch("DETACH DATABASE src")
            .map_err(sql_error)?;

        let (violating_songs, violating_tables) = foreign_key_violations(&connection, schema)?;
        broken_songs.extend(violating_songs);
        broken_tables |= violating_tables;
        if broken_songs.is_empty() && !broken_tables {
            Ok(Applied::Clean)
        } else {
            Ok(Applied::Broken {
                song_ids: broken_songs.into_iter().collect(),
            })
        }
    })();
    close(connection, result)
}

/// Replace every remote row of `song` with the local rows. Child rows are
/// deleted before the song and inserted after it.
fn replace_song_rows(connection: &Connection, schema: &Schema, song: &str) -> rusqlite::Result<()> {
    let song_tables: Vec<&TableShape> = schema.song_tables().collect();
    for table in song_tables.iter().rev() {
        let key = table.song_key.as_deref().unwrap_or(SONG_KEY_COLUMN);
        connection.execute(
            &format!(
                "DELETE FROM main.{} WHERE {} = ?1",
                quote(&table.name),
                quote(key)
            ),
            [song],
        )?;
    }
    for table in &song_tables {
        let key = table.song_key.as_deref().unwrap_or(SONG_KEY_COLUMN);
        let name = quote(&table.name);
        connection.execute(
            &format!(
                "INSERT INTO main.{name} SELECT * FROM src.{name} WHERE {} = ?1",
                quote(key)
            ),
            [song],
        )?;
    }
    Ok(())
}

fn foreign_key_violations(
    connection: &Connection,
    schema: &Schema,
) -> RemoteResult<(BTreeSet<String>, bool)> {
    let violations: Vec<(String, Option<i64>)> = {
        let mut statement = connection
            .prepare("PRAGMA main.foreign_key_check")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))
            .map_err(sql_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql_error)?
    };
    let mut songs = BTreeSet::new();
    let mut shared = false;
    for (table_name, rowid) in violations {
        let song_key = schema
            .tables
            .iter()
            .find(|table| table.name == table_name)
            .and_then(|table| table.song_key.clone());
        match (song_key, rowid) {
            (Some(key), Some(rowid)) => {
                let song: String = connection
                    .query_row(
                        &format!(
                            "SELECT {} FROM main.{} WHERE rowid = ?1",
                            quote(&key),
                            quote(&table_name)
                        ),
                        [rowid],
                        |row| row.get(0),
                    )
                    .map_err(sql_error)?;
                songs.insert(song);
            }
            _ => shared = true,
        }
    }
    Ok((songs, shared))
}

/// One comparable table: its columns in order and, for song tables, the
/// column that names the song.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TableShape {
    name: String,
    columns: Vec<String>,
    song_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Schema {
    /// Sorted by name, with `songs` first among the song tables.
    tables: Vec<TableShape>,
}

impl Schema {
    fn song_tables(&self) -> impl Iterator<Item = &TableShape> {
        self.tables.iter().filter(|table| table.song_key.is_some())
    }

    fn shared_tables(&self) -> impl Iterator<Item = &TableShape> {
        self.tables.iter().filter(|table| table.song_key.is_none())
    }
}

/// The schema all `databases` share, or `None` when their comparable tables
/// differ in name, columns or column order.
fn common_schema(databases: &[&Path]) -> RemoteResult<Option<Schema>> {
    let mut schemas = Vec::with_capacity(databases.len());
    for database in databases {
        let connection = open_readonly(database)?;
        schemas.push(read_schema(&connection)?);
    }
    let first = schemas.remove(0);
    if first.tables.iter().all(|table| table.name != SONGS_TABLE) {
        return Ok(None);
    }
    Ok(schemas
        .iter()
        .all(|schema| *schema == first)
        .then_some(first))
}

fn read_schema(connection: &Connection) -> RemoteResult<Schema> {
    let entries: Vec<(String, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT name, COALESCE(sql, '') FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sql_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql_error)?
    };
    let virtual_tables: Vec<&str> = entries
        .iter()
        .filter(|(_, sql)| {
            sql.trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE VIRTUAL TABLE")
        })
        .map(|(name, _)| name.as_str())
        .collect();

    let mut tables = Vec::new();
    for (name, _) in &entries {
        let derived = virtual_tables.iter().any(|virtual_table| {
            name == virtual_table || name.starts_with(&format!("{virtual_table}_"))
        });
        if derived || name == OUTBOX_TABLE {
            continue;
        }
        let columns = table_columns(connection, name)?;
        let song_key = if name == SONGS_TABLE {
            Some(SONGS_KEY.to_owned())
        } else if columns.iter().any(|column| column == SONG_KEY_COLUMN) {
            Some(SONG_KEY_COLUMN.to_owned())
        } else {
            None
        };
        tables.push(TableShape {
            name: name.clone(),
            columns,
            song_key,
        });
    }
    tables.sort_by(|left, right| {
        (left.name != SONGS_TABLE, &left.name).cmp(&(right.name != SONGS_TABLE, &right.name))
    });
    Ok(Schema { tables })
}

fn table_columns(connection: &Connection, table: &str) -> RemoteResult<Vec<String>> {
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .map_err(sql_error)?;
    let rows = statement
        .query_map([table], |row| row.get(0))
        .map_err(sql_error)?;
    rows.collect::<rusqlite::Result<Vec<String>>>()
        .map_err(sql_error)
}

/// Content digests per song and per shared table.
struct Fingerprint {
    songs: BTreeMap<String, Vec<u8>>,
    tables: BTreeMap<String, Vec<u8>>,
}

impl Fingerprint {
    fn read(database: &Path, schema: &Schema) -> RemoteResult<Self> {
        let connection = open_readonly(database)?;
        let mut song_hashers: BTreeMap<String, Sha256> = BTreeMap::new();
        let mut tables = BTreeMap::new();
        for table in &schema.tables {
            let ordering = (1..=table.columns.len() + 1)
                .map(|position| position.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let key = table.song_key.as_deref().map(quote);
            let sql = format!(
                "SELECT {}, * FROM main.{} ORDER BY {ordering}",
                key.as_deref().unwrap_or("NULL"),
                quote(&table.name)
            );
            let mut statement = connection.prepare(&sql).map_err(sql_error)?;
            let column_count = table.columns.len();
            let mut rows = statement.query([]).map_err(sql_error)?;
            let mut table_hasher = Sha256::new();
            while let Some(row) = rows.next().map_err(sql_error)? {
                let hasher = match &table.song_key {
                    Some(_) => {
                        let song = match row.get_ref(0).map_err(sql_error)? {
                            ValueRef::Text(text) => String::from_utf8_lossy(text).into_owned(),
                            _ => continue,
                        };
                        let hasher = song_hashers.entry(song).or_default();
                        hasher.update(table.name.as_bytes());
                        hasher.update([0]);
                        hasher
                    }
                    None => &mut table_hasher,
                };
                for index in 1..=column_count {
                    hash_value(hasher, row.get_ref(index).map_err(sql_error)?);
                }
            }
            if table.song_key.is_none() {
                tables.insert(table.name.clone(), table_hasher.finalize().to_vec());
            }
        }
        let songs = song_hashers
            .into_iter()
            .map(|(song, hasher)| (song, hasher.finalize().to_vec()))
            .collect();
        Ok(Self { songs, tables })
    }
}

fn hash_value(hasher: &mut Sha256, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => hasher.update([0]),
        ValueRef::Integer(integer) => {
            hasher.update([1]);
            hasher.update(integer.to_le_bytes());
        }
        ValueRef::Real(real) => {
            hasher.update([2]);
            hasher.update(real.to_bits().to_le_bytes());
        }
        ValueRef::Text(bytes) => {
            hasher.update([3]);
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
        ValueRef::Blob(bytes) => {
            hasher.update([4]);
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
    }
}

/// Title and artist of each song, read from `databases` in order (a song the
/// local side deleted is still named by the remote snapshot).
pub(crate) fn describe_songs(
    song_ids: &[String],
    databases: &[&Path],
) -> RemoteResult<Vec<ConflictingSong>> {
    let mut songs: Vec<ConflictingSong> = song_ids
        .iter()
        .map(|song_id| ConflictingSong {
            song_id: song_id.clone(),
            title: None,
            artist: None,
        })
        .collect();
    for database in databases {
        let connection = open_readonly(database)?;
        let columns = table_columns(&connection, SONGS_TABLE)?;
        let column = |name: &str| {
            if columns.iter().any(|column| column == name) {
                quote(name)
            } else {
                "NULL".to_owned()
            }
        };
        let sql = format!(
            "SELECT {}, {} FROM {SONGS_TABLE} WHERE {SONGS_KEY} = ?1",
            column("title"),
            column("artist")
        );
        let mut statement = connection.prepare(&sql).map_err(sql_error)?;
        for song in songs.iter_mut().filter(|song| song.title.is_none()) {
            let found = statement
                .query_row([&song.song_id], |row| {
                    Ok((row.get::<_, Option<String>>(0)?, row.get(1)?))
                })
                .map(Some)
                .or_else(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })
                .map_err(sql_error)?;
            if let Some((title, artist)) = found {
                song.title = title;
                song.artist = artist;
            }
        }
    }
    Ok(songs)
}

fn table_exists(connection: &Connection, table: &str) -> RemoteResult<bool> {
    connection
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")
        .and_then(|mut statement| statement.exists([table]))
        .map_err(sql_error)
}

fn open_writable(path: &Path) -> RemoteResult<Connection> {
    Connection::open(path).map_err(|error| {
        integrity_error(format!(
            "failed to open {} for rebase: {error}",
            path.display()
        ))
    })
}

fn close<T>(connection: Connection, result: RemoteResult<T>) -> RemoteResult<T> {
    let closed = connection.close();
    match (result, closed) {
        (Err(error), _) => Err(error),
        (Ok(_), Err((_, error))) => Err(sql_error(error)),
        (Ok(value), Ok(())) => Ok(value),
    }
}

fn is_constraint_violation(error: &rusqlite::Error) -> bool {
    error.sqlite_error_code() == Some(ErrorCode::ConstraintViolation)
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn sql_error(error: rusqlite::Error) -> crate::error::RemoteError {
    integrity_error(format!("rebase query failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = "CREATE TABLE songs (hash TEXT PRIMARY KEY, title TEXT, artist TEXT);
        CREATE TABLE stems (
            song_hash TEXT PRIMARY KEY REFERENCES songs(hash),
            vocals_path TEXT
        );
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
        CREATE TABLE remote_publish_outbox (operation_id TEXT PRIMARY KEY);";

    struct Databases {
        dir: tempfile::TempDir,
    }

    impl Databases {
        /// Base, remote and local databases that all start from `seed`.
        fn new(seed: &str) -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            for name in ["base", "remote", "local"] {
                let connection = Connection::open(dir.path().join(format!("{name}.sqlite")))
                    .expect("create database");
                connection.execute_batch(SCHEMA).expect("schema");
                connection.execute_batch(seed).expect("seed");
            }
            Self { dir }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(format!("{name}.sqlite"))
        }

        fn execute(&self, name: &str, sql: &str) {
            Connection::open(self.path(name))
                .expect("open database")
                .execute_batch(sql)
                .expect("mutate database");
        }

        fn merge(&self, changes: &ChangeSet, policy: ConflictPolicy) -> MergeResult {
            merge_onto_remote(
                &self.path("base"),
                &self.path("remote"),
                &self.path("local"),
                changes,
                &policy,
                &self.path("merged"),
            )
            .expect("merge")
        }

        fn rows(&self, name: &str, sql: &str) -> Vec<String> {
            let connection = open_readonly(&self.path(name)).expect("open database");
            let mut statement = connection.prepare(sql).expect("prepare");
            statement
                .query_map([], |row| row.get(0))
                .expect("query")
                .collect::<rusqlite::Result<Vec<String>>>()
                .expect("collect")
        }

        fn songs(&self, name: &str) -> Vec<String> {
            self.rows(
                name,
                "SELECT hash || ':' || COALESCE(title, '') FROM songs ORDER BY hash",
            )
        }
    }

    fn keep_local(song_ids: &[&str], whole_repository: bool) -> ConflictPolicy {
        ConflictPolicy::KeepLocal(ApprovedConflict {
            song_ids: song_ids.iter().map(|id| (*id).to_owned()).collect(),
            whole_repository,
        })
    }

    fn songs(ids: &[&str]) -> ChangeSet {
        ChangeSet {
            song_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
            whole_repository: false,
        }
    }

    const TWO_SONGS: &str = "INSERT INTO songs (hash, title) VALUES ('s1', 'One'), ('s2', 'Two');
        INSERT INTO settings VALUES ('theme', 'dark');";

    #[test]
    fn songs_only_one_side_changed_are_merged_with_their_child_rows() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "UPDATE songs SET title = 'Two (remote)' WHERE hash = 's2';
             INSERT INTO songs (hash, title) VALUES ('s3', 'Three');",
        );
        databases.execute(
            "local",
            "UPDATE songs SET title = 'One (local)' WHERE hash = 's1';
             INSERT INTO songs (hash, title) VALUES ('s4', 'Four');
             INSERT INTO stems VALUES ('s4', 'stems/s4.ogg');
             INSERT INTO remote_publish_outbox VALUES ('op-local');",
        );

        let result = databases.merge(&songs(&["s1", "s4"]), ConflictPolicy::Stop);

        assert_eq!(result, MergeResult::Merged);
        assert_eq!(
            databases.songs("merged"),
            ["s1:One (local)", "s2:Two (remote)", "s3:Three", "s4:Four"]
        );
        assert_eq!(
            databases.rows("merged", "SELECT song_hash FROM stems"),
            ["s4"]
        );
        assert!(
            databases
                .rows("merged", "SELECT operation_id FROM remote_publish_outbox")
                .is_empty(),
            "the machine-local outbox never moves into a rebased snapshot"
        );
    }

    #[test]
    fn a_song_deleted_locally_is_deleted_from_the_rebased_snapshot() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "INSERT INTO songs (hash, title) VALUES ('s3', 'Three')",
        );
        databases.execute("local", "DELETE FROM songs WHERE hash = 's1'");

        let result = databases.merge(&songs(&["s1"]), ConflictPolicy::Stop);

        assert_eq!(result, MergeResult::Merged);
        assert_eq!(databases.songs("merged"), ["s2:Two", "s3:Three"]);
    }

    #[test]
    fn a_song_both_sides_changed_conflicts_even_when_the_outbox_missed_it() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "UPDATE songs SET title = 'Remote' WHERE hash = 's1'",
        );
        databases.execute(
            "local",
            "UPDATE songs SET title = 'Local' WHERE hash = 's1';
             UPDATE songs SET title = 'Two (local)' WHERE hash = 's2';",
        );

        let result = databases.merge(&songs(&["s2"]), ConflictPolicy::Stop);

        assert_eq!(
            result,
            MergeResult::Conflict {
                song_ids: vec!["s1".to_owned()],
                whole_repository: false,
            }
        );
        assert!(!databases.path("merged").exists());
    }

    #[test]
    fn a_song_the_outbox_names_but_left_unchanged_never_overrides_the_remote() {
        for policy in [ConflictPolicy::Stop, keep_local(&["s1"], false)] {
            let databases = Databases::new(TWO_SONGS);
            databases.execute(
                "remote",
                "UPDATE songs SET title = 'One (remote)' WHERE hash = 's1'",
            );
            databases.execute(
                "local",
                "UPDATE songs SET title = 'Two (local)' WHERE hash = 's2'",
            );

            let result = databases.merge(&songs(&["s1", "s2"]), policy.clone());

            assert_eq!(result, MergeResult::Merged, "{policy:?}");
            assert_eq!(
                databases.songs("merged"),
                ["s1:One (remote)", "s2:Two (local)"],
                "{policy:?}"
            );
        }
    }

    #[test]
    fn identical_changes_on_both_sides_are_not_a_conflict() {
        let databases = Databases::new(TWO_SONGS);
        for side in ["remote", "local"] {
            databases.execute(side, "UPDATE songs SET title = 'Same' WHERE hash = 's1'");
        }

        let result = databases.merge(&songs(&["s1"]), ConflictPolicy::Stop);

        assert_eq!(result, MergeResult::Merged);
        assert_eq!(databases.songs("merged"), ["s1:Same", "s2:Two"]);
    }

    #[test]
    fn keep_local_resolves_a_song_conflict_in_favour_of_the_local_rows() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "UPDATE songs SET title = 'Remote' WHERE hash = 's1';
             UPDATE songs SET title = 'Two (remote)' WHERE hash = 's2';",
        );
        databases.execute(
            "local",
            "UPDATE songs SET title = 'Local' WHERE hash = 's1'",
        );

        let result = databases.merge(&songs(&["s1"]), keep_local(&["s1"], false));

        assert_eq!(result, MergeResult::Merged);
        assert_eq!(databases.songs("merged"), ["s1:Local", "s2:Two (remote)"]);
    }

    #[test]
    fn keep_local_never_overrides_a_song_the_user_was_not_asked_about() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "UPDATE songs SET title = 'One (remote)' WHERE hash = 's1';
             UPDATE songs SET title = 'Two (remote)' WHERE hash = 's2';",
        );
        databases.execute(
            "local",
            "UPDATE songs SET title = 'One (local)' WHERE hash = 's1';
             UPDATE songs SET title = 'Two (local)' WHERE hash = 's2';",
        );

        let result = databases.merge(&songs(&["s1", "s2"]), keep_local(&["s1"], false));

        assert_eq!(
            result,
            MergeResult::Conflict {
                song_ids: vec!["s1".to_owned(), "s2".to_owned()],
                whole_repository: false,
            },
            "the new conflict names every song, so the user is asked again"
        );
        assert!(!databases.path("merged").exists());
    }

    #[test]
    fn song_scoped_keep_local_never_resolves_a_whole_repository_conflict() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute("remote", "UPDATE settings SET value = 'light'");
        databases.execute(
            "local",
            "UPDATE settings SET value = 'system';
             UPDATE songs SET title = 'One (local)' WHERE hash = 's1';",
        );

        let song_scoped = databases.merge(&songs(&["s1"]), keep_local(&["s1"], false));
        let whole = databases.merge(&songs(&["s1"]), keep_local(&[], true));

        assert_eq!(
            song_scoped,
            MergeResult::Conflict {
                song_ids: Vec::new(),
                whole_repository: true,
            }
        );
        assert_eq!(whole, MergeResult::Merged);
        assert_eq!(
            databases.rows("merged", "SELECT value FROM settings"),
            ["system"]
        );
        assert_eq!(databases.songs("merged"), ["s1:One (local)", "s2:Two"]);
    }

    #[test]
    fn rows_that_break_a_constraint_are_published_only_with_a_whole_repository_approval() {
        let databases = Databases::new(TWO_SONGS);
        for side in ["base", "remote", "local"] {
            databases.execute(
                side,
                "CREATE TABLE favorites (song TEXT NOT NULL REFERENCES songs(hash))",
            );
        }
        databases.execute("remote", "DELETE FROM songs WHERE hash = 's1'");
        databases.execute(
            "local",
            "INSERT INTO favorites VALUES ('s1');
             UPDATE songs SET title = 'Two (local)' WHERE hash = 's2';",
        );
        let broken = MergeResult::Conflict {
            song_ids: Vec::new(),
            whole_repository: true,
        };

        assert_eq!(
            databases.merge(&songs(&["s2"]), ConflictPolicy::Stop),
            broken,
            "a favorite of a song the remote removed breaks the foreign key"
        );
        assert_eq!(
            databases.merge(&songs(&["s2"]), keep_local(&["s2"], false)),
            broken
        );
        assert_eq!(
            databases.merge(&songs(&["s1"]), keep_local(&[], true)),
            MergeResult::Unmergeable
        );
    }

    #[test]
    fn a_shared_table_both_sides_changed_is_a_whole_repository_conflict() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute("remote", "UPDATE settings SET value = 'light'");
        databases.execute("local", "UPDATE settings SET value = 'system'");

        let result = databases.merge(&ChangeSet::default(), ConflictPolicy::Stop);

        assert_eq!(
            result,
            MergeResult::Conflict {
                song_ids: Vec::new(),
                whole_repository: true,
            }
        );
    }

    #[test]
    fn a_shared_table_only_the_local_side_changed_is_applied() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "INSERT INTO songs (hash, title) VALUES ('s3', 'Three')",
        );
        databases.execute("local", "UPDATE settings SET value = 'system'");

        let result = databases.merge(&ChangeSet::default(), ConflictPolicy::Stop);

        assert_eq!(result, MergeResult::Merged);
        assert_eq!(
            databases.rows("merged", "SELECT value FROM settings"),
            ["system"]
        );
        assert_eq!(databases.songs("merged"), ["s1:One", "s2:Two", "s3:Three"]);
    }

    #[test]
    fn whole_repository_changes_always_conflict() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "INSERT INTO songs (hash, title) VALUES ('s3', 'Three')",
        );
        databases.execute("local", "DELETE FROM songs");

        let result = databases.merge(
            &ChangeSet {
                song_ids: Vec::new(),
                whole_repository: true,
            },
            ConflictPolicy::Stop,
        );

        assert_eq!(
            result,
            MergeResult::Conflict {
                song_ids: Vec::new(),
                whole_repository: true,
            }
        );
    }

    #[test]
    fn a_child_row_whose_song_the_remote_removed_breaks_the_rebase() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute("remote", "DELETE FROM songs WHERE hash = 's1'");
        databases.execute("local", "INSERT INTO stems VALUES ('s1', 'stems/s1.ogg')");

        let result = databases.merge(&songs(&["s1"]), ConflictPolicy::Stop);

        assert_eq!(
            result,
            MergeResult::Conflict {
                song_ids: vec!["s1".to_owned()],
                whole_repository: false,
            }
        );
    }

    #[test]
    fn snapshots_with_different_schemas_cannot_be_merged() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute("remote", "ALTER TABLE songs ADD COLUMN rating INTEGER");
        databases.execute(
            "local",
            "UPDATE songs SET title = 'Local' WHERE hash = 's1'",
        );

        let result = databases.merge(&songs(&["s1"]), ConflictPolicy::Stop);

        assert_eq!(result, MergeResult::Unmergeable);
    }

    #[test]
    fn a_committed_snapshot_carrying_the_local_changes_is_recognized() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "UPDATE songs SET title = 'One (local)' WHERE hash = 's1';
             INSERT INTO songs (hash, title) VALUES ('s3', 'Three');",
        );
        databases.execute(
            "local",
            "UPDATE songs SET title = 'One (local)' WHERE hash = 's1'",
        );
        let carries = |committed: &str| {
            carries_local_changes(
                &databases.path("base"),
                &databases.path(committed),
                &databases.path("local"),
            )
            .expect("compare")
        };

        assert!(carries("remote"));
        assert!(!carries("base"));
    }

    #[test]
    fn pending_outbox_rows_are_carried_into_a_replacement_database() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "local",
            "INSERT INTO remote_publish_outbox VALUES ('op-pending')",
        );

        carry_outbox_rows(&databases.path("remote"), &databases.path("local")).expect("carry");

        assert_eq!(
            databases.rows("remote", "SELECT operation_id FROM remote_publish_outbox"),
            ["op-pending"]
        );
    }

    #[test]
    fn conflicting_songs_are_described_by_the_first_database_that_has_them() {
        let databases = Databases::new(TWO_SONGS);
        databases.execute(
            "remote",
            "UPDATE songs SET artist = 'Remote Artist' WHERE hash = 's2'",
        );
        databases.execute("local", "DELETE FROM songs WHERE hash = 's2'");

        let described = describe_songs(
            &["s1".to_owned(), "s2".to_owned()],
            &[&databases.path("local"), &databases.path("remote")],
        )
        .expect("describe");

        assert_eq!(
            described,
            [
                ConflictingSong {
                    song_id: "s1".to_owned(),
                    title: Some("One".to_owned()),
                    artist: None,
                },
                ConflictingSong {
                    song_id: "s2".to_owned(),
                    title: Some("Two".to_owned()),
                    artist: Some("Remote Artist".to_owned()),
                },
            ]
        );
    }
}

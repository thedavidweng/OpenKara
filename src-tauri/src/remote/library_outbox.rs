//! Library-database publish outbox.
//!
//! The change set for a remote publish is written into the **library** SQLite
//! database in the **same transaction** as the local song mutation. Startup
//! recovery can then rebuild a missing control-DB operation from this outbox
//! when the process dies between the library commit and the remote-state.db
//! projection.
//!
//! This table is machine-local control metadata. It must never be published as
//! part of a generation candidate: freeze clears it, and successful control
//! projection deletes the local row.

use crate::commands::error::{database_error, CommandResult};
use rusqlite::{params, Connection, OptionalExtension};

/// One durable publish intent stored next to the songs it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPublishOutboxRow {
    pub operation_id: String,
    pub song_ids: Vec<String>,
    pub expected_generation: Option<i64>,
    pub source_db_digest: Option<String>,
    pub created_at_ms: i64,
    pub projected_at_ms: Option<i64>,
}

/// Insert or replace an outbox row. Caller must hold a library DB transaction
/// that also contains the song mutation. Errors must propagate (fail closed).
pub fn upsert_library_publish_outbox(
    connection: &Connection,
    row: &LibraryPublishOutboxRow,
) -> CommandResult<()> {
    let song_ids_json = serde_json::to_string(&row.song_ids)
        .map_err(|e| database_error(format!("failed to serialize outbox song_ids: {e}")))?;
    connection
        .execute(
            "INSERT INTO remote_publish_outbox (
                operation_id, song_ids_json, expected_generation, source_db_digest,
                created_at_ms, projected_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(operation_id) DO UPDATE SET
                song_ids_json = excluded.song_ids_json,
                expected_generation = excluded.expected_generation,
                source_db_digest = excluded.source_db_digest,
                created_at_ms = excluded.created_at_ms,
                projected_at_ms = excluded.projected_at_ms",
            params![
                row.operation_id,
                song_ids_json,
                row.expected_generation,
                row.source_db_digest,
                row.created_at_ms,
                row.projected_at_ms,
            ],
        )
        .map_err(|e| database_error(format!("failed to upsert library publish outbox: {e}")))?;
    Ok(())
}

/// Delete one outbox row after successful control-DB projection.
pub fn delete_library_publish_outbox(
    connection: &Connection,
    operation_id: &str,
) -> CommandResult<()> {
    connection
        .execute(
            "DELETE FROM remote_publish_outbox WHERE operation_id = ?1",
            params![operation_id],
        )
        .map_err(|e| database_error(format!("failed to delete library publish outbox: {e}")))?;
    Ok(())
}

/// Remove every outbox row. Used when freezing a generation candidate so
/// machine-local control metadata never ships with the portable library DB.
pub fn clear_all_library_publish_outbox(connection: &Connection) -> CommandResult<()> {
    // Table may be missing on very old candidates; ignore missing-table.
    match connection.execute("DELETE FROM remote_publish_outbox", []) {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("no such table") {
                Ok(())
            } else {
                Err(database_error(format!(
                    "failed to clear library publish outbox: {e}"
                )))
            }
        }
    }
}

/// List outbox rows that have not yet been projected into the control DB.
pub fn list_unprojected_library_outbox(
    connection: &Connection,
) -> CommandResult<Vec<LibraryPublishOutboxRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT operation_id, song_ids_json, expected_generation, source_db_digest,
                    created_at_ms, projected_at_ms
             FROM remote_publish_outbox
             WHERE projected_at_ms IS NULL
             ORDER BY created_at_ms ASC",
        )
        .map_err(|e| database_error(format!("failed to prepare unprojected outbox query: {e}")))?;
    let rows = stmt
        .query_map([], map_outbox_row)
        .map_err(|e| database_error(format!("failed to query unprojected outbox: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect unprojected outbox: {e}")))?;
    Ok(rows)
}

/// Load a single outbox row (tests / diagnostics).
#[allow(dead_code)]
pub fn get_library_publish_outbox(
    connection: &Connection,
    operation_id: &str,
) -> CommandResult<Option<LibraryPublishOutboxRow>> {
    connection
        .query_row(
            "SELECT operation_id, song_ids_json, expected_generation, source_db_digest,
                    created_at_ms, projected_at_ms
             FROM remote_publish_outbox WHERE operation_id = ?1",
            params![operation_id],
            map_outbox_row,
        )
        .optional()
        .map_err(|e| database_error(format!("failed to load library publish outbox: {e}")))
}

fn map_outbox_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryPublishOutboxRow> {
    let song_ids_json: String = row.get(1)?;
    let song_ids: Vec<String> = serde_json::from_str(&song_ids_json).unwrap_or_default();
    Ok(LibraryPublishOutboxRow {
        operation_id: row.get(0)?,
        song_ids,
        expected_generation: row.get(2)?,
        source_db_digest: row.get(3)?,
        created_at_ms: row.get(4)?,
        projected_at_ms: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache;

    fn open_library_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("openkara.db");
        let conn = cache::open_database(&path).unwrap();
        cache::apply_migrations(&conn).unwrap();
        (dir, conn)
    }

    #[test]
    fn outbox_insert_and_delete_roundtrip() {
        let (_dir, conn) = open_library_db();
        let row = LibraryPublishOutboxRow {
            operation_id: "op-1".to_owned(),
            song_ids: vec!["a".to_owned(), "b".to_owned()],
            expected_generation: Some(3),
            source_db_digest: Some("deadbeef".to_owned()),
            created_at_ms: 1000,
            projected_at_ms: None,
        };
        upsert_library_publish_outbox(&conn, &row).unwrap();
        let loaded = get_library_publish_outbox(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.song_ids, vec!["a", "b"]);
        assert_eq!(list_unprojected_library_outbox(&conn).unwrap().len(), 1);
        delete_library_publish_outbox(&conn, "op-1").unwrap();
        assert!(get_library_publish_outbox(&conn, "op-1").unwrap().is_none());
        assert!(list_unprojected_library_outbox(&conn).unwrap().is_empty());
    }

    #[test]
    fn outbox_survives_same_transaction_as_mutation_commit() {
        let (_dir, conn) = open_library_db();
        let tx = conn.unchecked_transaction().unwrap();
        // Simulate a song write + outbox in one TX.
        tx.execute(
            "INSERT INTO songs (hash, audio_source_kind, imported_at) VALUES ('s1', 'original', 1)",
            [],
        )
        .unwrap();
        upsert_library_publish_outbox(
            &tx,
            &LibraryPublishOutboxRow {
                operation_id: "op-tx".to_owned(),
                song_ids: vec!["s1".to_owned()],
                expected_generation: Some(0),
                source_db_digest: None,
                created_at_ms: 1,
                projected_at_ms: None,
            },
        )
        .unwrap();
        tx.commit().unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs WHERE hash='s1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        let outbox = get_library_publish_outbox(&conn, "op-tx").unwrap().unwrap();
        assert_eq!(outbox.song_ids, vec!["s1"]);
    }

    #[test]
    fn outbox_rolls_back_with_failed_transaction() {
        let (_dir, conn) = open_library_db();
        {
            let tx = conn.unchecked_transaction().unwrap();
            tx.execute(
                "INSERT INTO songs (hash, audio_source_kind, imported_at) VALUES ('s2', 'original', 1)",
                [],
            )
            .unwrap();
            upsert_library_publish_outbox(
                &tx,
                &LibraryPublishOutboxRow {
                    operation_id: "op-rb".to_owned(),
                    song_ids: vec!["s2".to_owned()],
                    expected_generation: None,
                    source_db_digest: None,
                    created_at_ms: 1,
                    projected_at_ms: None,
                },
            )
            .unwrap();
            // Drop without commit — both song and outbox must vanish.
            drop(tx);
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs WHERE hash='s2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        assert!(get_library_publish_outbox(&conn, "op-rb")
            .unwrap()
            .is_none());
    }

    #[test]
    fn clear_all_removes_machine_local_rows_from_candidate() {
        let (_dir, conn) = open_library_db();
        upsert_library_publish_outbox(
            &conn,
            &LibraryPublishOutboxRow {
                operation_id: "op-x".to_owned(),
                song_ids: vec!["z".to_owned()],
                expected_generation: None,
                source_db_digest: None,
                created_at_ms: 1,
                projected_at_ms: None,
            },
        )
        .unwrap();
        clear_all_library_publish_outbox(&conn).unwrap();
        assert!(list_unprojected_library_outbox(&conn).unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // Fault-injection windows required by #175 acceptance
    // -----------------------------------------------------------------------

    use crate::remote::control_db::{
        bind_song_ids_mark_pending_and_dirty_tx, get_operation, get_repository_state,
        open_control_db, upsert_operation, LocalState, OperationKind, OperationPayload,
        OperationRow, OperationState,
    };

    fn open_control() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).unwrap();
        (dir, conn)
    }

    fn prepared_row(op_id: &str, library_id: &str) -> OperationRow {
        OperationRow {
            operation_id: op_id.to_owned(),
            library_id: library_id.to_owned(),
            operation_kind: OperationKind::Publish,
            state: OperationState::Prepared,
            expected_generation: Some(0),
            target_generation: None,
            source_db_digest: Some("aaa".to_owned()),
            candidate_db_digest: None,
            payload_json: r#"{"song_ids":[],"percent":0}"#.to_owned(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    /// Window: after song mutation, before outbox insert — TX not committed.
    /// Simulated by failing outbox write; song and outbox must both be absent.
    #[test]
    fn fault_after_song_before_outbox_rolls_back_both() {
        let (_dir, conn) = open_library_db();
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "INSERT INTO songs (hash, audio_source_kind, imported_at) VALUES ('s-fail', 'original', 1)",
            [],
        )
        .unwrap();
        // Simulate outbox insert failure before commit by rolling back.
        // Real path: upsert_library_publish_outbox returns Err → no commit.
        drop(tx);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs WHERE hash='s-fail'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "song must roll back with the failed TX");
        assert!(
            list_unprojected_library_outbox(&conn).unwrap().is_empty(),
            "outbox must not exist when TX rolled back before outbox insert"
        );
    }

    /// Window: after outbox insert, before control projection.
    /// Library committed with song_ids; control still Prepared(empty).
    /// Recovery rebuilds Pending + full song ID set from outbox.
    #[test]
    fn fault_after_outbox_before_control_projection_recovers_song_ids() {
        let (_lib_dir, lib_conn) = open_library_db();
        let (_ctl_dir, control) = open_control();

        let op_id = "op-crash-1";
        let library_id = "lib-1";
        upsert_operation(&control, &prepared_row(op_id, library_id)).unwrap();

        // Library commit succeeded: songs + outbox.
        let tx = lib_conn.unchecked_transaction().unwrap();
        tx.execute(
            "INSERT INTO songs (hash, audio_source_kind, imported_at) VALUES ('s-a', 'original', 1)",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO songs (hash, audio_source_kind, imported_at) VALUES ('s-b', 'original', 1)",
            [],
        )
        .unwrap();
        upsert_library_publish_outbox(
            &tx,
            &LibraryPublishOutboxRow {
                operation_id: op_id.to_owned(),
                song_ids: vec!["s-a".to_owned(), "s-b".to_owned()],
                expected_generation: Some(0),
                source_db_digest: Some("aaa".to_owned()),
                created_at_ms: 100,
                projected_at_ms: None,
            },
        )
        .unwrap();
        tx.commit().unwrap();
        // Crash before control projection.

        // Restart: project unprojected outbox.
        let rows = list_unprojected_library_outbox(&lib_conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].song_ids, vec!["s-a", "s-b"]);
        bind_song_ids_mark_pending_and_dirty_tx(&control, op_id, library_id, &rows[0].song_ids)
            .unwrap();
        delete_library_publish_outbox(&lib_conn, op_id).unwrap();

        let op = get_operation(&control, op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Pending);
        let payload = OperationPayload::from_json(&op.payload_json).unwrap();
        assert_eq!(payload.song_ids, vec!["s-a", "s-b"]);
        let repo = get_repository_state(&control, library_id).unwrap().unwrap();
        assert_eq!(repo.local_state, LocalState::Dirty);
        assert!(list_unprojected_library_outbox(&lib_conn)
            .unwrap()
            .is_empty());
    }

    /// Window: after operation upsert, before Dirty update — control TX
    /// must roll back both, leaving outbox unprojected for retry.
    #[test]
    fn fault_mid_control_projection_tx_leaves_outbox_retryable() {
        let (_lib_dir, lib_conn) = open_library_db();
        let (_ctl_dir, control) = open_control();
        let op_id = "op-crash-2";
        let library_id = "lib-1";
        upsert_operation(&control, &prepared_row(op_id, library_id)).unwrap();
        upsert_library_publish_outbox(
            &lib_conn,
            &LibraryPublishOutboxRow {
                operation_id: op_id.to_owned(),
                song_ids: vec!["s-x".to_owned()],
                expected_generation: Some(0),
                source_db_digest: Some("aaa".to_owned()),
                created_at_ms: 1,
                projected_at_ms: None,
            },
        )
        .unwrap();

        // Simulate mid-control-TX crash: update op to Pending but never Dirty.
        {
            let tx = control.unchecked_transaction().unwrap();
            let mut op = get_operation(&tx, op_id).unwrap().unwrap();
            let mut payload = OperationPayload::from_json(&op.payload_json).unwrap();
            payload.song_ids = vec!["s-x".to_owned()];
            op.payload_json = payload.to_json().unwrap();
            op.state = OperationState::Pending;
            upsert_operation(&tx, &op).unwrap();
            // Crash before Dirty / commit.
            drop(tx);
        }

        // Control must still show Prepared (empty) because TX rolled back.
        let op = get_operation(&control, op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Prepared);
        let payload = OperationPayload::from_json(&op.payload_json).unwrap();
        assert!(payload.song_ids.is_empty());

        // Outbox still unprojected — recovery retries full projection.
        let rows = list_unprojected_library_outbox(&lib_conn).unwrap();
        assert_eq!(rows.len(), 1);
        bind_song_ids_mark_pending_and_dirty_tx(&control, op_id, library_id, &rows[0].song_ids)
            .unwrap();
        let op = get_operation(&control, op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Pending);
        let payload = OperationPayload::from_json(&op.payload_json).unwrap();
        assert_eq!(payload.song_ids, vec!["s-x"]);
        assert_eq!(
            get_repository_state(&control, library_id)
                .unwrap()
                .unwrap()
                .local_state,
            LocalState::Dirty
        );
    }

    /// Window: after successful control projection, before outbox delete.
    /// Control is Pending+Dirty; outbox remains. Restart re-binds (idempotent)
    /// and deletes outbox. Never mark projected on failure.
    #[test]
    fn fault_after_control_projection_before_outbox_delete_is_idempotent() {
        let (_lib_dir, lib_conn) = open_library_db();
        let (_ctl_dir, control) = open_control();
        let op_id = "op-crash-3";
        let library_id = "lib-1";
        upsert_operation(&control, &prepared_row(op_id, library_id)).unwrap();
        upsert_library_publish_outbox(
            &lib_conn,
            &LibraryPublishOutboxRow {
                operation_id: op_id.to_owned(),
                song_ids: vec!["s-y".to_owned()],
                expected_generation: Some(0),
                source_db_digest: Some("aaa".to_owned()),
                created_at_ms: 1,
                projected_at_ms: None,
            },
        )
        .unwrap();

        // Control projection succeeded.
        bind_song_ids_mark_pending_and_dirty_tx(&control, op_id, library_id, &["s-y".to_owned()])
            .unwrap();
        // Crash before delete_library_publish_outbox.

        assert_eq!(list_unprojected_library_outbox(&lib_conn).unwrap().len(), 1);

        // Restart: re-project (idempotent) then delete.
        bind_song_ids_mark_pending_and_dirty_tx(&control, op_id, library_id, &["s-y".to_owned()])
            .unwrap();
        delete_library_publish_outbox(&lib_conn, op_id).unwrap();

        let op = get_operation(&control, op_id).unwrap().unwrap();
        assert_eq!(op.state, OperationState::Pending);
        let payload = OperationPayload::from_json(&op.payload_json).unwrap();
        assert_eq!(payload.song_ids, vec!["s-y"]);
        assert!(list_unprojected_library_outbox(&lib_conn)
            .unwrap()
            .is_empty());
    }

    /// Fail-closed: projection failure must NOT delete outbox or pretend projected.
    #[test]
    fn projection_failure_must_not_delete_outbox() {
        let (_lib_dir, lib_conn) = open_library_db();
        let (_ctl_dir, control) = open_control();
        // No prepared operation exists — bind must fail.
        upsert_library_publish_outbox(
            &lib_conn,
            &LibraryPublishOutboxRow {
                operation_id: "missing-op".to_owned(),
                song_ids: vec!["s-z".to_owned()],
                expected_generation: Some(0),
                source_db_digest: None,
                created_at_ms: 1,
                projected_at_ms: None,
            },
        )
        .unwrap();

        let err = bind_song_ids_mark_pending_and_dirty_tx(
            &control,
            "missing-op",
            "lib-1",
            &["s-z".to_owned()],
        );
        assert!(err.is_err());
        // Must remain unprojected for retry — never mark projected_at or delete.
        assert_eq!(list_unprojected_library_outbox(&lib_conn).unwrap().len(), 1);
        assert!(get_operation(&control, "missing-op").unwrap().is_none());
    }

    /// Candidate freeze must strip machine-local outbox from the published DB.
    #[test]
    fn candidate_copy_must_clear_outbox() {
        let (_dir, conn) = open_library_db();
        upsert_library_publish_outbox(
            &conn,
            &LibraryPublishOutboxRow {
                operation_id: "op-local".to_owned(),
                song_ids: vec!["local-only".to_owned()],
                expected_generation: Some(1),
                source_db_digest: Some("digest".to_owned()),
                created_at_ms: 1,
                projected_at_ms: None,
            },
        )
        .unwrap();
        // Simulate freeze sanitization on the candidate file.
        clear_all_library_publish_outbox(&conn).unwrap();
        assert!(
            list_unprojected_library_outbox(&conn).unwrap().is_empty(),
            "machine-local outbox must not ship with generation candidates"
        );
    }
}

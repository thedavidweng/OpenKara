use openkara_lib::{cache, library_root::LibraryRoot};
use rusqlite::Connection;

fn database_lifecycle_setup() {
    let _tmp = tempfile::tempdir().expect("temp dir should create");
    let _library =
        LibraryRoot::create(_tmp.path().join("library").as_path()).expect("library should create");
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
}

#[test]
fn scaffold_compiles() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement clean database creation"]
fn clean_database_creation_builds_schema_on_empty_path() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement upgrade from preceding stable schema"]
fn upgrade_from_preceding_stable_schema_migrates_existing_database() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement forced termination during migration"]
fn forced_termination_during_migration_leaves_consistent_state() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement restart after interrupted migration"]
fn restart_after_interrupted_migration_completes_or_recovers() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement duplicate import"]
fn duplicate_import_is_idempotent() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement missing source file"]
fn missing_source_file_reports_failure() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement renamed source file"]
fn renamed_source_file_triggers_reimport() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement read-only library directory"]
fn read_only_library_directory_reports_error() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement removable library directory disconnected during playback"]
fn removable_library_directory_disconnected_during_playback_gracefully_errors() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement symlinked library path"]
fn symlinked_library_path_is_resolved() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement non-ASCII and long filenames"]
fn non_ascii_and_long_filenames_are_supported() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement Windows reserved names"]
fn windows_reserved_names_are_rejected_or_escaped() {
    database_lifecycle_setup();
}

#[test]
#[ignore = "TODO: implement full disk during metadata write"]
fn full_disk_during_metadata_write_returns_error() {
    database_lifecycle_setup();
}

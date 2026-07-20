-- Local-only control-plane database for remote repository state.
--
-- This database lives at <app-data>/remote-state.db and is NEVER uploaded to
-- any cloud provider. It is the authoritative local record of:
--   * repository cleanliness and expected remote generation
--   * durable operation/outbox state
--   * resumable upload sessions and offsets
--   * verified cache catalog and LRU metadata
--   * recovery after process termination
--   * deferred remote cleanup
--
-- All schema objects use CREATE TABLE IF NOT EXISTS so that re-running the
-- migration on an already-migrated database is a no-op (idempotent).

CREATE TABLE IF NOT EXISTS remote_repository_state (
  library_id TEXT PRIMARY KEY,
  committed_generation INTEGER NOT NULL,
  committed_manifest_revision TEXT,
  local_base_generation INTEGER NOT NULL,
  local_db_digest TEXT,
  local_state TEXT NOT NULL CHECK (
    local_state IN ('clean', 'dirty', 'publishing', 'conflicted', 'reauth_required')
  ),
  active_operation_id TEXT,
  last_success_at_ms INTEGER,
  last_error_code TEXT,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS remote_operations (
  operation_id TEXT PRIMARY KEY,
  library_id TEXT NOT NULL,
  operation_kind TEXT NOT NULL CHECK (
    operation_kind IN ('publish', 'pull', 'download_asset', 'delete_asset', 'gc')
  ),
  state TEXT NOT NULL CHECK (
    state IN (
      'prepared', 'pending', 'running', 'retry_wait', 'committing',
      'verifying', 'completed', 'failed', 'conflicted', 'cancelled'
    )
  ),
  expected_generation INTEGER,
  target_generation INTEGER,
  source_db_digest TEXT,
  candidate_db_digest TEXT,
  payload_json TEXT NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  next_attempt_at_ms INTEGER,
  error_code TEXT,
  error_detail TEXT,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS remote_transfer_parts (
  operation_id TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  direction TEXT NOT NULL CHECK (direction IN ('upload', 'download')),
  expected_size INTEGER,
  expected_digest TEXT,
  provider_revision TEXT,
  provider_session_id TEXT,
  transferred_bytes INTEGER NOT NULL DEFAULT 0,
  state TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  PRIMARY KEY (operation_id, relative_path, direction)
);

CREATE TABLE IF NOT EXISTS remote_cache_entries (
  cache_key TEXT PRIMARY KEY,
  library_id TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  provider_revision TEXT,
  content_digest TEXT,
  expected_size INTEGER NOT NULL,
  downloaded_ranges_json TEXT NOT NULL,
  complete INTEGER NOT NULL,
  pinned_count INTEGER NOT NULL DEFAULT 0,
  last_access_at_ms INTEGER NOT NULL,
  verified_at_ms INTEGER,
  data_path TEXT NOT NULL
);

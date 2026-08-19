PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meeting_records (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    transcription_status TEXT NOT NULL,
    minutes_status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    stopped_at TEXT,
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_meeting_records_created_at
    ON meeting_records(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_meeting_records_deleted_at
    ON meeting_records(deleted_at);

CREATE TABLE IF NOT EXISTS recording_runs (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL REFERENCES meeting_records(id) ON DELETE CASCADE,
    sequence_number INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    status TEXT NOT NULL,
    sources_json TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    UNIQUE(meeting_id, sequence_number)
);

CREATE TABLE IF NOT EXISTS recording_assets (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL REFERENCES meeting_records(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    format TEXT NOT NULL,
    status TEXT NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    byte_size INTEGER NOT NULL DEFAULT 0,
    checksum TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS transcript_segments (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL REFERENCES meeting_records(id) ON DELETE CASCADE,
    recording_run_id TEXT,
    revision INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    confidence REAL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(meeting_id, revision, id)
);

CREATE INDEX IF NOT EXISTS idx_transcript_segments_meeting_revision
    ON transcript_segments(meeting_id, revision, start_ms);

CREATE TABLE IF NOT EXISTS meeting_minutes (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL REFERENCES meeting_records(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    content TEXT NOT NULL,
    provider_label TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(meeting_id, revision)
);

CREATE TABLE IF NOT EXISTS processing_jobs (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL REFERENCES meeting_records(id) ON DELETE CASCADE,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    input_revision INTEGER,
    error_summary TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_processing_jobs_status
    ON processing_jobs(status, updated_at);

CREATE TABLE IF NOT EXISTS provider_profiles (
    id TEXT PRIMARY KEY,
    capability TEXT NOT NULL,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    model TEXT NOT NULL,
    endpoint_flavor TEXT NOT NULL,
    secret_reference TEXT,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

PRAGMA user_version = 1;

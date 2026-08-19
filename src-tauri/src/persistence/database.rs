use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::{migrations, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingRecordRow {
    pub id: String,
    pub title: String,
    pub status: String,
    pub transcription_status: String,
    pub minutes_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub stopped_at: Option<String>,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewMeetingRecord {
    pub id: String,
    pub title: String,
    pub status: String,
    pub transcription_status: String,
    pub minutes_status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingMinutesRow {
    pub id: String,
    pub meeting_id: String,
    pub revision: i64,
    pub content: String,
    pub provider_label: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAssetRow {
    pub relative_path: String,
    pub format: String,
    pub status: String,
    pub duration_ms: i64,
    pub byte_size: i64,
}

#[derive(Debug, Clone)]
pub struct NewProcessingJob {
    pub id: String,
    pub meeting_id: String,
    pub job_type: String,
    pub input_revision: Option<i64>,
    pub created_at: String,
}

impl NewProcessingJob {
    #[cfg(test)]
    pub fn for_test(
        id: &str,
        meeting_id: &str,
        job_type: &str,
        input_revision: Option<i64>,
    ) -> Self {
        Self {
            id: id.to_string(),
            meeting_id: meeting_id.to_string(),
            job_type: job_type.to_string(),
            input_revision,
            created_at: "2026-08-19T00:00:00Z".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessingJobRow {
    pub id: String,
    pub meeting_id: String,
    pub job_type: String,
    pub status: String,
    pub attempts: i64,
    pub input_revision: Option<i64>,
    pub error_summary: Option<String>,
}

impl NewMeetingRecord {
    #[cfg(test)]
    pub fn for_test(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            status: "preparing".to_string(),
            transcription_status: "idle".to_string(),
            minutes_status: "idle".to_string(),
            created_at: "2026-08-19T00:00:00Z".to_string(),
        }
    }
}

pub struct MeetingRepository {
    connection: Connection,
}

impl MeetingRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        migrations::migrate(&connection)?;
        Ok(Self { connection })
    }

    pub fn table_names(&self) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn create_meeting(&self, meeting: &NewMeetingRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO meeting_records (
                id, title, status, transcription_status, minutes_status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                meeting.id,
                meeting.title,
                meeting.status,
                meeting.transcription_status,
                meeting.minutes_status,
                meeting.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn update_meeting_states(
        &self,
        meeting_id: &str,
        status: &str,
        transcription_status: &str,
        minutes_status: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records
             SET status = ?2, transcription_status = ?3, minutes_status = ?4, updated_at = ?5
             WHERE id = ?1",
            params![
                meeting_id,
                status,
                transcription_status,
                minutes_status,
                updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn update_transcription_status(
        &self,
        meeting_id: &str,
        status: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records SET transcription_status = ?2, updated_at = ?3 WHERE id = ?1",
            params![meeting_id, status, updated_at],
        )?;
        Ok(())
    }

    pub fn update_recording_status(
        &self,
        meeting_id: &str,
        status: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![meeting_id, status, updated_at],
        )?;
        Ok(())
    }

    pub fn update_minutes_status(
        &self,
        meeting_id: &str,
        status: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records SET minutes_status = ?2, updated_at = ?3 WHERE id = ?1",
            params![meeting_id, status, updated_at],
        )?;
        Ok(())
    }

    pub fn latest_recording_generation(&self, meeting_id: &str) -> Result<i64> {
        self.connection
            .query_row(
                "SELECT COALESCE(MAX(generation), 0) FROM recording_runs WHERE meeting_id = ?1",
                params![meeting_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn list_meetings(&self, deleted: bool) -> Result<Vec<MeetingRecordRow>> {
        let predicate = if deleted {
            "deleted_at IS NOT NULL"
        } else {
            "deleted_at IS NULL"
        };
        let sql = format!(
            "SELECT id, title, status, transcription_status, minutes_status,
                    created_at, updated_at, stopped_at, deleted_at
             FROM meeting_records WHERE {predicate} ORDER BY created_at DESC"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok(MeetingRecordRow {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                transcription_status: row.get(3)?,
                minutes_status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                stopped_at: row.get(7)?,
                deleted_at: row.get(8)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_meeting(&self, meeting_id: &str) -> Result<Option<MeetingRecordRow>> {
        self.connection
            .query_row(
                "SELECT id, title, status, transcription_status, minutes_status,
                        created_at, updated_at, stopped_at, deleted_at
                 FROM meeting_records WHERE id = ?1",
                params![meeting_id],
                map_meeting_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn rename_meeting(&self, meeting_id: &str, title: &str, updated_at: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![meeting_id, title, updated_at],
        )?;
        Ok(())
    }

    pub fn latest_recording_asset(&self, meeting_id: &str) -> Result<Option<RecordingAssetRow>> {
        self.connection
            .query_row(
                "SELECT relative_path, format, status, duration_ms, byte_size
                 FROM recording_assets WHERE meeting_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![meeting_id],
                |row| {
                    Ok(RecordingAssetRow {
                        relative_path: row.get(0)?,
                        format: row.get(1)?,
                        status: row.get(2)?,
                        duration_ms: row.get(3)?,
                        byte_size: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn full_transcript(&self, meeting_id: &str) -> Result<String> {
        let mut statement = self.connection.prepare(
            "SELECT text FROM transcript_segments
             WHERE meeting_id = ?1 AND status = 'final'
             ORDER BY start_ms, revision, id",
        )?;
        let rows = statement.query_map(params![meeting_id], |row| row.get::<_, String>(0))?;
        let parts = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(parts.join("\n"))
    }

    pub fn recover_incomplete_meetings(&self, updated_at: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT id FROM meeting_records
             WHERE deleted_at IS NULL AND status IN ('preparing', 'recording', 'stopping', 'processing')",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let ids = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        for meeting_id in &ids {
            self.connection.execute(
                "UPDATE meeting_records SET status = 'interrupted', updated_at = ?2 WHERE id = ?1",
                params![meeting_id, updated_at],
            )?;
            self.connection.execute(
                "UPDATE recording_runs SET status = 'interrupted', ended_at = COALESCE(ended_at, ?2)
                 WHERE meeting_id = ?1 AND status = 'recording'",
                params![meeting_id, updated_at],
            )?;
        }
        Ok(ids)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_recording_run(
        &self,
        run_id: &str,
        meeting_id: &str,
        sequence_number: i64,
        generation: i64,
        sources_json: &str,
        started_at: &str,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO recording_runs (
                id, meeting_id, sequence_number, generation, status, sources_json, started_at
             ) VALUES (?1, ?2, ?3, ?4, 'recording', ?5, ?6)",
            params![
                run_id,
                meeting_id,
                sequence_number,
                generation,
                sources_json,
                started_at
            ],
        )?;
        Ok(())
    }

    pub fn finish_recording_run(&self, run_id: &str, ended_at: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE recording_runs SET status = 'ready', ended_at = ?2 WHERE id = ?1",
            params![run_id, ended_at],
        )?;
        Ok(())
    }

    pub fn upsert_recording_asset(
        &self,
        meeting_id: &str,
        relative_path: &str,
        status: &str,
        duration_ms: i64,
        byte_size: i64,
        created_at: &str,
    ) -> Result<()> {
        let asset_id = format!("{meeting_id}-recording");
        self.connection.execute(
            "INSERT INTO recording_assets (
                id, meeting_id, relative_path, format, status, duration_ms, byte_size, created_at
             ) VALUES (?1, ?2, ?3, 'ogg_opus', ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                duration_ms = excluded.duration_ms,
                byte_size = excluded.byte_size",
            params![
                asset_id,
                meeting_id,
                relative_path,
                status,
                duration_ms,
                byte_size,
                created_at
            ],
        )?;
        Ok(())
    }

    pub fn mark_meeting_stopped(
        &self,
        meeting_id: &str,
        status: &str,
        transcription_status: &str,
        minutes_status: &str,
        stopped_at: &str,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records SET
                status = ?2,
                transcription_status = ?3,
                minutes_status = ?4,
                stopped_at = ?5,
                updated_at = ?5
             WHERE id = ?1",
            params![
                meeting_id,
                status,
                transcription_status,
                minutes_status,
                stopped_at
            ],
        )?;
        Ok(())
    }

    pub fn soft_delete_meeting(&self, meeting_id: &str, deleted_at: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![meeting_id, deleted_at],
        )?;
        Ok(())
    }

    pub fn restore_meeting(&self, meeting_id: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE meeting_records SET deleted_at = NULL WHERE id = ?1",
            params![meeting_id],
        )?;
        Ok(())
    }

    pub fn permanently_delete_meeting(&self, meeting_id: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM meeting_records WHERE id = ?1 AND deleted_at IS NOT NULL",
            params![meeting_id],
        )?;
        Ok(())
    }

    pub fn next_transcript_revision(&self, meeting_id: &str) -> Result<i64> {
        let revision = self.connection.query_row(
            "SELECT COALESCE(MAX(revision), 0) + 1 FROM transcript_segments WHERE meeting_id = ?1",
            params![meeting_id],
            |row| row.get(0),
        )?;
        Ok(revision)
    }

    pub fn latest_transcript_revision(&self, meeting_id: &str) -> Result<i64> {
        self.connection
            .query_row(
                "SELECT COALESCE(MAX(revision), 0) FROM transcript_segments WHERE meeting_id = ?1",
                params![meeting_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn append_transcript_segment(
        &self,
        segment_id: &str,
        meeting_id: &str,
        revision: i64,
        start_ms: i64,
        end_ms: i64,
        text: &str,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO transcript_segments (
                id, meeting_id, revision, start_ms, end_ms, text, status, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'final', CURRENT_TIMESTAMP)",
            params![segment_id, meeting_id, revision, start_ms, end_ms, text],
        )?;
        Ok(())
    }

    pub fn transcript_for_revision(&self, meeting_id: &str, revision: i64) -> Result<String> {
        let mut statement = self.connection.prepare(
            "SELECT text FROM transcript_segments
             WHERE meeting_id = ?1 AND revision = ?2 ORDER BY start_ms, id",
        )?;
        let rows =
            statement.query_map(params![meeting_id, revision], |row| row.get::<_, String>(0))?;
        let parts = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(parts.join("\n"))
    }

    pub fn save_minutes(
        &self,
        minutes_id: &str,
        meeting_id: &str,
        revision: i64,
        content: &str,
        provider_label: &str,
    ) -> Result<()> {
        let current = self.connection.query_row(
            "SELECT COALESCE(MAX(revision), 0) FROM meeting_minutes WHERE meeting_id = ?1",
            params![meeting_id],
            |row| row.get::<_, i64>(0),
        )?;
        if current >= revision {
            return Err(super::PersistenceError::StaleRevision {
                current,
                attempted: revision,
            });
        }

        self.connection.execute(
            "INSERT INTO meeting_minutes (
                id, meeting_id, revision, content, provider_label, status, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'ready', CURRENT_TIMESTAMP)",
            params![minutes_id, meeting_id, revision, content, provider_label],
        )?;
        Ok(())
    }

    pub fn latest_minutes(&self, meeting_id: &str) -> Result<Option<MeetingMinutesRow>> {
        self.connection
            .query_row(
                "SELECT id, meeting_id, revision, content, provider_label, status
                 FROM meeting_minutes WHERE meeting_id = ?1
                 ORDER BY revision DESC LIMIT 1",
                params![meeting_id],
                |row| {
                    Ok(MeetingMinutesRow {
                        id: row.get(0)?,
                        meeting_id: row.get(1)?,
                        revision: row.get(2)?,
                        content: row.get(3)?,
                        provider_label: row.get(4)?,
                        status: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn enqueue_processing_job(&self, job: &NewProcessingJob) -> Result<()> {
        self.connection.execute(
            "INSERT INTO processing_jobs (
                id, meeting_id, job_type, status, attempts, input_revision, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'queued', 0, ?4, ?5, ?5)",
            params![
                job.id,
                job.meeting_id,
                job.job_type,
                job.input_revision,
                job.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn claim_next_processing_job(
        &self,
        job_type: &str,
        updated_at: &str,
    ) -> Result<Option<ProcessingJobRow>> {
        self.connection
            .query_row(
                "UPDATE processing_jobs
                 SET status = 'running', attempts = attempts + 1, updated_at = ?2
                 WHERE id = (
                    SELECT id FROM processing_jobs
                    WHERE job_type = ?1 AND status = 'queued'
                    ORDER BY created_at, id LIMIT 1
                 )
                 RETURNING id, meeting_id, job_type, status, attempts, input_revision, error_summary",
                params![job_type, updated_at],
                |row| {
                    Ok(ProcessingJobRow {
                        id: row.get(0)?,
                        meeting_id: row.get(1)?,
                        job_type: row.get(2)?,
                        status: row.get(3)?,
                        attempts: row.get(4)?,
                        input_revision: row.get(5)?,
                        error_summary: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

fn map_meeting_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingRecordRow> {
    Ok(MeetingRecordRow {
        id: row.get(0)?,
        title: row.get(1)?,
        status: row.get(2)?,
        transcription_status: row.get(3)?,
        minutes_status: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        stopped_at: row.get(7)?,
        deleted_at: row.get(8)?,
    })
}

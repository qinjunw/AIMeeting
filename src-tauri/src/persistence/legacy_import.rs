use std::path::Path;

use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::{migrations, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMeetingImport {
    pub source_id: String,
    pub title: String,
    pub transcript: String,
    pub minutes: String,
    pub created_at: String,
    pub updated_at: String,
    pub stopped_at: Option<String>,
}

pub fn import_legacy_meetings(
    database_path: impl AsRef<Path>,
    meetings: &[LegacyMeetingImport],
) -> Result<usize> {
    let mut connection = Connection::open(database_path)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    migrations::migrate(&connection)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut imported = 0;

    for meeting in meetings {
        let Some(meeting_id) = legacy_meeting_id(&meeting.source_id) else {
            continue;
        };
        let created_at = fallback_timestamp(&meeting.created_at);
        let updated_at = if meeting.updated_at.trim().is_empty() {
            created_at
        } else {
            meeting.updated_at.trim()
        };
        let stopped_at = meeting
            .stopped_at
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(updated_at);
        let transcript = meeting.transcript.trim();
        let minutes = meeting.minutes.trim();
        let title = if meeting.title.trim().is_empty() {
            "旧版会议记录"
        } else {
            meeting.title.trim()
        };
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO meeting_records (
                id, title, status, transcription_status, minutes_status,
                created_at, updated_at, stopped_at
             ) VALUES (?1, ?2, 'ready', 'ready', ?3, ?4, ?5, ?6)",
            params![
                meeting_id,
                title,
                if minutes.is_empty() {
                    "pending"
                } else {
                    "ready"
                },
                created_at,
                updated_at,
                stopped_at,
            ],
        )?;
        if inserted == 0 {
            continue;
        }
        imported += 1;

        if !transcript.is_empty() {
            transaction.execute(
                "INSERT INTO transcript_segments (
                    id, meeting_id, recording_run_id, revision, start_ms, end_ms,
                    text, confidence, status, created_at
                 ) VALUES (?1, ?2, NULL, 1, 0, 0, ?3, NULL, 'final', ?4)",
                params![
                    format!("{meeting_id}_transcript"),
                    meeting_id,
                    transcript,
                    created_at
                ],
            )?;
        }
        if !minutes.is_empty() {
            transaction.execute(
                "INSERT INTO meeting_minutes (
                    id, meeting_id, revision, content, provider_label, status, created_at
                 ) VALUES (?1, ?2, ?3, ?4, 'legacy-import', 'ready', ?5)",
                params![
                    format!("{meeting_id}_minutes"),
                    meeting_id,
                    if transcript.is_empty() { 0 } else { 1 },
                    minutes,
                    updated_at,
                ],
            )?;
        }
    }

    transaction.commit()?;
    Ok(imported)
}

fn legacy_meeting_id(source_id: &str) -> Option<String> {
    let normalized = source_id
        .trim()
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
        .take(120)
        .collect::<String>();
    (!normalized.is_empty()).then(|| format!("legacy_{normalized}"))
}

fn fallback_timestamp(value: &str) -> &str {
    if value.trim().is_empty() {
        "1970-01-01T00:00:00Z"
    } else {
        value.trim()
    }
}

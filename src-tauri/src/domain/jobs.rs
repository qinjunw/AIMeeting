use serde::{Deserialize, Serialize};

use super::{MinutesStatus, TranscriptionStatus};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionUpdate {
    pub meeting_id: String,
    pub run_generation: u64,
    pub revision: u64,
    pub status: TranscriptionStatus,
}

impl TranscriptionUpdate {
    pub fn new(
        meeting_id: impl Into<String>,
        run_generation: u64,
        revision: u64,
        status: TranscriptionStatus,
    ) -> Self {
        Self {
            meeting_id: meeting_id.into(),
            run_generation,
            revision,
            status,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionJob {
    pub meeting_id: String,
    pub run_generation: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesJob {
    pub meeting_id: String,
    pub transcript_revision: u64,
    pub status: MinutesStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateDisposition {
    Applied,
    IgnoredStaleGeneration,
    IgnoredStaleRevision,
}

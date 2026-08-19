mod database;
mod files;
mod legacy_import;
mod migrations;
pub mod recovery;
pub mod secrets;

pub use database::{
    MeetingMinutesRow, MeetingRecordRow, MeetingRepository, NewMeetingRecord, NewProcessingJob,
    ProcessingJobRow, RecordingAssetRow,
};
pub use files::{migrate_legacy_data_root, DataPaths, DataRootMigration};
pub use legacy_import::{import_legacy_meetings, LegacyMeetingImport};

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid meeting identifier: {0}")]
    InvalidMeetingId(String),
    #[error("meeting data already exists at {0}")]
    DestinationExists(String),
    #[error("stale revision {attempted}; current revision is {current}")]
    StaleRevision { current: i64, attempted: i64 },
}

pub type Result<T> = std::result::Result<T, PersistenceError>;

use std::path::{Path, PathBuf};

use super::{PersistenceError, Result};

#[derive(Debug, Clone)]
pub struct DataPaths {
    root: PathBuf,
    meetings: PathBuf,
    trash: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let meetings = root.join("meetings");
        let trash = root.join("trash");
        std::fs::create_dir_all(&meetings)?;
        std::fs::create_dir_all(&trash)?;
        Ok(Self {
            root,
            meetings,
            trash,
        })
    }

    pub fn database_path(&self) -> PathBuf {
        self.root.join("aimeeting.db")
    }

    pub fn meeting_dir(&self, meeting_id: &str) -> Result<PathBuf> {
        validate_meeting_id(meeting_id)?;
        Ok(self.meetings.join(meeting_id))
    }

    pub fn trash_dir(&self, meeting_id: &str) -> Result<PathBuf> {
        validate_meeting_id(meeting_id)?;
        Ok(self.trash.join(meeting_id))
    }

    pub fn move_to_trash(&self, meeting_id: &str) -> Result<()> {
        let source = self.meeting_dir(meeting_id)?;
        let destination = self.trash_dir(meeting_id)?;
        ensure_destination_missing(&destination)?;
        std::fs::rename(source, destination)?;
        Ok(())
    }

    pub fn restore_from_trash(&self, meeting_id: &str) -> Result<()> {
        let source = self.trash_dir(meeting_id)?;
        let destination = self.meeting_dir(meeting_id)?;
        ensure_destination_missing(&destination)?;
        std::fs::rename(source, destination)?;
        Ok(())
    }
}

fn validate_meeting_id(meeting_id: &str) -> Result<()> {
    let valid = !meeting_id.is_empty()
        && meeting_id
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(PersistenceError::InvalidMeetingId(meeting_id.to_string()))
    }
}

fn ensure_destination_missing(path: &Path) -> Result<()> {
    if path.exists() {
        return Err(PersistenceError::DestinationExists(
            path.display().to_string(),
        ));
    }
    Ok(())
}

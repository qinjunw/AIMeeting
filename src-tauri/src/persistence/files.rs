use std::path::{Path, PathBuf};

use super::{PersistenceError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataRootMigration {
    SameLocation,
    NoLegacyData,
    TargetAlreadyExists,
    CopiedLegacyData,
}

pub fn migrate_legacy_data_root(
    legacy_root: impl AsRef<Path>,
    target_root: impl AsRef<Path>,
) -> Result<DataRootMigration> {
    let legacy_root = legacy_root.as_ref();
    let target_root = target_root.as_ref();
    if legacy_root == target_root {
        return Ok(DataRootMigration::SameLocation);
    }
    if !legacy_root.exists() {
        return Ok(DataRootMigration::NoLegacyData);
    }
    if !legacy_root.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "legacy application data root is not a directory",
        )
        .into());
    }
    if target_root.exists() {
        if !target_root.is_dir() || std::fs::read_dir(target_root)?.next().is_some() {
            return Ok(DataRootMigration::TargetAlreadyExists);
        }
        std::fs::remove_dir(target_root)?;
    }

    let parent = target_root.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "local application data root has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let target_name = target_root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "local application data root has no valid directory name",
            )
        })?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging = parent.join(format!(
        ".{target_name}.migration-{}-{nonce}",
        std::process::id()
    ));
    if staging.exists() {
        return Err(PersistenceError::DestinationExists(
            staging.display().to_string(),
        ));
    }

    if let Err(error) = copy_directory_tree(legacy_root, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    std::fs::rename(staging, target_root)?;
    Ok(DataRootMigration::CopiedLegacyData)
}

fn copy_directory_tree(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "legacy application data contains an unsupported symbolic link: {}",
                    entry.path().display()
                ),
            )
            .into());
        }
        let destination_path = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_directory_tree(&entry.path(), &destination_path)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), &destination_path)?;
        }
    }
    Ok(())
}

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

    pub fn permanently_delete_from_trash(&self, meeting_id: &str) -> Result<()> {
        let path = self.trash_dir(meeting_id)?;
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
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

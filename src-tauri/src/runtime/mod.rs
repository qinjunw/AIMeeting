pub mod registry;

use std::path::Path;
use std::sync::Mutex;

use crate::persistence::{DataPaths, MeetingRepository, Result};

pub struct DesktopState {
    pub paths: DataPaths,
    pub repository: Mutex<MeetingRepository>,
    pub recordings: Mutex<registry::RecordingRegistry>,
}

impl DesktopState {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let paths = DataPaths::new(root)?;
        let repository = MeetingRepository::open(paths.database_path())?;
        let recovered = repository.recover_incomplete_meetings(&chrono::Utc::now().to_rfc3339())?;
        for meeting_id in recovered {
            let recording = paths.meeting_dir(&meeting_id)?.join("recording.opus");
            if recording.exists() {
                let _ = crate::audio::ogg_opus::recover_truncated_file(&recording);
            }
        }
        Ok(Self {
            paths,
            repository: Mutex::new(repository),
            recordings: Mutex::new(registry::RecordingRegistry::default()),
        })
    }
}

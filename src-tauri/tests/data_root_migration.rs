use std::fs;

use aimeeting_lib::persistence::{migrate_legacy_data_root, DataRootMigration};
use tempfile::tempdir;

#[test]
fn roaming_data_is_copied_to_local_root_without_deleting_the_source() {
    let root = tempdir().expect("temporary root");
    let legacy = root.path().join("Roaming").join("com.aimeeting.app");
    let local = root.path().join("Local").join("com.aimeeting.app");
    fs::create_dir_all(legacy.join("meetings").join("meeting-1")).expect("legacy meeting");
    fs::write(legacy.join("aimeeting.db"), b"database").expect("legacy database");
    fs::write(
        legacy
            .join("meetings")
            .join("meeting-1")
            .join("recording.opus"),
        b"recording",
    )
    .expect("legacy recording");

    let result = migrate_legacy_data_root(&legacy, &local).expect("migration succeeds");

    assert_eq!(result, DataRootMigration::CopiedLegacyData);
    assert_eq!(fs::read(local.join("aimeeting.db")).unwrap(), b"database");
    assert_eq!(
        fs::read(
            local
                .join("meetings")
                .join("meeting-1")
                .join("recording.opus")
        )
        .unwrap(),
        b"recording"
    );
    assert!(legacy.join("aimeeting.db").exists());
    assert_eq!(
        migrate_legacy_data_root(&legacy, &local).unwrap(),
        DataRootMigration::TargetAlreadyExists
    );
}

#[test]
fn existing_local_data_is_never_overwritten_by_roaming_data() {
    let root = tempdir().expect("temporary root");
    let legacy = root.path().join("roaming");
    let local = root.path().join("local");
    fs::create_dir_all(&legacy).unwrap();
    fs::create_dir_all(&local).unwrap();
    fs::write(legacy.join("aimeeting.db"), b"legacy").unwrap();
    fs::write(local.join("aimeeting.db"), b"current").unwrap();

    assert_eq!(
        migrate_legacy_data_root(&legacy, &local).unwrap(),
        DataRootMigration::TargetAlreadyExists
    );
    assert_eq!(fs::read(local.join("aimeeting.db")).unwrap(), b"current");
}

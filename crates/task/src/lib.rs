//! Task layer: serializable job specs, the task runner, and the bridge from
//! VFS changesets to engine operations.

pub mod job;
#[cfg(windows)]
pub mod launch;
pub mod runner;

pub use job::{
    AddItem, FormatSpec, JobFile, JobFileError, JobSpec, JOB_FILE_VERSION, LevelSpec,
    OverwriteSpec,
};
pub use runner::{CancelFlag, TaskEvent, TaskRunner, changeset_to_ops};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn job_file_roundtrip() {
        let spec = JobSpec::Extract {
            archive: PathBuf::from("C:/a.7z"),
            items: vec![0, 1],
            target: PathBuf::from("C:/out"),
            overwrite: OverwriteSpec::Overwrite,
            password_hint: true,
        };
        let file = JobFile::new("job-1", spec);
        let json = file.to_json().unwrap();
        // Passwords must never appear in the serialized form (the hint flag
        // is fine; a literal `"password"` key is not).
        assert!(!json.contains("\"password\""), "json: {json}");
        let parsed = JobFile::from_json(&json).unwrap();
        assert_eq!(parsed, file);
        assert_eq!(parsed.version, JOB_FILE_VERSION);
    }

    #[test]
    fn unsupported_version_rejected() {
        let json = r#"{"version": 99, "id": "x", "spec": {"op": "test", "archive": "a.7z", "password_hint": false}}"#;
        let err = JobFile::from_json(json).unwrap_err();
        assert!(matches!(err, JobFileError::UnsupportedVersion(99)));
    }

    #[test]
    fn changeset_maps_to_ops() {
        let mut cs = vfs::Changeset::new();
        cs.additions.push(vfs::AddOp {
            fs_path: PathBuf::from("C:/new.txt"),
            archive_path: "new.txt".into(),
        });
        cs.deletions.push(vfs::DeleteOp { archive_index: 3 });
        let ops = changeset_to_ops(&cs);
        assert_eq!(ops.len(), 2);
        assert!(matches!(&ops[0], bit7z_rs::EngineOp::Add { archive_path, .. } if archive_path == "new.txt"));
        assert!(matches!(&ops[1], bit7z_rs::EngineOp::Delete { archive_index: 3 }));
    }
}

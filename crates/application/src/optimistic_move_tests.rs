use super::*;
use std::{
    fs,
    sync::{Condvar, Mutex},
};

struct GatedFilesystem {
    gate: Arc<(Mutex<bool>, Condvar)>,
}
impl WorkspaceFilesystem for GatedFilesystem {
    fn open_workspace(
        &self,
        path: &Path,
    ) -> Result<(Workspace, Vec<DocumentId>, Vec<String>), FilesystemError> {
        RealFilesystem.open_workspace(path)
    }
    fn read_document(&self, root: &Path, id: &DocumentId) -> Result<String, FilesystemError> {
        RealFilesystem.read_document(root, id)
    }
    fn write_document(
        &self,
        root: &Path,
        id: &DocumentId,
        content: &str,
    ) -> Result<(), FilesystemError> {
        RealFilesystem.write_document(root, id, content)
    }
    fn rename_entry(
        &self,
        root: &Path,
        id: &DocumentId,
        target: &DocumentId,
    ) -> Result<(), FilesystemError> {
        let (lock, condition) = &*self.gate;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = condition.wait(released).unwrap();
        }
        RealFilesystem.rename_entry(root, id, target)
    }
}

#[test]
fn optimistic_folder_move_is_immediate_and_rollback_preserves_edits_and_queued_save() {
    for reject in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("src/nested")).unwrap();
        fs::create_dir(directory.path().join("dest")).unwrap();
        fs::write(directory.path().join("src/nested/a.rs"), "original").unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let mut application =
            Application::with_filesystem(Box::new(GatedFilesystem { gate: gate.clone() }));
        let id = |path| DocumentId::parse(path).unwrap();
        application
            .dispatch(AppCommand::OpenWorkspace(directory.path().into()))
            .unwrap();
        application
            .dispatch(AppCommand::OpenDocument(id("src/nested/a.rs")))
            .unwrap();
        let started = Instant::now();
        let moved = application
            .dispatch(AppCommand::MoveEntryOptimistic {
                id: id("src"),
                directory: "dest".into(),
            })
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "Drop waited for filesystem confirmation"
        );
        assert_eq!(
            moved.active_document.unwrap().id,
            id("dest/src/nested/a.rs")
        );
        assert!(moved.directories.contains(&"dest/src/nested".into()));
        assert!(directory.path().join("src/nested/a.rs").exists());
        assert!(!directory.path().join("dest/src").exists());
        assert!(
            application
                .dispatch_if_changed(AppCommand::PollWorkspaceChanges)
                .unwrap()
                .is_none()
        );
        application
            .dispatch(AppCommand::ChangeDocument {
                id: id("dest/src/nested/a.rs"),
                content: "typed while moving".into(),
            })
            .unwrap();
        application
            .dispatch(AppCommand::SaveDocument(id("dest/src/nested/a.rs")))
            .unwrap();
        application
            .dispatch(AppCommand::OpenDocument(id("dest/src/nested/a.rs")))
            .unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("src/nested/a.rs")).unwrap(),
            "original"
        );
        if reject {
            fs::create_dir(directory.path().join("dest/src")).unwrap();
            fs::write(directory.path().join("dest/src/external.txt"), "external").unwrap();
        }
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let deadline = Instant::now() + Duration::from_secs(3);
        while application.pending_move.is_some() && Instant::now() < deadline {
            application
                .dispatch_if_changed(AppCommand::PollSearchResults)
                .unwrap();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(application.pending_move.is_none());
        let final_path = if reject {
            "src/nested/a.rs"
        } else {
            "dest/src/nested/a.rs"
        };
        let snapshot = application.snapshot();
        let active = snapshot.active_document.unwrap();
        assert_eq!(active.id, id(final_path));
        assert_eq!(active.content, "typed while moving");
        assert!(!active.dirty);
        assert_eq!(
            fs::read_to_string(directory.path().join(final_path)).unwrap(),
            "typed while moving"
        );
        assert_eq!(application.take_background_error().is_some(), reject);
        if reject {
            assert_eq!(
                fs::read_to_string(directory.path().join("dest/src/external.txt")).unwrap(),
                "external"
            );
        }
    }
}

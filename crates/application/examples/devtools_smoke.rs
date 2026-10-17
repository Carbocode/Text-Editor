//! Run against the separate diagnostic service; also succeeds if it is absent.
use native_workspace_application::{AppCommand, Session};
use std::{
    thread,
    time::{Duration, Instant},
};

fn main() {
    let root = std::env::args_os().nth(1).expect("workspace directory");
    let mut session = Session::new();
    session.enable_devtools("core-smoke");
    let observer = session.diagnostic_observer();
    session
        .dispatch(AppCommand::OpenWorkspace(root.into()))
        .unwrap();
    session
        .dispatch(AppCommand::QueueSearchQuery(
            "superseded_private_query".into(),
        ))
        .unwrap();
    thread::sleep(Duration::from_millis(20));
    session
        .dispatch(AppCommand::QueueSearchQuery("pub".into()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut applied = session.revision();
    while session.snapshot().search_in_progress {
        session.poll();
        if session.revision() != applied {
            let snapshot = session.snapshot();
            applied = snapshot.revision;
            observer.snapshot_applied(snapshot.diagnostic_operation, snapshot.revision, 1000);
        }
        assert!(Instant::now() < deadline, "search completion deadline");
        thread::sleep(Duration::from_millis(20));
    }
    println!(
        "search completed: {} matches",
        session.snapshot().search_results.len()
    );
    let old_operation = session.snapshot().diagnostic_operation;
    let workspace = session.snapshot().workspace_root().unwrap().to_path_buf();
    session
        .dispatch(AppCommand::OpenWorkspace(workspace))
        .unwrap();
    if old_operation != 0 {
        assert_ne!(
            session.snapshot().diagnostic_operation,
            old_operation,
            "new workspace must not inherit an old search operation"
        );
    }
    drop(session);
    thread::sleep(Duration::from_millis(500));
}

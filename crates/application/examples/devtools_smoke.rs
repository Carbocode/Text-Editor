//! Run against the separate diagnostic service; also succeeds if it is absent.
use native_workspace_application::{AppCommand, Application};
use std::{
    thread,
    time::{Duration, Instant},
};
fn main() {
    let root = std::env::args_os().nth(1).expect("workspace directory");
    let mut app = Application::new();
    app.enable_devtools("core-smoke");
    app.dispatch(AppCommand::OpenWorkspace(root.into()))
        .unwrap();
    app.dispatch(AppCommand::QueueSearchQuery(
        "superseded_private_query".into(),
    ))
    .unwrap();
    thread::sleep(Duration::from_millis(20));
    app.dispatch(AppCommand::QueueSearchQuery("pub".into()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.snapshot().search_in_progress {
        if let Some(snapshot) = app
            .dispatch_if_changed(AppCommand::PollSearchResults)
            .unwrap()
        {
            app.diagnostic_snapshot_applied(snapshot.diagnostic_operation, snapshot.revision, 1000);
        }
        assert!(Instant::now() < deadline, "search completion deadline");
        thread::sleep(Duration::from_millis(20));
    }
    println!(
        "search completed: {} matches",
        app.snapshot().search_results.len()
    );
    let old_operation = app.snapshot().diagnostic_operation;
    let workspace = app.snapshot().workspace_root.unwrap();
    let reopened = app.dispatch(AppCommand::OpenWorkspace(workspace)).unwrap();
    if old_operation != 0 {
        assert_ne!(
            reopened.diagnostic_operation, old_operation,
            "new workspace must not inherit an old search operation"
        );
    }
    drop(app);
    thread::sleep(Duration::from_millis(500));
}

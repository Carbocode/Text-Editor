//! Run with `cargo run -p native-workspace-application --release --example search_benchmark -- [workspace] [query]`.

use native_workspace_application::{AppCommand, Application};
use std::{
    env, fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

struct TemporaryWorkspace(PathBuf);

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn synthetic_workspace() -> TemporaryWorkspace {
    let root = env::temp_dir().join(format!("native-workspace-benchmark-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create benchmark root");
    for directory in 0..20 {
        for subdirectory in 0..5 {
            let folder = root.join(format!("section-{directory:02}/chapter-{subdirectory:02}"));
            fs::create_dir_all(&folder).expect("create benchmark directory");
            for file in 0..10 {
                let content = format!(
                    "{}\n{}\n",
                    "ordinary text line".repeat(64),
                    if file % 10 == 0 {
                        "benchmark-needle"
                    } else {
                        "ordinary line"
                    }
                );
                fs::write(folder.join(format!("document-{file:02}.txt")), content)
                    .expect("write benchmark file");
            }
        }
    }
    TemporaryWorkspace(root)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let generated = (args.len() < 2).then(synthetic_workspace);
    let root = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| generated.as_ref().unwrap().0.clone());
    let query = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("benchmark-needle");
    let mut app = Application::new();
    let started = Instant::now();
    let opened = app
        .dispatch(AppCommand::OpenWorkspace(root))
        .expect("open workspace");
    let open_time = started.elapsed();
    let started = Instant::now();
    let queued = app
        .dispatch(AppCommand::QueueSearchQuery(query.to_owned()))
        .expect("queue search");
    let queue_time = started.elapsed();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut first_result = None;
    let completed = loop {
        let snapshot = app
            .dispatch(AppCommand::PollSearchResults)
            .expect("poll search");
        if first_result.is_none() && !snapshot.search_results.is_empty() {
            first_result = Some(started.elapsed());
        }
        if !snapshot.search_in_progress {
            break snapshot;
        }
        assert!(Instant::now() < deadline, "search exceeded 60 seconds");
        thread::sleep(Duration::from_millis(5));
    };
    println!(
        "files={} directories={} query={query:?}",
        opened.documents.len(),
        opened.directories.len()
    );
    println!(
        "open={open_time:?} queue={queue_time:?} first_result={first_result:?} search={:?} results={} truncated={} immediate_results={}",
        started.elapsed(),
        completed.search_results.len(),
        completed.search_truncated,
        queued.search_results.len()
    );
}

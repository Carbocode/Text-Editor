use native_workspace_devtools_protocol::{Ack, Descriptor, SessionSummary, synthetic};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Collector(Child);
impl Drop for Collector {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn launch(dir: &std::path::Path) -> Collector {
    Collector(
        Command::new(env!("CARGO_BIN_EXE_native-workspace-devtools"))
            .env("NATIVE_WORKSPACE_DIAGNOSTICS_DIR", dir)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}
fn ready(dir: &std::path::Path, client: &reqwest::blocking::Client) -> Descriptor {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(d) = Descriptor::read(&dir.join("service.json"))
            && client
                .get(format!("{}/v1/health", d.base_url()))
                .bearer_auth(&d.token)
                .send()
                .is_ok_and(|r| r.status().is_success())
        {
            return d;
        }
        assert!(Instant::now() < deadline, "collector did not become ready");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn acknowledged_batch_survives_forced_process_termination() {
    let dir = tempfile::tempdir().unwrap();
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let mut collector = launch(dir.path());
    let d = ready(dir.path(), &client);
    let batch = synthetic();
    let ack: Ack = client
        .post(format!("{}/v1/batches", d.base_url()))
        .bearer_auth(&d.token)
        .json(&batch)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(ack.committed_sequence.0, 14);
    collector.0.kill().unwrap();
    collector.0.wait().unwrap();
    drop(collector);
    let _collector = launch(dir.path());
    let d = ready(dir.path(), &client);
    let sessions: Vec<SessionSummary> = client
        .get(format!("{}/v1/sessions", d.base_url()))
        .bearer_auth(&d.token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, "interrupted");
    assert_eq!(sessions[0].records.0, 14);
    assert_eq!(sessions[0].committed_sequence.0, 14);
}

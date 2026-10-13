use native_workspace_devtools::{server::Service, store::Store};
use native_workspace_devtools_protocol::{Ack, Descriptor, Page, Uuid, VERSION, synthetic};
#[tokio::test]
async fn authenticates_commits_and_serves_persisted_pages() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("db")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let descriptor = Descriptor {
        version: VERSION,
        service: Uuid::new_v4(),
        port: listener.local_addr().unwrap().port(),
        token: "a".repeat(64),
    };
    let base = descriptor.base_url();
    let service = Service::new(store, descriptor.clone()).unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, service.router()).await.unwrap();
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("{base}/v1/health"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/health"))
            .bearer_auth(&descriptor.token)
            .header("Origin", "http://evil.test")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/health"))
            .bearer_auth(&descriptor.token)
            .header("Host", "evil.test")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let batch = synthetic();
    let ack: Ack = client
        .post(format!("{base}/v1/batches"))
        .bearer_auth(&descriptor.token)
        .json(&batch)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ack.committed_sequence.0, 14);
    let page: Page = client
        .get(format!(
            "{base}/v1/sessions/{}/events?after=0&limit=3",
            batch.session.id
        ))
        .bearer_auth(&descriptor.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page.records.len(), 3);
    assert_eq!(page.next.unwrap().0, 3);
    task.abort();
}

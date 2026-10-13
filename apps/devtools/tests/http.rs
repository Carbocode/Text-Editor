use native_workspace_devtools::{server::Service, store::Store};
use native_workspace_devtools_protocol::{
    Ack, Descriptor, Health, Page, Pin, SessionSummary, Uuid, VERSION, synthetic,
};
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
    let operation: Page = client
        .get(format!(
            "{base}/v1/sessions/{}/events?operation=2&after=0&limit=2&internal=false",
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
    assert_eq!(operation.records.len(), 2);
    assert!(
        operation
            .records
            .iter()
            .all(|record| record.operation.0 == 2)
    );
    assert_eq!(operation.next.unwrap().0, 5);
    let tail: Page = client
        .get(format!(
            "{base}/v1/sessions/{}/events?operation=1&tail=true&limit=1",
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
    assert_eq!(tail.records[0].sequence.0, 3);
    assert!(tail.next.is_none());
    assert_eq!(
        client
            .get(format!(
                "{base}/v1/sessions/{}/events?operation=18446744073709551616",
                batch.session.id
            ))
            .bearer_auth(&descriptor.token)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let health: Health = client
        .get(format!("{base}/v1/health"))
        .bearer_auth(&descriptor.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        health
            .capabilities
            .iter()
            .any(|cap| cap == native_workspace_devtools_protocol::OPERATION_QUERY)
    );
    assert!(
        health
            .capabilities
            .iter()
            .any(|cap| cap == native_workspace_devtools_protocol::SESSION_PIN)
    );
    client
        .post(format!("{base}/v1/sessions/{}/pin", batch.session.id))
        .bearer_auth(&descriptor.token)
        .json(&Pin { pinned: true })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let sessions: Vec<SessionSummary> = client
        .get(format!("{base}/v1/sessions"))
        .bearer_auth(&descriptor.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        sessions
            .iter()
            .find(|session| session.session.id == batch.session.id)
            .unwrap()
            .pinned
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/sessions/{}/pin", batch.session.id))
            .json(&Pin { pinned: false })
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let header: SessionSummary = client
        .get(format!("{base}/v1/sessions/{}", batch.session.id))
        .bearer_auth(&descriptor.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(header.session.id, batch.session.id);
    assert!(header.pinned);
    assert_eq!(
        client
            .get(format!("{base}/v1/sessions/{}", Uuid::new_v4()))
            .bearer_auth(&descriptor.token)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let missing_pin = client
        .post(format!("{base}/v1/sessions/{}/pin", Uuid::new_v4()))
        .bearer_auth(&descriptor.token)
        .json(&Pin { pinned: true })
        .send()
        .await
        .unwrap();
    assert_eq!(missing_pin.status(), 404);
    assert_eq!(
        missing_pin.json::<serde_json::Value>().await.unwrap()["code"],
        "session_not_found"
    );
    task.abort();
}

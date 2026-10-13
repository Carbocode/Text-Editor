use crate::store::{Result, Store};
use axum::{
    Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        ws::{Message, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use native_workspace_devtools_protocol::{
    Ack, Batch, Control, Counter, Descriptor, Health, MAX_ARCHIVE_BYTES, MAX_BATCH_BYTES, Page,
    SessionSummary, Uuid, VERSION,
};
use serde::Deserialize;
use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, oneshot};

#[derive(Clone)]
pub struct Service {
    descriptor: Descriptor,
    write: mpsc::SyncSender<Write>,
    read: mpsc::SyncSender<Read>,
    live: broadcast::Sender<String>,
    bulk: Arc<Semaphore>,
    resources: Arc<crate::resources::ResourceState>,
    archive: mpsc::SyncSender<Archive>,
}
enum Write {
    Ingest(Batch, oneshot::Sender<Result<Ack>>),
    Samples(Vec<native_workspace_devtools_protocol::ResourceSample>),
    Control(Uuid, bool, oneshot::Sender<Result<()>>),
    Pin(Uuid, bool, oneshot::Sender<Result<()>>),
    Import(
        Vec<u8>,
        Arc<OwnedSemaphorePermit>,
        oneshot::Sender<Result<Uuid>>,
    ),
}
enum Archive {
    Export(
        Uuid,
        Arc<OwnedSemaphorePermit>,
        oneshot::Sender<Result<Vec<u8>>>,
    ),
}
enum Read {
    Sessions(oneshot::Sender<Result<Vec<SessionSummary>>>),
    Session(Uuid, oneshot::Sender<Result<Option<SessionSummary>>>),
    Page(
        Uuid,
        u64,
        usize,
        bool,
        Option<u64>,
        oneshot::Sender<Result<Page>>,
    ),
    Tail(
        Uuid,
        usize,
        bool,
        Option<u64>,
        oneshot::Sender<Result<Page>>,
    ),
    Resources(
        Uuid,
        u64,
        oneshot::Sender<Result<Vec<native_workspace_devtools_protocol::ResourceSample>>>,
    ),
    Analysis(
        Uuid,
        oneshot::Sender<Result<native_workspace_devtools_protocol::Analysis>>,
    ),
    Control(Uuid, oneshot::Sender<Result<bool>>),
}
impl Service {
    pub fn new(mut store: Store, descriptor: Descriptor) -> Result<Self> {
        let resources = Arc::new(crate::resources::ResourceState::default());
        let writer_resources = resources.clone();
        let reader = store.reader()?;
        let mut archive_reader = store.reader()?;
        let (archive, arx) = mpsc::sync_channel(1);
        let (write, wrx) = mpsc::sync_channel(16);
        let (read, rrx) = mpsc::sync_channel(16);
        let (live, _) = broadcast::channel(32);
        let notifications = live.clone();
        thread::Builder::new()
            .name("devtools-writer".into())
            .spawn(move || {
                while let Ok(job) = wrx.recv() {
                    match job {
                        Write::Ingest(batch, reply) => {
                            writer_resources.queue.fetch_sub(1,std::sync::atomic::Ordering::Relaxed);
                            let result = store.ingest(&batch);
                            if result.is_ok() {
                                writer_resources.register_editor(&batch.session,result.as_ref().is_ok_and(|ack|ack.recording));
                                if result.as_ref().is_ok_and(|ack|!ack.deduplicated) { writer_resources.committed.fetch_add(batch.records.len() as u64,std::sync::atomic::Ordering::Relaxed); }
                                writer_resources.last_commit.store(native_workspace_devtools_protocol::unix_ms(),std::sync::atomic::Ordering::Relaxed);
                            } else {writer_resources.rejected.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
                            if result.is_ok() {
                                let _ = notifications.send(serde_json::json!({"session":batch.session.id,"committed":batch.records.last().map(|r|r.sequence),"reload":true}).to_string());
                            }
                            let _ = reply.send(result);
                        }
                        Write::Samples(samples) => {
                            if store.persist_samples(&samples).is_err() {writer_resources.gaps.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
                        }
                        Write::Control(id, value, reply) => {
                            let result = store.set_control(id, value);
                            if result.is_ok() { writer_resources.set_recording(id,value); let _ = notifications.send(serde_json::json!({"session":id,"recording":value,"control":true}).to_string()); }
                            let _ = reply.send(result);
                        }
                        Write::Pin(id, pinned, reply) => {
                            let result = store.set_pin(id, pinned);
                            if result.is_ok() {
                                let _ = notifications.send(serde_json::json!({"session":id,"reload":true}).to_string());
                            }
                            let _ = reply.send(result);
                        }
                        Write::Import(bytes, _budget, reply) => {
                            let _ = reply.send(store.import(&bytes));
                        }
                    }
                }
            })?;
        thread::Builder::new()
            .name("devtools-reader".into())
            .spawn(move || {
                while let Ok(job) = rrx.recv() {
                    match job {
                        Read::Sessions(reply) => {
                            let _ = reply.send(reader.sessions());
                        }
                        Read::Session(id, reply) => {
                            let _ = reply.send(reader.session(id));
                        }
                        Read::Page(id, after, limit, internal, operation, reply) => {
                            let _ = reply
                                .send(reader.query_page(id, after, limit, internal, operation));
                        }
                        Read::Tail(id, limit, internal, operation, reply) => {
                            let _ = reply.send(reader.query_tail(id, limit, internal, operation));
                        }
                        Read::Resources(id, after, reply) => {
                            let _ = reply.send(reader.resource_samples(id, after));
                        }
                        Read::Analysis(id, reply) => {
                            let _ = reply.send(reader.analysis(id));
                        }
                        Read::Control(id, reply) => {
                            let _ = reply.send(reader.control(id));
                        }
                    }
                }
            })?;
        thread::Builder::new()
            .name("devtools-archive".into())
            .spawn(move || {
                while let Ok(Archive::Export(id, _budget, reply)) = arx.recv() {
                    let _ = reply.send(archive_reader.export(id));
                }
            })?;
        Ok(Self {
            descriptor,
            write,
            read,
            live,
            bulk: Arc::new(Semaphore::new(1)),
            resources,
            archive,
        })
    }
    pub fn start_resources(&self, path: std::path::PathBuf) -> Result<()> {
        let writer = self.write.clone();
        let live = self.live.clone();
        self.resources.clone().start(path, move |samples| {
            let result = writer.try_send(Write::Samples(samples)).is_ok();
            let _ = live.send("{\"resources\":true}".into());
            result
        })?;
        Ok(())
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/v1/health", get(health))
            .route(
                "/v1/batches",
                post(ingest).layer(DefaultBodyLimit::max(MAX_BATCH_BYTES)),
            )
            .route("/v1/sessions", get(sessions))
            .route("/v1/console", post(console_hello))
            .route("/v1/console/focus", post(console_focus))
            .route("/v1/sessions/{id}", get(session))
            .route("/v1/sessions/{id}/resources", get(resources))
            .route("/v1/sessions/{id}/analysis", get(analysis))
            .route("/v1/sessions/{id}/events", get(page))
            .route("/v1/sessions/{id}/control", get(control).post(set_control))
            .route("/v1/sessions/{id}/export", get(export))
            .route("/v1/sessions/{id}/pin", post(set_pin))
            .route(
                "/v1/import",
                post(import).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES as usize)),
            )
            .route("/v1/live", get(live))
            .layer(middleware::from_fn_with_state(self.clone(), authenticate))
            .with_state(self)
    }
}
async fn authenticate(
    State(service): State<Service>,
    headers: HeaderMap,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let token = format!("Bearer {}", service.descriptor.token);
    let host = format!("127.0.0.1:{}", service.descriptor.port);
    // This is a desktop protocol: reject all browser origins, including "null".
    if headers.contains_key("origin")
        || headers.get("host").and_then(|h| h.to_str().ok()) != Some(host.as_str())
        || headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(token.as_str())
    {
        return error(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    if request.uri().path() == "/v1/import" || request.uri().path().ends_with("/export") {
        let Ok(permit) = service.bulk.clone().try_acquire_owned() else {
            return busy().into_response();
        };
        request.extensions_mut().insert(Arc::new(permit));
    }
    next.run(request).await
}
fn error(status: StatusCode, code: &str) -> (StatusCode, axum::Json<serde_json::Value>) {
    (status, axum::Json(serde_json::json!({"code":code})))
}
type ApiResult<T> = std::result::Result<axum::Json<T>, (StatusCode, axum::Json<serde_json::Value>)>;
async fn wait<T>(
    rx: oneshot::Receiver<Result<T>>,
) -> std::result::Result<T, (StatusCode, axum::Json<serde_json::Value>)> {
    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(Ok(v))) => Ok(v),
        Ok(Ok(Err(_))) => Err(error(StatusCode::CONFLICT, "storage_rejected")),
        _ => Err(error(StatusCode::SERVICE_UNAVAILABLE, "service_busy")),
    }
}
fn busy() -> (StatusCode, axum::Json<serde_json::Value>) {
    error(StatusCode::SERVICE_UNAVAILABLE, "service_busy")
}
async fn health(State(s): State<Service>) -> axum::Json<Health> {
    axum::Json(Health {
        version: VERSION,
        service: s.descriptor.service,
        capabilities: vec![
            native_workspace_devtools_protocol::OPERATION_QUERY.into(),
            native_workspace_devtools_protocol::SESSION_PIN.into(),
            native_workspace_devtools_protocol::SESSION_LOOKUP.into(),
        ],
    })
}
async fn ingest(State(s): State<Service>, axum::Json(batch): axum::Json<Batch>) -> ApiResult<Ack> {
    batch
        .validate()
        .map_err(|code| error(StatusCode::BAD_REQUEST, code))?;
    let (tx, rx) = oneshot::channel();
    s.resources.received.fetch_add(
        batch.records.len() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    s.resources
        .queue
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if s.write.try_send(Write::Ingest(batch, tx)).is_err() {
        s.resources
            .queue
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        return Err(busy());
    }
    Ok(axum::Json(wait(rx).await?))
}
async fn sessions(State(s): State<Service>) -> ApiResult<Vec<SessionSummary>> {
    let (tx, rx) = oneshot::channel();
    s.read.try_send(Read::Sessions(tx)).map_err(|_| busy())?;
    Ok(axum::Json(wait(rx).await?))
}
async fn session(State(s): State<Service>, Path(id): Path<Uuid>) -> ApiResult<SessionSummary> {
    let (tx, rx) = oneshot::channel();
    s.read.try_send(Read::Session(id, tx)).map_err(|_| busy())?;
    let header = wait(rx)
        .await?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "session_not_found"))?;
    Ok(axum::Json(header))
}
#[derive(Deserialize)]
struct PageQuery {
    #[serde(default)]
    after: Option<Counter>,
    limit: Option<usize>,
    tail: Option<bool>,
    internal: Option<bool>,
    operation: Option<Counter>,
}
async fn page(
    State(s): State<Service>,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> ApiResult<Page> {
    let (tx, rx) = oneshot::channel();
    let job = if q.tail.unwrap_or(false) {
        Read::Tail(
            id,
            q.limit.unwrap_or(500),
            q.internal.unwrap_or(true),
            q.operation.map(|id| id.0),
            tx,
        )
    } else {
        Read::Page(
            id,
            q.after.unwrap_or_default().0,
            q.limit.unwrap_or(250),
            q.internal.unwrap_or(true),
            q.operation.map(|id| id.0),
            tx,
        )
    };
    s.read.try_send(job).map_err(|_| busy())?;
    Ok(axum::Json(wait(rx).await?))
}
async fn set_pin(
    State(s): State<Service>,
    Path(id): Path<Uuid>,
    axum::Json(pin): axum::Json<native_workspace_devtools_protocol::Pin>,
) -> ApiResult<native_workspace_devtools_protocol::Pin> {
    let (tx, rx) = oneshot::channel();
    s.write
        .try_send(Write::Pin(id, pin.pinned, tx))
        .map_err(|_| busy())?;
    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => {
            let (status, code) = match e.to_string().as_str() {
                "session_not_found" => (StatusCode::NOT_FOUND, "session_not_found"),
                "pin_limit_reached" => (StatusCode::CONFLICT, "pin_limit_reached"),
                _ => (StatusCode::CONFLICT, "storage_rejected"),
            };
            return Err(error(status, code));
        }
        _ => return Err(busy()),
    }
    Ok(axum::Json(pin))
}
async fn control(State(s): State<Service>, Path(id): Path<Uuid>) -> ApiResult<Control> {
    let (tx, rx) = oneshot::channel();
    s.read.try_send(Read::Control(id, tx)).map_err(|_| busy())?;
    Ok(axum::Json(Control {
        recording: wait(rx).await?,
    }))
}
async fn set_control(
    State(s): State<Service>,
    Path(id): Path<Uuid>,
    axum::Json(c): axum::Json<Control>,
) -> ApiResult<Control> {
    let (tx, rx) = oneshot::channel();
    s.write
        .try_send(Write::Control(id, c.recording, tx))
        .map_err(|_| busy())?;
    wait(rx).await?;
    Ok(axum::Json(c))
}
async fn export(
    State(s): State<Service>,
    Path(id): Path<Uuid>,
    axum::Extension(budget): axum::Extension<Arc<OwnedSemaphorePermit>>,
) -> std::result::Result<impl IntoResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    let (tx, rx) = oneshot::channel();
    s.archive
        .try_send(Archive::Export(id, budget, tx))
        .map_err(|_| busy())?;
    Ok((
        [
            ("content-type", "application/zip"),
            (
                "content-disposition",
                "attachment; filename=recording.nwdiag",
            ),
        ],
        wait(rx).await?,
    ))
}
async fn import(
    State(s): State<Service>,
    axum::Extension(budget): axum::Extension<Arc<OwnedSemaphorePermit>>,
    bytes: Bytes,
) -> ApiResult<Uuid> {
    let (tx, rx) = oneshot::channel();
    s.write
        .try_send(Write::Import(bytes.to_vec(), budget, tx))
        .map_err(|_| busy())?;
    Ok(axum::Json(wait(rx).await?))
}
async fn live(State(s): State<Service>, ws: WebSocketUpgrade) -> Response {
    let mut notifications = s.live.subscribe();
    ws.on_upgrade(move |mut socket| async move {
        // Notifications contain cursors, never payloads. A slow client gets an
        // explicit reload instruction and cannot apply backpressure to ingest.
        loop {
            let message = match notifications.recv().await {
                Ok(message) => message,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    "{\"gap\":true,\"reload\":true}".into()
                }
                Err(_) => break,
            };
            if !matches!(
                tokio::time::timeout(
                    Duration::from_secs(2),
                    socket.send(Message::Text(message.into()))
                )
                .await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    })
}

async fn console_hello(
    State(s): State<Service>,
    axum::Json(client): axum::Json<native_workspace_devtools_protocol::ConsoleHello>,
) -> StatusCode {
    s.resources.register_console(client.pid);
    StatusCode::NO_CONTENT
}
async fn console_focus(State(s): State<Service>) -> StatusCode {
    let _ = s.live.send("{\"focus\":true}".into());
    StatusCode::NO_CONTENT
}
#[derive(Deserialize)]
struct ResourceQuery {
    #[serde(default)]
    after: Option<Counter>,
}
async fn resources(
    State(s): State<Service>,
    Path(id): Path<Uuid>,
    Query(query): Query<ResourceQuery>,
) -> ApiResult<native_workspace_devtools_protocol::ResourcePage> {
    let (tx, rx) = oneshot::channel();
    s.read
        .try_send(Read::Resources(id, query.after.unwrap_or_default().0, tx))
        .map_err(|_| busy())?;
    Ok(axum::Json(
        native_workspace_devtools_protocol::ResourcePage {
            samples: wait(rx).await?,
            collector: s.resources.snapshot(),
        },
    ))
}
async fn analysis(
    State(s): State<Service>,
    Path(id): Path<Uuid>,
) -> ApiResult<native_workspace_devtools_protocol::Analysis> {
    let (tx, rx) = oneshot::channel();
    s.read
        .try_send(Read::Analysis(id, tx))
        .map_err(|_| busy())?;
    Ok(axum::Json(wait(rx).await?))
}

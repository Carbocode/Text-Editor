use fs2::FileExt;
use native_workspace_devtools::{
    server::Service,
    store::{Result, Store},
};
use native_workspace_devtools_protocol::{Descriptor, Uuid, VERSION, diagnostics_dir, synthetic};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let dir = diagnostics_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("service.lock"))?;
    lock.try_lock_exclusive()
        .map_err(|_| "diagnostic service already owns this directory")?;
    let mut store = Store::open(&dir.join("analytics.duckdb"))?;
    if std::env::args().any(|s| s == "--demo") {
        store.ingest(&synthetic())?;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let descriptor = Descriptor {
        version: VERSION,
        service: Uuid::new_v4(),
        port: listener.local_addr()?.port(),
        token: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
    };
    let path = dir.join("service.json");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    descriptor.write(&path)?;
    let service = Service::new(store, descriptor)?;
    service.start_resources(dir.join("analytics.duckdb"))?;
    println!(
        "Developer Tools service ready; descriptor: {}",
        path.display()
    );
    let result = axum::serve(listener, service.router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let _ = std::fs::remove_file(path);
    result?;
    drop(lock);
    Ok(())
}

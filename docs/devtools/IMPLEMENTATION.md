# Developer Tools: implementation and verification

Native Developer Tools implement live activity, resource monitoring, operation
analysis, state inspection and portable recordings from the architecture in
[README.md](README.md). The complete four-phase plan still has outstanding items
listed below, including the performance/accessibility gate on every platform.

## Run

```sh
cargo xtask devtools --demo  # build both processes; start/reuse service, open console
cargo xtask devtools        # same without seeding a fixture
```

The service stays alive after the console closes. Run them separately when useful:

```sh
cargo xtask devtools-service
cargo xtask devtools-ui
```

Normal editor launches do not start collection, the service or the console.
The console interface is in English. Open **Developer Tools** with the button in
the editor (on macOS also Developer menu / Cmd–Option–D). The button starts collection and bootstraps the two separate
processes in the background. Clicking again focuses the existing console.

```sh
cargo xtask run-macos
cargo xtask run-gnome
cargo xtask run-kde
```

`cargo xtask run-macos` opens the editor bundle through Launch Services and
brings it to the foreground. Explicit development environment overrides are
forwarded to the launched editor. Executables are installed atomically so a
failed rebuild leaves the previous editor usable.

These development builds include the `devtools` feature by default. macOS packages
the collector beside the editor and the console in a separate helper app with its
own bundle identity. The GNOME/KDE xtasks build all three executables together,
then run the selected editor executable explicitly. The console has a distinct
Wayland app ID, and Qt declares the editor desktop ID. Core-only builds retain no
default diagnostics feature. Use `NATIVE_WORKSPACE_DEVTOOLS_BUILD=0` for a macOS build without it, or
`--no-default-features` for the frontend Rust packages. For command-line probes,
`NATIVE_WORKSPACE_DEVTOOLS=1` enables recording without clicking the button.
`NATIVE_WORKSPACE_DEVTOOLS_SERVICE` and `NATIVE_WORKSPACE_DEVTOOLS_UI` override
executable discovery. No build or network request runs on the editor UI thread.

`NATIVE_WORKSPACE_DIAGNOSTICS_DIR` overrides the diagnostic directory only. It
never changes `NATIVE_WORKSPACE_STATE_DB`. Default paths are the platform app-data
area, with `analytics.duckdb`, `service.lock`, and private `service.json`. The
launcher verifies authenticated service identity/version before reusing a
collector. A lock prevents two collectors owning the same directory. Tokens
are random, stored in a mode-0600 descriptor, absent from command lines and archives.
The server binds IPv4 loopback and checks Authorization, Host and browser Origin
on HTTP and WebSocket requests.

## Implemented contracts and limits

- Protocol/database/archive version 1. UUID session, producer and batch identities;
  decimal-string 64-bit counters and nanoseconds; monotonic duration clocks.
- Fixed-size numeric/static-name records: dispatch start/end with ok/error/no-op,
  search queue/debounce/execute/publication/delivery, cancellation or stale
  generation, snapshot application duration, lifecycle and loss records.
- Search context travels in jobs and results and across UniFFI snapshots. The
  frontend callback reports snapshot application, **not** frame presentation.
  A debounce span starts when the worker dequeues a job, separate from its queue
  event. Query text, paths, document contents and free-form error text are never
  emitted. Producer wall time is session metadata; interprocess clock calibration
  is not implemented and the console explicitly avoids precision claims.
- Lock-free queue: 4,096 fixed-size records; no main-thread network, serialization,
  formatting or waiting for space. The application observer is independent of the
  Swift application's mutex. When disabled, hooks do not timestamp/enqueue.
- Transport: at most 256 records/256 KiB per batch, 250 ms aggregation, one
  unacknowledged batch, 200 ms connect/750 ms request timeout. Eight failed attempts
  expire an unconfirmed batch rather than accumulating memory. Queue overflow is
  certain loss; `producer.unconfirmed_expired` means possible loss (a commit whose
  ACK was lost may already exist). The reported total includes both. Termination
  can lose buffered records; absence of a confirmed session end remains explicit.
- DuckDB engine and binding are bundled and fixed by Cargo.lock and
  `duckdb = 1.10506.0` (engine 1.5.6). Eframe 0.33.0 uses WGPU, AccessKit, Wayland/X11.
  Their licenses are MIT and MIT/Apache-2.0, respectively. The older DuckDB 1.4.1
  bundled Parquet sources fail against the local Xcode libc++; they are not used.
- Dedicated writer, reader, archive threads; bounded queues of 16/16/1 jobs.
  SQL is service-owned and parameterized; no arbitrary SQL API. Writer Appender
  flush and records/batch watermark commit precede the ACK. Repeated batch IDs
  verify a content digest; overlaps and identity collisions are rejected.
- DuckDB: 2 execution threads, 128 MiB engine memory and 256 MiB temporary spill
  limit. These do **not** bound total process RSS. Requests have a 10 s response
  timeout; in-progress DuckDB jobs are not yet interruptible at that timeout.
  One bulk permit is acquired before buffering an archive upload and retained
  by the worker until completion. Export has a separate connection/snapshot.
- Sessions still recording at collector restart become interrupted. Their last
  committed sequence remains queryable. Collection can be resumed with Start
  collection; status alone is not a producer heartbeat or liveness guarantee.
- Authenticated WebSocket commit/control notifications; 32-notification buffer;
  lagged clients receive an explicit gap/reload instruction. Producer controls
  also have an HTTP fallback. Producer-observed collection start/stop transitions
  bypass the event filter and remain in the persisted sequence, identifying paused
  intervals (subject to the same queue/loss budget). Console live updates are aggregated; fallback
  refresh is 1 Hz, and repaint occurs on input or changed query results. Paused
  or minimized views stop automatic query updates without stopping collection.
- Console holds up to 200 session headers and one page of at most 500 records;
  event rows are virtualized and timeline bars are clipped to the visible time
  window. Text/outcome filters apply to the current page. Operation filtering is
  applied by the database before pagination, so Show this operation retrieves
  related events from the entire recording, including across unrelated pages. Tail queries remove poll/state noise
  before applying the page limit; live selection follows the newest real editor
  session. Demo/offline sessions require explicit selection.
- Up to 64 recordings can be pinned persistently and appear first, including
  recordings older than the 200-header recent list. Live selection still chooses
  the newest real session, independently of pin order. A selected/imported session
  missing from the recent list is loaded by its UUID without exceeding the
  console's 200-header budget. Pins are local preferences and are not exported.
- Collector health advertises supported API capabilities. An older running
  collector can still serve existing views; the console explicitly disables
  unsupported operation lookup/pinning and asks for a diagnostic service restart.
  Additive header fields decode with defaults and do not change protocol/archive
  version 1. Request failures keep the cached view and distinguish a reachable
  collector from a disconnected one.
- Timeline: five stable logical actor lanes (application, search worker,
  filesystem, frontend, collection), shared time axis, internal overlap tracks,
  fit, zoom, pan, interval measurement and click/hover details. Missing span ends
  remain visibly incomplete; a lane is never added for a new operation.
- Resource sampling uses OS process counters at 1 Hz on a separate service thread:
  CPU (100% = one core), resident/virtual memory and process read/write counters.
  Editor, collector and console costs are distinguishable. CPU first sample is
  a warmup; OS disk counters do not represent every logical file access. Editor
  sampling pauses with collection. Collector status includes queue depth, commit,
  loss/rejection counters, database size and system memory. Charts have 60/300 s
  windows; missing samples are gaps. Offline imports retain their resource history.
- Core facts include document lifecycle/edit/save, errors, state scalar counters,
  search generations/results, watcher change/error counts and reconciliation
  reasons. The UI separates overview, resources, events, timeline, operation
  percentiles, state, filesystem and recordings, with readable activity labels.
- `.nwdiag`: versioned manifest + `events.parquet` and optional `resource_samples.parquet` in ZIP, SHA-256, committed
  watermark, session metadata/capabilities and redaction list. Export is the
  entire confirmed session using a read transaction; the UI reviews metadata,
  destination, redaction and overwrite before submitting. Local output uses a
  same-directory temporary file, sync and rename. ZIP and expanded contents are
  limited to 64 MiB, the manifest to 32 KiB, imports to 1,000,000 rows. Only known
  entry names are read; duplicates, traversal, symlinks, unexpected schema,
  invalid identifiers/enums, sequence duplicates and checksum failures are rejected.
  DuckDB automatic extension installation/loading is disabled. Import is atomic,
  generates a new session namespace, preserves source provenance, and disables
  editor controls. The console can reopen these data with no editor connected.

## Coverage still required by the full plan

The current contracts deliberately have no arbitrary attribute bag. A tracing
subscriber layer, clock handshakes, frontend loop lag, rich parent-span/causal
links and frame hooks remain to be added. FFI wait/hold spans,
minimap/highlighting and complete state checkpoints/deltas are not yet
instrumented. Scalar state and aggregated watcher/reconciliation facts are live. The synthetic watcher is labelled as synthetic capability.

Marker/note editing, automatic retention/disk quotas, time-range
export, artifact export, JSONL/CSV/Perfetto export, advanced server-side filters/rollups,
comparison panels, query cancellation and packaging
installers remain open. Disk errors currently reject ingestion and cause bounded
producer retry/loss; they do not alter editor commands. Editor/collector/console RSS and CPU are available; separate per-hook producer
cost and controlled before/after p95 overhead dashboards remain open.

## Checks

```sh
cargo fmt --all --check
cargo test
cargo test -p native-workspace-devtools-protocol -p native-workspace-devtools-producer \
  -p native-workspace-devtools -p native-workspace-application \
  --features native-workspace-application/devtools
cargo clippy -p native-workspace-devtools -p native-workspace-devtools-ui \
  -p native-workspace-devtools-protocol -p native-workspace-devtools-producer \
  -p native-workspace-application -p native-workspace-ffi-swift -p xtask \
  --all-features -- -D warnings
cargo run -p native-workspace-devtools --example benchmark --release -- 200
```

The storage probe includes serialization, Appender flush/commit, cursor queries,
Parquet ZIP round trip and database reopen. The checked-in
[debug probe](benchmarks/macos-debug.json) is a 51,200-record macOS arm64 run made
while compilation was active: it verifies functionality, **not** a performance
baseline, CPU/RSS budget or the plan's 1% editor latency target. Release runs,
repeated fixed-corpus trials, concurrent reader/ingestion probes
are still required before signing off the phase 0 performance gate.

The synthetic fixture covers a superseded search, successful delivery, aggregated
watcher, loss and an unfinished span. Integration tests check durable retry,
collision rollback, restart classification, pagination, Parquet correlations,
repeated import namespaces, unsafe ZIP paths/checksums and local authentication. Additional tests cover hard
collector termination/recovery, filtered live tails, resource archive round trips,
offline isolation and stable actor lanes with overlapping/incomplete operations.
Operation queries are tested with a span separated by 1,200 unrelated events,
64-bit identifiers above 2^63, filtered pagination/tails and session isolation.
Pin tests cover restart persistence, idempotency, the 64-pin budget, visibility
beyond 200 sessions, archive isolation and authenticated APIs.

macOS: the packaged Swift application and all native smoke checks passed. A
normal isolated launch had no telemetry until the Developer Tools shortcut was
used; that action started the collector/console and persisted the live session.
OS resource sampling was verified against the editor, collector and console.
GTK and Qt frontend checks passed on macOS; the plain Qt launch displayed the
editor with its Developer Tools button. Linux GNOME/KDE and Kirigami rendering
still need verification on their target desktops.

End-to-end core probe (run once with the collector and once with it absent):

```sh
NATIVE_WORKSPACE_DEVTOOLS=1 cargo run -p native-workspace-application \
  --features devtools --example devtools_smoke -- crates
```

Platform verification results are recorded below as they are executed; Linux
GNOME/Wayland/X11 and Linux KDE verification require compatible hosts and are not
implied by compilation of the shared crates on macOS.

mod network;
mod theme;
mod timeline;
mod widgets;

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use native_workspace_devtools_protocol::{
    Kind, Outcome, Record, ResourceSample, SessionSummary, diagnostics_dir,
};
use network::{Command, View};
use std::{
    collections::HashMap,
    sync::mpsc::{Receiver, SyncSender},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Overview,
    Resources,
    Events,
    Timeline,
    Operations,
    State,
    Filesystem,
    Recordings,
}
impl Panel {
    fn all() -> [(Self, &'static str); 8] {
        [
            (Self::Overview, "Overview"),
            (Self::Resources, "Resources"),
            (Self::Events, "Events"),
            (Self::Timeline, "Timeline"),
            (Self::Operations, "Operations"),
            (Self::State, "Editor state"),
            (Self::Filesystem, "Filesystem"),
            (Self::Recordings, "Recordings"),
        ]
    }
    fn title(self) -> &'static str {
        Self::all()
            .into_iter()
            .find(|(panel, _)| *panel == self)
            .unwrap()
            .1
    }
}
struct Console {
    commands: SyncSender<Command>,
    updates: Receiver<View>,
    view: View,
    panel: Panel,
    paused: bool,
    minimized: bool,
    filter: String,
    errors_only: bool,
    show_internal: bool,
    operation: Option<u64>,
    detail: Option<Record>,
    seconds: u64,
    timeline: timeline::Timeline,
    archive_path: String,
    export_dialog: bool,
    import_dialog: bool,
}
fn main() -> eframe::Result<()> {
    eframe::run_native(
        "Native Workspace — Developer Tools",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_app_id("org.nativeworkspace.devtools")
                .with_inner_size([1280.0, 840.0])
                .with_min_inner_size([920.0, 620.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            let (commands, updates) = network::spawn(cc.egui_ctx.clone());
            Ok(Box::new(Console {
                commands,
                updates,
                view: View::default(),
                panel: if std::env::args().any(|arg| arg == "--timeline") {
                    Panel::Timeline
                } else {
                    Panel::Overview
                },
                paused: false,
                minimized: false,
                filter: String::new(),
                errors_only: false,
                show_internal: false,
                operation: None,
                detail: None,
                seconds: 60,
                timeline: timeline::Timeline::default(),
                archive_path: diagnostics_dir()
                    .join("recording.nwdiag")
                    .display()
                    .to_string(),
                export_dialog: false,
                import_dialog: false,
            }))
        }),
    )
}
impl Console {
    fn send(&mut self, command: Command) {
        if self.commands.try_send(command).is_err() {
            self.view.message = "An operation is in progress. Try again shortly.".into();
        }
    }
    fn session(&self) -> Option<&SessionSummary> {
        self.view
            .sessions
            .iter()
            .find(|s| Some(s.session.id) == self.view.selected)
    }
    fn records(&self) -> &[Record] {
        self.view.page.as_ref().map_or(&[], |p| &p.records)
    }
    fn state(&self, name: &str) -> Option<u64> {
        self.view
            .analysis
            .state
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.value.0)
    }
    fn editor_sample(&self) -> Option<&ResourceSample> {
        let pid = self.session()?.session.process_id;
        self.view
            .resources
            .samples
            .iter()
            .rev()
            .find(|s| s.pid == pid && s.role == "editor")
    }
    fn filtered(&self, filesystem: bool) -> Vec<Record> {
        let filter = self.filter.to_lowercase();
        self.records()
            .iter()
            .filter(|r| {
                (!filesystem
                    || r.name.starts_with("filesystem.")
                    || r.name.starts_with("watcher.")
                    || r.name.starts_with("reconcile.")
                    || r.name.contains("save")
                    || r.name.contains("workspace_refresh")
                    || r.name == "error.filesystem")
                    && (self.operation.is_none_or(|id| r.operation.0 == id))
                    && (!self.errors_only
                        || r.outcome == Outcome::Error
                        || r.name.starts_with("error."))
                    && (self.show_internal
                        || widgets::meaningful(r)
                        || self.panel == Panel::Timeline)
                    && (filter.is_empty()
                        || widgets::name(&r.name).to_lowercase().contains(&filter)
                        || r.name.to_lowercase().contains(&filter)
                        || widgets::component(r).to_lowercase().contains(&filter))
            })
            .cloned()
            .collect()
    }
    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .exact_height(84.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(22, 14)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Developer Tools").size(20.0).strong());
                        ui.label(theme::muted("Native Workspace").size(12.0));
                    });
                    ui.add_space(28.0);
                    let label = self
                        .session()
                        .map(|s| {
                            format!(
                                "{} · {}",
                                widgets::frontend(&s.session.frontend),
                                widgets::time(s.session.started_unix_ms.0)
                            )
                        })
                        .unwrap_or_else(|| "No editor connected".into());
                    egui::ComboBox::from_id_salt("session-picker")
                        .selected_text(label)
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(false, "Automatically follow the editor")
                                .clicked()
                            {
                                self.operation = None;
                                self.detail = None;
                                self.send(Command::AutoSelect);
                            }
                            let choices = self.view.sessions.clone();
                            for session in choices {
                                if ui
                                    .selectable_label(
                                        Some(session.session.id) == self.view.selected,
                                        format!(
                                            "{} · {}",
                                            widgets::frontend(&session.session.frontend),
                                            widgets::time(session.session.started_unix_ms.0)
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.operation = None;
                                    self.detail = None;
                                    self.send(Command::Select(session.session.id));
                                }
                            }
                        });
                    let online = self.editor_sample().is_some_and(|sample| {
                        sample.alive
                            && native_workspace_devtools_protocol::unix_ms()
                                .saturating_sub(sample.unix_ms.0)
                                < 4_000
                    }) && self
                        .session()
                        .is_some_and(|session| session.status == "recording");
                    let (status, color) = match self.session() {
                        Some(session) if session.status == "offline" => ("ARCHIVE", theme::MUTED),
                        Some(session) if session.status == "completed" => {
                            ("COMPLETED", theme::MUTED)
                        }
                        Some(session) if !session.recording => ("COLLECTION STOPPED", theme::AMBER),
                        Some(_) if self.paused => ("VIEW PAUSED", theme::AMBER),
                        Some(_) if online => ("LIVE", theme::GREEN),
                        Some(_) => ("RECORDING", theme::MUTED),
                        None => ("WAITING", theme::AMBER),
                    };
                    theme::badge(ui, status, color);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Archive").clicked() {
                            self.panel = Panel::Recordings;
                        }
                        if ui
                            .button(if self.paused {
                                "Resume view"
                            } else {
                                "Pause view"
                            })
                            .clicked()
                        {
                            self.paused = !self.paused;
                            self.send(Command::Paused(self.paused || self.minimized));
                        }
                        if let Some(session) = self.session().cloned()
                            && matches!(session.status.as_str(), "recording" | "interrupted")
                        {
                            let text = if session.recording {
                                "Stop collection"
                            } else {
                                "Start collection"
                            };
                            if ui.button(text).clicked() {
                                self.send(Command::Control(session.session.id, !session.recording));
                            }
                        }
                    });
                });
            });
    }
    fn navigation(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(190.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 22)),
            )
            .show(ctx, |ui| {
                ui.label(theme::muted("OBSERVE").size(11.0));
                ui.add_space(12.0);
                for (panel, label) in Panel::all() {
                    let selected = self.panel == panel;
                    let response = ui.add_sized(
                        [162.0, 38.0],
                        egui::Button::new(RichText::new(label).color(if selected {
                            theme::TEXT
                        } else {
                            theme::MUTED
                        }))
                        .fill(if selected {
                            egui::Color32::from_rgb(33, 49, 75)
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE),
                    );
                    if response.clicked() {
                        self.panel = panel;
                        self.filter.clear();
                        self.operation = None;
                    }
                }
                ui.add_space(24.0);
                ui.separator();
                ui.add_space(12.0);
                ui.label(theme::muted("CONNECTION").size(11.0));
                ui.horizontal(|ui| {
                    let color = if self.view.connected {
                        theme::GREEN
                    } else {
                        theme::AMBER
                    };
                    widgets::dot(ui, color);
                    ui.label(if self.view.connected {
                        "Service connected"
                    } else {
                        "Connecting"
                    });
                });
                if let Some(session) = self.session() {
                    ui.label(
                        theme::muted(format!("{} recorded events", session.records.0)).size(12.0),
                    );
                    if session.dropped.0 > 0 {
                        ui.colored_label(
                            theme::AMBER,
                            format!("{} lost or unconfirmed events", session.dropped.0),
                        );
                    }
                }
                ui.add_space(8.0);
                if ui.button("Refresh").clicked() {
                    self.send(Command::Refresh);
                }
                ui.add_space(18.0);
                ui.label(
                    theme::muted("Collection continues when you close this window.").size(12.0),
                );
            });
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        if self.view.selected.is_none() {
            theme::card().show(ui,|ui|{ui.label(RichText::new("Connect the editor").size(20.0).strong());ui.label("Click Developer Tools in the editor. Collection starts immediately and the console selects the new session.");ui.label(theme::muted("Previous sessions are available in Recordings."));});
            ui.add_space(18.0);
        }
        let sample = self.editor_sample().cloned();
        let alive = sample.as_ref().is_some_and(|s| s.alive);
        let cpu = sample
            .as_ref()
            .filter(|_| alive)
            .map_or("—".into(), |s| format!("{:.1}%", s.cpu_percent));
        let memory = sample
            .as_ref()
            .filter(|_| alive)
            .map_or("—".into(), |s| widgets::bytes(s.memory_bytes.0));
        let disk = sample.as_ref().filter(|_| alive).map_or("—".into(), |s| {
            format!("{}/s", widgets::bytes(s.read_bytes.0 + s.written_bytes.0))
        });
        let records = self.records();
        let recent = records.iter().filter(|r| widgets::meaningful(r)).count();
        let columns = if ui.available_width() > 850.0 { 4 } else { 2 };
        let width = (ui.available_width() - 12.0 * (columns - 1) as f32) / columns as f32;
        let cards = [
            ("Editor CPU", cpu, "100% equals one CPU core", theme::ACCENT),
            (
                "Resident memory",
                memory,
                "Editor process RAM",
                theme::GREEN,
            ),
            (
                "Disk activity",
                disk,
                "Reads + writes per second",
                theme::AMBER,
            ),
            (
                "Recent activity",
                recent.to_string(),
                "Operations in the live window",
                theme::TEXT,
            ),
        ];
        egui::Grid::new("overview-cards")
            .num_columns(columns)
            .spacing([12.0, 12.0])
            .show(ui, |ui| {
                for (index, (label, value, subtitle, color)) in cards.iter().enumerate() {
                    widgets::metric(ui, width, label, value, subtitle, *color);
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(18.0);
        self.editor_charts(ui);
        ui.add_space(20.0);
        ui.label(RichText::new("What is happening").size(18.0).strong());
        ui.label(theme::muted(
            "The latest editor operations, updated automatically.",
        ));
        ui.add_space(8.0);
        let recent: Vec<Record> = self
            .records()
            .iter()
            .rev()
            .filter(|r| widgets::meaningful(r))
            .take(8)
            .cloned()
            .collect();
        if recent.is_empty() {
            theme::card().show(ui, |ui| {
                ui.label(
                    "No recent activity. Open a file, edit text or start a search in the editor.",
                );
            });
        }
        for record in recent {
            let (outcome, color) = widgets::outcome(record.outcome);
            theme::card().inner_margin(12).show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::dot(ui, color);
                    if ui
                        .selectable_label(false, widgets::name(&record.name))
                        .clicked()
                    {
                        self.detail = Some(record.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(theme::muted(format!(
                            "{:.2}s",
                            record.monotonic_ns.0 as f64 / 1e9
                        )));
                        ui.label(theme::muted(outcome));
                    });
                });
            });
        }
    }
    fn editor_charts(&self, ui: &mut egui::Ui) {
        let pid = self.session().map_or(0, |s| s.session.process_id);
        if ui.available_width() > 720.0 {
            ui.columns(2, |cols| {
                widgets::chart(
                    &mut cols[0],
                    "CPU over time",
                    "Measured samples · one point per second",
                    &self.view.resources.samples,
                    pid,
                    self.seconds,
                    true,
                    theme::ACCENT,
                );
                widgets::chart(
                    &mut cols[1],
                    "Memory over time",
                    "Resident memory · MiB",
                    &self.view.resources.samples,
                    pid,
                    self.seconds,
                    false,
                    theme::GREEN,
                );
            });
        } else {
            widgets::chart(
                ui,
                "CPU over time",
                "Measured samples · one point per second",
                &self.view.resources.samples,
                pid,
                self.seconds,
                true,
                theme::ACCENT,
            );
            ui.add_space(12.0);
            widgets::chart(
                ui,
                "Memory over time",
                "Resident memory · MiB",
                &self.view.resources.samples,
                pid,
                self.seconds,
                false,
                theme::GREEN,
            );
        }
    }
    fn resources(&self, ui: &mut egui::Ui) {
        ui.label(theme::muted("Editor, collector and console run as separate processes. Sampling runs outside the editor thread."));
        ui.add_space(14.0);
        let mut latest = HashMap::new();
        for sample in &self.view.resources.samples {
            latest.insert(sample.pid, sample.clone());
        }
        let mut samples: Vec<_> = latest.into_values().collect();
        samples.sort_by_key(|s| {
            (
                match s.role.as_str() {
                    "editor" => 0,
                    "collector" => 1,
                    _ => 2,
                },
                s.pid,
            )
        });
        theme::card().show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(false)
                .column(Column::remainder())
                .column(Column::exact(70.0))
                .column(Column::exact(90.0))
                .column(Column::exact(110.0))
                .column(Column::exact(110.0))
                .header(32.0, |mut header| {
                    for title in ["Process", "CPU", "RAM", "Read/s", "Write/s"] {
                        header.col(|ui| {
                            ui.label(theme::muted(title).strong());
                        });
                    }
                })
                .body(|body| {
                    body.rows(42.0, samples.len(), |mut row| {
                        let s = &samples[row.index()];
                        row.col(|ui| {
                            ui.label(RichText::new(widgets::role(&s.role)).strong());
                            ui.label(
                                theme::muted(format!(
                                    "PID {}{}",
                                    s.pid,
                                    if s.alive { "" } else { " · exited" }
                                ))
                                .size(11.0),
                            );
                        });
                        row.col(|ui| {
                            ui.label(if s.alive {
                                format!("{:.1}%", s.cpu_percent)
                            } else {
                                "—".into()
                            });
                        });
                        row.col(|ui| {
                            ui.label(widgets::bytes(s.memory_bytes.0));
                        });
                        row.col(|ui| {
                            ui.label(widgets::bytes(s.read_bytes.0));
                        });
                        row.col(|ui| {
                            ui.label(widgets::bytes(s.written_bytes.0));
                        });
                    });
                });
        });
        ui.add_space(18.0);
        self.editor_charts(ui);
        ui.add_space(18.0);
        let c = &self.view.resources.collector;
        theme::card().show(ui, |ui| {
            ui.label(RichText::new("Collection health").size(18.0).strong());
            egui::Grid::new("collector-status")
                .num_columns(2)
                .spacing([40.0, 10.0])
                .show(ui, |ui| {
                    for (label, value) in [
                        ("Queued batches", c.writer_queue.0.to_string()),
                        ("Received events", c.received_records.0.to_string()),
                        ("Committed events", c.committed_records.0.to_string()),
                        ("Rejected batches", c.rejected_batches.0.to_string()),
                        ("Lost samples", c.sampling_gaps.0.to_string()),
                        ("Local database", widgets::bytes(c.database_bytes.0)),
                        (
                            "System RAM",
                            format!(
                                "{} / {}",
                                widgets::bytes(c.system_used_memory_bytes.0),
                                widgets::bytes(c.system_memory_bytes.0)
                            ),
                        ),
                    ] {
                        ui.label(theme::muted(label));
                        ui.label(value);
                        ui.end_row();
                    }
                });
        });
    }
    fn event_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Search displayed activity…")
                    .desired_width(290.0),
            );
            ui.checkbox(&mut self.errors_only, "Errors only");
            if ui
                .checkbox(&mut self.show_internal, "Internal events")
                .changed()
            {
                self.send(Command::Internal(self.show_internal));
            }
            let mut follow = self.view.follow;
            if ui.checkbox(&mut follow, "Follow live").changed() {
                self.send(Command::Follow(follow));
            }
            if let Some(id) = self.operation {
                theme::badge(ui, &format!("Operation {id}"), theme::ACCENT);
                if ui.small_button("Clear filter").clicked() {
                    self.operation = None;
                }
            }
        });
        ui.add_space(12.0);
    }
    fn events(&mut self, ui: &mut egui::Ui, filesystem: bool) {
        self.event_toolbar(ui);
        let records = self.filtered(filesystem);
        if records.is_empty() {
            theme::card().show(ui,|ui|{ui.label("No events match the filters. Perform an action in the editor or clear the filters.");});
            return;
        }
        let mut ends = HashMap::new();
        for r in self.records() {
            if r.kind == Kind::SpanEnd {
                ends.insert(r.span.0, r.monotonic_ns.0);
            }
        }
        let starts: HashMap<_, _> = self
            .records()
            .iter()
            .filter(|r| r.kind == Kind::SpanStart)
            .map(|r| (r.span.0, r.monotonic_ns.0))
            .collect();
        let mut selected = None;
        theme::card().inner_margin(12).show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(Column::exact(82.0))
                .column(Column::remainder())
                .column(Column::initial(105.0))
                .column(Column::initial(105.0))
                .column(Column::exact(82.0))
                .header(30.0, |mut header| {
                    for title in ["Time", "Activity", "Component", "Outcome", "Duration"] {
                        header.col(|ui| {
                            ui.label(theme::muted(title).strong());
                        });
                    }
                })
                .body(|body| {
                    body.rows(34.0, records.len(), |mut row| {
                        let record = &records[row.index()];
                        let (outcome, color) = widgets::outcome(record.outcome);
                        row.col(|ui| {
                            ui.label(
                                theme::muted(format!("{:.3}s", record.monotonic_ns.0 as f64 / 1e9))
                                    .monospace(),
                            );
                        });
                        row.col(|ui| {
                            if ui
                                .selectable_label(
                                    self.detail
                                        .as_ref()
                                        .is_some_and(|r| r.sequence == record.sequence),
                                    widgets::name(&record.name),
                                )
                                .clicked()
                            {
                                selected = Some(record.clone());
                            }
                        });
                        row.col(|ui| {
                            ui.label(theme::muted(widgets::component(record)));
                        });
                        row.col(|ui| {
                            ui.colored_label(color, outcome);
                        });
                        row.col(|ui| {
                            let duration =
                                if record.name == "snapshot.apply" && record.kind == Kind::Event {
                                    Some(record.value.0)
                                } else {
                                    starts.get(&record.span.0).and_then(|start| {
                                        ends.get(&record.span.0)
                                            .map(|end| end.saturating_sub(*start))
                                    })
                                };
                            ui.label(
                                duration
                                    .map_or("—".into(), |d| format!("{:.2} ms", d as f64 / 1e6)),
                            );
                        });
                    });
                });
        });
        if let Some(record) = selected {
            self.detail = Some(record);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(
                theme::muted(format!(
                    "{} activities · time since session start",
                    records.len()
                ))
                .size(12.0),
            );
            if !self.view.follow {
                if ui.button("First page").clicked() {
                    self.send(Command::Page(0));
                }
                if let Some(next) = self.view.page.as_ref().and_then(|page| page.next)
                    && ui.button("Next page").clicked()
                {
                    self.send(Command::Page(next.0));
                }
            }
        });
    }
    fn timeline(&mut self, ui: &mut egui::Ui) {
        self.event_toolbar(ui);
        ui.label(theme::muted(
            "One lane per actor. Bars show elapsed time; markers show events.",
        ));
        ui.add_space(12.0);
        let records = self.records().to_vec();
        if let Some(record) = self.timeline.show(
            ui,
            self.view.selected,
            &records,
            &self.filter,
            self.operation,
            self.errors_only,
            self.show_internal,
        ) {
            self.detail = Some(record);
        }
    }
    fn operations(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::muted("Durations of completed operations in this recording. Cancelled searches are shown separately."));
        ui.add_space(16.0);
        theme::card().show(ui, |ui| {
            let stats = &self.view.analysis.operations;
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::remainder())
                .column(Column::exact(60.0))
                .column(Column::exact(90.0))
                .column(Column::exact(90.0))
                .column(Column::exact(90.0))
                .column(Column::exact(80.0))
                .header(32.0, |mut h| {
                    for title in ["Operation", "Count", "Median", "p95", "Maximum", "Errors"] {
                        h.col(|ui| {
                            ui.label(theme::muted(title).strong());
                        });
                    }
                })
                .body(|body| {
                    body.rows(36.0, stats.len(), |mut row| {
                        let stat = &stats[row.index()];
                        row.col(|ui| {
                            ui.label(widgets::name(&stat.name));
                        });
                        for value in [
                            stat.count.0.to_string(),
                            format!("{:.2} ms", stat.p50_ms),
                            format!("{:.2} ms", stat.p95_ms),
                            format!("{:.2} ms", stat.max_ms),
                            stat.errors.0.to_string(),
                        ] {
                            row.col(|ui| {
                                ui.label(value);
                            });
                        }
                    });
                });
        });
    }
    fn state_panel(&self, ui: &mut egui::Ui) {
        ui.label(theme::muted(
            "Latest values reported by the core. Document contents are excluded from telemetry.",
        ));
        ui.add_space(16.0);
        theme::card().show(ui, |ui| {
            egui::Grid::new("state-values")
                .num_columns(2)
                .spacing([70.0, 18.0])
                .show(ui, |ui| {
                    for (label, key) in [
                        ("Revision", "state.revision"),
                        ("Workspace documents", "state.documents"),
                        ("Directories", "state.directories"),
                        ("Search results", "state.search_results"),
                        ("Search generation", "state.search_generation"),
                        ("Search in progress", "state.search_pending"),
                        ("Watcher active", "state.watcher_active"),
                        ("Document text size", "document.buffer_bytes"),
                    ] {
                        ui.label(theme::muted(label));
                        let value = self.state(key).map_or("Not observed yet".into(), |v| {
                            if key == "document.buffer_bytes" {
                                widgets::bytes(v)
                            } else if key == "state.search_pending" || key == "state.watcher_active"
                            {
                                if v == 0 { "No" } else { "Yes" }.into()
                            } else {
                                v.to_string()
                            }
                        });
                        ui.label(RichText::new(value).strong());
                        ui.end_row();
                    }
                });
        });
    }
    fn recordings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Open archive…").clicked() {
                self.import_dialog = true;
            }
            if ui
                .add_enabled(
                    self.view.selected.is_some(),
                    egui::Button::new("Export recording…"),
                )
                .clicked()
            {
                self.export_dialog = true;
            }
        });
        ui.add_space(18.0);
        let sessions = self.view.sessions.clone();
        for session in sessions {
            theme::card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(widgets::frontend(&session.session.frontend)).strong(),
                        );
                        ui.label(theme::muted(format!(
                            "{} · {} events",
                            widgets::time(session.session.started_unix_ms.0),
                            session.records.0
                        )));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Open").clicked() {
                            self.send(Command::Select(session.session.id));
                            self.panel = Panel::Overview;
                        }
                        let state = match session.status.as_str() {
                            "recording" => "Recording",
                            "completed" => "Completed",
                            "interrupted" => "Interrupted",
                            "offline" => "Offline archive",
                            other => other,
                        };
                        theme::badge(
                            ui,
                            state,
                            if session.status == "recording" {
                                theme::GREEN
                            } else {
                                theme::MUTED
                            },
                        );
                    });
                });
            });
        }
    }
    fn inspector(&mut self, ctx: &egui::Context) {
        let Some(record) = self.detail.clone() else {
            return;
        };
        let mut close = false;
        egui::SidePanel::right("inspector")
            .default_width(300.0)
            .width_range(260.0..=380.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(20))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Details").size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Close").clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(18.0);
                ui.label(
                    RichText::new(widgets::name(&record.name))
                        .size(19.0)
                        .strong(),
                );
                let (outcome, color) = widgets::outcome(record.outcome);
                theme::badge(ui, outcome, color);
                ui.add_space(16.0);
                ui.label(theme::muted("Component"));
                ui.label(widgets::component(&record));
                ui.label(theme::muted("Time since start"));
                ui.label(format!("{:.3} seconds", record.monotonic_ns.0 as f64 / 1e9));
                ui.label(theme::muted("Revision"));
                ui.label(record.revision.0.to_string());
                if record.value.0 > 0 {
                    ui.label(theme::muted("Recorded value"));
                    ui.label(if record.name.contains("bytes") {
                        widgets::bytes(record.value.0)
                    } else if record.name == "snapshot.apply" {
                        format!("{:.3} ms", record.value.0 as f64 / 1e6)
                    } else {
                        record.value.0.to_string()
                    });
                }
                ui.add_space(16.0);
                if record.operation.0 != 0 && ui.button("Show this operation").clicked() {
                    self.operation = Some(record.operation.0);
                    self.panel = Panel::Timeline;
                }
                ui.add_space(16.0);
                ui.collapsing("Technical details", |ui| {
                    ui.monospace(&record.name);
                    ui.label(format!(
                        "Sequence: {}\nOperation: {}\nSpan: {}\nGeneration: {}\nThread: {}",
                        record.sequence.0,
                        record.operation.0,
                        record.span.0,
                        record.generation.0,
                        record.thread
                    ));
                    if ui.button("Copy JSON").clicked() {
                        ctx.copy_text(serde_json::to_string_pretty(&record).unwrap_or_default());
                    }
                });
            });
        if close {
            self.detail = None;
        }
    }
    fn archive_dialogs(&mut self, ctx: &egui::Context) {
        if self.export_dialog {
            egui::Window::new("Export recording")
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .show(ctx, |ui| {
                    ui.label("Export all committed data from the selected session.");
                    ui.label(theme::muted(
                        "Document contents, query text and absolute paths are excluded.",
                    ));
                    ui.add_space(12.0);
                    ui.label("Destination");
                    ui.add(egui::TextEdit::singleline(&mut self.archive_path).desired_width(440.0));
                    if std::path::Path::new(&self.archive_path).exists() {
                        ui.colored_label(theme::AMBER, "The existing file will be replaced.");
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Export").clicked() {
                            if let Some(id) = self.view.selected {
                                self.send(Command::Export(id, self.archive_path.clone().into()));
                            }
                            self.export_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.export_dialog = false;
                        }
                    });
                });
        }
        if self.import_dialog {
            egui::Window::new("Open offline archive")
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .show(ctx, |ui| {
                    ui.label("Path to a .nwdiag file");
                    ui.add(egui::TextEdit::singleline(&mut self.archive_path).desired_width(440.0));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            self.send(Command::Import(self.archive_path.clone().into()));
                            self.import_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.import_dialog = false;
                        }
                    });
                });
        }
    }
}
impl eframe::App for Console {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(view) = self.updates.try_recv() {
            if view.selected != self.view.selected {
                self.detail = None;
                self.operation = None;
            }
            self.view = view;
        }
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimized != self.minimized {
            self.minimized = minimized;
            self.send(Command::Paused(self.paused || minimized));
        }
        self.header(ctx);
        self.navigation(ctx);
        self.inspector(ctx);
        self.archive_dialogs(ctx);
        egui::TopBottomPanel::bottom("footer")
            .exact_height(30.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(20, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::muted(if self.view.message.is_empty() {
                            format!(
                                "Last sample: {} · real-time updates",
                                widgets::time(self.view.received_ms)
                            )
                        } else {
                            self.view.message.clone()
                        })
                        .size(11.0),
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(24))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.panel.title()).size(28.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if matches!(self.panel, Panel::Overview | Panel::Resources) {
                            egui::ComboBox::from_id_salt("chart-range")
                                .selected_text(if self.seconds == 60 {
                                    "Last minute"
                                } else {
                                    "Last 5 minutes"
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.seconds, 60, "Last minute");
                                    ui.selectable_value(&mut self.seconds, 300, "Last 5 minutes");
                                });
                        }
                    });
                });
                ui.add_space(16.0);
                egui::ScrollArea::vertical()
                    .id_salt(self.panel.title())
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.panel {
                        Panel::Overview => self.overview(ui),
                        Panel::Resources => self.resources(ui),
                        Panel::Events => self.events(ui, false),
                        Panel::Timeline => self.timeline(ui),
                        Panel::Operations => self.operations(ui),
                        Panel::State => self.state_panel(ui),
                        Panel::Filesystem => self.events(ui, true),
                        Panel::Recordings => self.recordings(ui),
                    });
            });
    }
}

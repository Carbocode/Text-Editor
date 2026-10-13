use crate::theme;
use eframe::egui;
use native_workspace_devtools_protocol::{Kind, Outcome, Record, ResourceSample};

pub fn bytes(value: u64) -> String {
    let n = value as f64;
    if value >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", n / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024 * 1024 {
        format!("{:.1} MiB", n / (1024.0 * 1024.0))
    } else if value >= 1024 {
        format!("{:.1} KiB", n / 1024.0)
    } else {
        format!("{value} B")
    }
}
pub fn time(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms.min(i64::MAX as u64) as i64)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}
pub fn frontend(value: &str) -> &str {
    match value {
        "swiftui" => "macOS editor",
        "gtk" => "GNOME editor",
        "qt" => "Qt editor",
        "synthetic" => "Demo dataset",
        other => other,
    }
}
pub fn role(value: &str) -> &str {
    match value {
        "editor" => "Editor",
        "collector" => "Diagnostic service",
        "console" => "Console",
        other => other,
    }
}
pub fn name(value: &str) -> String {
    let text = match value {
        "command.search_input" | "search.input_changed" => "Search input",
        "search.queued" => "Search queued",
        "search.debounce" => "Debounce wait",
        "search.execute" => "Search execution",
        "search.published" => "Results published",
        "search.delivered" | "search.results_applied" => "Results received",
        "search.superseded" => "Search superseded by new input",
        "search.stale_generation" => "Stale search result",
        "command.document_open" | "document.opened" => "Document opened",
        "command.document_close" | "document.closed" => "Document closed",
        "command.document_change" => "Text changed",
        "document.buffer_bytes" => "Text size",
        "document.loaded_bytes" => "Document read from disk",
        "command.document_save" | "document.saved" => "Document saved",
        "command.workspace_open" | "workspace.opened" => "Workspace opened",
        "command.workspace_refresh" | "workspace.refreshed" => "Workspace refreshed",
        "command.entry_rename" | "filesystem.entry_renamed" => "File renamed",
        "command.entry_move" | "command.entry_move_optimistic" | "filesystem.entry_moved" => {
            "File moved"
        }
        "command.entry_create" => "File or directory created",
        "command.document_reload" => "Reloaded from disk",
        "command.document_overwrite" => "File overwritten",
        "snapshot.apply" => "Interface updated",
        "session.start" => "Session started",
        "session.end" => "Session ended",
        "collection.started" => "Collection resumed",
        "collection.stopped" => "Collection stopped",
        "producer.queue_full" => "Events lost: queue full",
        "producer.unconfirmed_expired" => "Unconfirmed data expired",
        "error.save_conflict" => "Conflict between disk and local edits",
        "error.filesystem" => "Filesystem error",
        "error.storage" => "Storage error",
        "error.unsaved_changes" => "Unsaved changes",
        "error.validation" => "Command rejected",
        "watcher.batch" | "watcher.aggregated" => "External changes detected",
        "reconcile.watcher" => "Scan after external change",
        "reconcile.periodic" => "Periodic scan",
        "reconcile.fallback" => "Fallback scan",
        "marker.user" => "Marker",
        _ => value,
    };
    text.into()
}
pub fn outcome(outcome: Outcome) -> (&'static str, egui::Color32) {
    match outcome {
        Outcome::Ok => ("Succeeded", theme::GREEN),
        Outcome::Error => ("Error", theme::RED),
        Outcome::Cancelled => ("Cancelled", theme::AMBER),
        Outcome::Discarded => ("Discarded", theme::AMBER),
        Outcome::Noop => ("No change", theme::MUTED),
        Outcome::Incomplete => ("Incomplete", theme::AMBER),
    }
}
pub fn component(record: &Record) -> &str {
    if record.component == "frontend" {
        "Interface"
    } else if record.name.starts_with("search.") {
        "Search"
    } else if record.name.starts_with("watcher.")
        || record.name.starts_with("reconcile.")
        || record.name.starts_with("filesystem.")
    {
        "Filesystem"
    } else if record.component == "producer" {
        "Collection"
    } else {
        "Application"
    }
}
pub fn meaningful(record: &Record) -> bool {
    record.kind != Kind::SpanStart
        && !record.name.starts_with("state.")
        && !matches!(
            record.name.as_str(),
            "command.search_poll" | "command.workspace_poll"
        )
        && record.outcome != Outcome::Noop
}
pub fn dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, color);
}
pub fn metric(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    value: &str,
    subtitle: &str,
    color: egui::Color32,
) {
    theme::card().show(ui, |ui| {
        ui.set_min_width(width - 36.0);
        ui.set_max_width(width - 36.0);
        ui.vertical(|ui| {
            ui.add(egui::Label::new(theme::muted(title)).wrap());
            ui.add_space(3.0);
            ui.label(egui::RichText::new(value).size(27.0).strong().color(color));
            ui.add(egui::Label::new(theme::muted(subtitle).size(12.0)).wrap());
        });
    });
}
#[allow(clippy::too_many_arguments)]
pub fn chart(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    samples: &[ResourceSample],
    pid: u32,
    seconds: u64,
    cpu: bool,
    color: egui::Color32,
) {
    theme::card().show(ui, |ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.label(theme::muted(subtitle).size(12.0));
        let points: Vec<_> = samples
            .iter()
            .filter(|s| s.pid == pid && s.alive)
            .map(|s| {
                (
                    s.unix_ms.0,
                    if cpu {
                        s.cpu_percent as f64
                    } else {
                        s.memory_bytes.0 as f64 / 1048576.0
                    },
                )
            })
            .collect();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 160.0),
            egui::Sense::hover(),
        );
        let plot = egui::Rect::from_min_max(
            rect.min + egui::vec2(44.0, 10.0),
            rect.max - egui::vec2(8.0, 25.0),
        );
        let end = points.last().map_or(0, |(time, _)| *time);
        let start = end.saturating_sub(seconds * 1000);
        let max = points
            .iter()
            .filter(|(t, _)| *t >= start)
            .map(|(_, v)| *v)
            .fold(if cpu { 10.0 } else { 16.0 }, f64::max)
            * 1.1;
        for i in 0..=2 {
            let y = plot.bottom() - plot.height() * i as f32 / 2.0;
            ui.painter().line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                egui::Stroke::new(1.0_f32, theme::BORDER),
            );
            ui.painter().text(
                egui::pos2(rect.left(), y),
                egui::Align2::LEFT_CENTER,
                format!("{:.0}{}", max * i as f64 / 2.0, if cpu { "%" } else { "" }),
                egui::FontId::monospace(10.0),
                theme::MUTED,
            );
        }
        ui.painter().text(
            egui::pos2(plot.left(), rect.bottom()),
            egui::Align2::LEFT_BOTTOM,
            format!("−{seconds}s"),
            egui::FontId::proportional(11.0),
            theme::MUTED,
        );
        ui.painter().text(
            egui::pos2(plot.right(), rect.bottom()),
            egui::Align2::RIGHT_BOTTOM,
            "now",
            egui::FontId::proportional(11.0),
            theme::MUTED,
        );
        let mut previous = None;
        for (t, value) in points.iter().filter(|(t, _)| *t >= start) {
            let p = egui::pos2(
                plot.left()
                    + plot.width() * ((*t - start) as f64 / (seconds * 1000).max(1) as f64) as f32,
                plot.bottom() - plot.height() * (*value / max) as f32,
            );
            if let Some((last_time, last_point)) = previous
                && t.saturating_sub(last_time) <= 2500
            {
                ui.painter()
                    .line_segment([last_point, p], egui::Stroke::new(2.0_f32, color));
            }
            previous = Some((*t, p));
        }
        if points.is_empty() {
            ui.painter().text(
                plot.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for samples…",
                egui::FontId::proportional(14.0),
                theme::MUTED,
            );
        }
        if let Some(pos) = response.hover_pos() {
            let target = start
                + (((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64
                    * (seconds * 1000) as f64) as u64;
            if let Some((t, value)) = points.iter().min_by_key(|(t, _)| t.abs_diff(target)) {
                response.on_hover_text(format!(
                    "{} · {:.2} {}",
                    time(*t),
                    value,
                    if cpu { "% CPU" } else { "MiB" }
                ));
            }
        }
    });
}

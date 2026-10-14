use crate::{theme, widgets};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind};
use native_workspace_devtools_protocol::{Kind, Outcome, Record, Uuid};
use std::collections::HashMap;

const ACTORS: [&str; 5] = [
    "Application",
    "Search worker",
    "Filesystem",
    "Interface",
    "Collection",
];
const COLORS: [Color32; 5] = [
    theme::ACCENT,
    theme::GREEN,
    theme::AMBER,
    Color32::from_rgb(183, 151, 244),
    theme::MUTED,
];
const LABEL: f32 = 148.0;
const TRACK: f32 = 28.0;

// Logical actors, not an inventory of OS threads. Classification is independent
// of operation IDs and record order, so a new operation never creates a lane.
fn actor(record: &Record) -> usize {
    if record.thread == "ui" || record.name.starts_with("snapshot.") {
        3
    } else if record.thread == "search-worker" {
        1
    } else if record.name == "drop.filesystem"
        || ["filesystem.", "watcher.", "reconcile."]
            .iter()
            .any(|prefix| record.name.starts_with(prefix))
    {
        2
    } else if record.component == "producer" || record.thread.starts_with("devtools") {
        4
    } else {
        0
    }
}

const TYPES: [&str; 6] = [
    "Input",
    "Rendering",
    "Commands",
    "Filesystem",
    "Search",
    "Collection",
];
const TYPE_COLORS: [Color32; 6] = [
    theme::ACCENT,
    Color32::from_rgb(183, 151, 244),
    theme::GREEN,
    theme::AMBER,
    Color32::from_rgb(98, 195, 225),
    theme::MUTED,
];

fn event_type(record: &Record) -> usize {
    let name = record.name.as_str();
    if name == "drop.filesystem"
        || name == "drop.dispatch_wait"
        || name == "drop.confirmation"
        || actor(record) == 2
    {
        3
    } else if name == "drop.sidebar_ready"
        || name == "drop.ui_delay"
        || name.starts_with("ui.")
        || name.starts_with("snapshot.")
    {
        1
    } else if name.starts_with("drop.") {
        0
    } else if actor(record) == 1 || name.starts_with("search.") {
        4
    } else if actor(record) == 4 {
        5
    } else {
        2
    }
}
// A heartbeat reports an interval ending at the sampling timestamp, rather
// than an instantaneous marker. Snapshot application also records elapsed
// work ending at emission. The drop-local sample is only contextual.
fn activity_start(record: &Record) -> u64 {
    if matches!(
        record.name.as_str(),
        "ui.event_loop_delay" | "snapshot.apply"
    ) {
        record.monotonic_ns.0.saturating_sub(record.value.0)
    } else {
        record.monotonic_ns.0
    }
}

struct Activity<'a> {
    record: &'a Record,
    end: Option<&'a Record>,
    track: usize,
}
impl Activity<'_> {
    fn start(&self) -> u64 {
        activity_start(self.record)
    }
    fn is_interval(&self) -> bool {
        self.record.kind == Kind::SpanStart
            || matches!(
                self.record.name.as_str(),
                "ui.event_loop_delay" | "snapshot.apply"
            )
    }
    fn finish(&self, latest: u64) -> u64 {
        if self.record.kind == Kind::SpanStart {
            self.end
                .map_or(latest, |end| end.monotonic_ns.0)
                .max(self.record.monotonic_ns.0)
        } else {
            self.record.monotonic_ns.0
        }
    }
    fn outcome(&self) -> Outcome {
        self.end.map_or_else(
            || {
                if self.record.kind == Kind::SpanStart {
                    Outcome::Incomplete
                } else {
                    self.record.outcome
                }
            },
            |end| end.outcome,
        )
    }
}

fn lanes(records: &[Record]) -> [Vec<Activity<'_>>; 5] {
    let starts: HashMap<_, _> = records
        .iter()
        .filter(|r| r.kind == Kind::SpanStart)
        .map(|r| (r.span.0, r))
        .collect();
    let ends: HashMap<_, _> = records
        .iter()
        .filter(|r| r.kind == Kind::SpanEnd)
        .map(|r| (r.span.0, r))
        .collect();
    let latest = records.iter().map(|r| r.monotonic_ns.0).max().unwrap_or(0);
    let mut lanes: [Vec<Activity<'_>>; 5] = std::array::from_fn(|_| Vec::new());
    for record in records {
        if record.kind == Kind::SpanEnd && starts.contains_key(&record.span.0) {
            continue;
        }
        // State snapshots and no-op timer polls have a dedicated state view.
        if record.name.starts_with("state.")
            || (record.outcome == Outcome::Noop && record.kind != Kind::SpanStart)
        {
            continue;
        }
        lanes[actor(record)].push(Activity {
            record,
            end: ends
                .get(&record.span.0)
                .copied()
                .filter(|_| record.kind == Kind::SpanStart),
            track: 0,
        });
    }
    for lane in &mut lanes {
        lane.sort_by_key(|activity| (activity.start(), activity.record.sequence.0));
        let mut occupied = Vec::new();
        for activity in lane {
            if !activity.is_interval() {
                continue;
            }
            let start = activity.start();
            let track = occupied
                .iter()
                .position(|&end| end <= start)
                .unwrap_or(occupied.len());
            let finish = activity.finish(latest).max(start.saturating_add(1));
            if track == occupied.len() {
                occupied.push(finish);
            } else {
                occupied[track] = finish;
            }
            activity.track = track;
        }
    }
    lanes
}

#[derive(Default)]
pub struct Timeline {
    session: Option<Uuid>,
    window: Option<(f64, f64)>,
    selection: Option<(f64, f64)>,
    drag_start: Option<f64>,
    drag_window: Option<(f64, f64)>,
    follow: bool,
    hidden_types: [bool; 6],
}

impl Timeline {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        session: Option<Uuid>,
        records: &[Record],
        filter: &str,
        operation: Option<u64>,
        errors_only: bool,
        internal: bool,
    ) -> Option<Record> {
        if self.session != session {
            *self = Self {
                session,
                follow: true,
                hidden_types: self.hidden_types,
                ..Self::default()
            };
        }
        let min = records.iter().map(activity_start).min().unwrap_or(0) as f64;
        let max = records.iter().map(|r| r.monotonic_ns.0).max().unwrap_or(0) as f64;
        let extent = (max - min).max(1_000_000.0);
        let bounds = (min - extent * 0.03, max.max(min + extent) + extent * 0.03);
        let (mut left, mut right) = self.window.unwrap_or(bounds);
        if self.follow {
            let width = right - left;
            right = bounds.1;
            left = right - width;
            if self.window.is_none() {
                left = bounds.0;
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Fit").clicked() {
                left = bounds.0;
                right = bounds.1;
                self.follow = true;
            }
            for (label, factor) in [("−", 2.0), ("+", 0.5)] {
                if ui.button(label).on_hover_text("Time zoom").clicked() {
                    let center = (left + right) * 0.5;
                    let half = ((right - left) * factor * 0.5).max(500.0);
                    left = center - half;
                    right = center + half;
                    self.follow = false;
                }
            }
            ui.checkbox(&mut self.follow, "Follow time");
            ui.label(theme::muted(format!("Window: {}", duration(right - left))));
            if let Some((a, b)) = self.selection {
                ui.label(format!("Selection: {}", duration((b - a).abs())));
                if ui.small_button("Clear selection").clicked() {
                    self.selection = None;
                }
            }
        });
        ui.label(
            theme::muted(
                "Scroll: zoom · Shift + drag: pan · Drag: measure an interval · Click: details",
            )
            .size(11.0),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("Event types:");
            for (index, label) in TYPES.iter().enumerate() {
                if ui
                    .selectable_label(
                        !self.hidden_types[index],
                        egui::RichText::new(*label).color(TYPE_COLORS[index]),
                    )
                    .clicked()
                {
                    self.hidden_types[index] = !self.hidden_types[index];
                }
            }
            if ui.small_button("All").clicked() {
                self.hidden_types.fill(false);
            }
        });
        ui.add_space(8.0);
        let lanes = lanes(records);
        let filter = filter.to_lowercase();
        let matches = |activity: &Activity<'_>| {
            let record = activity.record;
            !self.hidden_types[event_type(record)]
                && operation.is_none_or(|id| id == record.operation.0)
                && (internal
                    || (activity.outcome() != Outcome::Noop
                        && !matches!(
                            record.name.as_str(),
                            "command.search_poll" | "command.workspace_poll"
                        )))
                && (!errors_only || activity.outcome() == Outcome::Error)
                && (filter.is_empty()
                    || widgets::name(&record.name).to_lowercase().contains(&filter)
                    || record.name.to_lowercase().contains(&filter)
                    || ACTORS[actor(record)].to_lowercase().contains(&filter))
        };
        let mut selected = None;
        theme::card().inner_margin(12).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("actor_timeline")
                .show(ui, |ui| {
                    let heights: Vec<f32> = lanes
                        .iter()
                        .map(|lane| {
                            let tracks = lane
                                .iter()
                                .filter(|a| matches(a))
                                .map(|a| a.track + 1)
                                .max()
                                .unwrap_or(1);
                            34.0 + tracks as f32 * TRACK
                        })
                        .collect();
                    let height = 42.0 + heights.iter().sum::<f32>();
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(300.0), height),
                        Sense::click_and_drag(),
                    );
                    let plot =
                        Rect::from_min_max(egui::pos2(rect.left() + LABEL, rect.top()), rect.max);
                    let (draw_left, draw_right) = (left, right);
                    let x_at = |time: f64| {
                        plot.left()
                            + ((time - draw_left) / (draw_right - draw_left)) as f32 * plot.width()
                    };
                    let time_at = |x: f32| {
                        draw_left
                            + ((x - plot.left()) / plot.width()) as f64 * (draw_right - draw_left)
                    };
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 6.0, theme::BG);
                    let raw_step = (right - left) / (plot.width() / 100.0).max(2.0) as f64;
                    let base = 10.0_f64.powf(raw_step.log10().floor());
                    let step = [1.0, 2.0, 5.0, 10.0]
                        .into_iter()
                        .map(|n| base * n)
                        .find(|n| *n >= raw_step)
                        .unwrap_or(base * 10.0);
                    let mut tick = (left.max(0.0) / step).ceil() * step;
                    while tick < right {
                        let x = x_at(tick);
                        painter.line_segment(
                            [
                                egui::pos2(x, rect.top() + 32.0),
                                egui::pos2(x, rect.bottom()),
                            ],
                            Stroke::new(1.0_f32, theme::BORDER.gamma_multiply(0.6)),
                        );
                        painter.text(
                            egui::pos2(x + 3.0, rect.top() + 14.0),
                            Align2::LEFT_CENTER,
                            format!(
                                "{:.*} s",
                                if step < 1_000.0 {
                                    9
                                } else if step < 1_000_000.0 {
                                    6
                                } else {
                                    3
                                },
                                tick / 1e9
                            ),
                            FontId::monospace(11.0),
                            theme::MUTED,
                        );
                        tick += step;
                    }
                    painter.text(
                        egui::pos2(rect.left() + 10.0, rect.top() + 14.0),
                        Align2::LEFT_CENTER,
                        "ACTOR",
                        FontId::proportional(11.0),
                        theme::MUTED,
                    );
                    let mut top = rect.top() + 42.0;
                    for (index, lane) in lanes.iter().enumerate() {
                        let bottom = top + heights[index];
                        let lane_rect = Rect::from_min_max(
                            egui::pos2(rect.left(), top),
                            egui::pos2(rect.right(), bottom),
                        );
                        if index % 2 == 0 {
                            painter.rect_filled(lane_rect, 0.0, theme::PANEL.gamma_multiply(0.45));
                        }
                        painter.line_segment(
                            [lane_rect.left_bottom(), lane_rect.right_bottom()],
                            Stroke::new(1.0_f32, theme::BORDER),
                        );
                        painter.circle_filled(
                            egui::pos2(rect.left() + 12.0, top + 22.0),
                            3.5,
                            COLORS[index],
                        );
                        painter.text(
                            egui::pos2(rect.left() + 23.0, top + 22.0),
                            Align2::LEFT_CENTER,
                            ACTORS[index],
                            FontId::proportional(13.0),
                            theme::TEXT,
                        );
                        let count = lane.iter().filter(|a| matches(a)).count();
                        painter.text(
                            egui::pos2(rect.left() + 23.0, top + 42.0),
                            Align2::LEFT_CENTER,
                            format!("{count} activities"),
                            FontId::proportional(11.0),
                            theme::MUTED,
                        );
                        let lane_plot = Rect::from_min_max(
                            egui::pos2(plot.left(), top),
                            egui::pos2(plot.right(), bottom),
                        );
                        let ink = painter.with_clip_rect(lane_plot.intersect(ui.clip_rect()));
                        for activity in lane.iter().filter(|a| matches(a)) {
                            let start = activity.start() as f64;
                            let finish = activity.finish(max as u64) as f64;
                            if finish < left || start > right {
                                continue;
                            }
                            let color = match activity.outcome() {
                                Outcome::Error => theme::RED,
                                Outcome::Cancelled | Outcome::Discarded => theme::AMBER,
                                Outcome::Incomplete => theme::MUTED,
                                _ => TYPE_COLORS[event_type(activity.record)],
                            };
                            let y = top + 12.0 + activity.track as f32 * TRACK;
                            let is_span = activity.is_interval();
                            let shape = if is_span {
                                Rect::from_min_max(
                                    egui::pos2(x_at(start).max(plot.left()), y),
                                    egui::pos2(
                                        x_at(finish).max(x_at(start) + 4.0).min(plot.right()),
                                        y + 22.0,
                                    ),
                                )
                            } else {
                                Rect::from_center_size(
                                    egui::pos2(x_at(start), top + heights[index] - 10.0),
                                    egui::vec2(9.0, 12.0),
                                )
                                .intersect(lane_plot)
                            };
                            if !shape.is_positive() {
                                continue;
                            }
                            if is_span {
                                ink.rect_filled(shape, 4.0, color.gamma_multiply(0.26));
                                ink.rect_stroke(
                                    shape,
                                    4.0,
                                    Stroke::new(1.0_f32, color),
                                    StrokeKind::Inside,
                                );
                                if activity.record.kind == Kind::SpanStart && activity.end.is_none() {
                                    let mut x = shape.left();
                                    while x < shape.right() {
                                        ink.line_segment(
                                            [
                                                egui::pos2(x, shape.bottom()),
                                                egui::pos2(
                                                    (x + 8.0).min(shape.right()),
                                                    shape.top(),
                                                ),
                                            ],
                                            Stroke::new(1.0_f32, color.gamma_multiply(0.3)),
                                        );
                                        x += 10.0;
                                    }
                                }
                                if shape.width() > 60.0 {
                                    ink.with_clip_rect(shape.shrink(3.0)).text(
                                        shape.left_center() + egui::vec2(5.0, 0.0),
                                        Align2::LEFT_CENTER,
                                        widgets::name(&activity.record.name),
                                        FontId::proportional(11.0),
                                        theme::TEXT,
                                    );
                                }
                            } else {
                                ink.line_segment(
                                    [shape.center_top(), shape.center_bottom()],
                                    Stroke::new(2.0_f32, color),
                                );
                                ink.circle_filled(shape.center(), 3.0, color);
                            }
                            let item = ui.interact(
                                shape.expand(2.0).intersect(lane_plot),
                                ui.id().with(("activity", activity.record.sequence.0)),
                                Sense::click(),
                            );
                            if item.hovered() {
                                ink.rect_stroke(
                                    shape.expand(1.0),
                                    4.0,
                                    Stroke::new(1.5_f32, theme::TEXT),
                                    StrokeKind::Outside,
                                );
                            }
                            if item.clicked() {
                                selected = Some(activity.end.unwrap_or(activity.record).clone());
                            }
                            item.on_hover_ui(|ui| {
                                ui.strong(widgets::name(&activity.record.name));
                                ui.label(format!("{} · {}", ACTORS[index], TYPES[event_type(activity.record)]));
                                ui.label(format!("Operation: {}", activity.record.operation.0));
                                let (label, color) = widgets::outcome(activity.outcome());
                                ui.colored_label(color, label);
                                ui.label(format!("Start: {:.6} s", start / 1e9));
                                if is_span {
                                    ui.label(if activity.end.is_some() || activity.record.kind != Kind::SpanStart {
                                        format!("Duration: {}", duration(finish - start))
                                    } else {
                                        "End is not present in the current page".into()
                                    });
                                } else if let Some(value) = widgets::duration_value(activity.record) {
                                    ui.label(format!("Measured duration: {}", duration(value as f64)));
                                }
                                if activity.record.name == "ui.event_loop_delay" { ui.label("Sampled event-loop lateness; not a physical input timestamp."); }
                            });
                        }
                        top = bottom;
                    }
                    if let Some((a, b)) = self.selection {
                        let selection = Rect::from_min_max(
                            egui::pos2(x_at(a.min(b)).max(plot.left()), rect.top() + 32.0),
                            egui::pos2(x_at(a.max(b)).min(plot.right()), rect.bottom()),
                        );
                        if selection.is_positive() {
                            painter.rect_filled(selection, 0.0, theme::ACCENT.gamma_multiply(0.12));
                            for x in [selection.left(), selection.right()] {
                                painter.line_segment(
                                    [
                                        egui::pos2(x, selection.top()),
                                        egui::pos2(x, selection.bottom()),
                                    ],
                                    Stroke::new(1.0_f32, theme::ACCENT),
                                );
                            }
                        }
                    }
                    if ui.rect_contains_pointer(plot)
                        && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
                        && plot.contains(pos)
                    {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll.abs() > 0.1 {
                            let anchor = time_at(pos.x);
                            let factor = (-scroll as f64 * 0.005).exp().clamp(0.25, 4.0);
                            let width = ((right - left) * factor).clamp(1_000.0, extent * 100.0);
                            let fraction = (anchor - left) / (right - left);
                            left = anchor - width * fraction;
                            right = left + width;
                            self.follow = false;
                        }
                    }
                    if response.drag_started()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        self.drag_start = Some(time_at(pos.x.clamp(plot.left(), plot.right())));
                        self.drag_window = Some((left, right));
                    }
                    if response.dragged()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        if ui.input(|i| i.modifiers.shift) {
                            if let Some((a, b)) = self.drag_window {
                                let delta = response.total_drag_delta().unwrap_or_default().x
                                    as f64
                                    / plot.width() as f64
                                    * (b - a);
                                left = a - delta;
                                right = b - delta;
                                self.follow = false;
                            }
                        } else if let Some(start) = self.drag_start {
                            self.selection =
                                Some((start, time_at(pos.x.clamp(plot.left(), plot.right()))));
                        }
                    }
                    if response.drag_stopped() {
                        self.drag_start = None;
                        self.drag_window = None;
                    }
                });
        });
        self.window = Some((left, right));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for (label, color) in [
                ("Completed", theme::ACCENT),
                ("Cancelled / discarded", theme::AMBER),
                ("Error", theme::RED),
                ("Missing end", theme::MUTED),
            ] {
                ui.colored_label(color, label);
            }
        });
        ui.label(theme::muted("Logical application actors · overlaps within each lane · current page data (up to 500 records)").size(11.0));
        selected
    }
}

fn duration(ns: f64) -> String {
    if ns >= 1e9 {
        format!("{:.2} s", ns / 1e9)
    } else if ns >= 1e6 {
        format!("{:.2} ms", ns / 1e6)
    } else {
        format!("{:.1} µs", ns / 1e3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use native_workspace_devtools_protocol::Counter;

    fn record(sequence: u64, time: u64, span: u64, kind: Kind) -> Record {
        Record {
            sequence: Counter(sequence),
            monotonic_ns: Counter(time),
            component: "application".into(),
            thread: "dispatch".into(),
            name: "command.open_document".into(),
            kind,
            outcome: Outcome::Ok,
            operation: Counter(span),
            span: Counter(span),
            parent_span: Counter(0),
            generation: Counter(0),
            revision: Counter(0),
            value: Counter(0),
        }
    }

    #[test]
    fn operations_reuse_actor_lane_and_only_overlap_uses_internal_tracks() {
        let records = vec![
            record(1, 10, 1, Kind::SpanStart),
            record(2, 15, 2, Kind::SpanStart),
            record(3, 20, 1, Kind::SpanEnd),
            record(4, 25, 2, Kind::SpanEnd),
            record(5, 30, 3, Kind::SpanStart),
            record(6, 40, 3, Kind::SpanEnd),
        ];
        let lanes = lanes(&records);
        assert_eq!(lanes.len(), 5);
        assert_eq!(lanes[0].len(), 3);
        assert_eq!(
            lanes[0].iter().map(|a| a.track).collect::<Vec<_>>(),
            [0, 1, 0]
        );
        assert!(lanes[1..].iter().all(Vec::is_empty));
    }

    #[test]
    fn missing_end_remains_incomplete_and_orphan_end_remains_visible() {
        let records = vec![
            record(1, 10, 1, Kind::SpanStart),
            record(2, 20, 2, Kind::SpanEnd),
        ];
        let lanes = lanes(&records);
        assert_eq!(lanes[0].len(), 2);
        assert_eq!(lanes[0][0].outcome(), Outcome::Incomplete);
        assert!(lanes[0][0].end.is_none());
        assert_eq!(lanes[0][1].record.kind, Kind::SpanEnd);
    }
    #[test]
    fn sampled_stall_occupies_its_actual_interval_and_rendering_type() {
        for (name, elapsed) in [
            ("ui.event_loop_delay", 1_000_000_000),
            ("snapshot.apply", 40_000_000),
        ] {
            let mut sample = record(1, 2_000_000_000, 0, Kind::Event);
            sample.name = name.into();
            sample.thread = "ui".into();
            sample.value = Counter(elapsed);
            let records = [sample];
            let lanes = lanes(&records);
            let activity = &lanes[3][0];
            assert!(activity.is_interval());
            assert_eq!(activity.start(), 2_000_000_000 - elapsed);
            assert_eq!(activity.finish(2_000_000_000), 2_000_000_000);
            assert_eq!(event_type(activity.record), 1);
        }
    }

    #[test]
    fn drop_phases_share_operation_but_have_independent_event_types() {
        let mut phase = record(1, 0, 42, Kind::SpanStart);
        for (name, category) in [
            ("drop.handler", 0),
            ("drop.preview_hidden", 0),
            ("drop.sidebar_ready", 1),
            ("drop.dispatch_wait", 3),
            ("drop.filesystem", 3),
            ("drop.confirmation", 3),
        ] {
            phase.name = name.into();
            assert_eq!(event_type(&phase), category);
            assert_eq!(phase.operation.0, 42);
        }
    }
}

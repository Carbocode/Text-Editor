use gtk::prelude::*;
use std::{cell::RefCell, rc::Rc};

pub fn wrap(editor: &gtk::TextView, scroll: &gtk::ScrolledWindow) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let gutter = gtk::DrawingArea::new();
    gutter.set_content_width(48);
    gutter.set_hexpand(false);
    let minimap = gtk::DrawingArea::new();
    minimap.set_content_width(80);
    minimap.set_hexpand(false);

    let view = editor.clone();
    gutter.set_draw_func(move |_, context, _, height| {
        let visible = view.visible_rect();
        let first = view
            .iter_at_location(0, visible.y())
            .map_or(0, |iter| iter.line());
        let buffer = view.buffer();
        context.set_source_rgb(0.5, 0.5, 0.5);
        context.select_font_face(
            "monospace",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        context.set_font_size(11.0);
        for line in first..buffer.line_count() {
            let Some(iter) = buffer.iter_at_line(line) else {
                break;
            };
            let (y, line_height) = view.line_yrange(&iter);
            let draw_y = y - visible.y();
            if draw_y > height {
                break;
            }
            let label = (line + 1).to_string();
            let width = context
                .text_extents(&label)
                .map_or(0.0, |extents| extents.width());
            context.move_to(42.0 - width, f64::from(draw_y + line_height - 3));
            let _ = context.show_text(&label);
        }
    });

    let buffer = editor.buffer();
    let adjustment = scroll.vadjustment();
    let adjustment_draw = adjustment.clone();
    let line_lengths = Rc::new(RefCell::new(vec![0]));
    let lengths_draw = line_lengths.clone();
    minimap.set_draw_func(move |_, context, width, height| {
        let lengths = lengths_draw.borrow();
        let count = lengths.len().max(1);
        let stride = (count / (height.max(1) as usize / 2).max(1)).max(1);
        context.set_source_rgba(0.5, 0.5, 0.5, 0.45);
        for index in (0..lengths.len()).step_by(stride) {
            let y = index as f64 / count as f64 * f64::from(height);
            context.rectangle(
                4.0,
                y,
                (lengths[index] as f64 * 1.2).min(f64::from(width - 8)),
                1.0,
            );
        }
        let _ = context.fill();
        let total = adjustment_draw.upper().max(1.0);
        context.set_source_rgba(0.5, 0.5, 0.5, 0.3);
        context.rectangle(
            0.0,
            adjustment_draw.value() / total * f64::from(height),
            f64::from(width),
            (adjustment_draw.page_size() / total * f64::from(height)).max(8.0),
        );
        let _ = context.fill();
    });
    let click = gtk::GestureClick::new();
    let scroll_to = adjustment.clone();
    let map_for_click = minimap.clone();
    click.connect_pressed(move |_, _, _, y| {
        let height = map_for_click.height().max(1) as f64;
        let target = y / height * scroll_to.upper() - scroll_to.page_size() / 2.0;
        scroll_to
            .set_value(target.clamp(0.0, (scroll_to.upper() - scroll_to.page_size()).max(0.0)));
    });
    minimap.add_controller(click);
    let redraw_gutter = gutter.clone();
    let redraw_map = minimap.clone();
    adjustment.connect_value_changed(move |_| {
        redraw_gutter.queue_draw();
        redraw_map.queue_draw();
    });
    let redraw_gutter = gutter.clone();
    let redraw_map = minimap.clone();
    let lengths_update = line_lengths.clone();
    buffer.connect_changed(move |buffer| {
        let (start, end) = buffer.bounds();
        let content = buffer.text(&start, &end, false);
        lengths_update.replace(
            content
                .split('\n')
                .map(|line| line.chars().count())
                .collect(),
        );
        redraw_gutter.queue_draw();
        redraw_map.queue_draw();
    });
    row.append(&gutter);
    scroll.set_hexpand(true);
    row.append(scroll);
    row.append(&minimap);
    row
}

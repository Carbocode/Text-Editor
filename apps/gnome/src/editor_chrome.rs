use gtk::prelude::*;
use native_workspace_syntax::{highlight, minimap_glyph_alpha};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn scroll_to_map_position(
    y: f64,
    line_count: usize,
    map_offset: f64,
    adjustment: &gtk::Adjustment,
) {
    let content_height = (line_count * 2).max(1) as f64;
    let target =
        (y + map_offset) / content_height * adjustment.upper() - adjustment.page_size() / 2.0;
    adjustment.set_value(target.clamp(0.0, (adjustment.upper() - adjustment.page_size()).max(0.0)));
}

fn minimap_offset(
    map_height: i32,
    line_count: usize,
    scroll_top: f64,
    scroll_height: f64,
    page_height: f64,
) -> f64 {
    let content_height = (line_count * 2) as f64;
    let total = scroll_height.max(1.0);
    let viewport_top = scroll_top / total * content_height;
    let viewport_height = page_height / total * content_height;
    (viewport_top - (f64::from(map_height) - viewport_height) / 2.0)
        .clamp(0.0, (content_height - f64::from(map_height)).max(0.0))
        .floor()
}

#[derive(Default)]
struct MinimapModel {
    lines: Vec<Vec<(usize, u8, u8, u8)>>,
}

impl MinimapModel {
    fn update(&mut self, text: &str, file_name: &str) {
        let mut kinds = vec![0; text.chars().count()];
        for token in highlight(file_name, text) {
            kinds[token.start_char..token.end_char].fill(token.kind.code());
        }
        self.lines.clear();
        let mut offset = 0;
        for line in text.split('\n') {
            let mut cells = Vec::new();
            let mut column = 0;
            for (index, character) in line.chars().enumerate() {
                if column >= 92 {
                    break;
                }
                if character == '\t' {
                    column += 4 - column % 4;
                } else {
                    if !character.is_whitespace() {
                        let (top, bottom) = minimap_glyph_alpha(character);
                        cells.push((
                            column,
                            kinds.get(offset + index).copied().unwrap_or(0),
                            top,
                            bottom,
                        ));
                    }
                    column += 1;
                }
            }
            offset += line.chars().count() + 1;
            self.lines.push(cells);
        }
    }
}

#[derive(Clone)]
pub struct EditorChrome {
    pub row: gtk::Box,
    file_name: Rc<RefCell<String>>,
    model: Rc<RefCell<MinimapModel>>,
    buffer: gtk::TextBuffer,
    minimap: gtk::DrawingArea,
    cache: MinimapCache,
}

type MinimapCache = Rc<RefCell<Option<(i32, i32, i32, gtk::cairo::ImageSurface)>>>;

impl EditorChrome {
    pub fn set_file_name(&self, name: &str) {
        if self.file_name.borrow().as_str() == name {
            return;
        }
        let previous = self.file_name.replace(name.to_owned());
        if previous.rsplit('/').next() == name.rsplit('/').next() {
            return;
        }
        let (start, end) = self.buffer.bounds();
        self.model
            .borrow_mut()
            .update(&self.buffer.text(&start, &end, false), name);
        self.cache.replace(None);
        self.minimap.queue_draw();
    }
}

pub fn wrap(editor: &gtk::TextView, scroll: &gtk::ScrolledWindow) -> EditorChrome {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let text_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let overlay = gtk::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_child(Some(&text_row));
    let active_line = gtk::DrawingArea::new();
    active_line.set_can_target(false);
    overlay.add_overlay(&active_line);
    editor.set_hexpand(true);
    editor.set_vexpand(true);
    editor.set_bottom_margin(16);
    let view = editor.clone();
    let overlay_weak = overlay.downgrade();
    active_line.set_draw_func(move |_, context, _, _| {
        let Some(overlay) = overlay_weak.upgrade() else {
            return;
        };
        let buffer = view.buffer();
        let iter = buffer.iter_at_mark(&buffer.get_insert());
        let (y, height) = view.line_yrange(&iter);
        let (_, window_y) = view.buffer_to_window_coords(gtk::TextWindowType::Widget, 0, y);
        let Some(origin) =
            view.compute_point(&overlay, &gtk::graphene::Point::new(0.0, window_y as f32))
        else {
            return;
        };
        let Some(view_origin) = view.compute_point(&overlay, &gtk::graphene::Point::new(0.0, 0.0))
        else {
            return;
        };
        // Include the gutter, while leaving the scroller's borders and scrollbars clear.
        let width = f64::from(view_origin.x()) + f64::from(view.width());
        context.rectangle(
            0.0,
            f64::from(view_origin.y()),
            width,
            f64::from(view.height()),
        );
        context.clip();
        let color = view
            .style_context()
            .lookup_color("accent_bg_color")
            .unwrap_or_else(|| gtk::gdk::RGBA::new(0.25, 0.5, 0.9, 1.0));
        context.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            0.10,
        );
        context.rectangle(0.0, f64::from(origin.y()), width, f64::from(height));
        let _ = context.fill();
    });
    let gutter = gtk::DrawingArea::new();
    gutter.set_content_width(48);
    let click = gtk::GestureClick::new();
    click.set_button(1);
    let view = editor.clone();
    click.connect_pressed(move |_, _, _, y| {
        let (_, buffer_y) = view.window_to_buffer_coords(gtk::TextWindowType::Widget, 0, y as i32);
        let (iter, _) = view.line_at_y(buffer_y);
        view.grab_focus();
        view.buffer().place_cursor(&iter);
    });
    gutter.add_controller(click);
    let minimap = gtk::DrawingArea::new();
    minimap.set_content_width(100);

    let view = editor.clone();
    gutter.set_draw_func(move |_, context, width, height| {
        let visible = view.visible_rect();
        let first = view
            .iter_at_location(visible.x(), visible.y())
            .map_or(0, |iter| iter.line());
        let buffer = view.buffer();
        context.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
        context.clip();
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
            let (_, draw_y) = view.buffer_to_window_coords(gtk::TextWindowType::Widget, 0, y);
            if draw_y >= height {
                break;
            }
            if draw_y + line_height <= 0 {
                continue;
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
    let model = Rc::new(RefCell::new(MinimapModel::default()));
    let file_name = Rc::new(RefCell::new(String::new()));
    let model_draw = model.clone();
    let cache: MinimapCache = Rc::new(RefCell::new(None));
    let cache_draw = cache.clone();
    minimap.set_draw_func(move |_, context, width, height| {
        let count = model_draw.borrow().lines.len().max(1);
        let offset = minimap_offset(
            height,
            count,
            adjustment_draw.value(),
            adjustment_draw.upper(),
            adjustment_draw.page_size(),
        ) as i32;
        let surface = cache_draw
            .borrow()
            .as_ref()
            .filter(|(cached_width, cached_height, cached_offset, _)| {
                *cached_width == width && *cached_height == height && *cached_offset == offset
            })
            .map(|(_, _, _, surface)| surface.clone());
        let surface = surface.or_else(|| {
            let surface = gtk::cairo::ImageSurface::create(
                gtk::cairo::Format::ARgb32,
                width.max(1),
                height.max(1),
            )
            .ok()?;
            let paint = gtk::cairo::Context::new(&surface).ok()?;
            let model = model_draw.borrow();
            let first_line = (offset.max(0) as usize) / 2;
            let last_line = (offset.max(0) as usize + height.max(0) as usize)
                .div_ceil(2)
                .min(model.lines.len());
            for line in first_line..last_line {
                let y = (line * 2) as f64 - f64::from(offset);
                for (column, kind, top, bottom) in &model.lines[line] {
                    let (red, green, blue) = match kind {
                        1 => (0.67, 0.44, 0.83),
                        2 => (0.85, 0.55, 0.27),
                        3 => (0.37, 0.71, 0.41),
                        4 => (0.52, 0.56, 0.59),
                        5 => (0.34, 0.60, 0.88),
                        _ => (0.61, 0.64, 0.68),
                    };
                    for (dy, alpha) in [*top, *bottom].into_iter().enumerate() {
                        if alpha == 0 {
                            continue;
                        }
                        paint.set_source_rgba(
                            red,
                            green,
                            blue,
                            (f64::from(alpha) / 128.0).min(1.0),
                        );
                        paint.rectangle(4.0 + *column as f64, y + dy as f64, 1.0, 1.0);
                        let _ = paint.fill();
                    }
                }
            }
            cache_draw.replace(Some((width, height, offset, surface.clone())));
            Some(surface)
        });
        if let Some(surface) = surface {
            let _ = context.set_source_surface(&surface, 0.0, 0.0);
            let _ = context.paint();
        }
        let total = adjustment_draw.upper().max(1.0);
        let content_height = (count * 2) as f64;
        context.set_source_rgba(0.5, 0.5, 0.5, 0.2);
        context.rectangle(
            0.0,
            adjustment_draw.value() / total * content_height - f64::from(offset),
            f64::from(width),
            (adjustment_draw.page_size() / total * content_height).max(8.0),
        );
        let _ = context.fill();
    });
    let click = gtk::GestureClick::new();
    let scroll_to = adjustment.clone();
    let map_for_click = minimap.clone();
    let model_click = model.clone();
    click.connect_pressed(move |_, _, _, y| {
        scroll_to_map_position(
            y,
            model_click.borrow().lines.len(),
            minimap_offset(
                map_for_click.height(),
                model_click.borrow().lines.len(),
                scroll_to.value(),
                scroll_to.upper(),
                scroll_to.page_size(),
            ),
            &scroll_to,
        );
    });
    minimap.add_controller(click);
    let drag = gtk::GestureDrag::new();
    let drag_start = Rc::new(Cell::new(0.0));
    let drag_offset = Rc::new(Cell::new(0.0));
    let start = drag_start.clone();
    let offset = drag_offset.clone();
    let map_for_drag = minimap.clone();
    let model_drag = model.clone();
    let scroll_drag = adjustment.clone();
    drag.connect_drag_begin(move |_, _, y| {
        start.set(y);
        offset.set(minimap_offset(
            map_for_drag.height(),
            model_drag.borrow().lines.len(),
            scroll_drag.value(),
            scroll_drag.upper(),
            scroll_drag.page_size(),
        ));
        scroll_to_map_position(
            y,
            model_drag.borrow().lines.len(),
            offset.get(),
            &scroll_drag,
        );
    });
    let model_drag = model.clone();
    let scroll_drag = adjustment.clone();
    let offset = drag_offset.clone();
    drag.connect_drag_update(move |_, _, delta_y| {
        scroll_to_map_position(
            drag_start.get() + delta_y,
            model_drag.borrow().lines.len(),
            offset.get(),
            &scroll_drag,
        );
    });
    minimap.add_controller(drag);
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let scroll_wheel = adjustment.clone();
    wheel.connect_scroll(move |_, _, dy| {
        let delta = dy * scroll_wheel.step_increment().max(1.0) * 3.0;
        scroll_wheel.set_value((scroll_wheel.value() + delta).clamp(
            0.0,
            (scroll_wheel.upper() - scroll_wheel.page_size()).max(0.0),
        ));
        gtk::glib::Propagation::Stop
    });
    minimap.add_controller(wheel);
    let redraw_gutter = gutter.clone();
    let redraw_map = minimap.clone();
    let redraw_line = active_line.clone();
    adjustment.connect_value_changed(move |_| {
        redraw_line.queue_draw();
        redraw_gutter.queue_draw();
        redraw_map.queue_draw();
    });
    let redraw_gutter = gutter.clone();
    let redraw_map = minimap.clone();
    let redraw_line = active_line.clone();
    adjustment.connect_changed(move |_| {
        redraw_line.queue_draw();
        redraw_gutter.queue_draw();
        redraw_map.queue_draw();
    });
    let redraw_gutter = gutter.clone();
    let redraw_map = minimap.clone();
    let model_update = model.clone();
    let name_update = file_name.clone();
    let cache_update = cache.clone();
    let redraw_line = active_line.clone();
    buffer.connect_changed(move |buffer| {
        redraw_line.queue_draw();
        let (start, end) = buffer.bounds();
        let content = buffer.text(&start, &end, false);
        model_update
            .borrow_mut()
            .update(&content, &name_update.borrow());
        cache_update.replace(None);
        redraw_gutter.queue_draw();
        redraw_map.queue_draw();
    });
    let redraw_line = active_line.clone();
    buffer.connect_mark_set(move |buffer, _, mark| {
        if *mark == buffer.get_insert() {
            redraw_line.queue_draw();
        }
    });
    text_row.append(&gutter);
    scroll.set_hexpand(true);
    text_row.append(scroll);
    row.append(&overlay);
    row.append(&minimap);
    EditorChrome {
        row,
        file_name,
        model,
        buffer,
        minimap,
        cache,
    }
}

#[cfg(test)]
mod tests {
    use super::{MinimapModel, minimap_offset};

    #[test]
    fn miniature_text_keeps_indentation_and_token_color() {
        let mut model = MinimapModel::default();
        model.update("  let x = 1\n", "main.rs");
        assert_eq!(model.lines.len(), 2);
        assert_eq!(model.lines[0][0].0, 2);
        assert_eq!(model.lines[0][0].1, 1);
        assert!(model.lines[1].is_empty());
    }

    #[test]
    fn long_minimap_scrolls_past_the_viewport() {
        assert_eq!(minimap_offset(240, 1_000, 0.0, 16_000.0, 600.0), 0.0);
        assert!(minimap_offset(240, 1_000, 8_000.0, 16_000.0, 600.0) > 800.0);
        assert_eq!(
            minimap_offset(240, 1_000, 15_400.0, 16_000.0, 600.0),
            1_760.0
        );
    }
}

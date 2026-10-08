use gtk::prelude::*;
use native_workspace_syntax::{highlight, minimap_glyph_alpha};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn scroll_to_map_position(
    y: f64,
    map_height: i32,
    line_count: usize,
    adjustment: &gtk::Adjustment,
) {
    let used_height = map_height.min((line_count * 2) as i32).max(1) as f64;
    let target = y / used_height * adjustment.upper() - adjustment.page_size() / 2.0;
    adjustment.set_value(target.clamp(0.0, (adjustment.upper() - adjustment.page_size()).max(0.0)));
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
    cache: Rc<RefCell<Option<(i32, i32, gtk::cairo::ImageSurface)>>>,
}

impl EditorChrome {
    pub fn set_file_name(&self, name: &str) {
        if self.file_name.borrow().as_str() == name {
            return;
        }
        self.file_name.replace(name.to_owned());
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
    let gutter = gtk::DrawingArea::new();
    gutter.set_content_width(48);
    let minimap = gtk::DrawingArea::new();
    minimap.set_content_width(100);

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
    let model = Rc::new(RefCell::new(MinimapModel::default()));
    let file_name = Rc::new(RefCell::new(String::new()));
    let model_draw = model.clone();
    let cache: Rc<RefCell<Option<(i32, i32, gtk::cairo::ImageSurface)>>> =
        Rc::new(RefCell::new(None));
    let cache_draw = cache.clone();
    minimap.set_draw_func(move |_, context, width, height| {
        let count = model_draw.borrow().lines.len().max(1);
        let used_height = height.min((count * 2) as i32).max(1);
        let surface = cache_draw
            .borrow()
            .as_ref()
            .filter(|(cached_width, cached_height, _)| {
                *cached_width == width && *cached_height == height
            })
            .map(|(_, _, surface)| surface.clone());
        let surface = surface.or_else(|| {
            let surface = gtk::cairo::ImageSurface::create(
                gtk::cairo::Format::ARgb32,
                width.max(1),
                height.max(1),
            )
            .ok()?;
            let paint = gtk::cairo::Context::new(&surface).ok()?;
            let model = model_draw.borrow();
            let cell_height = if count * 2 <= height as usize {
                2.0
            } else {
                1.0
            };
            let rows = if model.lines.is_empty() {
                0
            } else if cell_height == 2.0 {
                count
            } else {
                height.max(1) as usize
            };
            for row in 0..rows {
                let line = if cell_height == 2.0 {
                    row
                } else {
                    row * (count - 1) / (rows - 1).max(1)
                };
                let y = row as f64 * cell_height;
                for (column, kind, top, bottom) in &model.lines[line] {
                    let (red, green, blue) = match kind {
                        1 => (0.67, 0.44, 0.83),
                        2 => (0.85, 0.55, 0.27),
                        3 => (0.37, 0.71, 0.41),
                        4 => (0.52, 0.56, 0.59),
                        5 => (0.34, 0.60, 0.88),
                        _ => (0.61, 0.64, 0.68),
                    };
                    let coverages = if cell_height == 1.0 {
                        [(*top).max(*bottom), 0]
                    } else {
                        [*top, *bottom]
                    };
                    for (row, coverage) in coverages.into_iter().enumerate() {
                        if coverage == 0 {
                            continue;
                        }
                        paint.set_source_rgba(
                            red,
                            green,
                            blue,
                            (f64::from(coverage) / 128.0).min(1.0),
                        );
                        paint.rectangle(4.0 + *column as f64, y + row as f64, 1.0, 1.0);
                        let _ = paint.fill();
                    }
                }
            }
            cache_draw.replace(Some((width, height, surface.clone())));
            Some(surface)
        });
        if let Some(surface) = surface {
            let _ = context.set_source_surface(&surface, 0.0, 0.0);
            let _ = context.paint();
        }
        let total = adjustment_draw.upper().max(1.0);
        context.set_source_rgba(0.5, 0.5, 0.5, 0.3);
        context.rectangle(
            0.0,
            adjustment_draw.value() / total * f64::from(used_height),
            f64::from(width),
            (adjustment_draw.page_size() / total * f64::from(used_height)).max(8.0),
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
            map_for_click.height(),
            model_click.borrow().lines.len(),
            &scroll_to,
        );
    });
    minimap.add_controller(click);
    let drag = gtk::GestureDrag::new();
    let drag_start = Rc::new(Cell::new(0.0));
    let start = drag_start.clone();
    let map_for_drag = minimap.clone();
    let model_drag = model.clone();
    let scroll_drag = adjustment.clone();
    drag.connect_drag_begin(move |_, _, y| {
        start.set(y);
        scroll_to_map_position(
            y,
            map_for_drag.height(),
            model_drag.borrow().lines.len(),
            &scroll_drag,
        );
    });
    let map_for_drag = minimap.clone();
    let model_drag = model.clone();
    let scroll_drag = adjustment.clone();
    drag.connect_drag_update(move |_, _, delta_y| {
        scroll_to_map_position(
            drag_start.get() + delta_y,
            map_for_drag.height(),
            model_drag.borrow().lines.len(),
            &scroll_drag,
        );
    });
    minimap.add_controller(drag);
    let redraw_gutter = gutter.clone();
    let redraw_map = minimap.clone();
    adjustment.connect_value_changed(move |_| {
        redraw_gutter.queue_draw();
        redraw_map.queue_draw();
    });
    let redraw_gutter = gutter.clone();
    let redraw_map = minimap.clone();
    let model_update = model.clone();
    let name_update = file_name.clone();
    let cache_update = cache.clone();
    buffer.connect_changed(move |buffer| {
        let (start, end) = buffer.bounds();
        let content = buffer.text(&start, &end, false);
        model_update
            .borrow_mut()
            .update(&content, &name_update.borrow());
        cache_update.replace(None);
        redraw_gutter.queue_draw();
        redraw_map.queue_draw();
    });
    row.append(&gutter);
    scroll.set_hexpand(true);
    row.append(scroll);
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
    use super::MinimapModel;

    #[test]
    fn miniature_text_keeps_indentation_and_token_color() {
        let mut model = MinimapModel::default();
        model.update("  let x = 1\n", "main.rs");
        assert_eq!(model.lines.len(), 2);
        assert_eq!(model.lines[0][0].0, 2);
        assert_eq!(model.lines[0][0].1, 1);
        assert!(model.lines[1].is_empty());
    }
}

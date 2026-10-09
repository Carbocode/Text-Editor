use gtk::prelude::*;
use native_workspace_syntax::highlight;

pub fn apply(buffer: &gtk::TextBuffer, file_name: &str) {
    let (start, end) = buffer.bounds();
    buffer.remove_all_tags(&start, &end);
    let content = buffer.text(&start, &end, false);
    for token in highlight(file_name, &content) {
        let (name, color) = match token.kind.code() {
            1 => ("syntax-keyword", "#9141ac"),
            2 => ("syntax-number", "#c64600"),
            3 => ("syntax-string", "#2e9f55"),
            4 => ("syntax-comment", "#808080"),
            _ => ("syntax-heading", "#3584e4"),
        };
        let tag = buffer.tag_table().lookup(name).unwrap_or_else(|| {
            let tag = gtk::TextTag::new(Some(name));
            tag.set_property("foreground", color);
            buffer.tag_table().add(&tag);
            tag
        });
        buffer.apply_tag(
            &tag,
            &buffer.iter_at_offset(token.start_char as i32),
            &buffer.iter_at_offset(token.end_char as i32),
        );
    }
}

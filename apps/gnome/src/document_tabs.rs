//! Native tab presentation, independent of window construction.
use adw::prelude::*;
use native_workspace_domain::DocumentId;

pub(super) fn tab_page_at_widget(widget: &gtk::Widget) -> Option<adw::TabPage> {
    if widget
        .find_property("page")
        .is_some_and(|property| property.value_type() == adw::TabPage::static_type())
    {
        widget.property::<Option<adw::TabPage>>("page")
    } else {
        None
    }
}

pub(super) fn style_preview_tab(
    widget: &gtk::Widget,
    pages: &[(DocumentId, adw::TabPage)],
    preview: Option<&DocumentId>,
) {
    if let Some(page) = tab_page_at_widget(widget) {
        let is_preview = pages
            .iter()
            .any(|(id, item)| item == &page && Some(id) == preview);
        if is_preview {
            widget.add_css_class("workspace-preview");
        } else {
            widget.remove_css_class("workspace-preview");
        }
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        style_preview_tab(&widget, pages, preview);
    }
}

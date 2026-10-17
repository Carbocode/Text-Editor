//! Shared wording for native confirmation presenters.
use native_workspace_domain::path;

pub struct Confirmation {
    pub title: String,
    pub body: &'static str,
    pub confirm: &'static str,
    pub cancel: &'static str,
    pub discard: &'static str,
}

pub fn quit_confirmation() -> Confirmation {
    Confirmation {
        title: "Save changes before quitting?".into(),
        body: "Unsaved changes will be lost. Your workspace and open tabs will reopen next time.",
        confirm: "Save All",
        cancel: "Cancel",
        discard: "Discard Changes",
    }
}

pub fn close_confirmation(value: &str) -> Confirmation {
    Confirmation {
        title: format!("Save changes to “{}”?", path::name(value)),
        body: "Your changes will be lost if you close without saving.",
        confirm: "Save",
        cancel: "Cancel",
        discard: "Discard Changes",
    }
}

pub fn delete_confirmation(value: &str) -> Confirmation {
    Confirmation {
        title: format!("Delete “{}”?", path::name(value)),
        body: "This permanently deletes the entry and all its contents. Modified documents must be saved first. This cannot be undone.",
        confirm: "Delete Permanently",
        cancel: "Cancel",
        discard: "Discard Changes",
    }
}

pub fn replacement_confirmation(occurrences: u64, files: u64) -> Confirmation {
    Confirmation {
        title: format!("Replace {occurrences} occurrences in {files} files?"),
        body: "Open documents keep unsaved changes. Closed files are saved to disk.",
        confirm: "Replace",
        cancel: "Cancel",
        discard: "",
    }
}

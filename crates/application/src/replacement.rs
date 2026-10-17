//! Reviewable search replacements. Files are checked again before each write;
//! open documents remain unsaved buffers, using the ordinary save/conflict rules.
use crate::{AppCommand, AppError, AppEvent, Application, SearchLocation, search::Matcher};
use native_workspace_domain::DocumentId;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplacementTarget {
    All,
    Document(DocumentId),
    Match {
        id: DocumentId,
        location: SearchLocation,
    },
}

impl ReplacementTarget {
    pub(crate) fn rebased(self, from: &DocumentId, to: &DocumentId) -> Self {
        match self {
            Self::All => Self::All,
            Self::Document(id) => Self::Document(id.rebased(from, to)),
            Self::Match { id, location } => Self::Match {
                id: id.rebased(from, to),
                location,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementPreview {
    pub occurrences: u64,
    pub file_count: u64,
    /// Unified snippets shared by all native preview dialogs.
    pub text: String,
    files: Vec<FileEdit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileEdit {
    id: DocumentId,
    before: Arc<str>,
    after: Arc<str>,
    disk: Arc<str>,
    loaded: bool,
}

impl Application {
    pub(crate) fn prepare_replacement(
        &self,
        replacement: &str,
        preserve_case: bool,
        target: ReplacementTarget,
    ) -> Result<Vec<AppEvent>, AppError> {
        let state = self.state();
        let root = self.workspace_root()?;
        if state.search_in_progress || state.search_truncated {
            return Err(AppError::IncompleteSearch);
        }
        let matcher = Matcher::configured(&state.search_query, &state.search_options)
            .map_err(AppError::InvalidSearch)?;
        let mut grouped = BTreeMap::<_, Vec<_>>::new();
        for result in state.search_results.iter() {
            let selected = match &target {
                ReplacementTarget::All => true,
                ReplacementTarget::Document(id) => id == &result.id,
                ReplacementTarget::Match { id, location } => {
                    id == &result.id && location == &result.location
                }
            };
            if selected {
                grouped
                    .entry(result.id.clone())
                    .or_default()
                    .push(&result.location);
            }
        }
        let mut preview = ReplacementPreview {
            occurrences: 0,
            file_count: 0,
            text: String::new(),
            files: Vec::new(),
        };
        for (id, locations) in grouped {
            let entry = self.document(&id)?;
            let disk: Arc<str> = self.filesystem.read_document(root, &id)?.into();
            if entry.is_loaded() && entry.saved_content() != Some(&disk) {
                return Err(AppError::ExternalChanges(id));
            }
            let before = entry.content().cloned().unwrap_or_else(|| disk.clone());
            let mut fresh = Vec::new();
            matcher.content_results(&id, &before, &mut fresh);
            let fresh_locations: HashSet<_> =
                fresh.into_iter().map(|result| result.location).collect();
            if locations
                .iter()
                .any(|location| !fresh_locations.contains(*location))
            {
                return Err(AppError::StaleReplacement(id));
            }
            let offsets: Vec<_> = before
                .char_indices()
                .map(|(byte, _)| byte)
                .chain(std::iter::once(before.len()))
                .collect();
            let mut after = String::new();
            let mut consumed = 0;
            let mut occurrences = 0;
            let mut preview_text = format!("\n{id}\n");
            for location in locations {
                let (start, end) = (
                    offsets[location.start_char as usize],
                    offsets[location.end_char as usize],
                );
                // Literal searches historically include overlapping occurrences.
                // Replace the leftmost non-overlapping occurrences exactly once.
                if start < consumed {
                    continue;
                }
                let mut expanded = matcher.replacement(&before, start, end, replacement);
                if preserve_case {
                    expanded = preserve(&before[start..end], &expanded);
                }
                after.push_str(&before[consumed..start]);
                after.push_str(&expanded);
                consumed = end;
                occurrences += 1;
                preview_text.push_str(&format!(
                    "@@ {},{} @@\n- {}\n+ {}\n",
                    location.line,
                    location.column,
                    excerpt(&before[start..end]),
                    excerpt(&expanded)
                ));
            }
            after.push_str(&before[consumed..]);
            if occurrences > 0 && *before != after {
                preview.occurrences += occurrences;
                preview.text.push_str(&preview_text);
                preview.files.push(FileEdit {
                    id,
                    before,
                    after: after.into(),
                    disk,
                    loaded: entry.is_loaded(),
                });
            }
        }
        preview.file_count = preview.files.len() as u64;
        if preview.files.is_empty() {
            return Err(AppError::NoReplacements);
        }
        Ok(vec![AppEvent::ReplacementPrepared {
            preview: Some(Arc::new(preview)),
        }])
    }

    fn validate_edit(&self, edit: &FileEdit) -> Result<(), AppError> {
        let entry = self.document(&edit.id)?;
        if entry.is_loaded() != edit.loaded
            || (edit.loaded && entry.content() != Some(&edit.before))
        {
            return Err(AppError::StaleReplacement(edit.id.clone()));
        }
        if self
            .filesystem
            .read_document(self.workspace_root()?, &edit.id)?
            .as_str()
            != &*edit.disk
        {
            return Err(AppError::StaleReplacement(edit.id.clone()));
        }
        Ok(())
    }

    pub(crate) fn apply_replacement(&mut self) -> Result<Vec<AppEvent>, AppError> {
        let preview = self
            .state()
            .replacement_preview
            .clone()
            .ok_or(AppError::NoReplacements)?;
        // Refuse a stale preview before making any changes.
        for edit in &preview.files {
            self.validate_edit(edit)?;
        }
        let mut events = vec![AppEvent::ReplacementPrepared { preview: None }];
        self.commit(&events)?;
        let result: Result<(), AppError> = (|| {
            for edit in &preview.files {
                self.validate_edit(edit)?;
                let event = if edit.loaded {
                    AppEvent::DocumentChanged {
                        id: edit.id.clone(),
                        content: edit.after.clone(),
                    }
                } else {
                    self.filesystem.write_document(
                        self.workspace_root()?,
                        &edit.id,
                        &edit.after,
                    )?;
                    AppEvent::SearchFileReplaced {
                        id: edit.id.clone(),
                    }
                };
                self.commit(std::slice::from_ref(&event))?;
                events.push(event);
            }
            Ok(())
        })();
        // Earlier successful files stay committed if a later write fails.
        events.extend(self.execute_one(AppCommand::RefreshSearch)?);
        result?;
        Ok(events)
    }
}

fn excerpt(text: &str) -> String {
    let mut result: String = text.chars().take(240).collect();
    if text.chars().count() > 240 {
        result.push('…');
    }
    result
}

fn preserve(original: &str, replacement: &str) -> String {
    let letters: Vec<_> = original.chars().filter(|ch| ch.is_alphabetic()).collect();
    if letters.is_empty() {
        return replacement.to_owned();
    }
    if letters.iter().all(|ch| ch.is_uppercase()) {
        return replacement.to_uppercase();
    }
    if letters.iter().all(|ch| ch.is_lowercase()) {
        return replacement.to_lowercase();
    }
    if letters[0].is_uppercase() && letters[1..].iter().all(|ch| ch.is_lowercase()) {
        let mut chars = replacement.chars();
        return chars.next().map_or_else(String::new, |ch| {
            format!("{}{}", ch.to_uppercase(), chars.as_str().to_lowercase())
        });
    }
    replacement.to_owned()
}

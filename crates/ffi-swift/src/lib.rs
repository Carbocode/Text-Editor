//! UniFFI adapter: one Session, shared rules, and changed lists only.

use native_workspace_application::{AppCommand, AppError, AppSnapshot, Session};
use native_workspace_domain::{DocumentId, path};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

uniffi::setup_scaffolding!();

#[uniffi::export]
pub fn minimap_glyphs() -> Vec<u8> {
    native_workspace_syntax::minimap_glyphs().to_vec()
}

#[derive(Clone, uniffi::Record)]
pub struct SyntaxToken {
    pub start_utf16: u64,
    pub end_utf16: u64,
    pub kind: u8,
}

#[uniffi::export]
pub fn highlight_document(file_name: String, text: String) -> Vec<SyntaxToken> {
    native_workspace_syntax::highlight(&file_name, &text)
        .into_iter()
        .map(|t| SyntaxToken {
            start_utf16: t.start_utf16 as u64,
            end_utf16: t.end_utf16 as u64,
            kind: t.kind.code(),
        })
        .collect()
}

#[uniffi::export]
pub fn workspace_path_name(value: String) -> String {
    path::name(&value).into()
}
#[uniffi::export]
pub fn workspace_path_join(directory: String, name: String) -> String {
    path::join(&directory, &name)
}
#[uniffi::export]
pub fn workspace_path_parent(value: String) -> String {
    path::parent(&value).into()
}
#[uniffi::export]
pub fn workspace_path_rebase(value: String, source: String, target: String) -> String {
    path::rebase(&value, &source, &target).into_owned()
}
#[uniffi::export]
pub fn workspace_path_is_within(value: String, ancestor: String) -> bool {
    path::is_within(&value, &ancestor)
}
#[uniffi::export]
pub fn workspace_file_icon(value: String) -> String {
    native_workspace_application::file_icon_name(&value).into()
}
#[uniffi::export]
pub fn workspace_folder_icon(value: String, expanded: bool) -> String {
    native_workspace_application::folder_icon_name(&value, expanded).into()
}

#[derive(Clone, uniffi::Record)]
pub struct TerminalFont {
    pub family: String,
    pub size: f64,
    pub cell_width: f64,
    pub cell_height: f64,
    pub baseline: f64,
    pub faces: Vec<Vec<u8>>,
}
#[uniffi::export]
pub fn workspace_terminal_font() -> TerminalFont {
    let font = native_workspace_application::terminal_font();
    TerminalFont {
        family: font.family.into(),
        size: font.size,
        cell_width: font.cell_width,
        cell_height: font.cell_height,
        baseline: font.baseline,
        faces: font.faces.map(<[u8]>::to_vec).into(),
    }
}

#[derive(Clone, uniffi::Record)]
pub struct Confirmation {
    pub title: String,
    pub body: String,
    pub confirm: String,
    pub cancel: String,
    pub discard: String,
}
impl From<native_workspace_application::Confirmation> for Confirmation {
    fn from(c: native_workspace_application::Confirmation) -> Self {
        Self {
            title: c.title,
            body: c.body.into(),
            confirm: c.confirm.into(),
            cancel: c.cancel.into(),
            discard: c.discard.into(),
        }
    }
}
#[uniffi::export]
pub fn workspace_replacement_confirmation(occurrences: u64, files: u64) -> Confirmation {
    native_workspace_application::replacement_confirmation(occurrences, files).into()
}

#[uniffi::export]
pub fn workspace_git_confirmation() -> Confirmation {
    native_workspace_application::git_action_confirmation().into()
}

#[uniffi::export]
pub fn workspace_terminal_confirmation() -> Confirmation {
    native_workspace_application::terminal_close_confirmation().into()
}

#[uniffi::export]
pub fn workspace_close_confirmation(value: String) -> Confirmation {
    native_workspace_application::close_confirmation(&value).into()
}
#[uniffi::export]
pub fn workspace_quit_confirmation() -> Confirmation {
    native_workspace_application::quit_confirmation().into()
}
#[uniffi::export]
pub fn workspace_delete_confirmation(value: String) -> Confirmation {
    native_workspace_application::delete_confirmation(&value).into()
}

#[derive(Clone, uniffi::Record)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub dirty: bool,
    pub external_change: bool,
}
#[derive(Clone, uniffi::Record)]
pub struct DocumentSnapshot {
    pub id: String,
    pub title: String,
    pub content: String,
    pub dirty: bool,
    pub external_change: bool,
}
#[derive(Clone, uniffi::Record)]
pub struct RecentWorkspace {
    pub root: String,
    pub name: String,
}
#[derive(Clone, uniffi::Record)]
pub struct SearchLocation {
    pub line: u64,
    pub column: u64,
    pub start_char: u64,
    pub end_char: u64,
    pub start_utf16: u64,
    pub end_utf16: u64,
}
#[derive(Clone, uniffi::Record)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub location: SearchLocation,
}
#[derive(Clone, uniffi::Record)]
pub struct Relocation {
    pub source: String,
    pub target: String,
}
#[derive(Clone, uniffi::Record)]
pub struct Activity {
    pub busy: bool,
    pub relocation: Option<Relocation>,
    pub relocation_slow: bool,
    pub editing_paused: bool,
}

#[derive(Clone, uniffi::Record)]
pub struct SidebarRow {
    pub git_status: String,
    pub title: String,
    pub dirty: bool,
    pub external_change: bool,
    pub path: String,
    pub depth: u64,
    pub directory: bool,
    pub expanded: bool,
    pub result_index: Option<u64>,
}

#[derive(Clone, uniffi::Record)]
pub struct SearchOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub include: String,
    pub exclude: String,
    pub use_ignore_files: bool,
    pub only_open_editors: bool,
}
impl From<native_workspace_application::SearchOptions> for SearchOptions {
    fn from(options: native_workspace_application::SearchOptions) -> Self {
        Self {
            match_case: options.match_case,
            whole_word: options.whole_word,
            regex: options.regex,
            include: options.include,
            exclude: options.exclude,
            use_ignore_files: options.use_ignore_files,
            only_open_editors: options.only_open_editors,
        }
    }
}
impl From<SearchOptions> for native_workspace_application::SearchOptions {
    fn from(options: SearchOptions) -> Self {
        Self {
            match_case: options.match_case,
            whole_word: options.whole_word,
            regex: options.regex,
            include: options.include,
            exclude: options.exclude,
            use_ignore_files: options.use_ignore_files,
            only_open_editors: options.only_open_editors,
        }
    }
}

/// None for a list means unchanged since the last delivery to this adapter.
/// `snapshot` always includes every list. Commands and `poll` deliver deltas.
#[derive(Clone, uniffi::Record)]
pub struct WorkspaceUpdate {
    pub git_json: Option<String>,
    pub terminal_only: bool,
    pub terminal_json: String,
    pub terminals_running: bool,
    pub diagnostic_operation: u64,
    pub revision: u64,
    pub editor_generation: u64,
    pub workspace_name: Option<String>,
    pub workspace_root: Option<String>,
    pub recent_workspaces: Vec<RecentWorkspace>,
    pub search_query: String,
    pub search_options: SearchOptions,
    pub search_error: Option<String>,
    pub replacement_preview: Option<String>,
    pub replacement_occurrences: u64,
    pub replacement_files: u64,
    pub search_results: Option<Vec<SearchResult>>,
    pub search_in_progress: bool,
    pub search_truncated: bool,
    pub directories: Option<Vec<String>>,
    pub documents: Option<Vec<DocumentSummary>>,
    pub file_filter_files: Option<Vec<String>>,
    pub file_filter_directories: Option<Vec<String>>,
    pub file_filter_query: String,
    pub file_filter_in_progress: bool,
    pub open_documents: Vec<String>,
    pub preview_document: Option<String>,
    pub active_document: Option<DocumentSnapshot>,
    pub activity: Activity,
    pub error: Option<String>,
}

fn update(snapshot: &AppSnapshot, previous: Option<&AppSnapshot>) -> WorkspaceUpdate {
    WorkspaceUpdate {
        git_json: previous
            .is_none_or(|p| !Arc::ptr_eq(&p.git, &snapshot.git))
            .then(|| snapshot.git.json()),
        terminal_only: false,
        terminal_json: String::new(),
        terminals_running: false,
        diagnostic_operation: snapshot.diagnostic_operation,
        revision: snapshot.revision,
        editor_generation: snapshot.editor_generation,
        workspace_name: snapshot.workspace_name().map(str::to_owned),
        workspace_root: snapshot
            .workspace_root()
            .map(|p| p.to_string_lossy().into_owned()),
        recent_workspaces: snapshot
            .recent_workspaces
            .iter()
            .map(|w| RecentWorkspace {
                root: w.root.to_string_lossy().into_owned(),
                name: w.name.clone(),
            })
            .collect(),
        search_query: snapshot.search_query.clone(),
        search_options: snapshot.search_options.clone().into(),
        search_error: snapshot.search_error.clone(),
        replacement_preview: snapshot
            .replacement_preview
            .as_ref()
            .map(|preview| preview.text.clone()),
        replacement_occurrences: snapshot
            .replacement_preview
            .as_ref()
            .map_or(0, |preview| preview.occurrences),
        replacement_files: snapshot
            .replacement_preview
            .as_ref()
            .map_or(0, |preview| preview.file_count),
        search_in_progress: snapshot.search_in_progress,
        search_truncated: snapshot.search_truncated,
        search_results: previous
            .is_none_or(|p| !Arc::ptr_eq(&p.search_results, &snapshot.search_results))
            .then(|| {
                snapshot
                    .search_results
                    .iter()
                    .map(|r| SearchResult {
                        id: r.id.to_string(),
                        title: r.id.name().into(),
                        snippet: r.snippet.clone(),
                        location: SearchLocation {
                            line: r.location.line,
                            column: r.location.column,
                            start_char: r.location.start_char,
                            end_char: r.location.end_char,
                            start_utf16: r.location.start_utf16,
                            end_utf16: r.location.end_utf16,
                        },
                    })
                    .collect()
            }),
        directories: previous
            .is_none_or(|p| !Arc::ptr_eq(&p.directories, &snapshot.directories))
            .then(|| snapshot.directories.as_ref().clone()),
        documents: previous
            .is_none_or(|p| !Arc::ptr_eq(&p.documents, &snapshot.documents))
            .then(|| {
                snapshot
                    .documents
                    .iter()
                    .map(|d| DocumentSummary {
                        id: d.id.to_string(),
                        title: d.id.name().into(),
                        dirty: d.dirty,
                        external_change: d.external_change,
                    })
                    .collect()
            }),
        file_filter_files: previous
            .is_none_or(|p| !Arc::ptr_eq(&p.file_filter, &snapshot.file_filter))
            .then(|| snapshot.file_filter.files.clone()),
        file_filter_directories: previous
            .is_none_or(|p| !Arc::ptr_eq(&p.file_filter, &snapshot.file_filter))
            .then(|| snapshot.file_filter.directories.clone()),
        file_filter_query: snapshot.file_filter.query.clone(),
        file_filter_in_progress: snapshot.file_filter_in_progress,
        open_documents: snapshot
            .open_documents
            .iter()
            .map(ToString::to_string)
            .collect(),
        preview_document: snapshot.preview_document.as_ref().map(ToString::to_string),
        active_document: snapshot.active_document.as_ref().map(|d| DocumentSnapshot {
            id: d.id.to_string(),
            title: d.id.name().into(),
            content: d.content.to_string(),
            dirty: d.dirty,
            external_change: d.external_change,
        }),
        activity: Activity {
            busy: snapshot.activity.busy,
            relocation: snapshot.activity.relocation.as_ref().map(|r| Relocation {
                source: r.source.to_string(),
                target: r.target.to_string(),
            }),
            relocation_slow: snapshot.activity.relocation_slow,
            editing_paused: snapshot.activity.editing_paused,
        },
        error: snapshot.error.clone(),
    }
}

/// The core supplies error wording. The adapter adds only transport failures.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum WorkspaceError {
    #[error("{message}")]
    Application { message: String },
    #[error("Application lock poisoned.")]
    LockPoisoned,
}
impl From<AppError> for WorkspaceError {
    fn from(error: AppError) -> Self {
        Self::Application {
            message: error.to_string(),
        }
    }
}

struct BridgeSession {
    terminal_revision: u64,
    session: Session,
    delivered: Option<AppSnapshot>,
}
impl BridgeSession {
    fn deliver(&mut self, full: bool) -> WorkspaceUpdate {
        let snapshot = self.session.snapshot();
        let terminal_only = !full
            && self
                .delivered
                .as_ref()
                .is_some_and(|last| last.revision == snapshot.revision);
        let mut view = snapshot.clone();
        if terminal_only {
            view.active_document = None;
        }
        let mut result = update(&view, if full { None } else { self.delivered.as_ref() });
        result.terminal_only = terminal_only;
        let terminal = self.session.terminal_snapshot();
        if full || terminal.revision != self.terminal_revision {
            result.terminal_json =
                serde_json::to_string(&*terminal).expect("terminal snapshot serializes");
        }
        result.terminals_running = self.session.has_running_terminals();
        self.terminal_revision = terminal.revision;
        self.delivered = Some(snapshot);
        result
    }
}

#[derive(uniffi::Object)]
pub struct WorkspaceDropTrace {
    inner: native_workspace_application::SidebarDropTrace,
}
#[uniffi::export]
impl WorkspaceDropTrace {
    pub fn preview_hidden(&self) {
        self.inner.preview_hidden();
    }
    pub fn handler_finished(&self, accepted: bool) {
        self.inner.handler_finished(accepted);
    }
    pub fn sidebar_ready(&self) {
        self.inner.sidebar_ready();
    }
}

#[derive(uniffi::Object)]
pub struct WorkspaceApplication {
    inner: Mutex<BridgeSession>,
    diagnostic_observer: std::sync::OnceLock<native_workspace_application::DiagnosticObserver>,
}
#[uniffi::export]
impl WorkspaceApplication {
    #[uniffi::constructor]
    pub fn new() -> Self {
        let mut session = Session::new();
        session.enable_devtools("swiftui");
        let observer = session.diagnostic_observer();
        let diagnostic_observer = std::sync::OnceLock::new();
        if observer.available() {
            let _ = diagnostic_observer.set(observer);
        }
        Self {
            inner: Mutex::new(BridgeSession {
                terminal_revision: 0,
                session,
                delivered: None,
            }),
            diagnostic_observer,
        }
    }
    pub fn diagnostic_ui_heartbeat(&self) {
        if let Some(o) = self.diagnostic_observer.get() {
            o.ui_heartbeat();
        }
    }
    pub fn begin_sidebar_drop(&self, origin: u8) -> Arc<WorkspaceDropTrace> {
        Arc::new(WorkspaceDropTrace {
            inner: native_workspace_application::SidebarDropTrace::begin(
                self.diagnostic_observer.get().cloned().unwrap_or_default(),
                origin,
            ),
        })
    }
    pub fn open_devtools(&self) -> bool {
        if self.diagnostic_observer.get().is_none() {
            let Ok(mut bridge) = self.inner.try_lock() else {
                return false;
            };
            bridge.session.start_devtools("swiftui");
            let observer = bridge.session.diagnostic_observer();
            if !observer.available() {
                return false;
            }
            let _ = self.diagnostic_observer.set(observer);
        }
        if let Some(o) = self.diagnostic_observer.get() {
            o.open_console();
            return true;
        }
        false
    }
    pub fn devtools_available(&self) -> bool {
        cfg!(feature = "devtools")
    }
    pub fn devtools_enabled(&self) -> bool {
        self.diagnostic_observer.get().is_some_and(|o| o.enabled())
    }
    pub fn diagnostic_snapshot_applied(&self, operation: u64, revision: u64, duration_ns: u64) {
        if let Some(o) = self.diagnostic_observer.get() {
            o.snapshot_applied(operation, revision, duration_ns);
        }
    }
    pub fn snapshot(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        Ok(self.lock()?.deliver(true))
    }
    pub fn enable_persistence(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        let mut bridge = self.lock()?;
        bridge.session.enable_persistence_default()?;
        Ok(bridge.deliver(false))
    }
    pub fn set_git_visible(&self, visible: bool) -> Result<(), WorkspaceError> {
        self.lock()?.session.set_git_visible(visible);
        Ok(())
    }
    pub fn git_request(
        &self,
        action: String,
        value: String,
        extra: String,
        flag: bool,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        let mut bridge = self.lock()?;
        bridge.session.git_request(&action, value, extra, flag)?;
        Ok(bridge.deliver(false))
    }
    pub fn git_resolve(&self, result: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        let mut bridge = self.lock()?;
        bridge.session.git_resolve(result)?;
        Ok(bridge.deliver(false))
    }
    pub fn git_stage_lines(&self, lines: Vec<u64>) -> Result<WorkspaceUpdate, WorkspaceError> {
        let mut bridge = self.lock()?;
        bridge
            .session
            .git_stage_lines(lines.into_iter().map(|line| line as usize).collect())?;
        Ok(bridge.deliver(false))
    }
    pub fn git_load_more(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        let limit = self.lock()?.session.snapshot().git.history_limit + 200;
        self.dispatch(AppCommand::LoadGitHistory(limit))
    }
    pub fn refresh_git(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::RefreshGit)
    }
    pub fn show_git_diff(
        &self,
        path: String,
        staged: bool,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::ShowGitDiff { path, staged })
    }
    pub fn restore_session(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::RestoreSession)
    }
    pub fn prepare_to_quit(&self, save: bool) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::PrepareToQuit { save })
    }
    pub fn poll(&self) -> Result<Option<WorkspaceUpdate>, WorkspaceError> {
        let mut bridge = self.lock()?;
        bridge.session.poll();
        if bridge.delivered.as_ref().is_some_and(|s| {
            s.revision == bridge.session.revision()
                && bridge.terminal_revision == bridge.session.terminal_snapshot().revision
        }) {
            return Ok(None);
        }
        Ok(Some(bridge.deliver(false)))
    }
    pub fn terminal_action(
        &self,
        action: String,
        id: u64,
        confirmed: bool,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        use native_workspace_application::TerminalCommand as T;
        let command = match action.as_str() {
            "new" => T::New,
            "activate" => T::Activate(id),
            "close" => T::Close { id, confirmed },
            "close_all" => T::CloseAll { confirmed },
            _ => {
                return Err(native_workspace_application::AppError::Terminal(
                    "Unknown terminal action".into(),
                )
                .into());
            }
        };
        let mut bridge = self.lock()?;
        bridge.session.terminal_command(command)?;
        Ok(bridge.deliver(false))
    }
    pub fn terminal_input(
        &self,
        id: u64,
        key: String,
        text: String,
        modifiers: u16,
        paste: bool,
    ) -> Result<(), WorkspaceError> {
        use native_workspace_application::{TerminalCommand, TerminalInput, TerminalKey};
        let input = if paste {
            TerminalInput::Paste(text)
        } else if key.is_empty() {
            TerminalInput::Text(text)
        } else {
            TerminalInput::Key(TerminalKey {
                key,
                text,
                modifiers,
            })
        };
        self.lock()?
            .session
            .terminal_command(TerminalCommand::Input { id, input })?;
        Ok(())
    }
    pub fn terminal_resize(&self, id: u64, cols: u16, rows: u16) -> Result<(), WorkspaceError> {
        use native_workspace_application::{TerminalCommand, TerminalInput};
        self.lock()?
            .session
            .terminal_command(TerminalCommand::Input {
                id,
                input: TerminalInput::Resize { cols, rows },
            })?;
        Ok(())
    }
    pub fn terminal_scroll(&self, id: u64, lines: i32) -> Result<(), WorkspaceError> {
        use native_workspace_application::{TerminalCommand, TerminalInput};
        self.lock()?
            .session
            .terminal_command(TerminalCommand::Input {
                id,
                input: TerminalInput::Scroll(lines),
            })?;
        Ok(())
    }
    pub fn filter_files(&self, query: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        let mut bridge = self.lock()?;
        bridge.session.filter_files(&query);
        Ok(bridge.deliver(false))
    }
    pub fn sidebar_rows(
        &self,
        search: bool,
        expanded: Vec<String>,
    ) -> Result<Vec<SidebarRow>, WorkspaceError> {
        use native_workspace_application::SidebarRowKind;
        let snapshot = self.lock()?.session.snapshot();
        let expanded: std::collections::HashSet<_> = expanded.into_iter().collect();
        let rows = if search {
            snapshot.search_rows()
        } else if snapshot.file_filter.is_active() {
            snapshot.file_filter_rows()
        } else {
            snapshot.file_tree_rows(|p| expanded.contains(p))
        };
        Ok(rows
            .into_iter()
            .map(|r| SidebarRow {
                git_status: snapshot.sidebar_git_status(&r.path).into(),
                title: path::name(&r.path).into(),
                dirty: snapshot.document_summary(&r.path).is_some_and(|d| d.dirty),
                external_change: snapshot
                    .document_summary(&r.path)
                    .is_some_and(|d| d.external_change),
                directory: r.is_directory(),
                expanded: matches!(r.kind, SidebarRowKind::Directory { expanded: true }),
                result_index: match r.kind {
                    SidebarRowKind::Match { result } => Some(result as u64),
                    _ => None,
                },
                path: r.path,
                depth: r.depth as u64,
            })
            .collect())
    }
    pub fn can_move_entry(&self, path: String, directory: String) -> bool {
        self.inner.lock().is_ok_and(|b| {
            DocumentId::parse(path).is_ok_and(|id| b.session.can_move_entry(id, directory))
        })
    }
    pub fn move_entry_traced(
        &self,
        path: String,
        directory: String,
        trace: Arc<WorkspaceDropTrace>,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        let id = self.parse_id(path)?;
        let mut bridge = self.lock()?;
        bridge
            .session
            .dispatch_traced(AppCommand::MoveEntry { id, directory }, trace.inner.clone())?;
        Ok(bridge.deliver(false))
    }
    pub fn set_search_query(&self, query: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::QueueSearchQuery(query))
    }
    pub fn configure_search(
        &self,
        options: SearchOptions,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::ConfigureSearch(options.into()))
    }
    pub fn refresh_search(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::RefreshSearch)
    }
    pub fn cancel_search(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::CancelSearch)
    }
    /// No path selects all results; a path selects a file; an index selects one occurrence.
    pub fn prepare_replacement(
        &self,
        replacement: String,
        preserve_case: bool,
        path: Option<String>,
        result_index: Option<u64>,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        let mut bridge = self.lock()?;
        let snapshot = bridge.session.snapshot();
        let target = snapshot.replacement_target(path.as_deref(), result_index)?;
        bridge.session.dispatch(AppCommand::PrepareReplacement {
            replacement,
            preserve_case,
            target,
        })?;
        Ok(bridge.deliver(false))
    }
    pub fn apply_replacement(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::ApplyReplacement)
    }
    pub fn discard_replacement(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::DiscardReplacement)
    }
    pub fn open_workspace(&self, path: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::OpenWorkspace(PathBuf::from(path)))
    }
    pub fn refresh_workspace(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::RefreshWorkspace)
    }
    pub fn move_entry(
        &self,
        path: String,
        directory: String,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::MoveEntry {
            id: self.parse_id(path)?,
            directory,
        })
    }
    pub fn rename_entry(
        &self,
        path: String,
        new_name: String,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::RenameEntry {
            id: self.parse_id(path)?,
            new_name,
        })
    }
    pub fn delete_entry(&self, path: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::DeleteEntry(self.parse_id(path)?))
    }
    pub fn create_entry(
        &self,
        path: String,
        directory: bool,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::CreateEntry {
            id: self.parse_id(path)?,
            directory,
        })
    }
    pub fn can_move_files(&self, paths: Vec<String>, directory: String) -> bool {
        self.lock().is_ok_and(|bridge| {
            bridge.session.can_move_files(
                &paths
                    .into_iter()
                    .map(std::path::PathBuf::from)
                    .collect::<Vec<_>>(),
                &directory,
            )
        })
    }
    pub fn open_paths(&self, paths: Vec<String>) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::OpenPaths(
            paths.into_iter().map(PathBuf::from).collect(),
        ))
    }

    pub fn open_files(&self, paths: Vec<String>) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::OpenFiles(
            paths.into_iter().map(std::path::PathBuf::from).collect(),
        ))
    }
    pub fn move_files(
        &self,
        paths: Vec<String>,
        directory: String,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::MoveFiles {
            paths: paths.into_iter().map(std::path::PathBuf::from).collect(),
            directory,
        })
    }
    pub fn open_document(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::OpenDocument(self.parse_id(id)?))
    }
    pub fn preview_document(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::PreviewDocument(self.parse_id(id)?))
    }
    pub fn keep_document_open(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::KeepDocumentOpen(self.parse_id(id)?))
    }
    pub fn move_document_tab(
        &self,
        id: String,
        index: u64,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::MoveDocumentTab {
            id: self.parse_id(id)?,
            index: index as usize,
        })
    }
    pub fn change_document(
        &self,
        id: String,
        content: String,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::ChangeDocument {
            id: self.parse_id(id)?,
            content,
        })
    }
    pub fn close_document(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::CloseDocument(self.parse_id(id)?))
    }
    pub fn discard_and_close_document(
        &self,
        id: String,
    ) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::DiscardAndCloseDocument(self.parse_id(id)?))
    }
    pub fn save_and_close_document(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::SaveAndCloseDocument(self.parse_id(id)?))
    }
    pub fn save_all_documents(&self) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::SaveAllDocuments)
    }
    pub fn save_document(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::SaveDocument(self.parse_id(id)?))
    }
    pub fn reload_document(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::ReloadDocument(self.parse_id(id)?))
    }
    pub fn overwrite_document(&self, id: String) -> Result<WorkspaceUpdate, WorkspaceError> {
        self.dispatch(AppCommand::OverwriteDocument(self.parse_id(id)?))
    }
    #[cfg(debug_assertions)]
    pub fn delay_next_background_job(&self, milliseconds: u64) -> Result<(), WorkspaceError> {
        self.lock()?
            .session
            .delay_next_background_job(std::time::Duration::from_millis(milliseconds));
        Ok(())
    }
}
impl WorkspaceApplication {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BridgeSession>, WorkspaceError> {
        self.inner.lock().map_err(|_| WorkspaceError::LockPoisoned)
    }
    fn parse_id(&self, value: String) -> Result<DocumentId, WorkspaceError> {
        match DocumentId::from_document_path(value) {
            Ok(id) => Ok(id),
            Err(_) => {
                self.lock()?.session.report(AppError::InvalidEntryName);
                Err(AppError::InvalidEntryName.into())
            }
        }
    }
    fn dispatch(&self, command: AppCommand) -> Result<WorkspaceUpdate, WorkspaceError> {
        let mut bridge = self.lock()?;
        bridge.session.dispatch(command)?;
        Ok(bridge.deliver(false))
    }
}
impl Default for WorkspaceApplication {
    fn default() -> Self {
        Self::new()
    }
}

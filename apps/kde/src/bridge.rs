use cxx_qt::CxxQtType;
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QStringList, QUrl, QVariant};
use native_workspace_application::{
    AppCommand, AppError, AppSnapshot, Session, SidebarRowKind, file_icon_name, folder_icon_name,
};
use native_workspace_domain::DocumentId;
use std::{path::PathBuf, pin::Pin, sync::Arc};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/core/qlist/qlist_QVariant.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, devtools_available)]
        #[qproperty(u64, revision)]
        #[qproperty(QString, workspace_name)]
        #[qproperty(QString, workspace_root)]
        #[qproperty(bool, search_match_case)]
        #[qproperty(bool, search_whole_word)]
        #[qproperty(bool, search_regex)]
        #[qproperty(QString, search_include)]
        #[qproperty(QString, search_exclude)]
        #[qproperty(bool, search_ignore)]
        #[qproperty(bool, search_open_editors)]
        #[qproperty(QString, search_error)]
        #[qproperty(QString, replacement_preview)]
        #[qproperty(u64, replacement_occurrences)]
        #[qproperty(u64, replacement_files)]
        #[qproperty(QString, search_query)]
        #[qproperty(bool, search_in_progress)]
        #[qproperty(bool, search_truncated)]
        #[qproperty(QList_QVariant, search_results)]
        #[qproperty(QString, file_filter_query)]
        #[qproperty(QStringList, file_filter_files)]
        #[qproperty(QStringList, file_filter_directories)]
        #[qproperty(bool, file_filter_in_progress)]
        #[qproperty(bool, snapshot_updating)]
        #[qproperty(QString, preview_document_id)]
        #[qproperty(QStringList, open_document_ids)]
        #[qproperty(QStringList, document_ids)]
        #[qproperty(QStringList, directory_ids)]
        #[qproperty(QStringList, dirty_document_ids)]
        #[qproperty(QStringList, conflicted_document_ids)]
        #[qproperty(QStringList, recent_workspace_paths)]
        #[qproperty(QStringList, recent_workspace_names)]
        #[qproperty(QString, active_document_id)]
        #[qproperty(QString, active_document_text)]
        #[qproperty(bool, active_document_dirty)]
        #[qproperty(bool, active_document_external_change)]
        #[qproperty(QString, pending_move_target)]
        #[qproperty(bool, move_pending)]
        #[qproperty(bool, editing_paused)]
        #[qproperty(QString, error_message)]
        type AppController = super::AppControllerRust;

        #[qsignal]
        fn document_switched(self: Pin<&mut Self>);

        #[qinvokable]
        fn open_devtools(self: Pin<&mut Self>);
        #[qinvokable]
        fn begin_sidebar_drop(self: Pin<&mut Self>);
        #[qinvokable]
        fn diagnostic_ui_heartbeat(self: Pin<&mut Self>);
        #[qinvokable]
        fn drop_preview_hidden(self: Pin<&mut Self>);
        #[qinvokable]
        fn drop_handler_finished(self: Pin<&mut Self>, accepted: bool);
        #[qinvokable]
        fn drop_sidebar_ready(self: Pin<&mut Self>);
        #[qinvokable]
        fn initialize_persistence(self: Pin<&mut Self>) -> bool;
        #[qinvokable]
        fn restore_session(self: Pin<&mut Self>);
        #[qinvokable]
        fn prepare_to_quit(self: Pin<&mut Self>, save: bool) -> bool;
        #[qinvokable]
        fn quit_confirmation(self: Pin<&mut Self>) -> QVariant;
        #[qinvokable]
        fn search_files(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        fn configure_search(self: Pin<&mut Self>, match_case: bool, whole_word: bool, regex: bool);
        #[qinvokable]
        fn configure_search_files(
            self: Pin<&mut Self>,
            include: &QString,
            exclude: &QString,
            ignore: bool,
            open_editors: bool,
        );
        #[qinvokable]
        fn refresh_search(self: Pin<&mut Self>);
        #[qinvokable]
        fn replacement_confirmation(self: Pin<&mut Self>, occurrences: u64, files: u64)
        -> QVariant;
        #[qinvokable]
        fn cancel_search(self: Pin<&mut Self>);
        #[qinvokable]
        fn prepare_replacement(
            self: Pin<&mut Self>,
            replacement: &QString,
            preserve: bool,
            path: &QString,
            result_index: i32,
        );
        #[qinvokable]
        fn apply_replacement(self: Pin<&mut Self>);
        #[qinvokable]
        fn discard_replacement(self: Pin<&mut Self>);
        #[qinvokable]
        fn filter_files(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        fn file_icon(self: Pin<&mut Self>, path: &QString) -> QString;
        #[qinvokable]
        fn folder_icon(self: Pin<&mut Self>, path: &QString, expanded: bool) -> QString;
        #[qinvokable]
        fn open_workspace(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        fn open_files(self: Pin<&mut Self>, urls: &QStringList);
        #[qinvokable]
        fn open_paths(self: Pin<&mut Self>, urls: &QStringList);
        #[qinvokable]
        fn move_files(self: Pin<&mut Self>, urls: &QStringList, directory: &QString);
        #[qinvokable]
        fn can_move_files(self: Pin<&mut Self>, urls: &QStringList, directory: &QString) -> bool;
        #[qinvokable]
        fn sidebar_children(self: Pin<&mut Self>, parent: &QString) -> QList_QVariant;
        #[qinvokable]
        fn sidebar_rows(
            self: Pin<&mut Self>,
            search: bool,
            expanded: &QStringList,
        ) -> QList_QVariant;
        #[qinvokable]
        fn can_move_entry(self: Pin<&mut Self>, path: &QString, directory: &QString) -> bool;
        #[qinvokable]
        fn poll(self: Pin<&mut Self>);
        #[qinvokable]
        fn confirmation(self: Pin<&mut Self>, path: &QString, deleting: bool) -> QVariant;
        #[cfg(debug_assertions)]
        #[qinvokable]
        fn delay_next_background_job(self: Pin<&mut Self>, milliseconds: u64);

        #[qinvokable]
        fn sidebar_has_children(self: Pin<&mut Self>, parent: &QString) -> bool;
        #[qinvokable]
        fn move_entry(self: Pin<&mut Self>, path: &QString, directory: &QString, text: &QString);
        #[qinvokable]
        fn rename_entry(self: Pin<&mut Self>, path: &QString, new_name: &QString);
        #[qinvokable]
        fn delete_entry(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        fn create_entry(self: Pin<&mut Self>, path: &QString, directory: bool);
        #[qinvokable]
        fn refresh_workspace(self: Pin<&mut Self>);
        #[qinvokable]
        fn open_document(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn preview_document(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn keep_document_open(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn change_document(self: Pin<&mut Self>, text: &QString);
        #[qinvokable]
        fn highlight_document(self: Pin<&mut Self>, text: &QString) -> QList_QVariant;
        #[qinvokable]
        fn minimap_glyphs(self: Pin<&mut Self>) -> QList_QVariant;
        #[qinvokable]
        fn save_document(self: Pin<&mut Self>);
        #[qinvokable]
        fn save_all_documents(self: Pin<&mut Self>);
        #[qinvokable]
        fn close_document(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn discard_and_close_document(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn save_and_close_document(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn reload_document(self: Pin<&mut Self>);
        #[qinvokable]
        fn overwrite_document(self: Pin<&mut Self>);
    }
}

#[derive(Default)]
pub struct AppControllerRust {
    revision: u64,
    workspace_name: QString,
    workspace_root: QString,
    search_match_case: bool,
    search_whole_word: bool,
    search_regex: bool,
    search_include: QString,
    search_exclude: QString,
    search_ignore: bool,
    search_open_editors: bool,
    search_error: QString,
    replacement_preview: QString,
    replacement_occurrences: u64,
    replacement_files: u64,
    search_query: QString,
    search_in_progress: bool,
    search_truncated: bool,
    search_results: QList<QVariant>,
    file_filter_query: QString,
    file_filter_files: QStringList,
    file_filter_directories: QStringList,
    file_filter_in_progress: bool,
    snapshot_updating: bool,
    open_document_ids: QStringList,
    preview_document_id: QString,
    document_ids: QStringList,
    directory_ids: QStringList,
    dirty_document_ids: QStringList,
    conflicted_document_ids: QStringList,
    recent_workspace_paths: QStringList,
    recent_workspace_names: QStringList,
    active_document_id: QString,
    active_document_text: QString,
    active_document_dirty: bool,
    active_document_external_change: bool,
    pending_move_target: QString,
    move_pending: bool,
    editing_paused: bool,
    error_message: QString,
    devtools_available: bool,
    application: Session,
    editor_generation: u64,
    drop_trace: Option<native_workspace_application::SidebarDropTrace>,
    published: Option<AppSnapshot>,
}

fn text(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}
fn object<const N: usize>(fields: [(&str, QVariant); N]) -> QVariant {
    let map: QMap<QMapPair_QString_QVariant> = fields
        .into_iter()
        .map(|(k, v)| (QString::from(k), v))
        .collect();
    QVariant::from(&map)
}
fn strings<'a>(values: impl Iterator<Item = &'a str>) -> QStringList {
    values.map(QString::from).collect()
}

fn local_file_paths(urls: &QStringList) -> Option<Vec<PathBuf>> {
    let paths: Option<Vec<_>> = urls
        .iter()
        .map(|value| {
            QUrl::from_user_input(value, &QString::default())
                .to_local_file()
                .map(|path| PathBuf::from(path.to_string()))
        })
        .collect();
    paths.filter(|paths| !paths.is_empty())
}

impl qobject::AppController {
    fn publish(mut self: Pin<&mut Self>) {
        let snapshot = self.rust().application.snapshot();
        self.as_mut().update_snapshot(snapshot);
    }
    fn dispatch(mut self: Pin<&mut Self>, command: AppCommand) {
        let _ = self.as_mut().rust_mut().application.dispatch(command);
        self.publish();
    }
    fn with_id(
        mut self: Pin<&mut Self>,
        value: &QString,
        command: impl FnOnce(DocumentId) -> AppCommand,
    ) {
        match DocumentId::from_document_path(value.to_string()) {
            Ok(id) => self.dispatch(command(id)),
            Err(_) => {
                self.as_mut()
                    .rust_mut()
                    .application
                    .report(AppError::InvalidEntryName);
                self.publish();
            }
        }
    }
    pub fn poll(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().application.poll();
        if self.rust().revision != self.rust().application.revision() {
            self.publish();
        }
    }
    fn update_snapshot(mut self: Pin<&mut Self>, snapshot: AppSnapshot) {
        self.as_mut().set_snapshot_updating(true);
        let observer = self.rust().application.diagnostic_observer();
        let started = observer.enabled().then(std::time::Instant::now);
        if self.rust().editor_generation != snapshot.editor_generation {
            self.as_mut().rust_mut().editor_generation = snapshot.editor_generation;
            self.as_mut().document_switched();
        }
        let (documents_changed, directories_changed, search_changed, filter_changed) = self
            .rust()
            .published
            .as_ref()
            .map_or((true, true, true, true), |last| {
                (
                    !Arc::ptr_eq(&last.documents, &snapshot.documents),
                    !Arc::ptr_eq(&last.directories, &snapshot.directories),
                    !Arc::ptr_eq(&last.search_results, &snapshot.search_results),
                    !Arc::ptr_eq(&last.file_filter, &snapshot.file_filter),
                )
            });
        if documents_changed {
            self.as_mut()
                .set_document_ids(strings(snapshot.documents.iter().map(|d| d.id.as_str())));
            self.as_mut().set_dirty_document_ids(strings(
                snapshot
                    .documents
                    .iter()
                    .filter(|d| d.dirty)
                    .map(|d| d.id.as_str()),
            ));
            self.as_mut().set_conflicted_document_ids(strings(
                snapshot
                    .documents
                    .iter()
                    .filter(|d| d.external_change)
                    .map(|d| d.id.as_str()),
            ));
        }
        if directories_changed {
            self.as_mut()
                .set_directory_ids(strings(snapshot.directories.iter().map(String::as_str)));
        }
        if search_changed {
            let results = snapshot
                .search_results
                .iter()
                .map(|r| {
                    object([
                        ("id", text(r.id.as_str())),
                        ("snippet", text(&r.snippet)),
                        ("line", QVariant::from(&r.location.line)),
                        ("column", QVariant::from(&r.location.column)),
                        ("start", QVariant::from(&r.location.start_utf16)),
                        ("end", QVariant::from(&r.location.end_utf16)),
                    ])
                })
                .collect();
            self.as_mut().set_search_results(results);
        }
        if filter_changed {
            self.as_mut()
                .set_file_filter_query(QString::from(snapshot.file_filter.query.trim()));
            self.as_mut().set_file_filter_files(strings(
                snapshot.file_filter.files.iter().map(String::as_str),
            ));
            self.as_mut().set_file_filter_directories(strings(
                snapshot.file_filter.directories.iter().map(String::as_str),
            ));
        }
        self.as_mut()
            .set_file_filter_in_progress(snapshot.file_filter_in_progress);
        self.as_mut()
            .set_search_match_case(snapshot.search_options.match_case);
        self.as_mut()
            .set_search_whole_word(snapshot.search_options.whole_word);
        self.as_mut()
            .set_search_regex(snapshot.search_options.regex);
        self.as_mut()
            .set_search_include(QString::from(&snapshot.search_options.include));
        self.as_mut()
            .set_search_exclude(QString::from(&snapshot.search_options.exclude));
        self.as_mut()
            .set_search_ignore(snapshot.search_options.use_ignore_files);
        self.as_mut()
            .set_search_open_editors(snapshot.search_options.only_open_editors);
        self.as_mut().set_search_error(QString::from(
            snapshot.search_error.as_deref().unwrap_or_default(),
        ));
        self.as_mut().set_replacement_occurrences(
            snapshot
                .replacement_preview
                .as_ref()
                .map_or(0, |preview| preview.occurrences),
        );
        self.as_mut().set_replacement_files(
            snapshot
                .replacement_preview
                .as_ref()
                .map_or(0, |preview| preview.file_count),
        );
        self.as_mut().set_replacement_preview(QString::from(
            snapshot
                .replacement_preview
                .as_ref()
                .map_or("", |preview| preview.text.as_str()),
        ));
        self.as_mut()
            .set_search_query(QString::from(&snapshot.search_query));
        self.as_mut()
            .set_search_in_progress(snapshot.search_in_progress);
        self.as_mut()
            .set_search_truncated(snapshot.search_truncated);
        self.as_mut()
            .set_workspace_name(QString::from(snapshot.workspace_name().unwrap_or_default()));
        self.as_mut().set_workspace_root(QString::from(
            snapshot
                .workspace_root()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ));
        self.as_mut().set_recent_workspace_paths(strings(
            snapshot
                .recent_workspaces
                .iter()
                .map(|w| w.root.to_str().unwrap_or_default()),
        ));
        self.as_mut().set_recent_workspace_names(strings(
            snapshot.recent_workspaces.iter().map(|w| w.name.as_str()),
        ));
        self.as_mut().set_open_document_ids(strings(
            snapshot.open_documents.iter().map(DocumentId::as_str),
        ));
        self.as_mut().set_preview_document_id(QString::from(
            snapshot
                .preview_document
                .as_ref()
                .map_or("", DocumentId::as_str),
        ));
        if let Some(doc) = &snapshot.active_document {
            self.as_mut()
                .set_active_document_id(QString::from(doc.id.as_str()));
            self.as_mut()
                .set_active_document_text(QString::from(doc.content.as_ref()));
            self.as_mut().set_active_document_dirty(doc.dirty);
            self.as_mut()
                .set_active_document_external_change(doc.external_change);
        } else {
            self.as_mut().set_active_document_id(QString::default());
            self.as_mut().set_active_document_text(QString::default());
            self.as_mut().set_active_document_dirty(false);
            self.as_mut().set_active_document_external_change(false);
        }
        self.as_mut().set_pending_move_target(QString::from(
            snapshot
                .activity
                .relocation
                .as_ref()
                .filter(|_| snapshot.activity.relocation_slow)
                .map_or("", |r| r.target.as_str()),
        ));
        self.as_mut().set_move_pending(snapshot.activity.busy);
        self.as_mut()
            .set_editing_paused(snapshot.activity.editing_paused);
        self.as_mut()
            .set_error_message(QString::from(snapshot.error.as_deref().unwrap_or_default()));
        self.as_mut().set_revision(snapshot.revision);
        self.as_mut().set_snapshot_updating(false);
        if let Some(started) = started {
            observer.snapshot_applied(
                snapshot.diagnostic_operation,
                snapshot.revision,
                started.elapsed().as_nanos() as u64,
            );
        }
        self.as_mut().rust_mut().published = Some(snapshot);
    }
    pub fn confirmation(self: Pin<&mut Self>, value: &QString, deleting: bool) -> QVariant {
        let value = value.to_string();
        let prompt = if deleting {
            native_workspace_application::delete_confirmation(&value)
        } else {
            native_workspace_application::close_confirmation(&value)
        };
        object([
            ("title", text(&prompt.title)),
            ("body", text(prompt.body)),
            ("confirm", text(prompt.confirm)),
            ("cancel", text(prompt.cancel)),
            ("discard", text(prompt.discard)),
        ])
    }
    #[cfg(debug_assertions)]
    pub fn delay_next_background_job(mut self: Pin<&mut Self>, milliseconds: u64) {
        self.as_mut()
            .rust_mut()
            .application
            .delay_next_background_job(std::time::Duration::from_millis(milliseconds));
    }
    pub fn initialize_persistence(mut self: Pin<&mut Self>) -> bool {
        self.as_mut().rust_mut().application.enable_devtools("qt");
        self.as_mut()
            .set_devtools_available(Session::devtools_supported());
        let ready = self
            .as_mut()
            .rust_mut()
            .application
            .enable_persistence_default()
            .is_ok();
        self.publish();
        ready
    }
    pub fn restore_session(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::RestoreSession);
    }
    pub fn prepare_to_quit(mut self: Pin<&mut Self>, save: bool) -> bool {
        let ready = self
            .as_mut()
            .rust_mut()
            .application
            .dispatch(AppCommand::PrepareToQuit { save })
            .is_ok();
        self.publish();
        ready
    }
    pub fn quit_confirmation(self: Pin<&mut Self>) -> QVariant {
        let prompt = native_workspace_application::quit_confirmation();
        object([
            ("title", text(&prompt.title)),
            ("body", text(prompt.body)),
            ("confirm", text(prompt.confirm)),
            ("cancel", text(prompt.cancel)),
            ("discard", text(prompt.discard)),
        ])
    }
    pub fn diagnostic_ui_heartbeat(self: Pin<&mut Self>) {
        self.rust().application.diagnostic_observer().ui_heartbeat();
    }
    pub fn begin_sidebar_drop(mut self: Pin<&mut Self>) {
        let trace = native_workspace_application::SidebarDropTrace::begin(
            self.rust().application.diagnostic_observer(),
            2,
        );
        self.as_mut().rust_mut().drop_trace = Some(trace);
    }
    pub fn drop_preview_hidden(self: Pin<&mut Self>) {
        if let Some(t) = &self.rust().drop_trace {
            t.preview_hidden();
        }
    }
    pub fn drop_handler_finished(self: Pin<&mut Self>, accepted: bool) {
        if let Some(t) = &self.rust().drop_trace {
            t.handler_finished(accepted);
        }
    }
    pub fn drop_sidebar_ready(self: Pin<&mut Self>) {
        if let Some(t) = &self.rust().drop_trace {
            t.sidebar_ready();
        }
    }
    pub fn open_devtools(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().application.start_devtools("qt");
        self.rust().application.diagnostic_observer().open_console();
    }
    pub fn search_files(self: Pin<&mut Self>, query: &QString) {
        self.dispatch(AppCommand::QueueSearchQuery(query.to_string()));
    }
    pub fn configure_search(self: Pin<&mut Self>, match_case: bool, whole_word: bool, regex: bool) {
        let mut options = self.rust().application.snapshot().search_options;
        options.match_case = match_case;
        options.whole_word = whole_word;
        options.regex = regex;
        self.dispatch(AppCommand::ConfigureSearch(options));
    }
    pub fn configure_search_files(
        self: Pin<&mut Self>,
        include: &QString,
        exclude: &QString,
        ignore: bool,
        open_editors: bool,
    ) {
        let mut options = self.rust().application.snapshot().search_options;
        options.include = include.to_string();
        options.exclude = exclude.to_string();
        options.use_ignore_files = ignore;
        options.only_open_editors = open_editors;
        self.dispatch(AppCommand::ConfigureSearch(options));
    }
    pub fn replacement_confirmation(
        self: Pin<&mut Self>,
        occurrences: u64,
        files: u64,
    ) -> QVariant {
        let prompt = native_workspace_application::replacement_confirmation(occurrences, files);
        object([
            ("title", text(&prompt.title)),
            ("body", text(prompt.body)),
            ("confirm", text(prompt.confirm)),
            ("cancel", text(prompt.cancel)),
        ])
    }
    pub fn refresh_search(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::RefreshSearch);
    }
    pub fn cancel_search(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::CancelSearch);
    }
    pub fn prepare_replacement(
        mut self: Pin<&mut Self>,
        replacement: &QString,
        preserve: bool,
        path: &QString,
        result_index: i32,
    ) {
        let path = path.to_string();
        let target = self.rust().application.snapshot().replacement_target(
            (!path.is_empty()).then_some(path.as_str()),
            u64::try_from(result_index).ok(),
        );
        match target {
            Ok(target) => self.dispatch(AppCommand::PrepareReplacement {
                replacement: replacement.to_string(),
                preserve_case: preserve,
                target,
            }),
            Err(error) => {
                self.as_mut().rust_mut().application.report(error);
                self.publish();
            }
        }
    }
    pub fn apply_replacement(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::ApplyReplacement);
    }
    pub fn discard_replacement(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::DiscardReplacement);
    }
    pub fn filter_files(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut()
            .rust_mut()
            .application
            .filter_files(&query.to_string());
        self.publish();
    }
    pub fn file_icon(self: Pin<&mut Self>, value: &QString) -> QString {
        QString::from(format!(
            "qrc:/qt/qml/org/nativeworkspace/Diagnostic/material/{}.png",
            file_icon_name(&value.to_string())
        ))
    }
    pub fn folder_icon(self: Pin<&mut Self>, value: &QString, expanded: bool) -> QString {
        QString::from(format!(
            "qrc:/qt/qml/org/nativeworkspace/Diagnostic/material/{}.png",
            folder_icon_name(&value.to_string(), expanded)
        ))
    }
    pub fn open_files(self: Pin<&mut Self>, urls: &QStringList) {
        match local_file_paths(urls) {
            Some(paths) => self.dispatch(AppCommand::OpenFiles(paths)),
            None => self.reject_file_urls(),
        }
    }
    pub fn open_paths(self: Pin<&mut Self>, urls: &QStringList) {
        match local_file_paths(urls) {
            Some(paths) => self.dispatch(AppCommand::OpenPaths(paths)),
            None => self.reject_file_urls(),
        }
    }
    pub fn move_files(self: Pin<&mut Self>, urls: &QStringList, directory: &QString) {
        match local_file_paths(urls) {
            Some(paths) => self.dispatch(AppCommand::MoveFiles {
                paths,
                directory: directory.to_string(),
            }),
            None => self.reject_file_urls(),
        }
    }
    pub fn can_move_files(self: Pin<&mut Self>, urls: &QStringList, directory: &QString) -> bool {
        local_file_paths(urls).is_some_and(|paths| {
            self.rust()
                .application
                .can_move_files(&paths, &directory.to_string())
        })
    }
    fn reject_file_urls(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .application
            .report(AppError::InvalidEntryName);
        self.publish();
    }
    pub fn open_workspace(self: Pin<&mut Self>, input: &QString) {
        let url = QUrl::from_user_input(input, &QString::default());
        let path = url
            .to_local_file()
            .map(|v| PathBuf::from(v.to_string()))
            .unwrap_or_else(|| PathBuf::from(input.to_string()));
        self.dispatch(AppCommand::OpenWorkspace(path));
    }
    pub fn sidebar_children(self: Pin<&mut Self>, parent: &QString) -> QList<QVariant> {
        self.rust()
            .application
            .snapshot()
            .sidebar_children(&parent.to_string())
            .into_iter()
            .map(|c| {
                object([
                    ("path", text(&c.path)),
                    ("directory", QVariant::from(&c.directory)),
                ])
            })
            .collect()
    }
    pub fn sidebar_has_children(self: Pin<&mut Self>, parent: &QString) -> bool {
        self.rust()
            .application
            .snapshot()
            .sidebar_has_children(&parent.to_string())
    }
    pub fn sidebar_rows(
        self: Pin<&mut Self>,
        search: bool,
        expanded: &QStringList,
    ) -> QList<QVariant> {
        let snapshot = self.rust().application.snapshot();
        let expanded: std::collections::HashSet<_> =
            expanded.iter().map(ToString::to_string).collect();
        let rows = if search {
            snapshot.search_rows()
        } else if snapshot.file_filter.is_active() {
            snapshot.file_filter_rows()
        } else {
            snapshot.file_tree_rows(|p| expanded.contains(p))
        };
        rows.into_iter()
            .map(|r| {
                object([
                    ("path", text(&r.path)),
                    ("directory", QVariant::from(&r.is_directory())),
                    (
                        "result",
                        QVariant::from(&match r.kind {
                            SidebarRowKind::Match { result } => result as i64,
                            _ => -1,
                        }),
                    ),
                ])
            })
            .collect()
    }
    pub fn can_move_entry(self: Pin<&mut Self>, source: &QString, directory: &QString) -> bool {
        DocumentId::parse(source.to_string()).is_ok_and(|id| {
            self.rust()
                .application
                .can_move_entry(id, directory.to_string())
        })
    }
    pub fn move_entry(
        mut self: Pin<&mut Self>,
        value: &QString,
        directory: &QString,
        content: &QString,
    ) {
        if let Ok(id) = DocumentId::from_document_path(self.rust().active_document_id.to_string()) {
            let _ = self
                .as_mut()
                .rust_mut()
                .application
                .dispatch(AppCommand::ChangeDocument {
                    id,
                    content: content.to_string(),
                });
        }
        let trace = self.rust().drop_trace.clone().unwrap_or_default();
        match DocumentId::parse(value.to_string()) {
            Ok(id) => {
                let _ = self.as_mut().rust_mut().application.dispatch_traced(
                    AppCommand::MoveEntry {
                        id,
                        directory: directory.to_string(),
                    },
                    trace,
                );
            }
            Err(_) => self
                .as_mut()
                .rust_mut()
                .application
                .report(AppError::InvalidEntryName),
        }
        self.publish();
    }
    pub fn rename_entry(self: Pin<&mut Self>, value: &QString, name: &QString) {
        self.with_id(value, |id| AppCommand::RenameEntry {
            id,
            new_name: name.to_string(),
        });
    }
    pub fn delete_entry(self: Pin<&mut Self>, value: &QString) {
        self.with_id(value, AppCommand::DeleteEntry);
    }
    pub fn create_entry(self: Pin<&mut Self>, value: &QString, directory: bool) {
        self.with_id(value, |id| AppCommand::CreateEntry { id, directory });
    }
    pub fn refresh_workspace(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::RefreshWorkspace);
    }
    pub fn open_document(self: Pin<&mut Self>, value: &QString) {
        self.with_id(value, AppCommand::OpenDocument);
    }
    pub fn preview_document(self: Pin<&mut Self>, value: &QString) {
        self.with_id(value, AppCommand::PreviewDocument);
    }
    pub fn keep_document_open(self: Pin<&mut Self>, value: &QString) {
        self.with_id(value, AppCommand::KeepDocumentOpen);
    }
    pub fn change_document(self: Pin<&mut Self>, content: &QString) {
        let value = self.rust().active_document_id.clone();
        self.with_id(&value, |id| AppCommand::ChangeDocument {
            id,
            content: content.to_string(),
        });
    }
    pub fn save_document(self: Pin<&mut Self>) {
        let value = self.rust().active_document_id.clone();
        self.with_id(&value, AppCommand::SaveDocument);
    }
    pub fn reload_document(self: Pin<&mut Self>) {
        let value = self.rust().active_document_id.clone();
        self.with_id(&value, AppCommand::ReloadDocument);
    }
    pub fn overwrite_document(self: Pin<&mut Self>) {
        let value = self.rust().active_document_id.clone();
        self.with_id(&value, AppCommand::OverwriteDocument);
    }
    pub fn save_all_documents(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::SaveAllDocuments);
    }
    pub fn close_document(self: Pin<&mut Self>, value: &QString) {
        self.with_id(value, AppCommand::CloseDocument);
    }
    pub fn discard_and_close_document(self: Pin<&mut Self>, value: &QString) {
        self.with_id(value, AppCommand::DiscardAndCloseDocument);
    }
    pub fn save_and_close_document(self: Pin<&mut Self>, value: &QString) {
        self.with_id(value, AppCommand::SaveAndCloseDocument);
    }
    pub fn highlight_document(self: Pin<&mut Self>, content: &QString) -> QList<QVariant> {
        native_workspace_syntax::highlight(
            &self.rust().active_document_id.to_string(),
            &content.to_string(),
        )
        .into_iter()
        .map(|t| {
            object([
                ("start", QVariant::from(&(t.start_utf16 as u64))),
                ("end", QVariant::from(&(t.end_utf16 as u64))),
                ("kind", QVariant::from(&(t.kind.code() as u32))),
            ])
        })
        .collect()
    }
    pub fn minimap_glyphs(self: Pin<&mut Self>) -> QList<QVariant> {
        native_workspace_syntax::minimap_glyphs()
            .iter()
            .map(|v| QVariant::from(&u32::from(*v)))
            .collect()
    }
}

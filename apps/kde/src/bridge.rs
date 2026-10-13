use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QStringList, QUrl};
use native_workspace_application::{
    AppCommand, AppSnapshot, Application, file_icon_name, folder_icon_name,
};
use native_workspace_domain::DocumentId;
use std::{path::PathBuf, pin::Pin};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, devtools_available)]
        #[qproperty(u64, counter)]
        #[qproperty(u64, revision)]
        #[qproperty(QString, workspace_name)]
        #[qproperty(QString, workspace_root)]
        #[qproperty(QString, search_query)]
        #[qproperty(bool, search_in_progress)]
        #[qproperty(QStringList, search_result_ids)]
        #[qproperty(QStringList, search_result_snippets)]
        #[qproperty(QStringList, search_result_lines)]
        #[qproperty(QStringList, search_result_columns)]
        #[qproperty(QStringList, search_result_starts)]
        #[qproperty(QStringList, search_result_ends)]
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
        #[qproperty(QString, error_message)]
        type AppController = super::AppControllerRust;

        #[qinvokable]
        fn open_devtools(self: Pin<&mut Self>);
        #[qinvokable]
        fn increment(self: Pin<&mut Self>);
        #[qinvokable]
        fn initialize_persistence(self: Pin<&mut Self>);
        #[qinvokable]
        fn search_files(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        fn file_icon(self: Pin<&mut Self>, path: &QString) -> QString;
        #[qinvokable]
        fn folder_icon(self: Pin<&mut Self>, path: &QString, expanded: bool) -> QString;
        #[qinvokable]
        fn open_workspace(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        fn move_entry(self: Pin<&mut Self>, path: &QString, directory: &QString);
        #[qinvokable]
        fn rename_entry(self: Pin<&mut Self>, path: &QString, new_name: &QString);
        #[qinvokable]
        fn create_entry(self: Pin<&mut Self>, path: &QString, directory: bool);
        #[qinvokable]
        fn refresh_workspace(self: Pin<&mut Self>);
        #[qinvokable]
        fn poll_workspace_changes(self: Pin<&mut Self>);
        #[qinvokable]
        fn poll_search_results(self: Pin<&mut Self>);
        #[qinvokable]
        fn open_document(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn change_document(self: Pin<&mut Self>, text: &QString);
        #[qinvokable]
        fn highlight_document(self: Pin<&mut Self>, text: &QString) -> QStringList;
        #[qinvokable]
        fn minimap_glyphs(self: Pin<&mut Self>) -> QString;
        #[qinvokable]
        fn save_document(self: Pin<&mut Self>);
        #[qinvokable]
        fn reload_document(self: Pin<&mut Self>);
        #[qinvokable]
        fn overwrite_document(self: Pin<&mut Self>);
    }
}

#[derive(Default)]
pub struct AppControllerRust {
    counter: u64,
    revision: u64,
    workspace_name: QString,
    workspace_root: QString,
    search_query: QString,
    search_in_progress: bool,
    search_result_ids: QStringList,
    search_result_snippets: QStringList,
    search_result_lines: QStringList,
    search_result_columns: QStringList,
    search_result_starts: QStringList,
    search_result_ends: QStringList,
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
    error_message: QString,
    devtools_available: bool,
    application: Application,
    background_move_error: Option<String>,
}

impl qobject::AppController {
    pub fn minimap_glyphs(self: Pin<&mut Self>) -> QString {
        use std::fmt::Write;
        let mut encoded = String::with_capacity(384);
        for byte in native_workspace_syntax::minimap_glyphs() {
            write!(encoded, "{byte:02X}").expect("write into string");
        }
        QString::from(encoded)
    }

    pub fn highlight_document(self: Pin<&mut Self>, text: &QString) -> QStringList {
        let file_name = self.rust().active_document_id.to_string();
        let text = text.to_string();
        native_workspace_syntax::highlight(&file_name, &text)
            .into_iter()
            .map(|token| {
                QString::from(format!(
                    "{}:{}:{}",
                    token.start_utf16,
                    token.end_utf16,
                    token.kind.code()
                ))
            })
            .collect()
    }

    fn dispatch(mut self: Pin<&mut Self>, command: AppCommand) {
        self.as_mut().rust_mut().application.enable_devtools("qt");
        self.as_mut().rust_mut().background_move_error = None;
        let result = self.as_mut().rust_mut().application.dispatch(command);
        match result {
            Ok(snapshot) => {
                self.as_mut().set_error_message(QString::default());
                self.as_mut().update_snapshot(snapshot);
            }
            Err(error) => self
                .as_mut()
                .set_error_message(QString::from(error.to_string())),
        }
        self.as_mut().report_background_error();
    }

    fn report_background_error(mut self: Pin<&mut Self>) {
        if let Some(error) = self.as_mut().rust_mut().application.take_background_error() {
            self.as_mut().rust_mut().background_move_error = Some(error.clone());
            let snapshot = self.rust().application.snapshot();
            self.as_mut().update_snapshot(snapshot);
            self.set_error_message(QString::from(error));
        }
    }

    fn update_snapshot(mut self: Pin<&mut Self>, snapshot: AppSnapshot) {
        let diagnostic_started = self
            .rust()
            .application
            .devtools_enabled()
            .then(std::time::Instant::now);
        let diagnostic_operation = snapshot.diagnostic_operation;
        let diagnostic_revision = snapshot.revision;
        self.as_mut().set_revision(snapshot.revision);
        self.as_mut().set_counter(snapshot.diagnostic_counter);
        self.as_mut()
            .set_search_query(QString::from(snapshot.search_query.0));
        self.as_mut()
            .set_search_in_progress(snapshot.search_in_progress);
        self.as_mut().set_search_result_ids(
            snapshot
                .search_results
                .iter()
                .map(|result| QString::from(result.id.as_str()))
                .collect(),
        );
        self.as_mut().set_search_result_snippets(
            snapshot
                .search_results
                .iter()
                .map(|result| QString::from(result.snippet.as_deref().unwrap_or("")))
                .collect(),
        );
        self.as_mut().set_search_result_lines(
            snapshot
                .search_results
                .iter()
                .map(|result| {
                    QString::from(
                        result
                            .location
                            .as_ref()
                            .map(|location| location.line.to_string())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
        );
        self.as_mut().set_search_result_columns(
            snapshot
                .search_results
                .iter()
                .map(|result| {
                    QString::from(
                        result
                            .location
                            .as_ref()
                            .map(|location| location.column.to_string())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
        );
        self.as_mut().set_search_result_starts(
            snapshot
                .search_results
                .iter()
                .map(|result| {
                    QString::from(
                        result
                            .location
                            .as_ref()
                            .map(|location| location.start_utf16.to_string())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
        );
        self.as_mut().set_search_result_ends(
            snapshot
                .search_results
                .iter()
                .map(|result| {
                    QString::from(
                        result
                            .location
                            .as_ref()
                            .map(|location| location.end_utf16.to_string())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
        );
        self.as_mut().set_recent_workspace_paths(
            snapshot
                .recent_workspaces
                .iter()
                .map(|item| QString::from(item.root.to_string_lossy().as_ref()))
                .collect(),
        );
        self.as_mut().set_recent_workspace_names(
            snapshot
                .recent_workspaces
                .iter()
                .map(|item| QString::from(item.name.as_str()))
                .collect(),
        );
        self.as_mut()
            .set_workspace_name(QString::from(snapshot.workspace_name.unwrap_or_default()));
        self.as_mut().set_workspace_root(QString::from(
            snapshot
                .workspace_root
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ));
        self.as_mut().set_document_ids(
            snapshot
                .documents
                .iter()
                .map(|doc| QString::from(doc.id.as_str()))
                .collect(),
        );
        self.as_mut().set_directory_ids(
            snapshot
                .directories
                .iter()
                .map(|path| QString::from(path.as_str()))
                .collect(),
        );
        self.as_mut().set_dirty_document_ids(
            snapshot
                .documents
                .iter()
                .filter(|doc| doc.dirty)
                .map(|doc| QString::from(doc.id.as_str()))
                .collect(),
        );
        self.as_mut().set_conflicted_document_ids(
            snapshot
                .documents
                .iter()
                .filter(|doc| doc.external_change)
                .map(|doc| QString::from(doc.id.as_str()))
                .collect(),
        );
        match snapshot.active_document {
            Some(doc) => {
                self.as_mut()
                    .set_active_document_id(QString::from(doc.id.as_str()));
                self.as_mut()
                    .set_active_document_text(QString::from(doc.content));
                self.as_mut().set_active_document_dirty(doc.dirty);
                self.as_mut()
                    .set_active_document_external_change(doc.external_change);
            }
            None => {
                self.as_mut().set_active_document_id(QString::default());
                self.as_mut().set_active_document_text(QString::default());
                self.as_mut().set_active_document_dirty(false);
                self.as_mut().set_active_document_external_change(false);
            }
        }
        if let Some(started) = diagnostic_started {
            self.rust().application.diagnostic_snapshot_applied(
                diagnostic_operation,
                diagnostic_revision,
                started.elapsed().as_nanos() as u64,
            );
        }
    }

    pub fn increment(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::IncrementDiagnostic);
    }

    pub fn search_files(self: Pin<&mut Self>, query: &QString) {
        self.dispatch(AppCommand::QueueSearchQuery(query.to_string()));
    }

    pub fn file_icon(self: Pin<&mut Self>, path: &QString) -> QString {
        QString::from(format!(
            "qrc:/qt/qml/org/nativeworkspace/Diagnostic/material/{}.png",
            file_icon_name(&path.to_string())
        ))
    }

    pub fn folder_icon(self: Pin<&mut Self>, path: &QString, expanded: bool) -> QString {
        QString::from(format!(
            "qrc:/qt/qml/org/nativeworkspace/Diagnostic/material/{}.png",
            folder_icon_name(&path.to_string(), expanded)
        ))
    }

    pub fn open_devtools(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().application.start_devtools("qt");
        self.rust().application.open_devtools();
    }

    pub fn initialize_persistence(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().application.enable_devtools("qt");
        let available = self.rust().application.devtools_supported();
        self.as_mut().set_devtools_available(available);
        let result = self
            .as_mut()
            .rust_mut()
            .application
            .enable_persistence_default();
        match result {
            Ok(snapshot) => self.update_snapshot(snapshot),
            Err(error) => self.set_error_message(QString::from(error.to_string())),
        }
    }

    pub fn open_workspace(self: Pin<&mut Self>, input: &QString) {
        let url = QUrl::from_user_input(input, &QString::default());
        let path = url
            .to_local_file()
            .map(|value| PathBuf::from(value.to_string()))
            .unwrap_or_else(|| PathBuf::from(input.to_string()));
        self.dispatch(AppCommand::OpenWorkspace(path));
    }

    pub fn move_entry(mut self: Pin<&mut Self>, path: &QString, directory: &QString) {
        match DocumentId::parse(path.to_string()) {
            Ok(id) => self.as_mut().dispatch(AppCommand::MoveEntryOptimistic {
                id,
                directory: directory.to_string(),
            }),
            Err(_) => self.set_error_message(QString::from("Invalid relative path")),
        }
    }

    pub fn rename_entry(mut self: Pin<&mut Self>, path: &QString, new_name: &QString) {
        match DocumentId::parse(path.to_string()) {
            Ok(id) => self.as_mut().dispatch(AppCommand::RenameEntry {
                id,
                new_name: new_name.to_string(),
            }),
            Err(_) => self.set_error_message(QString::from("Invalid relative path")),
        }
    }

    pub fn create_entry(mut self: Pin<&mut Self>, path: &QString, directory: bool) {
        match DocumentId::parse(path.to_string()) {
            Ok(id) => self
                .as_mut()
                .dispatch(AppCommand::CreateEntry { id, directory }),
            Err(_) => self.set_error_message(QString::from("Invalid relative path")),
        }
    }

    pub fn refresh_workspace(mut self: Pin<&mut Self>) {
        let result = self
            .as_mut()
            .rust_mut()
            .application
            .dispatch(AppCommand::RefreshWorkspace);
        match result {
            Ok(snapshot) if snapshot.revision != self.rust().revision => {
                self.as_mut().set_error_message(QString::default());
                self.update_snapshot(snapshot);
            }
            Ok(_) => {}
            Err(error) => self.set_error_message(QString::from(error.to_string())),
        }
    }

    pub fn poll_workspace_changes(mut self: Pin<&mut Self>) {
        let result = self
            .as_mut()
            .rust_mut()
            .application
            .dispatch_if_changed(AppCommand::PollWorkspaceChanges);
        match result {
            Ok(Some(snapshot)) => {
                if self.rust().background_move_error.is_none() {
                    self.as_mut().set_error_message(QString::default());
                }
                self.as_mut().update_snapshot(snapshot);
            }
            Ok(None) => {}
            Err(error) => self
                .as_mut()
                .set_error_message(QString::from(error.to_string())),
        }
        self.as_mut().report_background_error();
    }

    pub fn poll_search_results(mut self: Pin<&mut Self>) {
        let result = self
            .as_mut()
            .rust_mut()
            .application
            .dispatch_if_changed(AppCommand::PollSearchResults);
        match result {
            Ok(Some(snapshot)) => self.as_mut().update_snapshot(snapshot),
            Ok(None) => {}
            Err(error) => self
                .as_mut()
                .set_error_message(QString::from(error.to_string())),
        }
        self.as_mut().report_background_error();
    }

    pub fn open_document(mut self: Pin<&mut Self>, id: &QString) {
        match DocumentId::parse(id.to_string()) {
            Ok(id) => {
                if !self.rust().search_query.to_string().trim().is_empty() {
                    self.dispatch(AppCommand::OpenDocument(id));
                    return;
                }
                let result = self
                    .as_mut()
                    .rust_mut()
                    .application
                    .dispatch(AppCommand::OpenDocument(id));
                match result {
                    Ok(snapshot) => {
                        self.as_mut().set_error_message(QString::default());
                        self.as_mut().set_revision(snapshot.revision);
                        if let Some(doc) = snapshot.active_document {
                            self.as_mut()
                                .set_active_document_id(QString::from(doc.id.as_str()));
                            self.as_mut()
                                .set_active_document_text(QString::from(doc.content));
                            self.as_mut().set_active_document_dirty(doc.dirty);
                            self.as_mut()
                                .set_active_document_external_change(doc.external_change);
                        }
                    }
                    Err(error) => self
                        .as_mut()
                        .set_error_message(QString::from(error.to_string())),
                }
            }
            Err(_) => self
                .as_mut()
                .set_error_message(QString::from("Invalid document name")),
        }
        self.as_mut().report_background_error();
    }

    pub fn change_document(mut self: Pin<&mut Self>, text: &QString) {
        let id = self.rust().active_document_id.to_string();
        match DocumentId::parse(id) {
            Ok(id) => self.dispatch(AppCommand::ChangeDocument {
                id,
                content: text.to_string(),
            }),
            Err(_) => self
                .as_mut()
                .set_error_message(QString::from("No document is open")),
        }
    }

    pub fn save_document(mut self: Pin<&mut Self>) {
        let id = self.rust().active_document_id.to_string();
        match DocumentId::parse(id) {
            Ok(id) => self.dispatch(AppCommand::SaveDocument(id)),
            Err(_) => self
                .as_mut()
                .set_error_message(QString::from("No document is open")),
        }
    }

    pub fn reload_document(mut self: Pin<&mut Self>) {
        let id = self.rust().active_document_id.to_string();
        match DocumentId::parse(id) {
            Ok(id) => self.dispatch(AppCommand::ReloadDocument(id)),
            Err(_) => self
                .as_mut()
                .set_error_message(QString::from("No document is open")),
        }
    }

    pub fn overwrite_document(mut self: Pin<&mut Self>) {
        let id = self.rust().active_document_id.to_string();
        match DocumentId::parse(id) {
            Ok(id) => self.dispatch(AppCommand::OverwriteDocument(id)),
            Err(_) => self
                .as_mut()
                .set_error_message(QString::from("No document is open")),
        }
    }
}

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QStringList, QUrl};
use native_workspace_application::{AppCommand, AppSnapshot, Application};
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
        #[qproperty(u64, counter)]
        #[qproperty(u64, revision)]
        #[qproperty(QString, workspace_name)]
        #[qproperty(QStringList, document_ids)]
        #[qproperty(QStringList, dirty_document_ids)]
        #[qproperty(QStringList, recent_workspace_paths)]
        #[qproperty(QStringList, recent_workspace_names)]
        #[qproperty(QString, active_document_id)]
        #[qproperty(QString, active_document_text)]
        #[qproperty(bool, active_document_dirty)]
        #[qproperty(QString, error_message)]
        type AppController = super::AppControllerRust;

        #[qinvokable]
        fn increment(self: Pin<&mut Self>);
        #[qinvokable]
        fn initialize_persistence(self: Pin<&mut Self>);
        #[qinvokable]
        fn open_workspace(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        fn open_document(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        fn change_document(self: Pin<&mut Self>, text: &QString);
        #[qinvokable]
        fn save_document(self: Pin<&mut Self>);
    }
}

#[derive(Default)]
pub struct AppControllerRust {
    counter: u64,
    revision: u64,
    workspace_name: QString,
    document_ids: QStringList,
    dirty_document_ids: QStringList,
    recent_workspace_paths: QStringList,
    recent_workspace_names: QStringList,
    active_document_id: QString,
    active_document_text: QString,
    active_document_dirty: bool,
    error_message: QString,
    application: Application,
}

impl qobject::AppController {
    fn dispatch(mut self: Pin<&mut Self>, command: AppCommand) {
        let result = self.as_mut().rust_mut().application.dispatch(command);
        match result {
            Ok(snapshot) => {
                self.as_mut().set_error_message(QString::default());
                self.update_snapshot(snapshot);
            }
            Err(error) => self.set_error_message(QString::from(error.to_string())),
        }
    }

    fn update_snapshot(mut self: Pin<&mut Self>, snapshot: AppSnapshot) {
        self.as_mut().set_revision(snapshot.revision);
        self.as_mut().set_counter(snapshot.diagnostic_counter);
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
        self.as_mut().set_document_ids(
            snapshot
                .documents
                .iter()
                .map(|doc| QString::from(doc.id.as_str()))
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
        match snapshot.active_document {
            Some(doc) => {
                self.as_mut()
                    .set_active_document_id(QString::from(doc.id.as_str()));
                self.as_mut()
                    .set_active_document_text(QString::from(doc.content));
                self.set_active_document_dirty(doc.dirty);
            }
            None => {
                self.as_mut().set_active_document_id(QString::default());
                self.as_mut().set_active_document_text(QString::default());
                self.set_active_document_dirty(false);
            }
        }
    }

    pub fn increment(self: Pin<&mut Self>) {
        self.dispatch(AppCommand::IncrementDiagnostic);
    }

    pub fn initialize_persistence(mut self: Pin<&mut Self>) {
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

    pub fn open_document(mut self: Pin<&mut Self>, id: &QString) {
        match DocumentId::parse(id.to_string()) {
            Ok(id) => self.dispatch(AppCommand::OpenDocument(id)),
            Err(_) => self
                .as_mut()
                .set_error_message(QString::from("Invalid document name")),
        }
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
}

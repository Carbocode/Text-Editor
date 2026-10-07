use cxx_qt::CxxQtType;
use native_workspace_application::{AppCommand, Application};
use std::pin::Pin;

#[cxx_qt::bridge]
pub mod qobject {
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(u64, counter)]
        #[qproperty(u64, revision)]
        type AppController = super::AppControllerRust;

        #[qinvokable]
        fn increment(self: Pin<&mut Self>);
    }
}

#[derive(Default)]
pub struct AppControllerRust {
    counter: u64,
    revision: u64,
    application: Application,
}

impl qobject::AppController {
    pub fn increment(mut self: Pin<&mut Self>) {
        let result = self
            .as_mut()
            .rust_mut()
            .application
            .dispatch(AppCommand::IncrementDiagnostic);
        match result {
            Ok(snapshot) => {
                self.as_mut().set_counter(snapshot.diagnostic_counter);
                self.set_revision(snapshot.revision);
            }
            Err(error) => eprintln!("Increment failed: {error:?}"),
        }
    }
}

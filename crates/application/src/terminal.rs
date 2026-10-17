//! Transient terminal command/event state owned by the window's Session.
use crate::AppError;
use native_workspace_terminal::{Process, Update};
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub use native_workspace_terminal::{
    Cell as TerminalCell, Frame as TerminalFrame, Input as TerminalInput, KeyInput as TerminalKey,
};

#[derive(Clone, Debug)]
pub enum TerminalCommand {
    New,
    Activate(u64),
    Close { id: u64, confirmed: bool },
    CloseAll { confirmed: bool },
    Input { id: u64, input: TerminalInput },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TerminalTab {
    pub id: u64,
    pub title: String,
    pub running: bool,
    pub status: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TerminalSnapshot {
    pub revision: u64,
    pub tabs: Vec<TerminalTab>,
    pub active: Option<u64>,
    pub frame: Option<Arc<TerminalFrame>>,
}

struct Tab {
    id: u64,
    process: Process,
    update: Update,
}
enum TerminalEvent {
    Opened(Tab),
    Activated(u64),
    Closed(u64),
    AllClosed,
    Updated(Vec<(u64, Update)>),
}
pub(crate) struct Terminals {
    tabs: Vec<Tab>,
    closing: Vec<Tab>,
    next_id: u64,
    live: Arc<AtomicUsize>,
    snapshot: Arc<TerminalSnapshot>,
}

impl Terminals {
    pub fn new(live: Arc<AtomicUsize>) -> Self {
        Self {
            tabs: Vec::new(),
            closing: Vec::new(),
            next_id: 1,
            live,
            snapshot: Arc::default(),
        }
    }
    pub fn snapshot(&self) -> Arc<TerminalSnapshot> {
        self.snapshot.clone()
    }
    pub fn running(&self) -> bool {
        self.live.load(Ordering::Acquire) > 0
    }
    pub fn dispatch(
        &mut self,
        command: TerminalCommand,
        root: Option<&Path>,
    ) -> Result<(), AppError> {
        let event = match command {
            TerminalCommand::New => {
                if self.tabs.len() + self.closing.len() >= 16 {
                    return Err(AppError::Terminal(
                        "Close a terminal before opening more (limit: 16).".into(),
                    ));
                }
                let root = root.ok_or(AppError::WorkspaceNotOpen)?;
                let id = self.next_id;
                id.checked_add(1).ok_or(AppError::RevisionOverflow)?;
                let process = Process::spawn(root.to_path_buf()).map_err(AppError::Terminal)?;
                TerminalEvent::Opened(Tab {
                    id,
                    process,
                    update: Update {
                        revision: 0,
                        title: format!("Terminal {id}"),
                        running: true,
                        status: "Starting…".into(),
                        frame: None,
                    },
                })
            }
            TerminalCommand::Activate(id) => {
                self.find(id)?;
                TerminalEvent::Activated(id)
            }
            TerminalCommand::Close { id, confirmed } => {
                let tab = self.find(id)?;
                if tab.update.running && !confirmed {
                    return Err(AppError::TerminalsRunning);
                }
                tab.process.stop();
                TerminalEvent::Closed(id)
            }
            TerminalCommand::CloseAll { confirmed } => {
                if self.running() && !confirmed {
                    return Err(AppError::TerminalsRunning);
                }
                for tab in &self.tabs {
                    tab.process.stop();
                }
                TerminalEvent::AllClosed
            }
            TerminalCommand::Input { id, input } => {
                let tab = self.find(id)?;
                if !tab.update.running {
                    return Err(AppError::Terminal(
                        "Terminal has exited. Open a new terminal.".into(),
                    ));
                }
                tab.process.send(input).map_err(AppError::Terminal)?;
                return Ok(());
            }
        };
        self.apply(event);
        Ok(())
    }
    /// Terminal lifecycle reducer; all published tab state changes here.
    fn apply(&mut self, event: TerminalEvent) {
        let mut active = self.snapshot.active;
        match event {
            TerminalEvent::Opened(tab) => {
                active = Some(tab.id);
                self.next_id = tab.id + 1;
                self.tabs.push(tab);
            }
            TerminalEvent::Activated(id) => active = Some(id),
            TerminalEvent::Closed(id) => {
                let index = self
                    .tabs
                    .iter()
                    .position(|t| t.id == id)
                    .expect("validated terminal");
                self.closing.push(self.tabs.remove(index));
                if active == Some(id) {
                    active = self
                        .tabs
                        .get(index.min(self.tabs.len().saturating_sub(1)))
                        .map(|t| t.id);
                }
            }
            TerminalEvent::AllClosed => {
                self.closing.append(&mut self.tabs);
                active = None;
            }
            TerminalEvent::Updated(updates) => {
                for (id, update) in updates {
                    if let Some(tab) = self
                        .tabs
                        .iter_mut()
                        .chain(&mut self.closing)
                        .find(|t| t.id == id)
                    {
                        tab.update = update;
                    }
                }
                self.closing.retain(|t| t.update.running);
            }
        }
        self.publish(active);
    }
    fn find(&self, id: u64) -> Result<&Tab, AppError> {
        self.tabs
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| AppError::Terminal("Terminal is no longer open.".into()))
    }
    pub fn poll(&mut self) {
        let updates = self
            .tabs
            .iter()
            .chain(&self.closing)
            .filter_map(|tab| tab.process.poll(tab.update.revision).map(|u| (tab.id, u)))
            .collect::<Vec<_>>();
        if !updates.is_empty() {
            self.apply(TerminalEvent::Updated(updates));
        }
    }
    fn publish(&mut self, active: Option<u64>) {
        self.live.store(
            self.tabs
                .iter()
                .chain(&self.closing)
                .filter(|t| t.update.running)
                .count(),
            Ordering::Release,
        );
        self.snapshot = Arc::new(TerminalSnapshot {
            revision: self.snapshot.revision + 1,
            tabs: self
                .tabs
                .iter()
                .map(|t| TerminalTab {
                    id: t.id,
                    title: t.update.title.clone(),
                    running: t.update.running,
                    status: t.update.status.clone(),
                })
                .collect(),
            active,
            frame: active
                .and_then(|id| self.tabs.iter().find(|t| t.id == id))
                .and_then(|t| t.update.frame.clone()),
        });
    }
}

pub fn terminal_close_confirmation() -> crate::Confirmation {
    crate::Confirmation {
        title: "Terminate terminal processes?".into(),
        body: "The shell and programs running in this terminal will be terminated.",
        confirm: "Terminate",
        cancel: "Cancel",
        discard: "",
    }
}

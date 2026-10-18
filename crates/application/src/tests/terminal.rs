use super::*;

#[test]
fn failed_save_on_quit_keeps_documents_and_running_terminals() {
    let dir = workspace(&[("a.txt", "original")]);
    let mut session = Session::with_application(open(dir.path(), TestFilesystem::real()));
    session
        .dispatch(AppCommand::OpenDocument(id("a.txt")))
        .unwrap();
    session.terminal_command(TerminalCommand::New).unwrap();
    let terminal = session.terminal_snapshot().active;
    session
        .dispatch(AppCommand::ChangeDocument {
            id: id("a.txt"),
            content: "unsaved edits".into(),
        })
        .unwrap();
    fs::write(dir.path().join("a.txt"), "external change").unwrap();
    assert!(matches!(
        session.dispatch(AppCommand::PrepareToQuit { save: true }),
        Err(AppError::ExternalChanges(_))
    ));
    assert!(session.snapshot().active_document.unwrap().dirty);
    assert_eq!(session.terminal_snapshot().active, terminal);
    assert!(session.has_running_terminals());
    session
        .terminal_command(TerminalCommand::CloseAll { confirmed: true })
        .unwrap();
    wait_until(
        || {
            session.poll();
            session.has_running_terminals()
        },
        |running| !*running,
    );
}

#[test]
fn terminal_tabs_are_independent_and_do_not_change_document_revisions() {
    let dir = workspace(&[("a.txt", "content")]);
    let mut session = Session::with_application(open(dir.path(), TestFilesystem::real()));
    session
        .dispatch(AppCommand::OpenDocument(id("a.txt")))
        .unwrap();
    let revision = session.revision();
    session.terminal_command(TerminalCommand::New).unwrap();
    let first = session.terminal_snapshot().active.unwrap();
    session.terminal_command(TerminalCommand::New).unwrap();
    let second = session.terminal_snapshot().active.unwrap();
    assert_ne!(first, second);
    session
        .terminal_command(TerminalCommand::Input {
            id: first,
            input: crate::TerminalInput::Text("sleep 30\r".into()),
        })
        .unwrap();
    wait_until(
        || {
            session.poll();
            session.terminal_snapshot()
        },
        |snapshot| snapshot.tabs.iter().any(|t| t.id == first && t.busy),
    );
    assert!(matches!(
        session.terminal_command(TerminalCommand::Close {
            id: first,
            confirmed: false
        }),
        Err(AppError::TerminalsRunning)
    ));
    session
        .terminal_command(TerminalCommand::Activate(first))
        .unwrap();
    assert_eq!(session.terminal_snapshot().active, Some(first));
    session
        .terminal_command(TerminalCommand::Close {
            id: second,
            confirmed: false,
        })
        .unwrap();
    assert_eq!(session.terminal_snapshot().active, Some(first));
    // A view may still resize the tab it just closed.
    session
        .terminal_command(TerminalCommand::Input {
            id: second,
            input: crate::TerminalInput::Resize { cols: 80, rows: 24 },
        })
        .unwrap();
    session.terminal_command(TerminalCommand::New).unwrap();
    assert!(session.terminal_snapshot().active.unwrap() > second);
    assert_eq!(session.revision(), revision);
    session
        .terminal_command(TerminalCommand::CloseAll { confirmed: true })
        .unwrap();
    wait_until(
        || {
            session.poll();
            session.has_running_terminals()
        },
        |running| !*running,
    );
}

#[test]
fn workspace_switch_is_guarded_including_system_requests_and_files_still_open() {
    let first = workspace(&[("a.txt", "a")]);
    let second = workspace(&[("b.txt", "b")]);
    let mut session = Session::with_application(open(first.path(), TestFilesystem::real()));
    session.terminal_command(TerminalCommand::New).unwrap();
    session
        .dispatch(AppCommand::OpenPaths(vec![first.path().join("a.txt")]))
        .unwrap();
    settle(&mut session);
    assert_eq!(session.snapshot().active_id(), Some(&id("a.txt")));
    session
        .dispatch(AppCommand::OpenPaths(vec![second.path().into()]))
        .unwrap();
    let rejected = settle(&mut session);
    assert_eq!(
        rejected.workspace_root(),
        Some(first.path().canonicalize().unwrap().as_path())
    );
    assert!(rejected.error.unwrap().contains("terminal"));
    session
        .terminal_command(TerminalCommand::CloseAll { confirmed: true })
        .unwrap();
    wait_until(
        || {
            session.poll();
            session.has_running_terminals()
        },
        |running| !*running,
    );
    session
        .dispatch(AppCommand::OpenPaths(vec![second.path().into()]))
        .unwrap();
    assert_eq!(
        settle(&mut session).workspace_root(),
        Some(second.path().canonicalize().unwrap().as_path())
    );
}

#[test]
fn terminal_input_and_output_continue_during_a_blocked_filesystem_job() {
    let dir = workspace(&[("a.txt", "a")]);
    let gate = Gate::default();
    let hook = gate.clone();
    let filesystem = TestFilesystem::with(move |operation| {
        if matches!(operation, Operation::Rename(..)) {
            hook.wait();
        }
        Ok(())
    });
    let mut session = Session::with_application(open(dir.path(), filesystem));
    session.terminal_command(TerminalCommand::New).unwrap();
    let terminal = session.terminal_snapshot().active.unwrap();
    wait_until(
        || {
            session.poll();
            session.terminal_snapshot()
        },
        |s| s.frame.is_some(),
    );
    session
        .dispatch(AppCommand::RenameEntry {
            id: id("a.txt"),
            new_name: "b.txt".into(),
        })
        .unwrap();
    session
        .terminal_command(TerminalCommand::Input {
            id: terminal,
            input: TerminalInput::Text("printf '\\nTERMINAL_%s\\n' ALIVE\r".into()),
        })
        .unwrap();
    wait_until(
        || {
            session.poll();
            session.terminal_snapshot()
        },
        |s| {
            s.frame.as_ref().is_some_and(|f| {
                f.cells
                    .iter()
                    .map(|c| c.text.as_str())
                    .collect::<String>()
                    .contains("TERMINAL_ALIVE")
            })
        },
    );
    assert!(session.snapshot().activity.busy);
    gate.open();
    settle(&mut session);
    session
        .terminal_command(TerminalCommand::CloseAll { confirmed: true })
        .unwrap();
}

#[test]
fn terminal_grid_metrics_match_every_bundled_face() {
    fn table<'a>(face: &'a [u8], tag: &[u8]) -> &'a [u8] {
        let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
        let record = (0..count)
            .map(|i| &face[12 + i * 16..28 + i * 16])
            .find(|record| &record[..4] == tag)
            .expect("font table");
        let offset = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
        &face[offset..]
    }
    let i16_at =
        |table: &[u8], at: usize| f64::from(i16::from_be_bytes([table[at], table[at + 1]]));
    let font = terminal_font();
    for face in font.faces {
        let em = f64::from(u16::from_be_bytes(
            table(face, b"head")[18..20].try_into().unwrap(),
        ));
        let hhea = table(face, b"hhea");
        let (ascent, descent, gap) = (i16_at(hhea, 4), -i16_at(hhea, 6), i16_at(hhea, 8));
        // Every glyph of a Nerd Font Mono face has the same advance.
        let advance = f64::from(u16::from_be_bytes([hhea[10], hhea[11]]));
        let line = (ascent + descent + gap) * font.size / em;
        assert_eq!(advance * font.size / em, font.cell_width);
        assert!((font.cell_height - line).abs() < 1.);
        assert!(
            (font.baseline - (font.cell_height - line) / 2. - ascent * font.size / em).abs() < 1e-9
        );
    }
}

# ADR 0008: Integrated terminals

Status: accepted.

Each window's existing `application::Session` owns its terminal tabs. A terminal
is a transient process, not a document: its independent command/event reducer
does not replay PTY output through document relocations or persist it in SQLite.
Native adapters send terminal intentions through Session and render its immutable
terminal snapshot. They retain focus, selection and panel geometry.

The terminal I/O adapter owns PTYs, child processes and the emulator, the
pure-Rust `alacritty_terminal` crate. It replaced libghostty-vt, whose bindings
required a pinned Zig toolchain on every build machine. No toolkit or renderer is
embedded in the core. The adapter answers the emulator's terminal reports, ignores
OSC 52 clipboard requests and does not enable the Kitty keyboard protocol.

Each terminal drives its emulator on one background thread. Bounded reader and
writer queues apply backpressure without dropping terminal bytes. The UI never
waits for PTY reads, writes, process startup or termination. Visible frames are
coalesced; scrollback is bounded. Session polls terminal snapshots even while its
filesystem worker owns Application. Terminal activity does not modify document
revisions or invalidate filesystem reconciliation.

Tab identities are never reused. Hiding the panel and changing tabs retain
processes. Closing a tab requires explicit confirmation only while a program
other than the shell owns the terminal's foreground process group; an idle shell
closes immediately. Input that arrives for a closed or exited tab is dropped
rather than reported. Changing workspace
is rejected while terminals are live; the same guard covers OS open requests.
Returning Home retains the session. Application exit explicitly confirms process
termination and preserves the document quit/save contract. Restarting the app
does not resume processes or replay shell input.

Terminal frames carry resolved cell colors and graphemes, geometry and cursor
coordinates. Native views draw these cells using their platform text facilities.
The adapter's xterm key encoding and bracketed paste follow the emulator's current
terminal modes, shared by all adapters. Render snapshots cross FFI only when
changed, and include only the active terminal's visible grid. GPU rendering and shell integration are future
extensions; the underlying emulator is shared from the outset.

# Bonsplit document tab component

Source: https://github.com/almonk/bonsplit/tree/77b9ccebf1c6e6533c3df1030b5efa9a3db2f351
Revision: `77b9ccebf1c6e6533c3df1030b5efa9a3db2f351`. License: MIT (see LICENSE).

`Workspace/BonsplitDocumentTab.swift` contains the TabItemView, TabBarColors and TabBarMetrics components, plus the small tab metadata model, from this revision. The split-pane controller and drag/transfer machinery are not included. Sources are included directly in the existing swiftc build; no package download is required when building the app.

Local adaptations: floating rounded Liquid Glass backgrounds (isolated from foreground glyphs and hover changes) replace the flat upstream styling, with a solid system accent fill for selection; row sizing is shared with the sidebar for vertical centering; dirty dots overlay the leading edge independently of the icon, title and close button, preserving tab width and visibility when the title is truncated; names are prefixed to avoid app collisions; tab icons accept the bundled Material NSImage assets; the close button has an accessibility label and remains available for the selected tab; accessibility selection is explicit. The shared editor remains outside the tab component to preserve its state during renames.

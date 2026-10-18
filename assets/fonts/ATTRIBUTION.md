# Terminal font

`JetBrainsMonoNLNerdFontMono-*.ttf` are the Regular, Bold, Italic and Bold Italic
"JetBrainsMonoNL Nerd Font Mono" faces from the [Nerd Fonts](https://github.com/ryanoasis/nerd-fonts)
v3.5.1 release: [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) without ligatures,
patched with Powerline and Nerd Font symbols, every glyph one cell wide.

Copyright 2020 The JetBrains Mono Project Authors. Licensed under the SIL Open Font
License 1.1; see [OFL.txt](OFL.txt).

The grid metrics in `crates/application/src/terminal.rs` (`terminal_font`) are derived
from these faces and checked by a core test; update both together.

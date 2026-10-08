# Material Icon Theme assets

The SVGs, icon associations, and generated variants come from [vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) at commit `9d513c781fb5bd0619ca21bd519fd1df5f19f84a`. The adjacent PNGs are 64 × 64 renders of the SVG definitions. The assets are distributed under the MIT license in [LICENSE](LICENSE).

`manifest.json` preserves upstream file, extension, and folder associations. Its icon paths are adjusted to the bundled `svg` directory. `tools/update_material_icons.py` regenerates the bundled assets and GNOME lookup table from a prepared upstream checkout.

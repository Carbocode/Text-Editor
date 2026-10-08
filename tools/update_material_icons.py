"""Vendor Material Icon Theme from a prepared upstream checkout.

In the upstream checkout, run `npm ci --ignore-scripts`, then
`npm run generateJson && npm run generateOpenFolders && npm run generateClones`.
Run this script with the upstream checkout path. It requires rsvg-convert.
"""

from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import json
import re
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
SOURCE = Path(sys.argv[1]).resolve() if len(sys.argv) == 2 else None
if SOURCE is None:
    raise SystemExit("usage: python3 tools/update_material_icons.py /path/to/vscode-material-icon-theme")

manifest = json.loads((SOURCE / "dist/material-icons.json").read_text())
definitions = manifest["iconDefinitions"]
assets = ROOT / "assets/material-icon-theme"
for kind in ("svg", "png"):
    directory = assets / kind
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)

for svg in (SOURCE / "icons").glob("*.svg"):
    shutil.copy2(svg, assets / "svg" / svg.name)
for definition in definitions.values():
    definition["iconPath"] = "./svg/" + Path(definition["iconPath"]).name
(assets / "manifest.json").write_text(json.dumps(manifest, separators=(",", ":")) + "\n")
shutil.copy2(SOURCE / "LICENSE", assets / "LICENSE")


def render(item):
    name, definition = item
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", name):
        raise ValueError(f"unsafe icon name: {name}")
    svg = assets / "svg" / Path(definition["iconPath"]).name
    if not svg.is_file():
        raise FileNotFoundError(svg)
    subprocess.run(
        ["rsvg-convert", "-w", "64", "-h", "64", "-o", str(assets / "png" / f"{name}.png"), str(svg)],
        check=True,
    )


with ThreadPoolExecutor(max_workers=8) as pool:
    list(pool.map(render, definitions.items()))

lines = [
    "// Generated from the pinned Material Icon Theme assets. See ATTRIBUTION.md.",
    "pub fn png_bytes(name: &str) -> Option<&'static [u8]> {",
    "    match name {",
]
for name in sorted(definitions):
    lines.append(
        f'        "{name}" => Some(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), '
        f'"/../../assets/material-icon-theme/png/{name}.png"))),'
    )
lines.extend(["        _ => None,", "    }", "}"])
(ROOT / "apps/gnome/src/material_icons.rs").write_text("\n".join(lines) + "\n")
subprocess.run(["rustfmt", str(ROOT / "apps/gnome/src/material_icons.rs")], check=True)

revision = subprocess.check_output(["git", "-C", str(SOURCE), "rev-parse", "HEAD"], text=True).strip()
(assets / "ATTRIBUTION.md").write_text(
    "# Material Icon Theme assets\n\n"
    f"The SVGs, icon associations, and generated variants come from "
    f"[vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) "
    f"at commit `{revision}`. The adjacent PNGs are 64 × 64 renders of the SVG definitions. "
    "The assets are distributed under the MIT license in [LICENSE](LICENSE).\n\n"
    "`manifest.json` preserves upstream file, extension, and folder associations. "
    "Its icon paths are adjusted to the bundled `svg` directory. "
    "`tools/update_material_icons.py` regenerates the bundled assets and GNOME lookup table "
    "from a prepared upstream checkout.\n"
)
print(f"Vendored {len(definitions)} Material Icon Theme icons from {revision}")

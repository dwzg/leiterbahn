# hw-design

Electronics and mechanical design as code, for Claude Code, GitHub Copilot CLI and people.

- **A Python library, `hwdesign`.**
  - Generates KiCad schematics and boards from scripts, routes with Freerouting, cleans up
    after it, runs the layout checks DRC can't, and writes JLCPCB fab files.
  - Measures 3D-printed enclosures built in build123d.
- **A plugin for Claude Code and GitHub Copilot CLI** with skills and agents that know how to
  use the library, and the mistakes it was built to prevent.

It grew out of a real board-plus-housing project
([probe-wink](https://github.com/dwzg/probe-wink)). Every rule in the skills comes from
something that went wrong there.

## What's in it

| Part | What it does |
|---|---|
| `hwdesign.kicad.schematic` | Schematic generator, one sheet or several: parts declare their nets; wires, labels and power symbols realise them; text collisions are detected and fixed; project symbol libraries are searched before KiCad's |
| `hwdesign.kicad.verify` | ERC plus proof that KiCad's netlist equals the declared nets |
| `hwdesign.kicad.board` | Board builder for 2, 4 or 6 copper layers: placement, outline with slots and notches, hand routes, keepouts, parallel cached Freerouting, cleanup passes (`remove_redundant`, `widen`, `straighten`, `chamfer_corners`, `prune_dangling`), pours per region and stitching, layout checks (`path_length`, `foreign_copper`, `min_gap`, `narrow_tracks`, `acute_angles`), DRC and renders |
| `hwdesign.kicad.fab` | JLCPCB Gerbers for every copper layer, BOM and CPL with per-footprint rotation corrections |
| `hwdesign.kicad.planar` | Plane geometry without pcbnew: the gap between two groups of copper shapes, outline contours with notches |
| `hwdesign.kicad.geometry` | Board data (outline, parts, pads, keepouts) as JSON for an enclosure generator |
| `hwdesign.mcad` | Measure build123d solids (ray casts, distances), gasket and print estimates, headless renders |
| skill `pcb-as-code` | The workflow and order of passes, with an API reference and a board script skeleton |
| skill `pcb-review` | Turn someone's circles on a render into causes, fixes and new renders |
| skill `jlcpcb-order` | Fab export, rotation corrections, placement-preview check, order options |
| skill `printed-enclosure` | Board-driven housings with measured checks; sealing, fasteners, printing |
| agent `layout-reviewer` | Independent routing review of a generated board |
| agent `placement-preview-checker` | Checks a JLC placement-preview screenshot part by part |

## Requirements

- KiCad 10 (`kicad-cli`, and its Python module `pcbnew`).
- [Freerouting](https://github.com/freerouting/freerouting) as `freerouting` on `PATH`.
- For `hwdesign.mcad`: build123d, numpy, scipy (`pip install "hwdesign[mcad]"`); `f3d` for
  renders.

`pcbnew` is not on PyPI. Run the KiCad parts with the Python that KiCad installed, or in a
venv created with `--system-site-packages`.

## Install

**The plugin**, in Claude Code:

```
/plugin marketplace add dwzg/hw-design
/plugin install hw-design@hw-design
```

In GitHub Copilot CLI (the same manifest; agents are named `hw-design:layout-reviewer` and
`hw-design:placement-preview-checker`):

```bash
copilot plugin marketplace add dwzg/hw-design
copilot plugin install hw-design@hw-design
```

**The library in a project:** pin it as a submodule, so every build uses the same version:

```bash
git submodule add https://github.com/dwzg/hw-design tools/hw-design
```

```python
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "hw-design", "src"))
from hwdesign.kicad.board import Board
```

Clone such projects with `git clone --recurse-submodules`. Or install it as a package:
`pip install -e path/to/hw-design` (add `[mcad]` for the mechanical helpers).

## Quick start

1. **`schematic.py`:** build a `Schematic`, add parts with their nets, draw, `finish()`,
   `save()`. Save `netlist()` as JSON.
2. **`check.py`:** `verify.erc_and_parity(sch, "intended_netlist.json", "build")` must report
   no differences.
3. **`board.py`:** follow `skills/pcb-as-code/board-script.md`.
4. **`fab.py`:** `fab.export(pcb, "build/<name>.net", "build/fab", "<name>", rot_offset)`.

Ask Claude or Copilot to "design a board for …" or "review this board" with the plugin installed, and
the skills take it from there.

## Development

```bash
python3 -m venv --system-site-packages .venv      # lets the tests import KiCad's pcbnew
.venv/bin/pip install -e ".[mcad,dev]"
.venv/bin/pytest -q
ruff check .
python3 scripts/gen_api.py        # after changing a public signature
claude plugin validate .
```

Tests that need `pcbnew`, `kicad-cli` or KiCad's libraries are skipped where KiCad isn't
installed (as in CI). See `CONTRIBUTING.md`.

## Licence

MIT. See `LICENSE`.

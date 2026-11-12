# leiterbahn

Circuit boards as code, for Claude Code, GitHub Copilot CLI and people. (*Leiterbahn* is German
for a track on a circuit board.)

- **A Python library, `leiterbahn`.** It generates KiCad schematics and boards from scripts,
  routes them with a router of its own (or with Freerouting), runs the layout checks DRC
  can't, and writes JLCPCB fab files.
- **A plugin for Claude Code and GitHub Copilot CLI** with skills and agents that know how to
  use the library, and the mistakes it was built to prevent.

It grew out of a real board-plus-housing project
([probe-wink](https://github.com/dwzg/probe-wink)). Every rule in the skills comes from
something that went wrong there. Until 0.9 it was called `hw-design` and also held the
housing side, which is now [gehaeuse](https://github.com/dwzg/gehaeuse).

## What's in it

| Part | What it does |
|---|---|
| `leiterbahn.kicad.schematic` | Schematic generator, one sheet or several: parts declare their nets; wires, labels and power symbols realise them; text collisions are detected and fixed; project symbol libraries are searched before KiCad's |
| `leiterbahn.kicad.verify` | ERC plus proof that KiCad's netlist equals the declared nets |
| `leiterbahn.kicad.board` | Board builder for 2, 4 or 6 copper layers: placement, outline with slots and notches, hand routes, keepouts, parallel cached Freerouting, cleanup passes (`remove_redundant`, `widen`, `straighten`, `chamfer_corners`, `prune_dangling`), pours per region and stitching, layout checks (`path_length`, `foreign_copper`, `min_gap`, `narrow_tracks`, `acute_angles`), DRC and renders |
| `leiterbahn.kicad.place` | Placement by connections on both board sides: fixed parts by hand, the rest around anchors, with areas for domains and room reserved for hand routes; then `improve()` moves, turns and exchanges them for shorter connections (`leiterbahn-route place`, simulated annealing in Rust; with `spread=True` from a start found with forces, electrostatic as in ePlace) |
| `leiterbahn.kicad.negotiate` | Routing of all nets at once with `leiterbahn-route` (`router/`, Rust): negotiated congestion on a grid, net class widths and clearances, a shortening pass, and per net how far its tracks are from what they would cost alone on the board |
| `leiterbahn.kicad.intent` | What a circuit needs of its layout, read from the pin names and types in the netlist: supply pins and their capacitors, feedback and input nodes, boot capacitors, quiet and noisy nets. The placer uses it (`Placer.ties()`), `Placer.unmet()` reports what is not given |
| `leiterbahn-route board` | The same router on a `.kicad_pcb` file directly, without KiCad or Python: reads the board and its rules, writes the tracks into the file |
| `leiterbahn.kicad.fab` | JLCPCB Gerbers for every copper layer, BOM and CPL with per-footprint rotation corrections |
| `leiterbahn.kicad.planar` | Plane geometry without pcbnew: the gap between two groups of copper shapes, outline contours with notches |
| `leiterbahn.kicad.geometry` | Board data (outline, parts, pads, keepouts) as JSON for an enclosure generator ([gehaeuse](https://github.com/dwzg/gehaeuse)) |
| skill `pcb-as-code` | The workflow and order of passes, with an API reference and a board script skeleton |
| skill `pcb-review` | Turn someone's circles on a render into causes, fixes and new renders |
| skill `jlcpcb-order` | Fab export, rotation corrections, placement-preview check, order options |
| agent `layout-reviewer` | Independent routing review of a generated board |
| agent `placement-preview-checker` | Checks a JLC placement-preview screenshot part by part |

## Requirements

- KiCad 10 (`kicad-cli`, and its Python module `pcbnew`).
- Rust's `cargo`, for the router (`router/`, built on first use by `Board.negotiate()`).
- [Freerouting](https://github.com/freerouting/freerouting) as `freerouting` on `PATH`, only
  for `Board.autoroute()`.

`pcbnew` is not on PyPI. Run the KiCad parts with the Python that KiCad installed, or in a
venv created with `--system-site-packages`.

## Install

**The plugin**, in Claude Code:

```
/plugin marketplace add dwzg/leiterbahn
/plugin install leiterbahn@leiterbahn
```

In GitHub Copilot CLI (the same manifest; agents are named `leiterbahn:layout-reviewer` and
`leiterbahn:placement-preview-checker`):

```bash
copilot plugin marketplace add dwzg/leiterbahn
copilot plugin install leiterbahn@leiterbahn
```

**The library in a project:** pin it as a submodule, so every build uses the same version:

```bash
git submodule add https://github.com/dwzg/leiterbahn tools/leiterbahn
```

```python
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "leiterbahn", "src"))
from leiterbahn.kicad.board import Board
```

Clone such projects with `git clone --recurse-submodules`. Or install it as a package:
`pip install -e path/to/leiterbahn`.

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
.venv/bin/pip install -e ".[dev]"
.venv/bin/pytest -q
ruff check .
python3 scripts/gen_api.py        # after changing a public signature
claude plugin validate .
```

Tests that need `pcbnew`, `kicad-cli` or KiCad's libraries are skipped where KiCad isn't
installed (as in CI). See `CONTRIBUTING.md`.

## Licence

MIT. See `LICENSE`.

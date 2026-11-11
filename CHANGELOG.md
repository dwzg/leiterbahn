# Changelog

## 0.3.2

Added
- `Board.place_near(..., back=True)` places on the back of the board.

Fixed
- Placement on both sides: `fits()` and `place_near()` compared a part's courtyard with every
  other part, whichever side it was on, so nothing could go under a part on the other side.
  Parts on opposite sides now only collide if one of them has through-hole pads (a pin header
  takes the space on both sides). `courtyard_overlaps()` uses the same rule: it used to miss
  an SMD part placed onto the pins of a through-hole part from the other side.
- `Board.place(ref, ..., back=False)` now brings a part back to the front if it was on the back.

## 0.3.1

Fixed
- Auto-wiring, two pins of one part tied together (a NAND gate used as an inverter): the
  drawing had a bar across the two pin ends, a 1.27 mm hook next to it and a jog at the
  driving output. Three causes: `finish()` joined adjacent signal pins along the pin ends
  before the router ran (it now leaves them to the router, which joins them in front of the
  pins; supply pins and pins on one spot are still joined); the router wired a pin again
  that a wire already ended on; and running between the 2.54 mm lines cost so much that a
  30 mm wire jogged right at a pin placed between the lines.

## 0.3.0

Added
- `Schematic(..., autowire=True)`: `finish()` connects the pins of each net inside a block
  with wires and uses a label only where a net leaves the block. A block is a `frame()` and
  the parts in it. A schematic connected by labels alone was rejected in review: it is a
  netlist, the circuit cannot be read from it. `hwdesign.kicad.autowire` holds the router
  (orthogonal, 1.27 mm grid, never along or over a foreign wire's end, pins entered
  straight); `Page.autowire()` runs it on one page. Pins it cannot reach keep a label and
  give a warning.

Changed
- `autoplace_fields()` tries above and below first for a part lying on its side. Left and
  right of it are its wires.
- The name of a power symbol that lies on its side (pin on the left or right of a part) is
  written beyond the symbol's tip. At the library's position it lay along the stub, struck
  through by the wire.

Fixed
- Pins of a part placed with `mirror=True` and `rot=90` or `270` were modelled at the
  opposite points: the library mirrored before rotating, KiCad rotates first. KiCad's ERC
  reported all pins of such FETs as not connected and parity failed. 0 and 180 degrees were
  not affected.
- Field justification on parts rotated by 90 degrees or mirrored: KiCad swaps left and right
  for 90 and 180 degrees and once more for a mirrored part; the library knew only 180.
  Texts moved by `autoplace_fields()` were drawn on the body of every resistor lying on its
  side while `overlaps()` reported nothing.

## 0.2.1

Added
- `Schematic(power_symbols={net: lib_id})`: supply nets beyond the six built-in names, each
  drawn with a power symbol of your choice. The symbol's value names the net, so a second
  ground (`{"GNDF": "power:GNDA"}`) or another rail gets its own net.

Changed
- `finish()` joins pins of one part that share a net and are stacked or neighbours (up to
  5.08 mm apart, no other pin between) with a wire and gives them one label or power symbol. A connector with eight ground pins on
  one spot got eight symbols on top of each other.
- `finish()` puts a pin's power symbol beyond the net labels of the pins right next to it.
  At the fixed 2.54 mm stub it lay on the neighbour's label.
- `overlaps()` reports parts, texts, labels and `frame()` rectangles that reach into the
  drawing sheet's title block. Nothing checked that corner, and frames drawn across it
  only showed up in the PDF.
- `overlaps()` accepts global labels on neighbouring pins: their boxes are 0.32 mm apart by
  construction, which the 0.4 mm rule reported for every bus on a multi-sheet design.

Fixed
- `power_flag()` on a pin marked the pin as connected, so `finish()` gave it no label and
  the pin ended up on a net with only the flag. The pin now keeps its label or power symbol.

## 0.2.0

Boards with more than two layers and schematics with more than one sheet. Existing scripts
run unchanged; what differs for them is listed under "Changed".

Added
- `Board(layers=2, planes=())`: 2, 4 or 6 copper layers; `planes` are inner layers for pours
  only (KiCad layer type "power", exported to Freerouting as `(type power)`).
  `Board.copper_layers` and `board.copper_layers(board)` list the enabled copper layers.
- `Board.zone(..., rect=, polygon=)`: a pour for one region, so two ground nets can share a
  layer.
- `board.min_gap(board, nets_a, nets_b)`: the smallest copper-to-copper distance between two
  groups of nets on any one layer, and where it is. The geometry is in the new
  `hwdesign.kicad.planar`, which needs no pcbnew.
- `Board.cutout(x, y, w, h, r)`: a slot inside the board or a notch in one edge. `fits()`,
  `place_near()`, `stitch()`, `via_near()`, `label_near()`, `place_refs()` and
  `courtyard_overlaps()` respect it.
- `Schematic.sheet(name, file)`: more pages with the same drawing API (`Page`). One
  reference namespace, design-wide `netlist()`, `overlaps()` and `finish()`; `save()` writes
  all files. With sheets, labels are written as global labels.
- `SymbolLib([dir, ...])` and `Schematic(sym_dirs=[...])`: project symbol libraries before
  KiCad's own. `write_lib_tables()` addresses libraries inside the project through
  `${KIPRJMOD}`.
- `fab.copper_layers(pcb)` reads the copper layers from the board file; `fab.export()`
  returns them as `copper`.
- `read_netlist()` returns each part's instance `path` and `sheet`; footprints of parts on a
  sheet get the full path.

Fixed
- `fab.export()` wrote only F.Cu and B.Cu: a four-layer board came out with two copper
  Gerbers. It now exports every copper layer of the board file.
- `path_length()` returned `None` for a route through an inner layer, and
  `prune_dangling()` removed a via (and then its track) that fed an inner plane: both only
  looked at F.Cu and B.Cu.
- `stitch()` only tested the layers that carry the net's pour. With a ground pour on the
  back alone it put 37 vias onto a front-layer track of another net (14 DRC shorts on a
  test board). On layers without the pour, a via must now clear foreign pads and tracks.
- A multi-unit part placed with `unit=2` replaced the entry of unit 1: its pins were missing
  from `netlist()` and were not labelled by `finish()`. Units are now merged.
- GitHub Copilot CLI support: agents list `execute` next to `Bash`, so they keep a shell on
  Windows; README shows the Copilot install.

Changed
- Schematics are written in the KiCad 10 format: `version 20260306`,
  `generator_version "10.0"`, `body_style` and `in_pos_files` on symbols, `hide` on the
  property. The header said 9.0 while the embedded library symbols were already KiCad 10's.
  `kicad-cli sch upgrade` (10.0.8) now rewrites a generated file without adding or dropping
  a token (it only reorders, and empties the project name when no project file is present).
- Placing a reference (and unit) twice raises `ValueError`; it used to give two symbols with
  one reference.
- `keepout()` and `foreign_copper()` default to all copper layers of the board instead of
  F.Cu and B.Cu. On two-layer boards that is the same.
- `outline()` and `outline_rounded()` replace an earlier outline instead of adding a second.

Not verified
- `autoroute()` on a four-layer board: Freerouting was not available when this was written.
  The DSN was checked, the routed result was not.

## 0.1.0

First release, extracted from [probe-wink](https://github.com/dwzg/probe-wink).

- `hwdesign.kicad`: schematic generator, ERC and netlist parity (`verify`), board builder with
  Freerouting and cleanup passes, layout checks, JLCPCB export, enclosure geometry export.
  Netlist reading moved to `netlist.py`, so parity checks work without `pcbnew`.
- `hwdesign.mcad`: solid probing, gasket and print estimates, f3d renders (from the
  probe-wink housing checks; the colour table is now a parameter).
- Skills: `pcb-as-code`, `pcb-review`, `jlcpcb-order`, `printed-enclosure`.
- Agents: `layout-reviewer`, `placement-preview-checker`.

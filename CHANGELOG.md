# Changelog

## 0.8.2

Fixed
- `route_open()`: copper that hangs on a track it takes out of the way (a track ending on
  its middle or beside its via) is joined again with the track's own ends, in the same
  all-or-nothing step (`finish.cut_off`). 0.8.1 refused such tracks, which left 6
  connections open on a board where 2 had been.
- `route_open()` named an open connection by the two items KiCad reports, and those change
  from run to run. It now names it by the two nearest items of the two pieces, starts from
  the smaller piece and orders the list by the copper alone; a round follows as long as the
  last one routed something (a track from one piece may end on a third).
- `improve()` orders its runs by their position, not by item ids.

## 0.8.1

Added
- `board.repeatable()`, to be called first in a board script, and seeded item ids in
  `Board()` and `Board.reopen()`: the same script now gives the same board every time. Two
  runs of one script had left different connections open after `route_open()` (Python's
  set order and KiCad's random ids both changed from start to start), and rework drawn for
  one result met the other.

Fixed
- `route_open()` could open a connection: it took a track out of the way to which another
  track of the net was joined along the way (ending on its middle, or beside its via) and
  did not join that one again. Such a track now stays, and the connection that wanted to
  cross it is reported. `improve()` uses the same test (`finish.joined_only_at_ends`).

## 0.8.0

Added
- `Board.improve()` (`finish.improve`): routes again what is routed, one run at a time
  (the copper between two pads, junctions or locked tracks, through its vias), and keeps
  the new path if it is shorter, has fewer vias or reaches the net class's width. A run may
  push other runs out of its way; the exchange is kept if the sum gets smaller. Locked
  copper stays, connections are neither opened nor made (checked at the end). On a
  six-layer board with 3.4 m of autorouted track it took 2 minutes and saved 70 mm and 10
  of 288 vias; with every run alone on the board the saving could be 17 % at most.
- `finish.route_stats(board)`: track length, vias and length below class width of the
  unlocked copper. `finish.runs_of()`, `finish.overlapping()`, `finish.in_the_way()`.

Fixed
- `prune_dangling()` crashed pcbnew in its second round on a reopened board (tracks removed
  with `Remove()`); it deletes them now.

## 0.7.1

Fixed
- `acute_angles()` no longer reports a piece shorter than the track is wide as a sharp
  joint with the track after it (a 0.1 mm piece between two 0.6 mm tracks read as 38
  degrees). The tracks on both sides of such a piece are still compared with each other.

## 0.7.0

Added
- `Board.reopen(pcb_path, ...)`: a saved board as a `Board`, for rework after routing
  (`rip_up`, `track`, `via`, `route_open`, `prune_dangling`, `fill_zones`, `unrouted`,
  `save`). Rework on the saved file starts from the same board every time and takes
  minutes where the building run takes the autorouter's time again.
- `Board.rip_up(x0, y0, x1, y1, nets=, keep=, layers=, vias=)`: removes the unlocked tracks
  and vias with an end in a rectangle. With `route_open(only=[...])` this is local rip-up
  and reroute: on a six-layer board, two connections that no order of whole-board routing
  closed were closed by clearing 7 x 5 mm around them and routing them first.

## 0.6.4

Fixed
- `route_open()` keeps the clearance of the net classes: to copper of another net it now
  keeps the larger of the two classes' clearances instead of the board's default. A 5 V
  track had been laid 0.25 mm from a relay contact's via whose class asks for 0.5 mm (DRC
  reported it).

## 0.6.3

Fixed
- `fan_to_planes()` and `route_open()` no longer add tracks of no length (a via in the
  middle of its own pad got one). KiCad's DRC reported one of them as "tracks crossing" with
  a track 2.5 mm away.

## 0.6.2

Fixed
- `route_open()`: a movable track lying in a rule area or at the board edge marked those
  cells as "free once the track is moved", so a new track could follow it in. A via of a
  floating rail landed 0.05 mm inside an isolation gap this way (1.7 mm instead of 2 mm to
  the other domain; the gap check caught it). What is fixed is now stamped last.

Added
- `route_open(hug=0.0)`: optional extra cost for steps away from all copper.

## 0.6.1

Changed
- `route_open()` joins an open connection to the nearest copper of the other part of its net,
  starting anywhere on the copper already joined to the first pad; the ratsnest names one pad
  pair, and on a rail that pad was often shut in while a track of the net ran 1 mm away
  (9 open connections became 5 on a 287-part board). Nets with a pour are still routed pad
  to pad: the module does not see what a plane joins.
- `route_open(depth=2)`: a track that is moved may move others in turn, all or nothing.
  `limit` now bounds one search at 400000 cells (a failing search over a whole six-layer
  board took half a minute, eight of them per connection).
- A new track ends exactly on the track or via it joins and no longer adds a short piece that
  could fold back; the same segment is not drawn twice.

Fixed
- `prune_dangling()` asks KiCad what dangles (`TestTrackEndpointDangling`) instead of its
  own geometric test, which took a track end 0.36 mm from the centre of a 0.6 mm via for
  loose where KiCad saw the copper overlap: it removed three whole connections of a fully
  routed board, vias included.

## 0.6.0

Added
- `Board.route_open(tmp_dir)` (module `finish`): connects what the autorouter left open, one
  connection at a time, with a grid search over all routing layers (0.1 mm grid, 45 degree
  steps, vias; a second try on a 0.05 mm grid in a window around the connection). Freerouting
  2.5 and 1.9 both left single connections open on a 287-part board for no visible reason
  (two neighbouring header pins of one net, a test point under a pad) and later passes did
  not recover them; a finisher that only needs a free path closes them, and DRC stays the
  judge. Where there is no free path it may cross unlocked tracks, but only if each crossed
  track finds another free way; otherwise nothing changes, so a call never leaves more open
  than it found. `only=[nets], locked=True` routes chosen nets before the autorouter.

Changed
- `Board.fan_to_planes(lane=1.0)`: the way straight out of every long pad of a part with five
  or more pads is kept free of other nets' fan-out vias and tracks. A ground pin's via sat
  0.25 mm in front of the next pin of a 0.5 mm pitch QFN; that pin was then unroutable on an
  otherwise empty four-layer board, for Freerouting and for `route_open()` alike.

## 0.5.1

Changed
- `Schematic.write_lib_tables(directory, fp_dirs=())`: project libraries outside the project
  directory are written relative to `${KIPRJMOD}` (with `..`), and footprint libraries are
  also looked up in `fp_dirs`. Two boards sharing one `lib/` directory got an absolute path in
  `sym-lib-table` and a `${KIPRJMOD}/lib` entry that pointed nowhere: ERC then warned that the
  footprint library was not configured.

## 0.5.0

Added
- `Board.fan_to_planes()`: before autorouting, every SMD pad of a plane's net gets a via into
  its plane (nearest legal spot, straight track on the pad's side, a nearby via of the same
  net used again). `autoroute()` then takes the pads that are on a plane off the router's
  list. Freerouting does not connect pins to a plane: on a 491-part board 273 of 465 open
  connections after a pass were on the three plane nets, and the count did not fall.

Fixed
- `via_near()` drew the track from a back-side pad to its via on the front.

## 0.4.0

Added
- `hwdesign.kicad.place.Placer`: placement by connections on both board sides. Parts fixed
  by hand, the rest around an anchor per group (large parts first, then each part where its
  pads are closest to the pads they connect to; a decoupling capacitor at a supply pin that
  has none yet). `rules` keep parts out of regions (ground domains, height limits), `keep()`
  reserves room for hand routes, `face()` and `find()` help fixing sense resistors.
  Coordinates are millimetres from the board's corner.
- `Board.autoroute(fanout=False, optimize=True)`. Freerouting 2.5 gives every SMD pin a via
  before routing unless told otherwise: 1168 vias on a 491-part board with parts on both
  sides. Off by default now; `optimize=False` skips the optimiser for a quick routability test.
- A test that runs Freerouting (skipped where the command is missing).
- Skill: section "Many parts, both sides".

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

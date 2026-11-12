---
name: jlcpcb-order
description: Produce JLCPCB PCB and assembly (PCBA) files from a KiCad board, and guide the order. Covers Gerbers for every copper layer (two, four or six), BOM and CPL with per-footprint rotation corrections, board and PCBA options, the assembly remark, checking pin 1 and polarity in JLC's placement preview, footprint-name warnings and the price. Use when exporting fab files, when a Gerber zip is missing its inner layers, when JLC's placement preview shows rotated or flipped parts, when JLC warns that a part's footprint does not match the BOM, or when someone asks which JLC options to pick.
---

# JLCPCB order

## Export

`leiterbahn.kicad.fab.export(pcb, netlist, outdir, name, rot_offset)` writes:
- a Gerber and drill zip;
- `<name>-bom-jlc.csv` (one line per LCSC number);
- `<name>-cpl-jlc.csv` (positions and rotations).

A part is assembled when its schematic fields carry `LCSC` and `Assembly=SMT`. Everything else
(connectors, wire leads, test pads) is left out of both files, for hand soldering. The return
value's `missing` lists assembled parts the position file did not contain; it must be empty.

Put an exact LCSC number on every assembled part, and check its stock on the day of
ordering. Stock changes weekly.

The Gerber set has one file per copper layer the board file enables; the return value's
`copper` lists them. Compare it with the layer count you order. A four-layer test board
gave `-F_Cu.gtl`, `-In1_Cu.g1`, `-In2_Cu.g2` and `-B_Cu.gbl`. Up to 0.1.0 the layer list was
fixed to F.Cu and B.Cu, and the same board came out with two copper files and no error.

## Rotation corrections

JLC's part models do not always share KiCad's zero orientation. Corrections go in
`rot_offset`: degrees counter-clockwise, keyed by footprint name. Found in JLC's preview on
real orders:

| Footprint | Correction | Seen as |
|---|---|---|
| `SOT-23` | 180 | pin-1 dot on the corner opposite the board's pin 1 |
| `TSOT-23-6` | 180 | the same, and the chip's own pin-1 dimple confirms it |
| `Relay_2P2T_10x6mm_TE_IMxxG` | -90 | relay body lying across the two pad columns |

These describe JLC's library at that time. Treat them as a starting point, not a rule, and
check every polarised part in the preview on every order.

## Check the placement preview

For every part with a pin 1 or a polarity, compare JLC's mark with the board. The
`placement-preview-checker` agent does this from a screenshot.

- **Pin 1:** JLC draws a magenta dot. Compute where the board's pin 1 sits in the screenshot
  (calibrate on the board edges) and compare.
- **Diodes:** JLC marks the anode with a magenta `+`; the cathode band is on the body model.
  Both must sit on the board's cathode pad. Confirm which pad is the cathode from the netlist
  (KiCad names nets like `Net-(D3-K)`), not from the footprint drawing.
- **Electrolytics:** `+` on the + pad.
- **Body outline:** a body lying across its pads means a 90° error, even when the dot looks
  plausible.
- **Overhangs:** a module that deliberately overhangs the edge (an antenna) shows outside the
  board. Say so in the assembly remark.

After changing `rot_offset`, regenerate, upload the new CPL and check the preview again. If
a 90° correction went the wrong way, the part is upright but its dot is diagonal from pin 1:
use the other sign.

## Warnings JLC shows

- *"The footprint (TSOT-23-6) of this part does not match the one (SOT-6) provided in your
  BOM":* a naming difference. KiCad's SOT-23-6 and TSOT-23-6 land patterns are identical
  (six 1.325 × 0.6 mm pads, 0.95 mm pitch). Confirm the pads match, then accept; what matters
  is rotation.
- Small holes: check the current capabilities page
  (`https://jlcpcb.com/capabilities/pcb-capabilities`) before claiming a limit. On one
  check, 0.2 mm holes in 0.6 mm pads (thermal vias in a module's ground pad) were standard,
  with no surcharge.

## Options and price

Board and PCBA option tables, the remark text and price notes are in
[order-options.md](order-options.md).

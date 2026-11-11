# A schematic on several sheets

```python
sch = Schematic("my-board", title="My board", sym_dirs=[os.path.join(HERE, "lib")])   # project symbols first
uart = sch.sheet("UART", "uart.kicad_sch")              # also: at=(x, y), size=(w, h), paper=, title=
power = sch.sheet("Power", "sheets/power.kicad_sch")

sch.add("J1", "Connector_Generic:Conn_01x02", (60, 100), "IN", HEADER, {"1": "TX", "2": "GND"})
uart.add("R7", "mylib:RES", (100, 100), "1k", R0805, {"1": "TX", "2": "TX_ISO"})   # joins J1.1 by its net name
uart.wire(("R7", 2), ("R8", 1))                          # wires stay on their page
sch.power_flag("GND", (100, 100))                        # one flag per supply net, for the whole design
sch.power("GND", (100, 100), direction="down")

sch.finish()                                             # the root and every sheet not finished yet
assert not sch.overlaps()                                # whole design; uart.overlaps() checks one sheet
sch.save(os.path.join(HERE, "my-board.kicad_sch"))       # also writes uart.kicad_sch and sheets/power.kicad_sch
sch.write_lib_tables(HERE)                               # mylib -> ${KIPRJMOD}/lib/mylib.kicad_sym
with open(os.path.join(HERE, "intended_netlist.json"), "w") as f:
    json.dump(sch.netlist(), f)                          # the nets of all sheets
```

`check.py` and the board script stay as they are: `verify.erc_and_parity()` takes the root
sheet, and the exported netlist holds every sheet.

What the library does with sheets:

- **One reference namespace.** A second `add("R7", ...)` raises `ValueError` and names the
  sheet that has it. Units of one part share a reference (`unit=2`).
- **Flat net names.** Every label becomes a global label, so KiCad's net is `TX`, not
  `/UART/TX`, and a net without a label gets a name like `Net-(R5-Pad1)` on any sheet.
  `Board.resolve()` and the parity check take both forms. A one-sheet design keeps its local
  labels and `/TX`.
- **A flat hierarchy.** The root gets one sheet symbol per page, in a row along the top
  unless `at=` moves it; it counts in `overlaps()`. There are no sheet pins, no sheets
  inside sheets, and a sheet file is used once.
- **Supply nets.** Nets in `power_symbols` (the built-in six plus
  `Schematic(power_symbols={"GNDF": "power:GNDA", "VIO": "power:+3V3"})`) are drawn as power
  symbols instead of labels; the symbol's value is the net name. Put each net's one
  `power_flag()` on a pin of a real part (`sch.power_flag("VIN", ("J1", 1))`): on a wire
  that reaches only power symbols, ERC reported the flag as "pin not connected".
- **Project file.** kicad-cli reads the project's `sym-lib-table` only when a `.kicad_pro`
  sits next to the root sheet (see SKILL.md). `Board.write_project()` creates one, but that
  runs after ERC; until then a file with `{"meta": {"filename": "my-board.kicad_pro", "version": 3}}`
  is enough, and `write_project()` later adds to it.

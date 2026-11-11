"""The schematic generator on a hand-written symbol library: no KiCad needed."""
import pytest
from conftest import two_pin, write_lib

from hwdesign.kicad import schematic
from hwdesign.kicad.schematic import Schematic, SymbolLib
from hwdesign.kicad.sexp import find, find1, parse, walk


def load(path):
    return parse(path.read_text())


def heads(tree):
    return [n[0] for n in tree if isinstance(n, list)]


def design(symdir, **kw):
    return Schematic("demo", title="Demo", lib=SymbolLib(str(symdir)), **kw)


# ---- symbol search path -------------------------------------------------------
def test_project_library_wins_over_the_system_one(symdir, tmp_path):
    proj = tmp_path / "proj" / "lib"
    mine = write_lib(proj, "tlib", [two_pin("R2P"), two_pin("ONLY_HERE")])
    lib = SymbolLib([str(proj), str(symdir)])
    assert lib.path("tlib") == str(mine)
    assert lib.flat("tlib:ONLY_HERE")[1] == "tlib:ONLY_HERE"
    assert lib.path("power") == str(symdir / "power.kicad_sym")      # falls through to the next directory
    with pytest.raises(FileNotFoundError, match="nolib"):
        lib.path("nolib")


def test_one_directory_as_before(symdir):
    lib = SymbolLib(str(symdir))
    assert lib.dirs == [str(symdir)] and lib.dir == str(symdir)
    assert lib.flat("tlib:R2P")[1] == "tlib:R2P"


def test_sym_dirs_are_searched_before_kicads_own(symdir, tmp_path, monkeypatch):
    monkeypatch.setattr(schematic, "SYMBOL_DIR", str(symdir))
    proj = tmp_path / "proj"
    write_lib(proj / "lib", "plib", [two_pin("PART")])
    sch = Schematic("demo", sym_dirs=[str(proj / "lib")])
    sch.add("R1", "plib:PART", (100, 100), nets={"1": "A", "2": "GND"})
    sch.add("R2", "tlib:R2P", (120, 100), nets={"1": "A", "2": "GND"})
    sch.finish()
    sch.write_lib_tables(str(proj))
    table = (proj / "sym-lib-table").read_text()
    assert '(name "plib") (type "KiCad") (uri "${KIPRJMOD}/lib/plib.kicad_sym")' in table
    assert '(name "tlib") (type "KiCad") (uri "${KICAD10_SYMBOL_DIR}/tlib.kicad_sym")' in table
    assert '(name "power") (type "KiCad") (uri "${KICAD10_SYMBOL_DIR}/power.kicad_sym")' in table


def test_lib_and_sym_dirs_exclude_each_other(symdir):
    with pytest.raises(ValueError, match="not both"):
        Schematic("demo", lib=SymbolLib(str(symdir)), sym_dirs=[str(symdir)])
    with pytest.raises(ValueError):
        SymbolLib([])


def test_library_outside_project_and_system_gets_its_real_path(symdir, tmp_path):
    vendor = tmp_path / "vendor"
    write_lib(vendor, "vlib", [two_pin("V")])
    sch = Schematic("demo", lib=SymbolLib([str(vendor), str(symdir)]))
    sch.add("R1", "vlib:V", (100, 100), nets={"1": "A", "2": "B"})
    proj = tmp_path / "proj"
    proj.mkdir()
    sch.write_lib_tables(str(proj))
    assert '(uri "%s")' % (vendor / "vlib.kicad_sym") in (proj / "sym-lib-table").read_text()


# ---- single sheet ----------------------------------------------------------------
def test_single_sheet_keeps_local_labels_and_one_file(symdir, tmp_path):
    sch = design(symdir)
    sch.add("R1", "tlib:R2P", (100, 100), nets={"1": "SIG", "2": "GND"})
    sch.add("R2", "tlib:R2P", (120, 100), nets={"1": "SIG", "2": "GND"})
    sch.finish()
    sch.save(str(tmp_path / "demo.kicad_sch"))
    assert [p.name for p in tmp_path.glob("*.kicad_sch")] == ["demo.kicad_sch"]
    tree = load(tmp_path / "demo.kicad_sch")
    assert heads(tree).count("label") == 2 and "global_label" not in heads(tree) and "sheet" not in heads(tree)
    assert find1(tree, "sheet_instances") == ["sheet_instances", ["path", "/", ["page", "1"]]]
    for sym in find(tree, "symbol"):
        assert find1(find1(find1(sym, "instances"), "project"), "path")[1] == "/" + sch.root
    assert sch.netlist() == {"SIG": [("R1", "1"), ("R2", "1")], "GND": [("R1", "2"), ("R2", "2")]}


def test_header_is_the_kicad_10_format(symdir, tmp_path):
    sch = design(symdir)
    sch.add("R1", "tlib:R2P", (100, 100), nets={"1": "A", "2": "B"}, fields={"LCSC": "C1"})
    sch.save(str(tmp_path / "demo.kicad_sch"))
    tree = load(tmp_path / "demo.kicad_sch")
    assert find1(tree, "version")[1] == "20260306" and find1(tree, "generator_version")[1] == "10.0"
    sym = find1(tree, "symbol")
    assert find1(sym, "body_style")[1] == "1" and find1(sym, "in_pos_files")[1] == "yes"
    lcsc = next(p for p in find(sym, "property") if p[1] == "LCSC")
    assert find1(lcsc, "hide")[1] == "yes" and find1(find1(lcsc, "effects"), "hide") is None
    assert sch._field_box(sch.syminfo["R1"]["fields"]["LCSC"]) is None        # hidden: no text to collide


def test_duplicate_reference_raises(symdir):
    sch = design(symdir)
    sch.add("R1", "tlib:R2P", (100, 100), nets={"1": "A", "2": "B"})
    with pytest.raises(ValueError, match="duplicate reference R1"):
        sch.add("R1", "tlib:R2P", (120, 100), nets={"1": "A", "2": "B"})


def test_units_of_one_part_share_a_reference_and_all_their_pins_are_declared(symdir):
    sch = design(symdir)
    sch.add("U1", "tlib:DUAL", (100, 100), nets={"1": "A", "2": "B"}, unit=1)
    sch.add("U1", "tlib:DUAL", (100, 120), nets={"3": "A", "4": "C"}, unit=2)
    sch.finish()
    assert sch.netlist() == {"A": [("U1", "1"), ("U1", "3")], "B": [("U1", "2")], "C": [("U1", "4")]}
    assert len(sch.labels) == 4                       # both units got their pins labelled
    assert set(sch.syminfo) == {"U1", "U1B"}
    with pytest.raises(ValueError, match="duplicate reference U1"):
        sch.add("U1", "tlib:DUAL", (100, 140), nets={"3": "A", "4": "C"}, unit=2)


# ---- several sheets ---------------------------------------------------------------
def two_sheets(symdir):
    sch = design(symdir)
    io = sch.sheet("IO", "io.kicad_sch")
    pw = sch.sheet("Power", "sub/power.kicad_sch")
    sch.add("R1", "tlib:R2P", (100, 100), nets={"1": "SIG", "2": "GND"})
    io.add("R2", "tlib:R2P", (100, 100), nets={"1": "SIG", "2": "MID"})
    io.add("R3", "tlib:R2P", (120, 100), nets={"1": "MID", "2": "GND"})
    pw.add("R4", "tlib:R2P", (100, 100), nets={"1": "+3V3", "2": "SIG"})
    return sch, io, pw


def test_sheets_are_written_next_to_the_root_with_a_sheet_symbol_each(symdir, tmp_path):
    sch, io, pw = two_sheets(symdir)
    sch.finish()
    sch.save(str(tmp_path / "demo.kicad_sch"))
    root, t_io, t_pw = (load(tmp_path / f) for f in ("demo.kicad_sch", "io.kicad_sch", "sub/power.kicad_sch"))
    sheets = find(root, "sheet")
    assert [[p[2] for p in find(s, "property")] for s in sheets] == [["IO", "io.kicad_sch"],
                                                                    ["Power", "sub/power.kicad_sch"]]
    for s, page, n in zip(sheets, (io, pw), ("2", "3"), strict=True):
        path = find1(find1(find1(s, "instances"), "project"), "path")
        assert path[1] == "/" + sch.root and find1(path, "page")[1] == n
        assert find1(s, "uuid")[1] == page.symbol_uuid
    assert find1(root, "sheet_instances") is not None
    assert find1(t_io, "sheet_instances") is None and find1(t_pw, "sheet_instances") is None
    assert len({find1(t, "uuid")[1] for t in (root, t_io, t_pw)}) == 3
    assert find1(find1(t_io, "title_block"), "title")[1] == "Demo - IO"


def test_symbols_on_a_sheet_carry_the_sheet_in_their_instance_path(symdir, tmp_path):
    sch, io, pw = two_sheets(symdir)
    sch.finish()
    sch.save(str(tmp_path / "demo.kicad_sch"))
    for file, page in (("io.kicad_sch", io), ("sub/power.kicad_sch", pw)):
        syms = find(load(tmp_path / file), "symbol")
        assert syms
        for sym in syms:
            path = find1(find1(find1(sym, "instances"), "project"), "path")[1]
            assert path == "/%s/%s" % (sch.root, page.symbol_uuid)


def test_with_sheets_every_label_is_global_so_a_net_name_is_one_net(symdir, tmp_path):
    sch, io, pw = two_sheets(symdir)
    sch.finish()
    sch.save(str(tmp_path / "demo.kicad_sch"))
    names = {}
    for f in ("demo.kicad_sch", "io.kicad_sch", "sub/power.kicad_sch"):
        tree = load(tmp_path / f)
        assert "label" not in heads(tree)
        names[f] = sorted(g[1] for g in find(tree, "global_label"))
        for g in find(tree, "global_label"):
            assert find1(g, "shape")[1] == "passive"
    assert names == {"demo.kicad_sch": ["SIG"], "io.kicad_sch": ["MID", "MID", "SIG"],
                     "sub/power.kicad_sch": ["SIG"]}


def test_netlist_and_references_are_design_wide(symdir):
    sch, io, pw = two_sheets(symdir)
    sch.finish()
    assert sch.netlist() == {"SIG": [("R1", "1"), ("R2", "1"), ("R4", "2")],
                             "GND": [("R1", "2"), ("R3", "2")],
                             "MID": [("R2", "2"), ("R3", "1")], "+3V3": [("R4", "1")]}
    power_refs = [r for r in sch.parts if r.startswith("#PWR")]
    assert len(power_refs) == len(set(power_refs)) == 3            # numbered across sheets, no clash
    with pytest.raises(ValueError, match="duplicate reference R2 .already on sheet IO"):
        pw.add("R2", "tlib:R2P", (140, 100), nets={"1": "A", "2": "B"})
    with pytest.raises(ValueError, match="duplicate reference R1 .already on sheet root"):
        io.add("R1", "tlib:R2P", (140, 100), nets={"1": "A", "2": "B"})


def test_root_finish_finishes_every_sheet_once(symdir):
    sch, io, pw = two_sheets(symdir)
    io.add("R5", "tlib:R2P", (140, 100), nets={"1": "SIG"})          # pin 2 unlisted
    io.finish()
    n = len(io.items)
    sch.finish()
    assert len(io.items) == n and io.finished and pw.finished
    assert pw.labels and [w for w in sch.warnings if w.startswith("R5")]
    assert sch.warnings is io.warnings


def test_a_wire_cannot_reach_a_pin_on_another_sheet(symdir):
    sch, io, pw = two_sheets(symdir)
    with pytest.raises(ValueError, match="another sheet"):
        io.wire(("R2", 1), ("R4", 2))
    with pytest.raises(ValueError, match="another sheet"):
        sch.tag(("R3", 1))


def test_overlaps_per_sheet_and_for_the_design(symdir):
    sch, io, pw = two_sheets(symdir)
    sch.finish()
    assert sch.overlaps() == []
    io.text("a note", 100, 110)
    io.text("another", 100.5, 110)
    assert io.overlaps() and pw.overlaps() == [] and sch.overlaps(design=False) == []
    assert sch.overlaps() == ["IO: " + o for o in io.overlaps()]
    sch.text("on the sheet symbol", 30, 30)                          # root: text inside the IO box
    assert any("sheet IO" in o for o in sch.overlaps(design=False))


def test_sheet_arguments_are_checked(symdir):
    sch, io, pw = two_sheets(symdir)
    for name, file in (("IO", "other.kicad_sch"), ("Other", "io.kicad_sch"), ("X", "x.sch"), ("Y", "/abs/y.kicad_sch")):
        with pytest.raises(ValueError):
            sch.sheet(name, file)


def test_lib_symbols_are_embedded_per_file_and_tables_cover_all_sheets(symdir, tmp_path):
    sch, io, pw = two_sheets(symdir)
    sch.finish()
    sch.save(str(tmp_path / "demo.kicad_sch"))
    libs = [s[1] for s in find(find1(load(tmp_path / "sub/power.kicad_sch"), "lib_symbols"), "symbol")]
    assert sorted(libs) == ["power:+3V3", "tlib:R2P"]
    sch.write_lib_tables(str(tmp_path))
    table = (tmp_path / "sym-lib-table").read_text()
    assert '(name "power")' in table and '(name "tlib")' in table


def test_every_uuid_in_the_design_is_unique(symdir, tmp_path):
    sch, io, pw = two_sheets(symdir)
    sch.finish()
    sch.save(str(tmp_path / "demo.kicad_sch"))
    ids = [n[1] for f in ("demo.kicad_sch", "io.kicad_sch", "sub/power.kicad_sch")
           for n in walk(load(tmp_path / f)) if n and n[0] == "uuid"]
    assert len(ids) == len(set(ids))

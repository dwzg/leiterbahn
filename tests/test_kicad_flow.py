"""Schematic to fab on a real KiCad: a two-sheet design with a project symbol library, a
four-layer board with two ground domains and cut-outs, DRC, Gerbers and the layout checks.

Skipped where KiCad's Python module, kicad-cli or KiCad's libraries are missing (as in CI).
"""
import json
import math
import os
import re
import shutil
import subprocess
import zipfile

import pytest
from conftest import two_pin, write_lib

pcbnew = pytest.importorskip("pcbnew")

from hwdesign.kicad import fab, verify  # noqa: E402
from hwdesign.kicad.board import (  # noqa: E402
    FP_DIR,
    Board,
    copper_layers,
    drc,
    foreign_copper,
    min_gap,
    path_length,
)
from hwdesign.kicad.place import Placer  # noqa: E402
from hwdesign.kicad.schematic import FILE_VERSION, GENERATOR_VERSION, SYMBOL_DIR, Schematic  # noqa: E402
from hwdesign.kicad.sexp import find1, parse  # noqa: E402

pytestmark = pytest.mark.skipif(
    not (shutil.which("kicad-cli") and os.path.isdir(SYMBOL_DIR) and os.path.isdir(FP_DIR)),
    reason="needs kicad-cli and KiCad's symbol and footprint libraries")

R = "Resistor_SMD:R_0805_2012Metric"
HDR = "Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical"
LEFT, RIGHT = ("GND", "SIG", "MID"), ("GNDA", "ISO")


@pytest.fixture(scope="module")
def project(tmp_path_factory):
    """A two-sheet design, saved, with its lib tables, project file and KiCad's netlist.

    Left sheet: J1 - R1 - R2 on ground GND. Right sheet: J2, R3, R4 on ground GNDA. The
    connectors and the PWR_FLAGs sit on the root, so every net crosses a sheet border.
    """
    d = tmp_path_factory.mktemp("iso")
    write_lib(d / "lib", "proj", [two_pin("RES")])
    sch = Schematic("iso", title="Iso", sym_dirs=[str(d / "lib")])
    left = sch.sheet("Left", "left.kicad_sch")
    right = sch.sheet("Right", "sheets/right.kicad_sch")
    sch.add("J1", "Connector_Generic:Conn_01x02", (60, 100), "IN", HDR, {"1": "SIG", "2": "GND"})
    sch.add("J2", "Connector_Generic:Conn_01x02", (60, 130), "OUT", HDR, {"1": "ISO", "2": "GNDA"})
    for net, x in (("GND", 100), ("GNDA", 120)):
        sch.power_flag(net, (x, 100))
        sch.power(net, (x, 100), direction="down")
    left.add("R1", "proj:RES", (100, 100), "1k", R, {"1": "SIG", "2": "MID"})
    left.add("R2", "Device:R", (100, 115), "2k", R, {"1": "MID", "2": "GND"})
    left.wire(("R1", 2), ("R2", 1))                        # MID: a net without a label, on a sub-sheet
    right.add("R3", "proj:RES", (100, 100), "3k", R, {"1": "ISO", "2": "GNDA"})
    right.add("R4", "Device:R", (120, 100), "4k", R, {"1": "ISO", "2": "GNDA"})
    sch.finish()
    root = d / "iso.kicad_sch"
    sch.save(str(root))
    sch.write_lib_tables(str(d))
    # kicad-cli reads the project's sym-lib-table only when a project file sits next to the root sheet
    (d / "iso.kicad_pro").write_text(json.dumps({"meta": {"filename": "iso.kicad_pro", "version": 3}}))
    declared = d / "nets.json"
    declared.write_text(json.dumps(sch.netlist()))
    summary, net, diffs = verify.erc_and_parity(str(root), str(declared), str(d / "build"))
    return dict(dir=d, sch=sch, root=root, declared=str(declared), net=net, erc=summary, diffs=diffs)


def board(project, **kw):
    b = Board(project["net"], declared=project["declared"], **kw)
    b.outline(100, 100, 40, 30)
    return b


def place_all(b):
    """Left domain x < 118, right domain x > 120; nothing within 6 mm of the border."""
    b.place("J1", 104, 110)
    b.place("R1", 110, 110)
    b.place("R2", 110, 116)
    b.place("J2", 136, 110)
    b.place("R3", 129, 110, 180)
    b.place("R4", 129, 116, 180)


def vias(b):
    return [(pcbnew.ToMM(t.GetPosition().x), pcbnew.ToMM(t.GetPosition().y))
            for t in b.board.GetTracks() if t.GetClass() == "PCB_VIA"]


# ---- schematic ----------------------------------------------------------------
def test_multi_sheet_design_passes_erc_and_parity(project):
    assert project["diffs"] == []
    assert project["erc"] == "Found 0 violations", (project["dir"] / "build" / "erc.rpt").read_text()
    report = (project["dir"] / "build" / "erc.rpt").read_text()
    assert "***** Sheet /Left/" in report and "***** Sheet /Right/" in report


def test_nets_keep_flat_names_across_sheets(project):
    from hwdesign.kicad.netlist import read_netlist
    comps, nets = read_netlist(project["net"])
    assert sorted(nets["SIG"]) == [("J1", "1"), ("R1", "1")]                 # root and Left: one net
    assert sorted(nets["GNDA"]) == [("J2", "2"), ("R3", "2"), ("R4", "2")]   # root and Right
    assert not [n for n in nets if n.startswith("/")]
    assert comps["J1"]["sheet"] == "/" and comps["R1"]["sheet"] == "/Left/" and comps["R4"]["sheet"] == "/Right/"
    sheet_uuid = {s.name: s.symbol_uuid for s in project["sch"].sheets}
    assert comps["R1"]["path"] == "/%s/%s" % (sheet_uuid["Left"], comps["R1"]["uuid"])


def test_project_symbol_library_is_reached_through_kiprjmod(project):
    table = (project["dir"] / "sym-lib-table").read_text()
    assert '(name "proj") (type "KiCad") (uri "${KIPRJMOD}/lib/proj.kicad_sym")' in table
    assert '(name "Device") (type "KiCad") (uri "${KICAD10_SYMBOL_DIR}/Device.kicad_sym")' in table
    assert "lib_symbol" not in (project["dir"] / "build" / "erc.rpt").read_text()


def test_file_header_is_what_this_kicad_writes(project, tmp_path):
    version = subprocess.run(["kicad-cli", "--version"], capture_output=True, text=True).stdout.strip()
    if not version.startswith(GENERATOR_VERSION + "."):
        pytest.skip("FILE_VERSION was checked against KiCad %s, this is %s" % (GENERATOR_VERSION, version))
    copy = tmp_path / "iso.kicad_sch"
    shutil.copy(project["root"], copy)
    for s in project["sch"].sheets:
        os.makedirs(os.path.dirname(tmp_path / s.file), exist_ok=True)
        shutil.copy(project["dir"] / s.file, tmp_path / s.file)
    subprocess.run(["kicad-cli", "sch", "upgrade", "--force", str(copy)], capture_output=True, check=True)
    ours, theirs = parse(project["root"].read_text()), parse(copy.read_text())
    assert find1(theirs, "version")[1] == find1(ours, "version")[1] == FILE_VERSION
    assert find1(theirs, "generator_version")[1] == find1(ours, "generator_version")[1] == GENERATOR_VERSION
    # KiCad rewrites the file without adding or dropping anything: same items, same count of each kind
    count = lambda tree: sorted((k, sum(1 for n in tree if isinstance(n, list) and n[0] == k))  # noqa: E731
                                for k in {n[0] for n in tree if isinstance(n, list)})
    assert count(ours) == count(theirs)


def test_single_sheet_design_still_passes(tmp_path):
    sch = Schematic("one")
    sch.add("R1", "Device:R", (100, 100), "1k", R, {"1": "A", "2": "GND"})
    sch.add("R2", "Device:R", (120, 100), "2k", R, {"1": "A", "2": "GND"})
    sch.power_flag("GND", (140, 100))
    sch.power("GND", (140, 100), direction="down")
    sch.finish()
    sch.save(str(tmp_path / "one.kicad_sch"))
    (tmp_path / "nets.json").write_text(json.dumps(sch.netlist()))
    summary, net, diffs = verify.erc_and_parity(str(tmp_path / "one.kicad_sch"), str(tmp_path / "nets.json"),
                                                str(tmp_path / "build"))
    assert diffs == [] and summary == "Found 0 violations"
    assert '(name "/A")' in open(net).read()                  # a local label, as before


# ---- the whole flow --------------------------------------------------------------
def test_four_layer_board_with_two_ground_domains_end_to_end(project):
    d = project["dir"]
    b = board(project, layers=4, planes=[pcbnew.In1_Cu])
    b.cutout(118.5, 106, 1.0, 12, r=0.5)                    # slot in the isolation gap
    b.cutout(108, 99, 6, 3, r=0.5)                          # notch in the top edge
    place_all(b)
    assert b.courtyard_overlaps() == ([], [])
    b.track("SIG", [("J1", 1), ("R1", 1)])
    b.track("MID", [("R1", 2), (110.9125, 113), (109.0875, 113), ("R2", 1)])
    b.track("ISO", [("J2", 1), ("R3", 1)])
    b.track("ISO", [(132, 110), (132, 116), ("R4", 1)])
    b.zone("GND", pcbnew.In1_Cu, rect=(100.5, 100.5, 17.5, 29))            # x 100.5 .. 118
    b.zone("GNDA", pcbnew.In1_Cu, polygon=[(120, 100.5), (139.5, 100.5), (139.5, 129.5), (120, 129.5)])
    b.fill_zones()
    for net, ref in (("GND", "R2"), ("GNDA", "R3"), ("GNDA", "R4")):       # SMD ground pads down to the plane
        assert b.via_near(net, ref, 2) is not None
    assert b.prune_dangling() == 0
    assert b.unrouted() == 0
    b.place_refs()
    pcb = str(d / "iso.kicad_pcb")
    b.save(pcb)
    b.write_project(str(d / "iso.kicad_pro"))

    out = drc(pcb, str(d / "build" / "drc.rpt"))
    report = (d / "build" / "drc.rpt").read_text()
    assert "Found 0 violations" in out and "Found 0 unconnected items" in out, report
    assert "Found 0 schematic parity issues" in out, report

    pb = pcbnew.LoadBoard(pcb)
    assert [pb.GetLayerName(l) for l in copper_layers(pb)] == ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]
    gap = min_gap(pb, [b.resolve(n) for n in LEFT], [b.resolve(n) for n in RIGHT])
    assert gap[0] == pytest.approx(2.0, abs=0.01)
    assert gap[1] == "In1.Cu" and gap[2] == pytest.approx(119.0, abs=0.01) and gap[4:] == ("GND", "GNDA")
    assert min_gap(pb, "GND", "ISO")[0] > 2.0                # plane to the other side's pads and tracks
    assert path_length(pb, b.resolve("MID"), "R1.2", "R2.1") == (pytest.approx(7.83, abs=0.02), 0)

    res = fab.export(pcb, project["net"], str(d / "fab"), "iso")
    assert res["copper"] == ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]
    names = zipfile.ZipFile(res["zip"]).namelist()
    copper = sorted(n for n in names if n.endswith(("_Cu.gtl", "_Cu.gbl")) or re.search(r"_Cu\.g\d+$", n))
    assert copper == ["iso-B_Cu.gbl", "iso-F_Cu.gtl", "iso-In1_Cu.g1", "iso-In2_Cu.g2"]
    assert "iso-Edge_Cuts.gm1" in names and "iso.drl" in names
    plane = (d / "fab" / "gerbers" / "iso-In1_Cu.g1").read_text()
    assert plane.count("G36*") >= 2                          # both pours are in the inner-layer Gerber


def test_footprints_carry_the_instance_path_of_their_symbol(project):
    b = board(project)
    assert b.fps["J1"].GetPath().AsString() == b.comps["J1"]["path"] == "/" + b.comps["J1"]["uuid"]
    left = next(s for s in project["sch"].sheets if s.name == "Left")
    assert b.fps["R1"].GetPath().AsString() == "/%s/%s" % (left.symbol_uuid, b.comps["R1"]["uuid"])


# ---- layers -----------------------------------------------------------------------
def test_default_board_is_two_layers(project):
    b = board(project)
    assert b.copper_layers == [pcbnew.F_Cu, pcbnew.B_Cu] and b.board.GetCopperLayerCount() == 2 and b.planes == []


def test_layer_arguments_are_checked(project):
    with pytest.raises(ValueError, match="layers"):
        Board(project["net"], layers=3)
    with pytest.raises(ValueError, match="inner"):
        Board(project["net"], layers=4, planes=[pcbnew.F_Cu])
    with pytest.raises(ValueError, match="inner"):
        Board(project["net"], layers=4, planes=[pcbnew.In3_Cu])     # not enabled on four layers
    assert Board(project["net"], layers=6).copper_layers == [pcbnew.F_Cu, pcbnew.In1_Cu, pcbnew.In2_Cu,
                                                             pcbnew.In3_Cu, pcbnew.In4_Cu, pcbnew.B_Cu]


def test_plane_layers_reach_the_router_as_type_power(project, tmp_path):
    b = board(project, layers=4, planes=[pcbnew.In1_Cu])
    place_all(b)
    b.zone("GND", pcbnew.In1_Cu, rect=(100.5, 100.5, 17.5, 29))
    b.fill_zones()
    dsn = str(tmp_path / "route.dsn")
    assert pcbnew.ExportSpecctraDSN(b.board, dsn)
    text = open(dsn).read()
    types = dict(re.findall(r"\(layer (\S+)\s+\(type (\w+)\)", text))
    assert types == {"F.Cu": "signal", "In1.Cu": "power", "In2.Cu": "signal", "B.Cu": "signal"}
    assert re.search(r"\(plane GND \(polygon In1\.Cu ", text)
    b.save(str(tmp_path / "b.kicad_pcb"))
    assert '(4 "In1.Cu" power)' in (tmp_path / "b.kicad_pcb").read_text()


def test_path_length_follows_a_route_through_an_inner_layer(project, tmp_path):
    b = board(project, layers=4)
    place_all(b)
    (x1, y1), (x2, y2) = b.pad_xy("J1", 1), b.pad_xy("R1", 1)
    b.track("SIG", [(x1, y1), (106, y1)])
    b.via("SIG", 106, y1)
    b.track("SIG", [(106, y1), (106, 105), (108, 105)], layer=pcbnew.In2_Cu)
    b.via("SIG", 108, 105)
    b.track("SIG", [(108, 105), (x2, 105), (x2, y2)])
    b.board.BuildConnectivity()
    length, n_vias = path_length(b.board, b.resolve("SIG"), "J1.1", "R1.1")
    assert n_vias == 2
    assert length == pytest.approx((106 - x1) + 5 + 2 + (x2 - 108) + (y2 - 105), abs=0.02)
    assert ("SIG", "In2.Cu") in {(n, l) for n, l, _, _ in foreign_copper(b.board, (100, 100, 140, 130), set())}


def test_prune_dangling_keeps_a_via_that_only_feeds_an_inner_plane(project):
    b = board(project, layers=4, planes=[pcbnew.In1_Cu])
    place_all(b)
    x, y = b.pad_xy("R2", 2)
    b.zone("GND", pcbnew.In1_Cu, rect=(100.5, 100.5, 17.5, 29))
    b.track("GND", [("R2", 2), (x + 1.5, y)], locked=False)
    b.via("GND", x + 1.5, y, locked=False)
    b.track("GND", [(x, y), (x, y + 3)], locked=False)       # a stub that leads nowhere
    assert b.prune_dangling() == 1
    assert vias(b) == [pytest.approx((x + 1.5, y))]
    assert len([t for t in b.board.GetTracks() if t.GetClass() != "PCB_VIA"]) == 1


def test_keepout_and_foreign_copper_cover_inner_layers_by_default(project):
    b = board(project, layers=4)
    z = b.keepout(110, 110, 5, 5)
    assert [int(l) for l in z.GetLayerSet().CuStack()] == b.copper_layers
    z = b.keepout(110, 110, 5, 5, layers=(pcbnew.F_Cu,))
    assert [int(l) for l in z.GetLayerSet().CuStack()] == [pcbnew.F_Cu]
    b.track("ISO", [(111, 111), (113, 113)], layer=pcbnew.In1_Cu)
    assert foreign_copper(b.board, (110, 110, 115, 115), {"GND"}) == [("ISO", "In1.Cu", 111.0, 111.0)]
    assert foreign_copper(b.board, (110, 110, 115, 115), {"GND"}, layers=(pcbnew.F_Cu, pcbnew.B_Cu)) == []
    two = board(project)
    assert [int(l) for l in two.keepout(110, 110, 5, 5).GetLayerSet().CuStack()] == [pcbnew.F_Cu, pcbnew.B_Cu]


# ---- zones ------------------------------------------------------------------------
def test_zone_shapes(project):
    b = board(project, layers=4)

    def box(z):
        bb = z.GetBoundingBox()
        return tuple(round(pcbnew.ToMM(v), 3) for v in (bb.GetLeft(), bb.GetTop(), bb.GetRight(), bb.GetBottom()))
    assert box(b.zone("GND", pcbnew.F_Cu)) == (100.3, 100.3, 139.7, 129.7)                 # the board, as before
    assert box(b.zone("GND", pcbnew.In1_Cu, rect=(101, 102, 10, 20))) == (101, 102, 111, 122)
    tri = b.zone("GNDA", pcbnew.In1_Cu, polygon=[(120, 105), (135, 105), (120, 125)])
    assert box(tri) == (120, 105, 135, 125) and tri.Outline().Outline(0).PointCount() == 3
    with pytest.raises(ValueError):
        b.zone("GND", pcbnew.B_Cu, rect=(101, 102, 10, 20), polygon=[(0, 0), (1, 0), (1, 1)])
    with pytest.raises(ValueError):
        b.zone("GND", pcbnew.B_Cu, polygon=[(0, 0), (1, 0)])


def test_min_gap_sees_a_pad_inside_the_other_domain(project):
    b = board(project, layers=4)
    place_all(b)
    b.place("R2", 124, 116)                                   # a GND part dropped into the GNDA area
    b.zone("GND", pcbnew.In1_Cu, rect=(100.5, 100.5, 17.5, 29))
    for layer in (pcbnew.F_Cu, pcbnew.In1_Cu):
        b.zone("GNDA", layer, rect=(120, 100.5, 19.5, 29))
    b.fill_zones()
    gap, layer, x, y, na, nb = min_gap(b.board, ["GND", b.resolve("MID")], ["GNDA"])
    assert layer == "F.Cu" and gap == pytest.approx(0.3, abs=0.01)         # the pour's own clearance, not 2 mm
    assert math.hypot(x - 124, y - 116) < 2.5 and nb == "GNDA"
    assert min_gap(b.board, "GND", "no-such-net") is None
    with pytest.raises(ValueError):
        min_gap(b.board, ["GND", "GNDA"], ["GNDA"])


# ---- cut-outs -----------------------------------------------------------------------
def edge_shapes(b):
    return [d for d in b.board.GetDrawings() if d.GetLayer() == pcbnew.Edge_Cuts]


def test_outlines_are_drawn_as_before(project):
    b = board(project)
    assert len(edge_shapes(b)) == 4
    b.outline_rounded(100, 100, 40, 30, r=2)
    kinds = sorted(d.GetShape() for d in edge_shapes(b))
    assert kinds == sorted([pcbnew.SHAPE_T_SEGMENT] * 4 + [pcbnew.SHAPE_T_ARC] * 4)
    assert b.box == (100, 100, 40, 30) and b.cutouts == []


def test_slot_and_notch_make_a_valid_outline(project, tmp_path):
    b = board(project)
    assert b.cutout(118.5, 106, 1.0, 12, r=0.5) == (118.5, 106, 1.0, 12)      # round-ended slot
    assert b.cutout(137, 120, 8, 4) == (137, 120, 3, 4)                        # notch, clipped to the edge
    # slot: 2 sides + 4 arcs; outline: 3 whole sides, the right one in 2 pieces, 3 notch sides
    assert len(edge_shapes(b)) == 6 + 8
    with pytest.raises(ValueError, match="overlaps"):
        b.cutout(118, 110, 3, 3)
    with pytest.raises(ValueError):
        b.cutout(99, 99, 5, 5)                                                # across a corner
    place_all(b)
    pcb = str(tmp_path / "cut.kicad_pcb")
    b.save(pcb)
    drc(pcb, str(tmp_path / "drc.rpt"), schematic_parity=False)
    report = (tmp_path / "drc.rpt").read_text()
    assert "invalid_outline" not in report and "malformed" not in report.lower()
    edges = pcbnew.LoadBoard(pcb).GetBoardEdgesBoundingBox()
    assert (pcbnew.ToMM(edges.GetWidth()), pcbnew.ToMM(edges.GetHeight())) == pytest.approx((40.1, 30.1))


def test_parts_stay_out_of_cut_outs(project):
    b = board(project)
    b.cutout(116, 106, 6, 12)
    place_all(b)
    b.place("R2", 119, 112)
    assert not b.fits("R2")
    assert b.courtyard_overlaps() == ([], ["R2"])
    x, y, _ = b.place_near("R2", 119, 112)
    assert b.fits("R2") and b.courtyard_overlaps() == ([], [])
    bb = b._box_mm(b._crt(b.fps["R2"]).BBox())
    assert bb[2] <= 116 - 0.3 + 1e-6 or bb[0] >= 122 + 0.3 - 1e-6 or bb[3] <= 106 - 0.3 + 1e-6 or bb[1] >= 118.3 - 1e-6


@pytest.mark.parametrize("slot", [False, True])
def test_reference_labels_stay_out_of_cut_outs(project, slot):
    b = board(project)
    if slot:
        b.cutout(106, 106.3, 8, 2.4)                         # just above R1, where its reference would go
    place_all(b)
    b.place_refs()
    ref_y = pcbnew.ToMM(b.fps["R1"].Reference().GetPosition().y)
    assert ref_y > 110 if slot else ref_y < 110              # below the part instead of above


def test_pad_labels_stay_out_of_cut_outs(project):
    def label(cut=None):
        b = board(project)
        if cut:
            b.cutout(*cut)
        place_all(b)
        t = b.label_near("J2", 2, "OUT")
        return b, t, (pcbnew.ToMM(t.GetPosition().x), pcbnew.ToMM(t.GetPosition().y))
    _, _, (x0, y0) = label()
    cut = (x0 - 0.5, y0 - 0.5, 1.0, 1.0)                     # a slot exactly where the label went
    b, t, (x, y) = label(cut)
    assert (x, y) != (x0, y0)
    assert not b._hits_cutout(b._box_mm(t.GetBoundingBox()), 0.5)


def test_stitching_and_via_near_stay_out_of_cut_outs(project):
    b = board(project)
    b.cutout(116, 104, 6, 20)
    place_all(b)
    for layer in b.copper_layers:
        b.zone("GND", layer)
    assert b.stitch("GND", pitch=1.5, edge=1.0) > 20
    for x, y in vias(b):
        assert not (116 - 0.7 < x < 122 + 0.7 and 104 - 0.7 < y < 124 + 0.7), (x, y)
    b.place("R2", 113.5, 116)                                # GND pad 1.6 mm left of the slot
    spot = b.via_near("GND", "R2", 2, rmin=0.9, rmax=3.0)
    assert spot is not None and not (116 - 0.7 < spot[0] < 122 + 0.7 and 104 - 0.7 < spot[1] < 124 + 0.7)


def test_stitching_misses_foreign_tracks_on_layers_without_the_pour(project):
    b = board(project, layers=4, planes=[pcbnew.In1_Cu])
    place_all(b)
    b.track("SIG", [(101, 121), (139, 121)], width=0.3)                     # front layer, no GND pour there
    b.track("ISO", [(101, 125), (139, 125)], width=0.3, layer=pcbnew.In2_Cu)
    for layer in (pcbnew.In1_Cu, pcbnew.B_Cu):
        b.zone("GND", layer)
    assert b.stitch("GND", pitch=1.0, edge=1.0) > 50
    need = 0.3 + 0.15 + b.rules["clearance"]                               # via radius + half track + clearance
    for x, y in vias(b):
        assert abs(y - 121) >= need - 1e-6 and abs(y - 125) >= need - 1e-6, (x, y)
    b.save(str(project["dir"] / "stitch.kicad_pcb"))
    out = drc(str(project["dir"] / "stitch.kicad_pcb"), str(project["dir"] / "build" / "stitch.rpt"),
              schematic_parity=False)
    report = (project["dir"] / "build" / "stitch.rpt").read_text()
    assert "shorting_items" not in report and "[clearance]" not in report, out


def test_parts_on_opposite_sides_may_overlap_unless_one_goes_through_the_board(project):
    # 489 parts on 100 x 70 mm only fit on both sides. fits() compared every courtyard with
    # every other, so a resistor on the back had no room under a resistor on the front, and
    # place_near() could only place on the front.
    b = board(project)
    place_all(b)
    b.place("R2", 110, 110, back=True)                    # straight under R1
    assert b.fits("R2") and b.courtyard_overlaps() == ([], [])
    b.place("R2", 104, 110, back=True)                    # under the header J1: its pins come through
    assert not b.fits("R2")
    assert b.courtyard_overlaps()[0] == [("J1", "R2")]
    x, y, _ = b.place_near("R2", 104, 110, back=True)
    assert b.fps["R2"].IsFlipped() and b.fits("R2") and b.courtyard_overlaps() == ([], [])
    b.place("R2", 110, 110)                               # and back to the front, onto R1
    assert not b.fps["R2"].IsFlipped() and not b.fits("R2")


# ---- placement by connections --------------------------------------------------
def placer(project, **kw):
    b = board(project)
    return b, Placer(b, **kw)


def overlap(a, b):
    return a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]


def test_a_part_is_placed_next_to_the_pad_it_connects_to(project):
    b, p = placer(project)
    p.fix("J1", 4, 10)                                    # millimetres from the board's corner at (100, 100)
    p.fix("R1", 10, 10)
    assert b.pad_xy("J1", 1) == (104.0, 110.0) and p.pad("J1", 1) == (4.0, 10.0)
    p.near("R2", anchor=(30, 25))                         # R2.1 is MID, as R1.2; its other pad is on the plane
    (x, y), (rx, ry) = p.pad("R2", 1), p.pad("R1", 2)
    assert abs(x - rx) + abs(y - ry) < 3.0                # at R1, not at its anchor
    assert not p.at["R2"][3] and not overlap(p.boxes["R2"], p.boxes["R1"])
    with pytest.raises(ValueError):
        p.fix("R2", 20, 20)                               # placed already


def test_the_back_is_used_when_the_front_is_taken_but_not_under_through_hole_parts(project):
    b, p = placer(project)
    p.fix("J1", 4, 10)
    p.fix("R1", 10, 10)
    p.keep(0, 0, 40, 30, "F")                             # nothing more on the front
    p.near("R2", anchor=(10, 10))
    assert p.at["R2"][3] and b.fps["R2"].IsFlipped()
    assert overlap(p.boxes["R2"], p.boxes["R1"])          # straight under R1: nothing in the way there
    b, p = placer(project)
    p.fix("J1", 4, 10)
    p.near("R1", anchor=(4, 11), start=(4, 11), sides=(True,))      # asked for the spot under the header
    assert p.at["R1"][3] and not overlap(p.boxes["R1"], p.boxes["J1"])


def test_rules_keep_parts_out_and_fixed_parts_ignore_them(project):
    b, p = placer(project)
    p.rules.append(lambda ref, box, back, through: box[0] < 20)      # nothing left of x = 20
    p.fix("R1", 10, 10)                                   # by hand: allowed
    p.near("R2", anchor=(10, 10))
    assert p.boxes["R2"][0] >= 20
    assert not p.free("R4", 10, 20) and p.free("R4", 30, 20)
    assert not p.free("R4", 39.5, 20)                     # over the board edge


def test_face_and_find(project):
    b, p = placer(project)
    rot = p.face("R1", "MID", "left")
    p.fix("R1", 10, 10, rot)
    assert p.pad("R1", 2)[0] < 10 < p.pad("R1", 1)[0]
    assert b.pad_xy("R1", 2)[0] < 110                     # the footprint was turned, not only the bookkeeping
    assert p.face("R1", "MID", "down") != rot
    assert p.find("MID", "GND") == "R2" and p.find("MID", "SIG") == "R1"
    with pytest.raises(LookupError):
        p.find("MID")                                     # R1 and R2


def test_each_supply_pin_gets_its_own_capacitor(project):
    # Capacitors on a rail were all put next to the first supply pin, or between two pins.
    nets = {"VCC": [("J1", "1"), ("J1", "2"), ("R1", "1"), ("R2", "1"), ("R3", "1"), ("R4", "1")],
            "GND": [("R1", "2"), ("R2", "2"), ("R3", "2"), ("R4", "2"), ("J2", "1"), ("J2", "2")]}
    b, p = placer(project, nets=nets)
    p.fix("J1", 15, 10)                                   # its two pins stand in for two supply pins of an IC
    for ref in ("R1", "R2"):
        p.near(ref, anchor=(15, 10))
    pins = [p.pad("J1", 1), p.pad("J1", 2)]

    def nearest(ref):
        x, y = p.pad(ref, 1)
        return min(range(2), key=lambda i: abs(x - pins[i][0]) + abs(y - pins[i][1]))
    assert {nearest("R1"), nearest("R2")} == {0, 1}
    assert sorted(p.served.values()) == [1, 1]


def test_large_parts_go_first_and_everything_ends_up_placed(project):
    b, p = placer(project)
    assert p.big("J1") and not p.big("R1")
    refs = sorted(b.fps)
    p.large(refs, anchor=(20, 15))
    assert sorted(p.at) == ["J1", "J2"]                   # the headers; front side only
    p.small(refs, anchor=(20, 15))
    assert sorted(p.at) == refs and b.courtyard_overlaps() == ([], [])
    assert p.length() > 0


# ---- autorouting ------------------------------------------------------------------
@pytest.mark.skipif(not shutil.which("freerouting"), reason="needs the freerouting command")
def test_autoroute_connects_the_board_and_adds_no_fanout_vias(project, tmp_path):
    # Freerouting 2.5 starts with a "fanout" stage that gives every SMD pin a via: 1168 vias on
    # a 491-part board with parts on both sides, where they block the other side.
    b = board(project)
    place_all(b)
    log = b.autoroute(str(tmp_path), passes=5, timeout=300, strategies=[[]], optimize=False)
    assert b.unrouted() == 0
    assert "Fanout stage" not in log
    again = board(project)
    place_all(again)
    again.autoroute(str(tmp_path), passes=5, timeout=300, strategies=[[]], optimize=False)   # from the cache
    assert again.unrouted() == 0 and vias(again) == vias(b)
    fan = board(project)
    place_all(fan)
    log = fan.autoroute(str(tmp_path), passes=5, timeout=300, strategies=[[]], optimize=False, fanout=True)
    assert "Fanout stage" in log and len(vias(fan)) > len(vias(b))


# ---- ground pads into their planes ----------------------------------------------------
def planes_board(project, **kw):
    b = board(project, layers=4, planes=[pcbnew.In1_Cu], **kw)
    place_all(b)
    b.zone("GND", pcbnew.In1_Cu, rect=(100.5, 100.5, 17.5, 29))
    b.zone("GNDA", pcbnew.In1_Cu, polygon=[(120, 100.5), (139.5, 100.5), (139.5, 129.5), (120, 129.5)])
    return b


def test_fan_to_planes_gives_each_ground_pad_a_via_inside_its_plane(project, tmp_path):
    b = planes_board(project)
    b.place("R4", 129, 116, 180, back=True)                # one ground pad on the back
    b.track("GNDA", [("R3", 2), (126.5, 108.5)])           # R3's ground pad is routed by hand
    assert b.fan_to_planes() == []
    assert b.on_plane == {("J1", "2"), ("R2", "2"), ("J2", "2"), ("R4", "2")}   # not R3.2: it has a track
    at = sorted(vias(b))
    assert len(at) == 2
    for (vx, vy), (ref, lo, hi) in zip(at, (("R2", 100.5, 118.0), ("R4", 120.0, 139.5)), strict=True):
        px, py = b.pad_xy(ref, 2)
        assert math.hypot(vx - px, vy - py) < 2.0 and lo + 0.5 < vx < hi - 0.5
    r4 = pcbnew.VECTOR2I_MM(*b.pad_xy("R4", 2))            # the track to the via is on the pad's side
    wires = [t for t in b.board.GetTracks() if t.GetClass() != "PCB_VIA"]
    assert [t.GetLayer() for t in wires if t.GetStart() == r4] == [pcbnew.B_Cu]
    b.track("SIG", [("J1", 1), ("R1", 1)])
    b.track("MID", [("R1", 2), (110.9125, 113), (109.0875, 113), ("R2", 1)])
    b.track("ISO", [("J2", 1), ("R3", 1)])
    b.track("ISO", [(132, 110), (132, 116)])
    b.via("ISO", 132, 116)
    b.track("ISO", [(132, 116), ("R4", 1)], layer=pcbnew.B_Cu)
    b.via("GNDA", 126.5, 108.5)
    b.fill_zones()
    assert b.unrouted() == 0
    pcb = str(tmp_path / "iso.kicad_pcb")
    b.save(pcb)
    b.write_project(str(tmp_path / "iso.kicad_pro"))
    out = drc(pcb, str(tmp_path / "drc.rpt"), schematic_parity=False)
    assert "Found 0 violations" in out and "Found 0 unconnected items" in out, (tmp_path / "drc.rpt").read_text()


def test_fan_to_planes_keeps_vias_out_of_foreign_copper_and_rule_areas(project):
    b = planes_board(project)
    b.fan_to_planes()
    free = sorted(vias(b))[0]                               # where R2's via goes when nothing is in the way
    b = planes_board(project)
    b.track("SIG", [(free[0] - 3, free[1]), (free[0] + 3, free[1])], layer=pcbnew.B_Cu)   # a foreign track on the back
    assert b.fan_to_planes() == []
    moved = sorted(vias(b))[0]
    assert math.hypot(moved[0] - free[0], moved[1] - free[1]) > 0.2
    assert abs(moved[1] - free[1]) >= 0.25 + 0.125 + b.rules["clearance"]      # via radius + half track + clearance
    b = planes_board(project)
    px, py = b.pad_xy("R2", 2)
    b.keepout(px - 4, py - 4, 8, 8, tracks=False, vias=True, fills=False)
    assert b.fan_to_planes() == [("R2", "2")] and ("R2", "2") not in b.on_plane


def test_pins_on_a_plane_are_taken_off_the_routers_list(project, tmp_path):
    b = planes_board(project)
    b.fan_to_planes()
    dsn = str(tmp_path / "t.dsn")
    assert pcbnew.ExportSpecctraDSN(b.board, dsn)
    before = open(dsn).read()
    assert "R2-2" in before and "J1-2" in before
    assert Board._strip_pins(dsn, b.on_plane) == 5
    after = open(dsn).read()
    network = after[after.index("(network"):]
    assert "R2-2" not in network and "J1-2" not in network and "R1-1" in network
    assert after[:after.index("(network")] == before[:before.index("(network")]


@pytest.mark.skipif(not shutil.which("freerouting"), reason="needs the freerouting command")
def test_autoroute_after_fan_to_planes_leaves_nothing_open(project, tmp_path):
    # Without fan_to_planes() a 491-part board had 273 of its 465 open connections on the three
    # plane nets after a pass: Freerouting does not tie pins to a plane.
    b = planes_board(project)
    assert b.fan_to_planes() == []
    b.autoroute(str(tmp_path), passes=5, timeout=300, strategies=[[]], optimize=False)
    b.fill_zones()
    assert b.unrouted() == 0
    assert not [t for t in b.board.GetTracks() if not t.IsLocked() and t.GetNetname() in ("GND", "GNDA")]


def test_fan_to_planes_puts_the_via_into_a_large_pad(project):
    # Exposed pads of QFN packages are ringed by their pins: no spot beside them within reach.
    b = planes_board(project)
    pad = next(p for p in b.fps["R2"].Pads() if p.GetNumber() == "2")
    pad.SetSize(pcbnew.F_Cu, pcbnew.VECTOR2I_MM(1.2, 1.4))
    assert b.fan_to_planes() == []
    px, py = b.pad_xy("R2", 2)
    assert (px, py) in [(round(x, 4), round(y, 4)) for x, y in vias(b)]
    assert not [t for t in b.board.GetTracks() if t.GetClass() != "PCB_VIA" and t.GetLength() == 0]   # no track
    assert ("R2", "2") in b.on_plane                       # of no length from the pad's middle to the via on it


def test_route_open_connects_every_net_through_a_wall_and_to_the_back(project, tmp_path):
    """The finisher after the autorouter: a connection needs only a free path. Here one net has
    its last pad on the back and a foreign track stands between two pads."""
    b = planes_board(project)
    b.place("R4", 129, 116, 180, back=True)
    assert b.fan_to_planes() == []
    x0, y0 = b.pad_xy("J1", 1)
    x1, y1 = b.pad_xy("R1", 1)
    xm = (x0 + x1) / 2
    b.track("ISO", [(xm, min(y0, y1) - 3), (xm, max(y0, y1) + 3)])      # a wall across SIG's straight way
    before = b.unrouted()
    routed, failed = b.route_open(str(tmp_path))
    assert failed == [] and routed >= 3 and before >= 3
    iso = b.resolve("ISO")                                 # R4 on the back joins the nearest copper of its net it
    back = [t for t in b.board.GetTracks() if t.GetNetname() == iso and t.GetLayer() == pcbnew.B_Cu]   # can reach:
    assert back and not any(t.GetClass() == "PCB_VIA" for t in back)    # J2's pin, on the back, without a via
    b.fill_zones()
    assert b.unrouted() == 0
    pcb = str(tmp_path / "open.kicad_pcb")
    b.save(pcb)
    b.write_project(str(tmp_path / "open.kicad_pro"))
    out = drc(pcb, str(tmp_path / "drc.rpt"), schematic_parity=False)
    report = (tmp_path / "drc.rpt").read_text()
    found = re.findall(r"^\[(\w+)\]", report, re.M)
    assert "Found 0 unconnected items" in out and found == ["track_dangling"], report    # only the wall itself


def test_route_open_reports_a_connection_that_has_no_path(project, tmp_path):
    b = planes_board(project)
    b.fan_to_planes()
    x, y = b.pad_xy("R1", 1)
    b.keepout(x - 2.5, y - 2.5, 5, 5, name="box")          # R1 pad 1 is fenced in on every layer
    routed, failed = b.route_open(str(tmp_path), fine=False)
    nets = [f[0] for f in failed]
    assert any(n.endswith("SIG") for n in nets) and all(f[1].startswith("no ") for f in failed)
    assert routed >= 1                                     # the nets outside the fence are connected all the same


def fenced_board(project):
    """R1 inside a ring of unlocked ISO track on every routing layer, as an autorouter might leave it."""
    b = planes_board(project)
    b.fan_to_planes()
    (x0, y0), (x1, y1) = b.pad_xy("R1", 1), b.pad_xy("R1", 2)
    box = (min(x0, x1) - 1.6, min(y0, y1) - 1.6, max(x0, x1) + 1.6, max(y0, y1) + 1.6)
    ring = [(box[0], box[1]), (box[2], box[1]), (box[2], box[3]), (box[0], box[3]), (box[0], box[1])]
    for layer in (pcbnew.F_Cu, pcbnew.In2_Cu, pcbnew.B_Cu):
        b.track("ISO", ring, layer=layer, locked=False)
    return b


def test_route_open_removes_unlocked_tracks_where_there_is_no_other_way(project, tmp_path):
    b = fenced_board(project)
    routed, failed = b.route_open(str(tmp_path), rip=0, fine=False)
    assert {f[0] for f in failed} == {b.resolve("SIG"), b.resolve("MID")}       # both of R1's nets are shut in
    b = fenced_board(project)
    routed, failed = b.route_open(str(tmp_path))
    assert failed == []
    b.fill_zones()
    assert b.unrouted() == 0
    pcb = str(tmp_path / "rip.kicad_pcb")
    b.save(pcb)
    b.write_project(str(tmp_path / "rip.kicad_pro"))
    drc(pcb, str(tmp_path / "drc.rpt"), schematic_parity=False)
    found = set(re.findall(r"^\[(\w+)\]", (tmp_path / "drc.rpt").read_text(), re.M))
    assert found <= {"track_dangling"}, found              # what is left of the rings; nothing shorted or too close


def test_route_open_never_removes_locked_tracks(project, tmp_path):
    b = fenced_board(project)
    for t in b.board.GetTracks():
        t.SetLocked(True)
    n = len(b.board.GetTracks())
    routed, failed = b.route_open(str(tmp_path), fine=False)
    assert {f[0] for f in failed} == {b.resolve("SIG"), b.resolve("MID")}
    assert len(b.board.GetTracks()) >= n


def test_route_open_gives_a_track_it_crosses_another_way(project, tmp_path):
    """SIG's pad sits inside a loop of the unlocked track R1.2 - R2.1 (rings on the other layers
    keep it from diving under). That track is taken out and laid again clear of the new one."""
    b = planes_board(project)
    b.fan_to_planes()
    loop = [(110.91, 110), (110.91, 108), (106.5, 108), (106.5, 112), (110.91, 112)]
    b.track("MID", loop + [(110.91, 110)], locked=False)
    b.track("MID", [(110.91, 112), (109.09, 113.8), (109.09, 116)], locked=False)
    ring = [(106.2, 107.7), (111.4, 107.7), (111.4, 112.3), (106.2, 112.3), (106.2, 107.7)]
    for layer in (pcbnew.In2_Cu, pcbnew.B_Cu):
        b.track("ISO", ring, layer=layer, locked=False)
    b.fill_zones()
    open_before = b.unrouted()
    routed, failed = b.route_open(str(tmp_path))
    assert failed == [] and routed == open_before
    mid = b.resolve("MID")
    left = [t for t in b.board.GetTracks() if t.GetNetname() == mid and t.GetClass() != "PCB_VIA"
            and pcbnew.ToMM(t.GetStart().x) == 106.5 and pcbnew.ToMM(t.GetEnd().x) == 106.5]
    assert left == []                                      # the side of the loop that was in the way is gone
    b.fill_zones()
    assert b.unrouted() == 0                               # and R1.2 - R2.1 is connected again
    pcb = str(tmp_path / "again.kicad_pcb")
    b.save(pcb)
    b.write_project(str(tmp_path / "again.kicad_pro"))
    drc(pcb, str(tmp_path / "drc.rpt"), schematic_parity=False)
    found = set(re.findall(r"^\[(\w+)\]", (tmp_path / "drc.rpt").read_text(), re.M))
    assert found <= {"track_dangling"}, (tmp_path / "drc.rpt").read_text()


def qfn_board(project):
    """planes_board with a 0.5 mm pitch QFN over the left plane; pin 3 (left side) is on GND."""
    b = planes_board(project, track=0.15, clearance=0.1)
    fp = pcbnew.FootprintLoad(os.path.join(FP_DIR, "Package_DFN_QFN.pretty"), "QFN-24-1EP_4x4mm_P0.5mm_EP2.6x2.6mm")
    fp.SetReference("U1")
    b.board.Add(fp)
    b.fps["U1"] = fp
    b.place("U1", 111, 123)
    gnd = b.board.FindNet(b.resolve("GND"))
    for pd in fp.Pads():
        if pd.GetNumber() == "3":
            pd.SetNet(gnd)
    return b


@pytest.mark.parametrize("lane", [0, 1.0])
def test_fan_to_planes_leaves_the_neighbours_of_an_ic_pin_their_way_out(project, lane):
    b = qfn_board(project)
    before = set(vias(b))
    assert b.fan_to_planes(track=0.2, lane=lane) == []
    px, py = b.pad_xy("U1", 3)
    (vx, vy), = [v for v in set(vias(b)) - before if math.hypot(v[0] - px, v[1] - py) < 3]
    left = min(pcbnew.ToMM(pd.GetBoundingBox().GetLeft()) for pd in b.fps["U1"].Pads() if pd.GetNumber() == "3")
    in_front = [n for n in ("2", "4") if left - 1.0 < vx < left and abs(vy - b.pad_xy("U1", n)[1]) < 0.3 + 0.125 + 0.2]
    assert bool(in_front) == (lane == 0), (vx - px, vy - py)   # without lanes the via shuts a neighbour in


def test_prune_dangling_keeps_a_track_whose_end_only_overlaps_the_via(project):
    """An autorouted joint as found on a routed board: the track ends 0.35 mm from the centre of a
    0.6 mm via. KiCad counts the overlapping copper as connected, so nothing dangles."""
    b = planes_board(project)
    b.fan_to_planes()
    x0, y0 = b.pad_xy("R1", 1)
    x1, y1 = b.pad_xy("J1", 1)
    vx, vy = x0 - 2.0, y0 + 1.5
    b.track("SIG", [(x0, y0), (x0, vy - 0.25), (vx + 0.25, vy - 0.25)], locked=False)    # ends beside the via
    b.via("SIG", vx, vy, locked=False)
    b.track("SIG", [(vx, vy), (x1, vy), (x1, y1)], layer=pcbnew.B_Cu, locked=False)
    b.track("MID", [("R1", 2), ("R2", 1)])
    b.track("ISO", [("J2", 1), ("R3", 1)])
    b.track("ISO", [("R3", 1), ("R4", 1)])
    b.fill_zones()
    assert b.unrouted() == 0                               # the premise: KiCad sees SIG connected
    assert b.prune_dangling() == 0
    assert b.unrouted() == 0


def test_a_movable_track_does_not_open_a_rule_area_or_the_board_edge():
    """In the finisher's map a cell is 2 where only a movable track is in the way. Inside a rule
    area and at the board edge it stays 1 whatever lies there: a via once landed in an
    isolation gap along a track that ran beside it."""
    from hwdesign.kicad import finish
    grid = finish.Grid((0, 0, 10, 10), 0.1)
    track = dict(kind="seg", net="OTHER", layers={0}, hole=0.0, soft=True, seg=(1.0, 5.0, 10.0, 5.0), r=0.1)
    area = ((4.0, 3.0, 6.0, 7.0), {0}, True, True)
    blocked, no_via = finish.obstacles(grid, {"t": track}, [area], (0, 0, 10, 10), "NET", [0], 0.1, 0.1, 0.3)
    at = lambda plane, x, y: plane[grid.cell(x, y)[1]][grid.cell(x, y)[0]]      # noqa: E731
    assert at(blocked[0], 2.0, 5.0) == 2 and at(no_via, 2.0, 5.0) == 2    # on the track, in the open: movable
    assert at(blocked[0], 5.0, 5.0) == 1 and at(no_via, 5.0, 5.0) == 1    # on the track, inside the area: closed
    assert at(blocked[0], 9.9, 5.0) == 1 and at(no_via, 9.8, 5.0) == 1    # on the track, at the board edge: closed
    assert at(blocked[0], 2.0, 8.0) == 0

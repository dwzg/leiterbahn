from hwdesign.kicad.netlist import read_netlist
from hwdesign.kicad.schematic import compare_netlist

NET = """(export (version "E")
  (components
    (comp (ref "R1") (value "10k") (footprint "Resistor_SMD:R_0805_2012Metric") (tstamps "a1"))
    (comp (ref "R2") (value "1k") (footprint "Resistor_SMD:R_0805_2012Metric") (tstamps "b2")))
  (nets
    (net (code "1") (name "/SIG") (node (ref "R1") (pin "1")) (node (ref "R2") (pin "1")))
    (net (code "2") (name "GND") (node (ref "R1") (pin "2")) (node (ref "R2") (pin "2")))))
"""


def write(tmp_path):
    p = tmp_path / "t.net"
    p.write_text(NET)
    return str(p)


def test_read_netlist(tmp_path):
    comps, nets = read_netlist(write(tmp_path))
    assert comps["R1"]["value"] == "10k"
    assert comps["R2"]["footprint"].endswith("R_0805_2012Metric")
    assert sorted(nets["/SIG"]) == [("R1", "1"), ("R2", "1")]


def test_matching_declaration_has_no_diffs(tmp_path):
    declared = {"SIG": [["R1", "1"], ["R2", "1"]], "GND": [["R1", "2"], ["R2", "2"]]}
    assert compare_netlist(declared, write(tmp_path)) == []


def test_wrong_grouping_is_reported(tmp_path):
    declared = {"SIG": [["R1", "1"], ["R2", "2"]], "GND": [["R1", "2"], ["R2", "1"]]}
    diffs = compare_netlist(declared, write(tmp_path))
    assert any("SIG" in d for d in diffs) and any("not declared" in d for d in diffs)


def test_renamed_net_is_reported(tmp_path):
    declared = {"DATA": [["R1", "1"], ["R2", "1"]], "GND": [["R1", "2"], ["R2", "2"]]}
    assert compare_netlist(declared, write(tmp_path)) == ["net DATA is named SIG in KiCad"]


SHEETS = """(export (version "E")
  (components
    (comp (ref "J1") (value "J") (footprint "L:F") (sheetpath (names "/") (tstamps "/")) (tstamps "aa"))
    (comp (ref "R1") (value "1k") (footprint "L:F") (sheetpath (names "/IO/") (tstamps "/s1/")) (tstamps "bb")))
  (nets
    (net (code "1") (name "SIG") (node (ref "J1") (pin "1")) (node (ref "R1") (pin "1")))))
"""


def test_instance_path_includes_the_sheet(tmp_path):
    p = tmp_path / "s.net"
    p.write_text(SHEETS)
    comps, _ = read_netlist(str(p))
    assert (comps["J1"]["path"], comps["J1"]["sheet"]) == ("/aa", "/")
    assert (comps["R1"]["path"], comps["R1"]["sheet"]) == ("/s1/bb", "/IO/")


def test_netlist_without_sheetpath_is_on_the_root(tmp_path):
    comps, _ = read_netlist(write(tmp_path))
    assert comps["R1"]["path"] == "/a1" and comps["R1"]["sheet"] == "/"


def test_global_net_names_compare_without_a_sheet_prefix(tmp_path):
    p = tmp_path / "s.net"
    p.write_text(SHEETS)
    assert compare_netlist({"SIG": [["J1", "1"], ["R1", "1"]]}, str(p)) == []

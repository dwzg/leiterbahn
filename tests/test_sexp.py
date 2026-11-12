from leiterbahn.kicad.sexp import Sym, dump, find, find1, parse

SRC = '(kicad_pcb (version 20250114) (net 1 "/SIG") (footprint "R_0805" (at 1.5 -2 90)))'


def test_round_trip_keeps_structure():
    tree = parse(SRC)
    assert parse(dump(tree)) == tree


def test_bare_atoms_and_strings_differ():
    tree = parse(SRC)
    assert isinstance(tree[0], Sym)
    net = find1(tree, "net")
    assert net[2] == "/SIG" and not isinstance(net[2], Sym)


def test_find():
    tree = parse(SRC)
    assert len(find(tree, "footprint")) == 1
    assert find1(tree, "missing", "default") == "default"

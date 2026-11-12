"""negotiate.py without KiCad: the problem it writes for leiterbahn-route, and leiterbahn-route's answer to it."""
import shutil

import pytest

from leiterbahn.kicad import negotiate

RULES = dict(routing=[0, 6, 2], edge=(0.0, 0.0, 20.0, 10.0), edge_clear=0.3, clearance=0.2, via=(0.6, 0.3),
             hole_clear=0.25, width={"PWR": 0.5}, track=0.2, wider={"HV": 0.5}, areas=[])


def pad(net, x, y, layer=0, hole=0.0):
    return dict(kind="pad", net=net, layers={layer} if not hole else {0, 4, 6, 2}, hole=hole, soft=False,
                at=(x, y), box=(x - 0.3, y - 0.3, x + 0.3, y + 0.3))


def seg(net, x0, y0, x1, y1, layer=0):
    return dict(kind="seg", net=net, layers={layer}, hole=0.0, soft=True, seg=(x0, y0, x1, y1), r=0.1)


def test_a_net_in_pieces_is_routed_and_all_other_copper_is_in_the_way():
    items = {"a1": pad("A", 2, 5), "a2": pad("A", 18, 5), "t": seg("A", 2, 5, 6, 5), "p1": pad("PWR", 10, 2),
             "p2": pad("PWR", 10, 8, layer=2), "h": pad("HV", 10, 5, hole=0.4), "one": pad("DONE", 5, 8),
             "two": pad("DONE", 6, 8)}
    piece = {"a1": 0, "t": 0, "a2": 1, "p1": 2, "p2": 3, "h": 4, "one": 5, "two": 5}
    prob, routed = negotiate.problem(items, piece, RULES)
    assert routed == {"A": [["a1", "t"], ["a2"]], "PWR": [["p1"], ["p2"]]}
    nets = {n["name"]: n for n in prob["nets"]}
    assert (nets["PWR"]["width"], nets["PWR"]["narrow"], nets["A"]["width"]) == (0.5, 0.2, 0.2)
    assert nets["PWR"]["pieces"][1][0]["layers"] == [2]            # the back is the third routing layer
    assert len(prob["fixed"]) == 3                                 # HV's pad and the two pads of the joined net
    hv = next(s for s in prob["fixed"] if s["hole"])
    assert hv["clearance"] == 0.5 and hv["layers"] == [0, 1, 2]    # a drilled pad is on every routing layer
    heavy = negotiate.problem(items, piece, RULES, weights={"PWR": 4})[0]["nets"]
    assert nets["PWR"]["weight"] == 1.0 and {n["name"]: n["weight"] for n in heavy} == {"A": 1.0, "PWR": 4}
    assert negotiate.problem(items, piece, RULES, skip={"PWR"})[1].keys() == {"A"}
    assert negotiate.problem(items, piece, RULES, only={"PWR"})[1].keys() == {"PWR"}


def test_in_a_box_only_the_pieces_that_reach_into_it_are_joined():
    items = {"a1": pad("A", 2, 5), "a2": pad("A", 8, 5), "a3": pad("A", 18, 5)}
    prob, routed = negotiate.problem(items, {"a1": 0, "a2": 1, "a3": 2}, RULES, box=(0, 0, 10, 10))
    assert routed == {"A": [["a1"], ["a2"]]} and prob["box"] == [0, 0, 10, 10]
    prob, routed = negotiate.problem(items, {"a1": 0, "a2": 1, "a3": 2}, RULES, box=(0, 0, 5, 10))
    assert routed == {} and len(prob["fixed"]) == 1               # one piece in the box: nothing to join there


@pytest.mark.skipif(not shutil.which("cargo"), reason="needs cargo to build leiterbahn-route")
def test_the_router_joins_two_nets_that_cross_on_two_layers(tmp_path):
    items = {"a1": pad("A", 2, 5), "a2": pad("A", 18, 5), "b1": pad("B", 10, 1), "b2": pad("B", 10, 9)}
    prob, routed = negotiate.problem(items, {k: n for n, k in enumerate(items)}, RULES, step=0.1)
    lines = []
    answer = negotiate.solve(prob, str(tmp_path), lines.append)
    assert [n["open"] for n in answer["nets"]] == [[], []]
    assert lines[0].startswith("round 0: 2 nets laid") and answer["conflicts"][-1] == 0
    vias = sum(1 for n in answer["nets"] for p in n["paths"] for a, c in zip(p["cells"], p["cells"][1:], strict=False)
               if a[0] != c[0])
    assert vias == 2
    assert all(0 < n["alone"] <= n["cost"] + 1e-6 for n in answer["nets"])      # none shorter than alone on the board
    ends = {(n["name"], tuple(p["from"]), tuple(p["to"])) for n in answer["nets"] for p in n["paths"]}
    assert ends == {("A", (0, 0), (1, 0)), ("B", (0, 0), (1, 0))}

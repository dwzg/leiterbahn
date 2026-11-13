"""What the netlist says a layout must give: supply pins, quiet nodes, boot capacitors."""
import math

from leiterbahn.kicad import intent
from leiterbahn.kicad.netlist import read_pins

# A buck converter U1 (VIN, GND, EN, FB, SW, BOOT), an op-amp U2 on the same rail, and their parts.
PINS = {("U1", "1"): ("VIN", "power_in"), ("U1", "2"): ("GND", "power_in"), ("U1", "3"): ("EN", "input"),
        ("U1", "4"): ("FB", "input"), ("U1", "5"): ("SW", "output"), ("U1", "6"): ("BOOT", "passive"),
        ("U2", "8"): ("V+", "power_in"), ("U2", "4"): ("V-", "power_in"), ("U2", "3"): ("+", "input"),
        ("U2", "1"): ("", "output"), ("J1", "1"): ("VIN", "power_in")}
NETS = {"VIN": [("U1", "1"), ("U1", "3"), ("U2", "8"), ("C1", "1"), ("C2", "1"), ("J1", "1")],
        "GND": [("U1", "2"), ("U2", "4"), ("C1", "2"), ("C2", "2"), ("R2", "2")],
        "FB": [("U1", "4"), ("R1", "2"), ("R2", "1")],
        "SW": [("U1", "5"), ("L1", "1"), ("C3", "2")],
        "BOOT": [("U1", "6"), ("C3", "1")],
        "OUT": [("L1", "2"), ("R1", "1"), ("R3", "1")],
        "INP": [("U2", "3"), ("R3", "2")]}


def test_supply_pins_grounds_and_roles_come_from_what_the_pins_are_called():
    assert intent.grounds(PINS, NETS) == {"GND"}
    # the enable input on the rail and the connector are no supply pins; the op-amp's V- on ground is none either
    assert intent.supply_pins(PINS, NETS) == [("U1", "1", "VIN"), ("U2", "8", "VIN")]
    quiet, noisy = intent.roles(PINS, NETS)
    assert quiet == {"FB", "INP"} and noisy == {"SW", "BOOT"}


def test_a_negative_rail_that_one_part_calls_ground_is_a_rail():
    pins = {("U1", "4"): ("V-", "power_in"), ("U2", "2"): ("GND", "power_in"), ("U3", "2"): ("GND", "power_in")}
    nets = {"-2V": [("U1", "4"), ("U2", "2")], "Net-(U3-GND)": [("U3", "2"), ("R1", "1")]}
    assert intent.grounds(pins, nets) == {"Net-(U3-GND)"}


def test_findings_say_which_pin_lacks_what_and_by_how_much():
    at = {("U1", "1"): (0, 0), ("U1", "4"): (2, 0), ("U1", "6"): (4, 0), ("U2", "8"): (20, 0), ("U2", "3"): (22, 0),
          ("C1", "1"): (1, 1), ("C2", "1"): (3, 0), ("R1", "2"): (2, 14), ("R2", "1"): (3, 1), ("C3", "1"): (4, 9),
          ("R3", "2"): (23, 1)}
    found = intent.findings(PINS, NETS, lambda r, p: at.get((r, p)))
    assert [f[5] for f in found] == ["C2", "R1", "C3"]
    assert [(f[0], f[1], f[2], round(f[4], 1), f[6]) for f in found] == [
        ("supply", "U2", "8", 17.0, "nearest capacitor is C2"),      # both capacitors are at U1
        ("node", "U1", "4", 14.0, "FB reaches to R1"),
        ("boot", "U1", "6", 9.0, "its capacitor is C3")]
    at.update({("C2", "1"): (20, 1), ("R1", "2"): (2, 3), ("C3", "1"): (5, 1)})
    assert intent.findings(PINS, NETS, lambda r, p: at.get((r, p))) == []
    nets = dict(NETS, VIN=[t for t in NETS["VIN"] if t[0] != "C1" and t[0] != "C2"])
    worst = intent.findings(PINS, nets, lambda r, p: at.get((r, p)))[0]
    assert worst[:4] == ("supply", "U1", "1", "VIN") and worst[4] == math.inf


def test_a_capacitor_to_a_node_that_capacitors_hold_is_a_bypass():
    """A driver between two rails with two capacitors in series over the node between them."""
    pins = {("U1", "1"): ("VDD", "power_in"), ("U1", "2"): ("GND", "power_in")}
    nets = {"VD": [("U1", "1"), ("C1", "1")], "MID": [("C1", "2"), ("C2", "1"), ("Q1", "1")],
            "Net-(U1-GND)": [("U1", "2"), ("C2", "2")]}
    at = {("U1", "1"): (0, 0), ("C1", "1"): (1, 0)}
    assert intent.findings(pins, nets, lambda r, p: at.get((r, p))) == []


def test_read_pins_gives_name_and_kind_without_the_number_kicad_appends(tmp_path):
    p = tmp_path / "x.net"
    p.write_text('(export (components) (nets (net (code "1") (name "FB") '
                 '(node (ref "U1") (pin "4") (pinfunction "FB_4") (pintype "input")) '
                 '(node (ref "U1") (pin "A1") (pinfunction "A_1") (pintype "passive")) '
                 '(node (ref "R1") (pin "2") (pintype "passive")))))')
    assert read_pins(str(p)) == {("U1", "4"): ("FB", "input"), ("U1", "A1"): ("A_1", "passive"),
                                 ("R1", "2"): ("", "passive")}


def test_a_net_an_output_drives_is_no_quiet_node():
    """A follower: the amplifier's inverting input is on its own output. That net is low impedance, whatever the
    pin is called, and a resistor from it to a converter's input belongs at the converter."""
    pins = {("U1", "1"): ("", "output"), ("U1", "2"): ("-", "input"), ("U1", "3"): ("+", "input"),
            ("U2", "5"): ("AIN1N", "input"), ("U3", "1"): ("ALERT", "open_collector")}
    nets = {"BUF": [("U1", "1"), ("U1", "2"), ("R1", "1")], "AIN": [("U2", "5"), ("R1", "2"), ("C1", "1")],
            "SRC": [("U1", "3"), ("R2", "1"), ("U3", "1")]}
    assert intent.driven(pins, nets) == {"BUF"}            # an open collector drives nothing
    assert intent.roles(pins, nets)[0] == {"AIN", "SRC"}
    at = {("U1", "1"): (0, 0), ("U1", "2"): (0, 1), ("U1", "3"): (0, 2), ("R1", "1"): (1, 0), ("R1", "2"): (2, 0),
          ("U2", "5"): (30, 0), ("C1", "1"): (30, 1), ("R2", "1"): (1, 2), ("U3", "1"): (2, 2)}
    found = intent.findings(pins, nets, lambda r, p: at.get((r, p)))
    assert [(f[1], f[3], f[5]) for f in found] == [("U2", "AIN", "R1")]   # not U1 pin 2 on BUF, 30 mm from R1

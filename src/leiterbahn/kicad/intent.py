"""What a circuit needs of its layout, read from the netlist.

A netlist says which pads are joined. The symbols say more: which pin is a supply pin, which
is called FB or BOOT or SW. From that follows what a reviewer otherwise finds by hand on the
finished board: a supply pin with no capacitor near it, a feedback node spread over 30 mm, a
boot capacitor far from its pin. None of this is a design rule and DRC passes all of it.

Pure logic, no pcbnew: pins is {(ref, pad): (name, kind)} as netlist.read_pins() gives it,
nets is {net: [(ref, pad)]}, at is a function (ref, pad) -> (x, y) in mm or None.
"""
import math
import re

GROUND = re.compile(r"^(A|D|P|S)?(GND|VSS)(\d|[A-Z])*$", re.I)
# pins that take up what is near them, and pins that swing
QUIET = re.compile(r"^(V?FB\d*|COMP|ADJ|SS|RT(/SYNC)?|SLOPE|I?SNS.*|ISNS.*|CS[PNG]?|CSG|SENSE.*|IN?[+-]|[+-]|V?REF.*"
                   r"|AIN\d*[PN]?)$", re.I)
NOISY = re.compile(r"^(SW\d*|LX\d*|PH(ASE)?\d*|BOOT\d*|BST\d*|CB|[HL]DRV\d*|[HL]G(ATE)?\d*|[HL]O\d*"
                   r"|S?CL?K(IN|OUT)?|SCLK|XTAL\d*|XI|XO|OSC.*)$", re.I)
BOOT = re.compile(r"^(BOOT\d*|BST\d*|CB)$", re.I)


def passive(ref):
    return bool(re.match(r"^(R|C|L|FB|D|TP|RN)\d", ref))


def grounds(pins, nets):
    """The nets that are a ground: those named so, and those whose supply pins are all called GND,
    AGND, VSS ... (A negative rail that one part uses as its ground is a rail.)"""
    found = set()
    for net, pads in nets.items():
        power = [pins[t][0] for t in pads if pins.get(t, ("", ""))[1].startswith("power_in")]
        if GROUND.match(net.split("/")[-1]) or (power and all(GROUND.match(name) for name in power)):
            found.add(net)
    return found


def supply_pins(pins, nets, ground=None):
    """[(ref, pad, net)]: the pins a symbol calls a supply input, on a net that is no ground."""
    ground = grounds(pins, nets) if ground is None else ground
    return sorted((ref, pad, net) for net, pads in nets.items() if net not in ground
                  for ref, pad in pads if pins.get((ref, pad), ("", ""))[1].startswith("power_in") and not passive(ref)
                  and not re.match(r"^(J|P|H|MH|X)\d", ref))


def roles(pins, nets):
    """(quiet, noisy): the nets with a pin whose name says it takes up what is near it (FB, COMP,
    a sense or amplifier input, a reference) and those with a pin that swings (SW, BOOT, a gate
    drive, a clock). Supply rails and grounds are neither. For negotiate(quiet=, noisy=)."""
    ground = grounds(pins, nets)
    rails = {net for _r, _p, net in supply_pins(pins, nets, ground)}
    quiet, noisy = set(), set()
    for net, pads in nets.items():
        if net in ground or net in rails:
            continue
        names = [pins.get(t, ("", ""))[0] for t in pads if not passive(t[0])]
        if any(NOISY.match(n) for n in names):
            noisy.add(net)
        elif any(QUIET.match(n) for n in names):
            quiet.add(net)
    return quiet, noisy


def findings(pins, nets, at, near=4.0, node=10.0, boot=4.0):
    """What the placement does not give the circuit: [(kind, ref, pad, net, mm, part, text)], worst
    first; part is the capacitor or resistor that is too far (None: there is none).

    "supply": a supply pin whose nearest capacitor from its net to a ground is further than
    `near` mm (inf: the net has none). "node": a pin named as a quiet one (see roles()) whose
    net reaches further than `node` mm from it. "boot": a boot pin whose capacitor is further
    than `boot` mm. Distances are between pad centres.
    """
    ground = grounds(pins, nets)
    on = {}
    for net, pads in nets.items():
        for t in pads:
            on[t] = net

    def far(a, b):
        p, q = at(*a), at(*b)
        return math.inf if p is None or q is None else math.hypot(p[0] - q[0], p[1] - q[1])

    def others(ref, pad):
        return [net for (r, p), net in on.items() if r == ref and p != pad]

    found = []
    supplies = supply_pins(pins, nets, ground)
    rails = {n for _r, _p, n in supplies}
    held = {n for n, pads in nets.items() if sum(1 for r, _p in pads if r[0] == "C" and r[1:2].isdigit()) >= 2}
    for ref, pad, net in supplies:
        # a capacitor from the pin's net to a ground, to another supply, or to a net that capacitors hold
        # (a driver between two rails had its two capacitors in series over the node between them)
        back = ground | rails | held
        caps = [(r, p) for r, p in nets[net]
                if r[0] == "C" and r[1:2].isdigit() and any(o in back and o != net for o in others(r, p))]
        best = min(((far((ref, pad), c), c[0]) for c in caps), default=(math.inf, None))
        if best[0] > near:
            what = "no capacitor to ground on its net" if best[1] is None else "nearest capacitor is %s" % best[1]
            found.append(("supply", ref, pad, net, best[0], best[1], what))
    for (ref, pad), (name, _kind) in sorted(pins.items()):
        net = on.get((ref, pad))
        if net is None or passive(ref) or net in ground:
            continue
        if BOOT.match(name):
            caps = [(r, p) for r, p in nets[net] if r[0] == "C" and r[1:2].isdigit()]
            best = min(((far((ref, pad), c), c[0]) for c in caps), default=(math.inf, None))
            if best[0] > boot:
                found.append(("boot", ref, pad, net, best[0], best[1],
                              "no capacitor on its net" if best[1] is None else "its capacitor is %s" % best[1]))
        elif QUIET.match(name) and len(nets[net]) <= 8:
            reach = max(((far((ref, pad), t), t[0]) for t in nets[net]
                         if t != (ref, pad) and not t[0].startswith("TP")), default=(0.0, None))
            if node < reach[0] < math.inf:
                found.append(("node", ref, pad, net, reach[0], reach[1], "%s reaches to %s" % (name, reach[1])))
    return sorted(found, key=lambda f: (-f[4], f[:4]))

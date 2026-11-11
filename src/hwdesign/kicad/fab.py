"""Fabrication outputs for JLCPCB: Gerbers + drill zip, assembly BOM and CPL.

Parts are included in assembly when their schematic field Assembly == "SMT" and
they have an LCSC field. Everything else (hand-soldered THT, jumpers, holes) is
left out of BOM/CPL.

The Gerber set has one file per copper layer the board file enables, so a four-
or six-layer board is exported with its inner layers.
"""
import csv
import glob
import os
import re
import subprocess
import zipfile

from .netlist import read_netlist
from .sexp import parse

OTHER_LAYERS = "F.Paste,B.Paste,F.SilkS,B.SilkS,F.Mask,B.Mask,Edge.Cuts"
LAYERS = "F.Cu,B.Cu," + OTHER_LAYERS          # a two-layer board


def copper_layers(pcb):
    """Copper layer names a .kicad_pcb enables, front to back: ["F.Cu", "In1.Cu", ..., "B.Cu"].

    Read from the file's (layers ...) table, without pcbnew.
    """
    with open(pcb) as f:
        text = f.read()
    m = re.search(r"^\s*\(layers\b", text, re.M)        # the board's table, before any pad's (layers ...)
    if m is None:
        raise ValueError("no (layers ...) table in %s" % pcb)
    start = text.index("(", m.start())
    depth, end = 0, start
    for end in range(start, len(text)):         # the table ends at its matching parenthesis
        depth += {"(": 1, ")": -1}.get(text[end], 0)
        if depth == 0:
            break
    names = [str(row[1]) for row in parse(text[start:end + 1])[1:] if isinstance(row, list) and len(row) > 1]
    cu = [n for n in names if n.endswith(".Cu")]
    order = {"F.Cu": -1, "B.Cu": 10 ** 6}
    return sorted(cu, key=lambda n: order.get(n, int("0" + "".join(ch for ch in n if ch.isdigit()))))


def _run(*args):
    r = subprocess.run(list(args), capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(" ".join(args) + "\n" + r.stdout + r.stderr)
    return r.stdout


def export(pcb, netlist, outdir, name, rot_offset=None):
    """Write the Gerber and drill zip, the JLC BOM and CPL into outdir.

    Returns dict(zip, bom, cpl, parts, lines, missing, copper): `copper` lists the copper
    layers that were exported, `missing` the assembled parts absent from the position file.
    """
    rot_offset = rot_offset or {}
    copper = copper_layers(pcb)
    gdir = os.path.join(outdir, "gerbers")
    os.makedirs(gdir, exist_ok=True)
    for f in glob.glob(os.path.join(gdir, "*")):
        os.remove(f)
    _run("kicad-cli", "pcb", "export", "gerbers", "--layers", ",".join(copper) + "," + OTHER_LAYERS, "--no-x2",
         "--use-drill-file-origin", "-o", gdir + "/", pcb)
    _run("kicad-cli", "pcb", "export", "drill", "--format", "excellon", "--drill-origin", "absolute",
         "--excellon-units", "mm", "-o", gdir + "/", pcb)
    zpath = os.path.join(outdir, name + "-gerbers.zip")
    with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
        for f in sorted(glob.glob(os.path.join(gdir, "*"))):
            z.write(f, os.path.basename(f))

    comps, _ = read_netlist(netlist)
    smt = {r: c for r, c in comps.items()
           if c["fields"].get("Assembly") == "SMT" and c["fields"].get("LCSC")}

    groups = {}
    for ref, c in smt.items():
        key = (c["value"], c["footprint"].split(":")[-1], c["fields"]["LCSC"])
        groups.setdefault(key, []).append(ref)
    bom = os.path.join(outdir, name + "-bom-jlc.csv")
    with open(bom, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["Comment", "Designator", "Footprint", "LCSC Part #"])
        for (val, fp, lcsc), refs in sorted(groups.items(), key=lambda kv: kv[1][0]):
            refs.sort(key=lambda r: (r.rstrip("0123456789"), int("0" + r[len(r.rstrip("0123456789")):])))
            w.writerow([val, ",".join(refs), fp, lcsc])

    pos = os.path.join(outdir, "pos-kicad.csv")
    _run("kicad-cli", "pcb", "export", "pos", "--format", "csv", "--units", "mm", "--side", "both",
         "--use-drill-file-origin", "-o", pos, pcb)
    cpl = os.path.join(outdir, name + "-cpl-jlc.csv")
    placed = set()
    with open(pos) as fi, open(cpl, "w", newline="") as fo:
        w = csv.writer(fo)
        w.writerow(["Designator", "Mid X", "Mid Y", "Layer", "Rotation"])
        for row in csv.DictReader(fi):
            ref = row["Ref"]
            if ref not in smt:
                continue
            fp = smt[ref]["footprint"].split(":")[-1]
            rot = (float(row["Rot"]) + rot_offset.get(fp, 0)) % 360
            w.writerow([ref, row["PosX"] + "mm", row["PosY"] + "mm",
                        "Top" if row["Side"].lower().startswith("top") else "Bottom", "%g" % rot])
            placed.add(ref)
    missing = sorted(set(smt) - placed)
    return dict(zip=zpath, bom=bom, cpl=cpl, parts=len(smt), lines=len(groups), missing=missing, copper=copper)

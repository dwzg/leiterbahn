"""ERC plus netlist proof: KiCad's connectivity must equal the nets the generator declared."""
import json
import os
import subprocess

from .schematic import compare_netlist


def erc_and_parity(sch, declared_json, build_dir, netlist_name=None):
    """Run ERC on sch, export KiCad's netlist into build_dir and compare it with the nets the
    schematic generator declared (Schematic.netlist() saved as JSON).
    Returns (erc_summary_line, netlist_path, diffs); diffs is empty when they match.

    For a multi-sheet design sch is the root sheet: ERC and the netlist cover every sheet."""
    os.makedirs(build_dir, exist_ok=True)
    base = os.path.splitext(os.path.basename(sch))[0]
    net = os.path.join(build_dir, netlist_name or base + ".net")
    erc = subprocess.run(["kicad-cli", "sch", "erc", "--severity-all", "-o", os.path.join(build_dir, "erc.rpt"), sch],
                         capture_output=True, text=True).stdout
    summary = next((ln for ln in erc.splitlines() if "Found" in ln), erc.strip())
    subprocess.run(["kicad-cli", "sch", "export", "netlist", "--format", "kicadsexpr", "-o", net, sch],
                   capture_output=True, check=True)
    with open(declared_json) as f:
        diffs = compare_netlist(json.load(f), net)
    return summary, net, diffs

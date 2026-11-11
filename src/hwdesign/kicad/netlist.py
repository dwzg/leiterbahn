"""Read KiCad netlists without pcbnew (usable for checks on machines without KiCad's Python)."""
from .sexp import find, find1, parse


def read_netlist(path):
    """Parse a KiCad netlist (kicad-cli sch export netlist --format kicadsexpr).

    Returns (comps, nets): comps[ref] = dict(value, footprint, uuid, path, sheet, fields,
    exclude_from_bom, dnp); nets[name] = [(ref, pin), ...].
    path is the symbol's instance path, "/<uuid>" on the root sheet and "/<sheet uuid>/<uuid>"
    on a sub-sheet (what a footprint needs to stay linked to its symbol); sheet is the sheet's
    name path ("/" for the root).
    """
    with open(path) as f:
        t = parse(f.read())
    comps = {}
    for c in find(find1(t, "components"), "comp"):
        ref = str(find1(c, "ref")[1])
        fields = {}
        fl = find1(c, "fields")
        if fl is not None:
            for fld in find(fl, "field"):
                name = str(find1(fld, "name")[1])
                fields[name] = str(fld[-1]) if not isinstance(fld[-1], list) else ""
        ts = find1(c, "tstamps")
        uuid = str(ts[1]) if ts is not None else ""
        sp = find1(c, "sheetpath", [])
        sheet_ts = str(find1(sp, "tstamps", [None, "/"])[1])
        props = {str(find1(p, "name")[1]) for p in find(c, "property")}
        comps[ref] = dict(value=str(find1(c, "value")[1]),
                          footprint=str(find1(c, "footprint")[1]) if find1(c, "footprint") is not None else "",
                          uuid=uuid, path=sheet_ts.rstrip("/") + "/" + uuid if uuid else "",
                          sheet=str(find1(sp, "names", [None, "/"])[1]),
                          fields=fields, exclude_from_bom="exclude_from_bom" in props,
                          dnp="dnp" in props)
    nets = {}
    for n in find(find1(t, "nets"), "net"):
        name = str(find1(n, "name")[1])
        nets[name] = [(str(find1(x, "ref")[1]), str(find1(x, "pin")[1])) for x in find(n, "node")]
    return comps, nets

"""Export what an enclosure needs to know about a KiCad board, as JSON.

Runs with KiCad's Python (pcbnew). The enclosure generator (build123d, separate
venv) reads the JSON, so board and housing stay in sync.

Coordinates: board origin at the outline's top-left, x right, y down, mm.
"""
import json

import pcbnew


def _mm(v):
    return round(pcbnew.ToMM(v), 3)


def export(pcb_path, json_path, keep_free_radius=2.0, extra=None):
    b = pcbnew.LoadBoard(pcb_path)
    edge = b.GetBoardEdgesBoundingBox()
    ox, oy = edge.GetX(), edge.GetY()

    def pt(v):
        return round(pcbnew.ToMM(v.x - ox), 3), round(pcbnew.ToMM(v.y - oy), 3)

    out = {"board": {"w": _mm(edge.GetWidth()), "h": _mm(edge.GetHeight()),
                     "thickness": _mm(b.GetDesignSettings().GetBoardThickness())},
           "holes": [], "parts": [], "keepouts": []}
    for fp in b.GetFootprints():
        ref = fp.GetReference()
        cy = fp.GetCourtyard(pcbnew.F_CrtYd)
        bb = cy.BBox() if cy.OutlineCount() else fp.GetBoundingBox(False)
        x0, y0 = pt(bb.GetOrigin())
        pads = []
        for p in fp.Pads():
            px, py = pt(p.GetPosition())
            pads.append({"num": p.GetNumber(), "x": px, "y": py,
                         "drill": _mm(p.GetDrillSize().x), "net": p.GetNetname()})
            if not p.GetNumber() and p.GetDrillSize().x > 0 and p.GetAttribute() in (
                    pcbnew.PAD_ATTRIB_NPTH, pcbnew.PAD_ATTRIB_PTH) and ref.startswith("H"):
                out["holes"].append({"ref": ref, "x": px, "y": py, "d": _mm(p.GetDrillSize().x)})
        out["parts"].append({"ref": ref, "value": fp.GetValue(), "footprint": fp.GetFPIDAsString(),
                             "x0": x0, "y0": y0, "w": _mm(bb.GetWidth()), "h": _mm(bb.GetHeight()),
                             "tht": any(p.GetDrillSize().x > 0 for p in fp.Pads()), "pads": pads})
    for z in b.Zones():
        if z.GetIsRuleArea():
            zb = z.GetBoundingBox()
            x0, y0 = pt(zb.GetOrigin())
            out["keepouts"].append({"name": z.GetZoneName(), "x0": x0, "y0": y0,
                                    "w": _mm(zb.GetWidth()), "h": _mm(zb.GetHeight())})
    if extra:
        out.update(extra)
    with open(json_path, "w") as f:
        json.dump(out, f, indent=1)
    return out

"""Which copper layers a board file enables, read without pcbnew."""
import pytest

from leiterbahn.kicad.fab import copper_layers

HEAD = """(kicad_pcb
\t(version 20260206)
\t(generator "pcbnew")
\t(general
\t\t(thickness 1.6)
\t)
\t(paper "A4")
\t(layers
%s
\t\t(9 "F.Adhes" user "F.Adhesive")
\t\t(25 "Edge.Cuts" user)
\t)
\t(footprint "R_0805"
\t\t(pad "1" smd roundrect (at 0 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste"))
\t)
)
"""


def board(tmp_path, rows):
    p = tmp_path / "b.kicad_pcb"
    p.write_text(HEAD % "\n".join("\t\t" + r for r in rows))
    return str(p)


def test_two_layer_board(tmp_path):
    assert copper_layers(board(tmp_path, ['(0 "F.Cu" signal)', '(2 "B.Cu" signal)'])) == ["F.Cu", "B.Cu"]


def test_inner_layers_come_between_front_and_back_in_stack_order(tmp_path):
    rows = ['(0 "F.Cu" signal)', '(2 "B.Cu" signal)', '(6 "In2.Cu" signal "Sig")', '(4 "In1.Cu" power "GND plane")',
            '(8 "In3.Cu" signal)', '(10 "In4.Cu" power)']
    assert copper_layers(board(tmp_path, rows)) == ["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"]


def test_file_without_layer_table(tmp_path):
    p = tmp_path / "x.kicad_pcb"
    p.write_text("(kicad_pcb (version 1))")
    with pytest.raises(ValueError):
        copper_layers(str(p))

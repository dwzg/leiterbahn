"""Shared fixtures: a small symbol library written from text, so the schematic generator
can be tested on machines without KiCad's libraries."""
import pytest

_PROP = '(property "%s" "%s" (at 0 %s 0) %s(effects (font (size 1.27 1.27))))'
_PIN = ('(pin %s line (at %s %s %s) (length 1.27) (name "%s" (effects (font (size 1.27 1.27)))) '
        '(number "%s" (effects (font (size 1.27 1.27)))))')


def _props(ref, value, hide_ref=False, value_y=-5.08):
    return "\n".join([_PROP % ("Reference", ref, 5.08, "(hide yes) " if hide_ref else ""),
                      _PROP % ("Value", value, value_y, ""),
                      _PROP % ("Footprint", "", 0, "(hide yes) "), _PROP % ("Datasheet", "", 0, "(hide yes) "),
                      _PROP % ("Description", "test symbol", 0, "(hide yes) ")])


def two_pin(name="R2P"):
    """A two-pin passive part: pin 1 up, pin 2 down."""
    return ('(symbol "%s" (pin_numbers (hide yes)) (pin_names (offset 0)) (exclude_from_sim no) (in_bom yes) '
            '(on_board yes)\n%s\n(symbol "%s_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) '
            '(stroke (width 0.254) (type default)) (fill (type none))))\n(symbol "%s_1_1" %s %s)\n'
            '(embedded_fonts no))'
            % (name, _props("R", name), name, name, _PIN % ("passive", 0, 3.81, 270, "~", 1),
               _PIN % ("passive", 0, -3.81, 90, "~", 2)))


def dual():
    """A part with two units: unit 1 has pins 1 and 2, unit 2 has pins 3 and 4."""
    units = "\n".join('(symbol "DUAL_%d_1" (rectangle (start -2.54 -2.54) (end 2.54 2.54) (stroke (width 0.254) '
                      '(type default)) (fill (type none))) %s %s)'
                      % (u, _PIN % ("passive", -5.08, 0, 0, "A", 2 * u - 1),
                         _PIN % ("passive", 5.08, 0, 180, "B", 2 * u))
                      for u in (1, 2))
    return ('(symbol "DUAL" (exclude_from_sim no) (in_bom yes) (on_board yes)\n%s\n%s\n(embedded_fonts no))'
            % (_props("U", "DUAL"), units))


def comb(name="COMB"):
    """An IC with pins 1-4 on its left side 2.54 mm apart, pin 5 stacked on pin 4, pin 6 a further
    7.62 mm down, pin 8 5.08 mm below that, and pin 7 on the right."""
    ys = {1: 5.08, 2: 2.54, 3: 0, 4: -2.54, 5: -2.54, 6: -10.16, 8: -15.24}
    pins = " ".join(_PIN % ("passive", -7.62, y, 0, "L%d" % n, n) for n, y in ys.items())
    return ('(symbol "%s" (exclude_from_sim no) (in_bom yes) (on_board yes)\n%s\n(symbol "%s_1_1" (rectangle '
            '(start -6.35 7.62) (end 6.35 -17.78) (stroke (width 0.254) (type default)) (fill (type none))) %s %s)\n'
            '(embedded_fonts no))'
            % (name, _props("U", name, value_y=-20.32), name, pins, _PIN % ("passive", 7.62, 0, 180, "R", 7)))


def power(name, pin_type="power_in"):
    value_y = -3.81 if name.startswith("GND") else 3.81          # ground names below, supplies above
    return ('(symbol "%s" (power global) (pin_numbers (hide yes)) (pin_names (offset 0) (hide yes)) '
            '(exclude_from_sim no) (in_bom yes) (on_board yes)\n%s\n(symbol "%s_0_1" (polyline (pts (xy 0 0) '
            '(xy 0 -1.27)) (stroke (width 0) (type default)) (fill (type none))))\n(symbol "%s_1_1" %s)\n'
            '(embedded_fonts no))'
            % (name, _props("#PWR", name, hide_ref=True, value_y=value_y), name, name,
               _PIN % (pin_type, 0, 0, 270, "~", 1)))


def write_lib(directory, lib, symbols):
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / (lib + ".kicad_sym")
    path.write_text('(kicad_symbol_lib (version 20251024) (generator "leiterbahn_tests") (generator_version "10.0")\n'
                    + "\n".join(symbols) + "\n)\n")
    return path


@pytest.fixture
def symdir(tmp_path):
    """A stand-in for KiCad's symbol directory: libraries `tlib` (R2P, DUAL, COMB) and `power`."""
    d = tmp_path / "symbols"
    write_lib(d, "tlib", [two_pin(), dual(), comb()])
    write_lib(d, "power", [power("GND"), power("+3V3"), power("PWR_FLAG", "power_out")])
    return d

# JLC order options

Choices for a small two-layer board with top-side assembly, and why. Check prices and limits
on the order page on the day; these were right at the time of a real order.

No four-layer board has been ordered with these files yet. The export writes the inner
layers (see SKILL.md), but the four-layer choices on the order page (stackup, inner copper,
the layer order JLC shows in its Gerber viewer) are not covered below: check them on the
page, and add what you learn here.

## PCB

| Option | Choose | Why |
|---|---|---|
| Base material | FR-4 | |
| Layers | 2 | The board's copper layer count: `copper` in the return value of `fab.export` |
| Thickness | 1.6 mm | Enclosures are usually designed around it; change both together |
| Colour | Green | Cheapest and fastest |
| Surface finish | HASL lead-free | Fine for SMT and THT. ENIG costs a little more and gives flat, durable bare pads: worth it when pogo pins or test probes will contact pads often |
| Outer copper | 1 oz | Enough unless traces carry several amps |
| Via covering | Tented | Default |
| Board outline tolerance | Regular | |
| Mark on PCB | Remove Mark (small fee) | Otherwise JLC prints an order number wherever it finds room |
| Electrical test | Flying probe (free) | |
| Confirm production file | Yes | Shows the panel and any rails JLC adds |
| Gold fingers, castellations, edge plating, impedance control | No | Only if the design needs them |

## PCBA

| Option | Choose | Why |
|---|---|---|
| PCBA type | Economic | Standard is for double-sided assembly or special parts |
| Assembly side | Top | Keep the bottom free of parts if bare pads (pogo, test) live there |
| PCBA quantity | number needed + spares | Spares cost little next to a second order's shipping |
| Tooling holes | Added by JLCPCB | Ask in the remark for them on rails, not inside the board |
| Confirm parts placement | Yes | Your chance to catch rotations before they build |
| Parts selection | By customer | The BOM carries LCSC numbers |
| Bake components | No | JLC stores its stock correctly |
| Board cleaning | No | No-clean residue is harmless in a sealed housing |
| Depanel boards and edge rails | Yes, if rails are added | Snapped rails leave nubs that catch on enclosure end stops |
| Flying probe / function test | No | The bare board is already tested |
| Conformal coating | No, unless masked | It also covers pogo and test pads and solder jumpers |
| Solder paste | High temperature (SAC305) | Low-temperature paste is wrong for anything that gets warm, such as a car |
| Nitrogen reflow | Default | |
| Stencil (separate order) | Off | PCBA includes its own |

## Assembly remark

State anything deliberate that looks like a mistake. For a module whose antenna overhangs the
edge:

> U2 (ESP32-C3-WROOM-02) overhangs the right board edge by 6.2 mm on purpose (antenna outside
> the board). If tooling holes are needed, please add them on edge rails, not inside the board
> outline. Bottom side: no parts, pads intentionally bare.

## Price

On one order of five assembled 46 × 40 mm boards (26 BOM lines), the total was about €87
before shipping:

| Item | Amount |
|---|---|
| Components | about €40 |
| Extended-part fee | about €30 |
| Setup, stencil, assembly and confirmations | about €12 |
| Bare boards | €5.50 |

The extended-part fee is about €3 per *distinct* part that is not in JLC's basic library.
It's the only real lever: swapping passives to basic-library equivalents saves €3 each, but
every swap needs its voltage rating and capacitance-under-bias checked again. Modules,
regulators and relays usually have no basic alternative.

Look for coupons in the account before paying; one can cover much of the setup fee.

# Changelog

## 0.36.0

Changed
- A net's weight counts in the rounds as well: it pays that much less for a cell another
  net lies on, so it keeps to its short way and the others give way. (As routers for
  programmable logic weigh a net's delay against congestion by how critical it is.) The
  weight counted only in the passes that shorten, after the rounds had settled who goes
  round whom. What a cell that was fought over costs stays the same for every net: with
  that divided too, one of 24 placements of a board kept a connection open. Without
  weights nothing changes: nine demo boards and a board of 71 nets came out the same file.
  On a board of 159 nets with 13 of them weighted 3, 24 placements: all clean, those 13
  nets 2.7 % shorter than unweighted (in 18 placements, longer in 6) and 1.0 % shorter than
  with the weight in the passes only, all tracks 0.1 % shorter, but 56 % more narrow track
  over all placements. So it is worth a look per board: on the placement in use the 13
  nets went from 158 to 145 mm with the narrow track unchanged.

## 0.35.2

Fixed
- `leiterbahn-route board`: an outer layer of type `power` on which nothing is poured is
  routed on (and said so); a teardrop is no pour. With no tracks allowed there a board of
  two layers was one of a single layer: a demo board of through-hole parts, its top typed
  power and used for 37 bridges, kept 13 of 87 connections open and now has none, in 2 s
  instead of 13. Inner layers of type power stay planes, poured or not.
- `leiterbahn-route board`: a text on copper is taken as wide as KiCad sets it, by the
  widths of its font's letters measured there (1.05 of the width given, a few letters up to
  1.35; a line 1.35 of the height, each further line 1.62; the pen's width all round). It
  was 1.3 for every letter, and a line end written into the text counted as one: a label of
  seven letters, 6.35 mm wide, was taken for 11 mm and covered a pad 1.5 mm beside it,
  which was then reported as having no way out.

Ten demo boards, KiCad's DRC: all ten with nothing unconnected (8 before), no error added by
the routing but starved thermals and islands of pours.

## 0.35.1

Fixed
- `leiterbahn-route board`: two pads of a net touch where their copper does, not where the
  boxes round them do. Pads of one net side by side on a part at 45 degrees, 0.25 mm apart,
  were taken for one piece, and the connection between them was never made nor reported
  open: a board with 39 of its 94 parts at 45 degrees was "routed, nothing open" with 5
  connections missing in KiCad's DRC. Now 226 connections are made there instead of 197,
  and DRC finds none missing. The same for a track that ends beside a turned pad, and for
  a turned pad on a pour.
- `leiterbahn-route board`: a pad drawn as a polygon along its own axes (the L of a corner
  pin) is read as the boxes it is made of, not as the box round them all. A track of its net
  ended in the empty corner of the L and counted as joined: 3 connections missing on a
  board with 12 such pads, none now.

Ten demo boards, KiCad's DRC: 8 with nothing unconnected (6 before), no error added by the
routing but starved thermals and islands of pours.

## 0.35.0

Changed
- In the passes that shorten, a net that is pushed off its way and finds none free may push
  in turn, and those it crosses again, up to a dozen nets; the nets moved so far are closed
  to it, or it would go back across the one that pushed. It was given up at the first net
  without a free way. A current sense net of 7 mm alone on the board lay 67 mm long round
  half of it: its short way shut two nets in, one of which had a way only across seven
  others, one of those shut in again. Fifteen nets moved, and the net is 7.6 mm. Only where
  a push fails and the pusher has won 2 mm or more: letting every pushed net push had taken
  four times as long and saved no more. On a board of 159 nets, the same 24 placements
  before and after: all clean, the 13 nets that should be short 4.1 % shorter (in 12, longer
  in 2; the worst placement 259 -> 193 mm), all tracks 0.5 % shorter, 30 % less narrow
  track, routing 2 % longer. Ten demo boards: the same connections open, no error added.

## 0.34.0

Changed
- Tracks keep from fixed copper only what a straight step between two cells can come nearer
  than its ends: root(r squared + s squared / 4) less r, and a fiftieth of a cell (2 um on a
  0.05 mm grid; 0.31.0 kept 8 um). A slanted step needs nothing more: the search makes one
  only where the two cells beside it are free as well, and those four cells leave no corner
  nearer than r to the step. At a connector of 0.4 mm pitch with 0.2 mm between pads and
  0.2 mm clearance a track of 0.147 mm has 26 um to either side of a pad's middle line, and
  with 8 um gone a quarter of the pads had no cell to start from. Demo boards, checked with
  KiCad's DRC, no error added: one of fine pitch with 0 connections open of 208 (16 before),
  one of 1458 connections with 0 open in 3 minutes (34 open after 17 minutes).
- A net whose way at full width still crosses another when the rounds are about to stall
  tries a narrow track there. A narrow track was only laid where the full width found no
  way at all, and across the other net there always is one: two supply pins side by side at
  0.5 mm pitch, each with a 0.4 mm track to its capacitor, 0.1 mm between the tracks where
  0.15 mm is asked, held a board of 479 connections for 25 minutes; now 9. (Tried sooner,
  from the second or the sixth round without progress, it left twice as much narrow track
  on a board of 159 nets.)
- A piece that has no way to the rest of its net even with the net alone on the board is
  found before the rounds and left out of them, and reported open as before. Left in, it
  was laid across another net round after round.
- The orders of a knot's nets are no longer tried when one of them has more than 24 pieces:
  every order lays its nets whole, and a ground net of some hundred pads took minutes each.

All of it on a board of 159 nets, the same 24 placements routed before and after: all clean
both times, 13 % less narrow track (less in 19 of 24), tracks 0.2 % longer, 23 % more rounds.

## 0.33.0

Changed
- Nets that stay in each other's way are helped out of it. When the conflicts have not got
  fewer for four rounds, the nets in conflict give up all their tracks within 2 mm of where
  they are stuck, then within 4 mm, and so on, and after twelve rounds every net with a
  track there does; and from the second such round on, a net whose way still crosses
  another's is shown the whole board once, not only the ground about its own pads. Until
  now a net was laid again only where another lay on it and searched only near itself, as
  long as that gave any way at all. A board of 1458 connections sat on two crossings of
  five cells from round 25 on, 17 s a round, and did not end in 25 minutes; now the
  crossings are gone by round 47, 17 minutes. A board of 159 nets, the same 24 placements:
  30 % fewer rounds (fewer in 14, more in 3), tracks as long, all clean as before.

Fixed
- `Board.straighten()` no longer merges two pieces into one that leaves a joint of its net
  at a slant (a branch one cell beside its trunk, joined by one slanted step, would become
  a piece 8 degrees off the trunk), and `Board.improve()` no longer takes a new way that
  does: on its own grid a run from a via to a joint came back as such a piece, shorter and
  so better, and the board had an acute joint.

## 0.32.1

Fixed
- An opening drawn in the solder mask over a pad is that pad's and no longer in the way (0.32.0
  shut the two pads of a solder jumper under one opening off from their nets). Only an opening
  over no pad is kept clear of tracks.

Changed
- After the rounds have stalled, a net of more than 24 pieces is still laid again only where
  another lies on it; smaller nets are laid again whole, as before. A ground net of some
  hundred pads, laid whole in every round, took a board of 1458 connections past 25 minutes
  over two crossings of five cells each.

Added
- `leiterbahn-route board --planes found`: every inner layer that pours fill by more than
  half is taken for a plane, like a layer of type `power`. A board of four layers typed as
  signal with ground and supply poured inside: 34 s instead of 454 s, one connection open
  either way. Not the default: no share of fill tells a plane from a signal layer flooded
  with ground (inner layers their designers left without tracks: filled by 68 to 73 %; with
  up to 539 tracks: 58 to 74 %), and a board that needs its inner layers for tracks does
  not route without them.

## 0.32.0

`leiterbahn-route board`, further on the demo boards:

Fixed
- The outline as it is drawn, and every cut-out in it, is a line that copper keeps the edge
  clearance from. Only the box round the outline was known, and copper lay across rounded
  corners, notches and slots (46 errors on four boards; none now). Arcs are walked in steps
  of 15 degrees, not as two chords.
- A clearance given for a pad or its footprint is kept (a mounting hole that asks for
  1.7 mm round it had tracks at 0.9 mm).
- A drawing on a solder mask layer is in the way on the copper under it: a track there lay
  bare in an opening meant for something else (KiCad: solder mask bridge, 20 on one board).

## 0.31.0

Changed
- Tracks keep from fixed copper what the geometry asks for beyond the clearance, not half a
  cell. A track runs from cell centre to cell centre, and between two centres it can be
  nearer to a corner beside it than at either: by root(r squared + s squared / 2) - r for a
  distance r to keep and a cell of s, 3 um at r = 0.19 mm on a 0.05 mm grid. That and a
  tenth of a cell is kept now (8 um on that grid; it was 25 um). At a connector of 0.4 mm
  pitch, 0.2 mm between pads and 0.2 mm clearance, half a cell left a pad no cell to start
  from unless one lay within 11 um of its middle line. Demo boards, checked with KiCad's
  DRC: one of 164 connections all routed in 19 s (4 open after 253 s before), one of fine
  pitch with 17 open of 208 (39). A board of 159 nets on a 0.05 mm grid, the same 24
  placements routed before and after: tracks 0.9 % shorter (in 18 of 24), 1 % fewer vias,
  all clean both times; 61 % more routing rounds.
- The same allowance, without the tenth, between laid copper: on a 0.1 mm grid a track
  passed a via 2 um nearer than a clearance of 0.254 mm allows (four such errors on one
  board; none on a 0.05 mm grid, where it is under 1 um).

## 0.30.0

Fixed
- A pad of a part that lies at an angle is that pad turned, not the box round it
  (`Shape::angle` in a problem; `leiterbahn-route board` reads it from the board). At 45
  degrees the box round a pad of 0.25 x 0.8 mm is 0.74 mm square: the pins of such a part
  shut each other in. A board of 94 parts, 39 of them at 45 degrees: 5 connections left
  open of 128, before this 52, in 8 s on a 0.05 mm grid.

## 0.29.0

`leiterbahn-route board` (a KiCad board routed without KiCad) on boards it had never seen:
eleven of KiCad's demo boards, their tracks taken out, routed again and checked with KiCad's
DRC. It had shorts, tracks against the board's own rules and many pads left open. What that
showed, and what is done about it:

Fixed
- A pad whose copper lies off its hole (`(drill 0.75 (offset 0 0.4))`) was taken to lie on the
  hole: tracks were laid across the real pad (two shorts and eight clearance errors on a
  board of 68 parts).
- Text and drawings on copper layers were not seen, and tracks ran through them.
- The board's constraints (least track width, clearance, via diameter, annular ring, drill)
  now go before the net class: a project with a Default class below its own constraints was
  routed with 199 tracks and 32 vias that broke them. Net classes given per net
  (`netclass_assignments`, KiCad 6 and 7) are read.

Changed
- A pad on a pour of its net is joined by tracks like any other, unless the pour is on a
  plane layer. A filled pour on a layer with tracks had counted as a connection; the new
  tracks then cut it apart or shut pads off from it. Six boards routed both ways: 54
  connections left open with the pours trusted, 16 without. `--pours trust` is the old way.
  Tracks keep `--room` (0.8 mm) from pads on a pour where they can, for the spokes.

Added
- Vias into planes. A net with a pour on a plane layer: every piece of it that is not on the
  plane gets a way of its own that ends in a via inside the plane, wherever that is nearest
  and free (`Net::plane` in a problem; the board reader makes it). Until now such a pad was
  joined by tracks to the nearest drilled pad of its net, if there was one. A board of four
  layers with 210 parts: 110 more vias, 5 % less track, routed in half the time; as many
  connections left open as before (4 of 316 on a 0.05 mm grid).

Where the eleven stand now (0.1 mm grid; open connections of those to make): 0 of 14, 0 of
86, 0 of 173, 1 of 478, 4 of 164, 8 of 87, 8 of 316 (4 at 0.05 mm), 78 of 499, and three
boards of fine pitch with a third to a quarter open at either grid. No shorts. Left: copper
too near a board edge that is not a rectangle, starved thermals, a few clearance errors on
one board, and the three dense boards.

## 0.28.0

Added
- `Placer.improve(chains=16)`: that many placements are annealed side by side, each on a
  processor of its own, the first as warm as the schedule says and the others warmer by
  even steps up to 8 times, and neighbours change places every so often, readily when the
  warmer one is the cheaper (replica exchange). One placement's cost falls by about 3 % for
  every fourfold in moves and has not settled after a million (two minutes); 16 chains of
  80000 moves reach that cost in 14 s on 16 cores. A board of 287 parts: 3500 mm instead of
  3810 with one chain of 20000 moves. Routed, 24 placements each way and that twice: tracks
  2 % shorter, a tenth less left unmet, all 24 clean where one was not; quiet track beside
  noisy copper 0.6 mm per board instead of 0.4 mm. The same seed gives the same placement,
  and `chains=1` is the annealing as it was.
- `Placer.keep_track(points, width, sides)`: room for a track drawn by hand later, a slanted
  piece covered by small rectangles along it.

## 0.27.0

Changed
- A track is narrow only through each tight place on its way. When a net's width had no way
  out of a pin, the track was laid narrow and then searched again at full width between its
  first and last cell with room; where a second tight place lay further on, that search
  found nothing and the track stayed narrow all along (a gate drive: 8 mm at 0.127 mm for
  two places of 0.5 mm). Now it then keeps its way and takes the full width wherever that
  fits for 0.8 mm or more.
- What is narrow counts by as much as it is narrower (a quarter of the width, four times
  the length; it was half as much again for all), so the passes that shorten work harder
  to get a supply net out of a narrow way than a signal. Both together, 22 placements of a
  board of 159 nets: 2.6 mm of narrow track per board instead of 4.3 mm, the worst 6.3 mm
  instead of 14.5 mm; lengths and vias as before.
- `kinks()`: two tracks that leave a via or a pad of their net in a V are a branch, not a
  turn back (5 of 18 on one board).

## 0.26.1

Fixed
- `Board.reopen()` reads the net classes back from the project file beside the board. They
  were the rework script's to repeat; one that had not heard of a class added later took
  its nets for ordinary ones, and `improve()` replaced 17 mm of gate drive at 0.25 mm by
  10 mm at 0.127 mm (shorter, so better).

Changed
- Constructive placement is faster: the keep-outs are sorted by side once, and a part that
  takes the same room turned by 180 degrees is tested once per place. A board of 287
  parts: placement and a look at the routing in 19.5 s instead of 26.6 s, the same result.

## 0.26.0

Added
- `negotiate(race=8)`: the router runs that many times side by side, each with the nets in
  a slightly other order (`seed` in the problem), and the run that comes clean in the
  fewest rounds wins; a run that is past the rounds of a clean one is stopped. Where the
  rounds are a close thing any other order ends them elsewhere: eight placements of a board
  of 159 nets took 11 to 172 rounds, and 11 to 200 with a single tie broken otherwise.
  Four of them that took 102, 68, 40 and 47 rounds alone took 18, 18, 15 and 23 as a race
  of eight, 18 to 27 s. The same race gives the same board every time. A processor and
  about 0.6 GB per run.
- `intent.driven()`: the nets an output drives.

Fixed
- A net that an output drives is no quiet node: the inverting input of a follower sits on
  its own output, and that made the output a node to keep short and its resistors parts
  that belong at the amplifier. A resistor from a buffer to a converter's input was held by
  both ends and lay 22 mm from the converter. `Placer.ties()`, `intent.findings()` and
  `intent.roles()` now leave such nets out. Eight placements of that board: 5 unmet needs
  on average instead of 8, their distances summed 55 mm instead of 112 mm.
- Resistor networks (RN1) are passives for the placer: a network of four pads had counted
  as a part that others belong to, and belonged nowhere itself. Two pads of one part on a
  quiet node are tied like one.

## 0.25.0

Added
- The router says which parts are in the tracks' way, and the placer moves them out:
  `place.ease(judge, movable)` with `Placer.evict()`, `Placer.outside()` and
  `Placer.loose()`. `Board.glance()` now also returns `in_way` (the parts beside the nets in
  conflict, with the place each would have to leave), `heat` (every square of 1 mm the
  nets fought over in any round) and `settled` (the mean of the last four rounds: a single
  round's count swings by a third). `ease()` tries the parts named, one at a time, by
  evicting each and looking again, and keeps the move that leaves the fewest cells in
  conflict. On a board of 287 parts and 159 nets, eight placements: three settled at 142
  to 167 cells in conflict after 16 rounds and took 136 to 172 rounds to route (one kept a
  connection open); five settled at 0 to 57 (the one routed in full: 26 rounds). The heat
  maps of the eight differed in one place of 4 mm square below a row of controller pins,
  where the three had one resistor more; wire length, ties and pad density were the same.
  `ease()` found that resistor in one pass (four glances), and with it on the other side
  the three route clean in 25 to 29 rounds, 28 to 41 s instead of 146 to 282 s.
  (Tried first and dropped: allowing fewer pads in the hot squares and annealing again.
  Three such runs settled no lower than three runs without.)
- `Placer.improve(**more)` passes further fields to the annealer, as `warm=0.3, reach=3.0`
  for a run that starts cool and moves parts by 3 mm at most.

Changed
- Routing is faster, with the same kind of result: a board of 159 nets in 33 s instead of
  44 s, older placements of it that route clean in 37 to 78 s instead of 40 to 91 s.
  Four older placements that have never routed clean reliably (all with that resistor's
  kind of trouble at the controller) came out with 16 connections open between them
  instead of 9 with the old estimate of the next item, three worse and one better: the
  same costs, other ties. Not known whether that is chance.
  - A search for the nearest of more than twelve pads estimated the distance by one box
    round them all, which is no estimate inside the box: a net of forty pads searched in all
    directions from all it had laid (300 000 cells opened for a way of 50). Now the distance
    to the nearest goal is looked up per square of 8 cells, worked out when first touched.
  - A net pushed aside while shortening may only get as much longer as the pusher got
    shorter: its search ends there. A pushed net with no other way used to search its whole
    area to find that out (a supply net: 4.5 million cells, in every pass).
  - In the shortening passes a net is tried again only where something near its tracks has
    changed, and only those tracks are laid again.

## 0.24.0

Added
- Tracks leave room round a through-hole pad that hangs in a pour: `negotiate(room=0.8,
  pours=)`. Such a pad is joined to the pour by spokes, and a track led past it at
  clearance cuts them: a board came out with a ground pin on one spoke (DRC: starved
  thermal) after a change that had nothing to do with it. Now tracks keep `room` mm from
  such pads on the pour's layers where a way exists that is not more than `shy` times
  longer. `pours={net: [layers]}` names pours that are only added after routing, as is
  usual on outer layers; zones the board has already are seen without. The board in
  question: 14 such pads, 3 mm more track in 1.6 m, clean.

## 0.23.0

Changed
- The router lays a net again only where another net lies on it: the tracks nobody else is
  on stay, and the connections that went are searched again (connection-based negotiation;
  until now a net of forty pads with one branch in the way was laid again whole, forty
  searches for one). When the rounds stall for the first time it goes back to whole nets,
  which helps a board that is stuck. The nets pushed aside while shortening are treated
  the same way. A board of 159 nets: 50 s instead of 70 s, track 1 % shorter. Five older
  placements of it that route clean: 40, 63, 70, 91 and 47 s instead of 53, 85, 179, 272
  and 97 s (a sixth 542 s instead of 123 s); the four that do not still leave 10
  connections open between them.
- A try to shorten a net ends after its first search when even across other nets it is no
  shorter than it lies (most tries: 63 of 67 in a third pass).

Added
- `glance()` names the parts that stand where nets are in each other's way (`where[i][4]`):
  what could be moved.

## 0.22.0

Added
- What a circuit needs of its layout, read from the netlist (`leiterbahn.kicad.intent`).
  The symbols say which pin is a supply input and which is called FB, BOOT or SW;
  `read_pins()` reads that, `Board.pin_info` holds it. From it:
  - `Placer.unmet()` / `intent.findings()`: supply pins with no capacitor within 4 mm,
    feedback and amplifier-input nodes that reach further than 10 mm, boot capacitors far
    from their pin. On a board of 287 parts it listed 14, among them three a reviewer had
    found by measuring.
  - `intent.roles()`: the quiet and the noisy nets as far as pin names tell, for
    `negotiate(quiet=, noisy=)`.
  - `Placer.ties()` uses it: capacitors of a rail are shared out over the pins that are
    supply inputs (an enable input tied to the rail had taken a converter's capacitor), and
    a part on the net of a quiet pin belongs at that pin whatever its other end is on. The
    same board: 10 unmet instead of 14, 122 mm too far in sum instead of 197, routed in 20
    rounds instead of 36, 3 % more track.
  - `Placer.mend()`: after improve(), moves what unmet() lists to its pin where there is
    room nearer, and keeps a move only if the sum of all that is unmet fell (8 unmet, 110 mm).
- `Board.glance()` / `negotiate.glance()`: how a board will route, from 8 rounds and without
  drawing anything. Of ten placements of one board, those that routed clean had 88 to 132
  cells in conflict after 8 rounds, those that kept connections open 186 to 323.
- The router settles nets that go on changing places by trying the orders of laying them
  (`untangle.rs`), small knots while the rounds go on and all of them when they run out.

Fixed
- A net whose first pad was shut in came out with all its other pads apart as well: the
  router gave up at the first connection without a way. It now goes on from the next pad,
  so one pad shut in costs one connection.

Ten placements of a board with 158 nets, five of which had ended with 18 connections open
between them: 10 open now, and six of the ten clean.

## 0.21.0

Added
- Finished tracks are straightened (`negotiate(straight=True)`, on by default). On a grid
  with diagonals most ways have many shapes of one length, and the search left a bend
  wherever it stepped aside: 2962 pieces for 2 m of track on a board of 158 nets, with 131
  steps aside between parallel runs. Each stretch now takes the way with one bend between
  its ends, over as many of its bends as that way is free for: never longer, never nearer
  to copper the net keeps away from, no sharper where it meets what stays. The same board:
  1545 pieces and 40 steps aside, the same length, DRC clean, 2 s more.
- `board.kinks(board)`: steps aside (a short piece between two parallel runs) and turns
  back (135 degrees or more within 2 mm) of the autorouted tracks, with where.

Turns back are not fewer for it (23 and 22): each is the shortest way round something
that is there, and goes only when what it goes round moves.

## 0.20.0

Added
- Nets that are to keep apart. `negotiate(quiet=, noisy=, apart=0.5, shy=3)`: a track of a
  quiet net (a feedback node, a sense line, a reference) keeps `apart` mm from all copper
  of the noisy nets (a switch node, a gate drive, a clock), pads and hand routes too, and
  the other way round, where a way exists that is not more than `shy` times longer for it.
  No clearance rule says this, and a review had measured a current-limit amplifier's input
  0.14 mm beside a rectifier node for 2.2 mm. On that board, 158 nets of which 36 quiet and
  17 noisy: 5.6 mm of quiet track beside noisy copper became 0.1 mm, for 12 mm more track
  in 2.7 m and 4 rounds more; on a second placement 2.0 mm became 0.4 mm.
- `board.coupling(board, quiet, noisy, apart)`: where quiet tracks run beside noisy copper,
  how long and how near. Print it after routing.

Measured and not added: ways out for the pins of a fine-pitch part, laid and fixed before
the rounds (each pin joined to the pad of its net beside it, the rest planned by min-cost
flow to via places). It halves the rounds on a well-placed board and leaves connections
open on four of four worse ones: what is laid one after the other and then fixed takes
the room negotiation needs. And sides chosen by the placement forces: no better than the
sides the board script gives (cost 3565 against 3467 over six seeds).

## 0.19.0

Changed
- Router: a narrow track is only as long as it must be. A pin with no way out at its net's
  width got the whole connection at the board's track width, and what `widen()` could not
  restore stayed: 7 mm of 0.127 mm between a converter's input transistor and its 100 uF
  capacitor. Now the full width takes over from the first cell where it has room, if its
  way is not dearer than the narrow one by more than the ratio of the widths (at most 4:
  taking every full-width way left connections open on boards that had routed). On that
  board 1.1 mm of narrow track in 2 pieces, before 30 mm in 37; 8 of 8 seeds route.
- `strays()` also lists the parts placed with `first()`.

## 0.18.0

Changed
- `ties()`: of the capacitors between one supply rail and a plane, every supply pin is
  given one before any pin gets a second, the nearest pairs first. Each tied to its
  nearest pin, two shared a pin and others had none within 10 mm; which pins went without
  changed with the seed.

## 0.17.0

A second review of the same board, placed with 0.16.0: 8 of 27 findings gone, 8 better,
6 worse. What was worse had two causes, and these changes are for them.

Added
- `Placer.first(refs)`: place these now, in this order, each at the pins it belongs at, and
  keep them there. With every small part tied to its pins at the same weight, a
  controller's supply capacitor lay 5 mm out and its compensation 10 mm, behind parts that
  could have been anywhere. Placed first in the order of what must be nearest: supply
  capacitor 2.6 mm, feedback node 11 mm long instead of 22 to 42.
- `Placer.tie(ref, pad, to, to_pad)`: say where a pad belongs; replaces what `ties()` would
  find for it.
- `negotiate(edge_clear=)`: what tracks keep from the board's edge (0.6 mm tracks had been
  laid 0.3 mm from it).

Changed
- `ties()`: a large capacitor (`bulk()`) belongs at the largest pad on its net, a
  transistor's drain or an inductor, not at the nearest pin of an integrated circuit. The
  100 uF capacitor of a converter's output had been tied to the controller's sense pin
  and lay 32 mm from the transistor.

## 0.16.0

A review of a board placed by `improve()` found 15 things worth changing that no check had
seen: it routed, and DRC was clean. Nearly all had one cause: the placer shortens
connections and does not know which part belongs at which pin.

Added
- `Placer.ties()`: which pad of a resistor or capacitor belongs at which pin. A part whose
  nets off the planes all end at one part with three or more pads belongs to it (a boot
  capacitor across two pins of a controller, a timing resistor from a pin to ground, a
  supply capacitor); `improve()` ties it there, a capacitor at 3, a resistor at 1.5. Before,
  only parts on a supply rail were tied, at 1 or a quarter, and to any part on the rail: a
  regulator's output capacitors had gathered at the rail's test point, 13 and 21 mm from
  the regulator. Test points and other parts with fewer than three pads are no hosts now.
  On that board the boot capacitors came from 8 mm to 3 to 5 mm, with the routed length as
  before (eight seeds, all routed).
- `Placer.strays(limit)`: the parts further than `limit` mm from the pin they belong at.
  Print it after placing.
- `Placer.over(net, *rects)`: every pad on `net` lies within one of the rectangles, for a
  plane that covers part of the board. A pad over another net's plane gets no via into its
  own; three bulk capacitors hung on 10 mm of 0.127 mm track that way, and a 3 A diode
  between two grounds sat 20 mm inside the wrong one. Holds in `near()` and `improve()`.
- The router says, when its rounds stall, which nets are in each other's way and where:
  that is where the board is too full. (Ten placements of one board: every such place was
  at the pin rows of one QFN.)

## 0.15.0

Added
- `Placer.improve(spread=True)`: the annealing begins from where forces put the parts,
  not from where `near()` did. Connections pull; courtyards push each other apart as like
  charges do, the field found by solving Poisson's equation on a 1 mm grid (as in ePlace);
  keep-outs, fixed parts and what lies outside a part's rectangles are charge that does not
  move. Then every part is set down where it is allowed, the largest first, at the place
  near by where its connections are shortest (`router/src/place/spread.rs`). On a board of
  287 parts, eight seeds each: cost 2974 mm on average without and 2705 mm with, nets and
  tethers 15 % shorter, every seed better than the best without; 2.6 s without, 3.7 s
  with. Routed, the tracks came to about 2750 mm against about 3150 mm, and 8 of 8 seeds
  left nothing open, once three things in the board's script were put right that the
  forces had brought out (see the skill: two blocks on one anchor, an anchor where the
  block's largest part has no room, and the side opposite a fine-pitch part). Before
  that, 1 of 8.
- `improve()` also returns `started`: the cost the annealing began from.
- `leiterbahn-route place`: `spread`, `spill` (how much overlap the forces leave to the
  steps after; 0.2) and `reach` (how far the annealing shifts a part at first) in the
  problem.

Tried and not in: counting pads of both sides together in the density limit, to keep the
side opposite a QFN free for its vias. It changed nothing there (8 parts within 6 mm
before and after).

## 0.14.0

Added
- `Board.shut_in()`: the nets that have no way even alone on the board, with the pads
  concerned, in seconds (`leiterbahn-route` with `"alone": true`). No routing joins such a
  net. It found why 3 of 8 improved placements of a board left a gate drive open: a
  capacitor's ground via, set by `fan_to_planes(lane=0)`, came out on the other side
  0.2 mm in front of a controller pin. With `lane=0.5` those three route.
- `Placer(lane=0.5)`: room in front of the pins of a part with five or more pads; the
  part's box reaches that far beyond every long pad. Without it `improve()` put a capacitor
  in front of four pins of a 0.5 mm pitch QFN; the track to it ran along the row, three
  of those pins never got out, and the rounds went on to their limit. Of eight seeds on
  that board, routed: 3 clean before either change, 7 with `fan_to_planes(lane=0.5)`, 8
  with that and `Placer(lane=0.3)` or `0.5` (the tracks are then 1 to 3 % longer, for the
  parts stand further from the pins).

Changed
- The router's rounds no longer run to their limit when a few nets go on changing places
  over the same cells (2 nets, 18 cells, from round 30 to round 200 on one board): when
  the conflicts have not got fewer for 20 rounds (`stall`), the price for crossing starts
  low again, with what the cells cost so far remembered; after three such starts the
  rounds end. Of three boards that stalled, one came clean in 61 rounds instead of 187,
  one in 155 instead of 200, one stayed as it was (one connection open). Letting an open
  net push others two levels deep, at a high price for crossing and keeping what it had,
  was tried on these and closed nothing; it is not in.

## 0.13.0

Added
- `Placer.improve()`: the parts `near()`, `large()` and `small()` placed are moved, turned,
  put on the other side and exchanged where that shortens the connections (simulated
  annealing in `leiterbahn-route place`, Rust; the same result for the same seed; 5 million
  moves on 250 parts in a few seconds). The cost is the half perimeter of each net's pads,
  a tie of each capacitor to the supply pin it serves, the pull to the block's anchor, the
  price of the back, and a limit on pads per 2 mm square, set by what the placement that
  came in held: without that limit 7 of 8 results on a board with 287 parts left
  connections without a way, with it 5 of 8. So a result is a candidate: route it, and
  take another seed if something stays open. The best of eight on that board: routed
  length 2294 mm instead of 2618 mm, 225 vias instead of 236, the 13 nets weighted 3 at
  200 mm instead of 235 mm, DRC clean.
- Where parts may go, as data: `Placer.inside(refs, *rectangles)`, `Placer.only(refs, "F")`,
  and `keep()` as before. `improve()` works with these; it cannot see into a function in
  `rules` and refuses to run with one.
- `Board.separate_silk()`: drops silkscreen graphics that come nearer to a neighbour's than
  the rules allow (courtyards 0.2 mm apart do not keep a pin-1 mark off the next outline).

## 0.12.2

Fixed
- `place_refs()` looked at the front silkscreen only. A label of a part on the back was laid
  over the back's silk (two DRC warnings on a board with 142 parts there), and was kept away
  from pads and labels of the front, which are not in its way. Each side is now placed by
  itself, a label keeps the rules' distance to silk (new argument `silk`, 0.16 mm) instead
  of 0.1 mm, and it is positioned by the box of the text as drawn. On that board 115 of 287
  labels find room instead of 29; on a four-layer board with 91 parts, 81 instead of 54.

## 0.12.1

Changed
- The router is about twice as fast: the same board (158 nets, 0.05 mm grid) in 171 s
  instead of 335 s, although the run took 117 rounds instead of 35. A profile showed 43 %
  of the time in the binary heap of the search and the rest in the search loop. The heap is
  now a ring of buckets a quarter of a cell wide; the three values the search reads per cell
  lie together in 8 bytes, as do the three it writes. It needs 0.5 GB for that board
  instead of 0.2 GB. Results differ slightly from 0.12.0 (cells of nearly equal cost are
  taken in another order) and are the same from run to run.

## 0.12.0

Added
- The router shortens its result. Once all nets are laid without conflict, each net is laid
  again across the others where that is shorter, and those it crosses take what is then
  free; the exchange is kept if the sum of costs times weights got smaller and no connection
  was lost. Every step makes the result better, so it ends. On a six-layer board with 158
  nets the tracks of the nets with 2 to 12 pads went from 18 % to 9 % above the shortest
  tree over their pads (2621 to 2419 mm, 238 to 217 vias); one net that the rounds had
  pushed onto 82 mm came back to 16 mm (14 mm between its pads).
- Weights: `negotiate(weights={net: 4})`, `--weight NET=4`. A heavier net wins a short way
  from lighter ones in those exchanges: of two nets at a gap that holds one track, the
  heavier gets it. On the crowded board above, weights on thirteen nets at one controller
  gained nothing (315 mm without, 330 mm with; 260 mm with only those nets on the board),
  and neither did three ways of letting weights act during the rounds. Routing the thirteen
  first and keeping them left five other connections without a way: there the parts have
  to move, and the next figures say so.
- Per net, the router reports what its tracks cost and what they would cost with the net
  alone on the board: `negotiate()` returns `cost`, `alone` and `by_net`; `--report file`.
  Where the two are far apart the routing can still gain; where a net is long though they
  agree, its pads are far apart and the placement has to change.
- `leiterbahn-route board ... --only NET` routes the named nets and leaves the rest as it is
  (as `negotiate(only=...)` does).

Changed
- A search stays within 4 mm of a net's pads and tracks unless there is no way there: the
  rounds on that board took 106 s instead of 146 s. The shortening adds about 150 s; nearly
  all of the router's time is the search itself.

## 0.11.0

Added
- `leiterbahn-route board in.kicad_pcb out.kicad_pcb`: the router reads a KiCad board file
  itself (footprints, pads, tracks, vias, zones with their fills, rule areas, outline; rules
  and net classes from the `.kicad_pro` beside it), routes it and writes the tracks into the
  file. No KiCad and no Python are involved. What is joined already is found by the router:
  copper of a net that overlaps, and copper on the same island of a pour.
  Checked on two boards (91 and 287 parts, four and six layers): the problem read from the
  file is the same, shape for shape and piece for piece, as the one `Board.negotiate()`
  builds through KiCad; KiCad's DRC finds no open connection and no copper violation in the
  written files; reading them again leaves no net in pieces. `--problem file.json` writes
  the problem as read. The pours of the written file are as they were before routing;
  `kicad-cli pcb drc --refill-zones --save-board` refills them.

## 0.10.0

Changed
- **The project is now called `leiterbahn`** (was `hw-design`), and so is its Python package
  (was `hwdesign`): `from leiterbahn.kicad.board import Board`. The plugin and its agents are
  `leiterbahn` and `leiterbahn:<agent>`; the repository is `dwzg/leiterbahn` (GitHub forwards
  the old address). A project with the submodule changes its path, its URL and its imports.
- The router's program and crate are `leiterbahn-route` (was `hwroute`); `LEITERBAHN_ROUTE`
  names another binary (was `HWROUTE`).

Removed
- `hwdesign.mcad` and the skill `printed-enclosure`: they are a project of their own now,
  [gehaeuse](https://github.com/dwzg/gehaeuse) (`gehaeuse.probe`, `.gasket`, `.printing`,
  `.render`; functions unchanged). `leiterbahn.kicad.geometry` still writes the `board.json`
  a housing is generated from. The package's extra `[mcad]` is gone with it.

Fixed
- CI: nine schematic tests place parts from KiCad's own symbol libraries and failed where
  KiCad is not installed (every CI run since they were added); they skip themselves there now.

Earlier entries below use the names of their time.

## 0.9.0

Added
- `hwroute` (`router/`, Rust) and `Board.negotiate()`: a router of its own. All nets are
  routed at once and may at first lie on top of each other; a cell two nets want gets dearer
  round by round until one of them takes another way (negotiated congestion, PathFinder).
  No net is laid first, so the order of the nets no longer decides who gets the last via
  site. Tracks at their net class's width and clearance, a narrow track only where the
  class width has no way at all; vias keep hole distances; pieces of a net that are joined
  already (pours included) are left as they are. `negotiate(box=...)` routes a region only.
  Measured on two six- and four-layer boards of 65 x 56 mm, 0.05 mm grid: 287 parts, 451
  connections, nothing open after 92 s (Freerouting and `route_open()`: 5 open after 25
  minutes, closed by hand-written rework); 91 parts, 184 connections, nothing open after
  28 s. KiCad's DRC found no clearance, hole or edge violation on either. Track length 15 %
  and 12 % below the earlier result after `improve()`.
- `hwdesign.kicad.negotiate`: `problem()` (a board as hwroute's problem, plain data),
  `solve()`, `binary()` (builds hwroute with cargo when needed; `HWROUTE` names another).

Fixed
- The finishing passes took the board's edge from the outline's bounding box, which includes
  half the outline's line width: a via could sit 0.05 mm too near the edge.

## 0.8.3

Fixed
- `route_open()` on a net with a pour: which of the net's pieces a connection joined still
  depended on the two items KiCad happened to name, so two runs of one script differed in
  a few ground tracks. The pieces are now taken from KiCad's connectivity, pours included
  (`finish.cluster_of(items, key, pieces)`), and such a connection ends on the nearest
  copper of another piece, like any other net's.

## 0.8.2

Fixed
- `route_open()`: copper that hangs on a track it takes out of the way (a track ending on
  its middle or beside its via) is joined again with the track's own ends, in the same
  all-or-nothing step (`finish.cut_off`). 0.8.1 refused such tracks, which left 6
  connections open on a board where 2 had been.
- `route_open()` named an open connection by the two items KiCad reports, and those change
  from run to run. It now names it by the two nearest items of the two pieces, starts from
  the smaller piece and orders the list by the copper alone; a round follows as long as the
  last one routed something (a track from one piece may end on a third).
- `improve()` orders its runs by their position, not by item ids.

## 0.8.1

Added
- `board.repeatable()`, to be called first in a board script, and seeded item ids in
  `Board()` and `Board.reopen()`: the same script now gives the same board every time. Two
  runs of one script had left different connections open after `route_open()` (Python's
  set order and KiCad's random ids both changed from start to start), and rework drawn for
  one result met the other.

Fixed
- `route_open()` could open a connection: it took a track out of the way to which another
  track of the net was joined along the way (ending on its middle, or beside its via) and
  did not join that one again. Such a track now stays, and the connection that wanted to
  cross it is reported. `improve()` uses the same test (`finish.joined_only_at_ends`).

## 0.8.0

Added
- `Board.improve()` (`finish.improve`): routes again what is routed, one run at a time
  (the copper between two pads, junctions or locked tracks, through its vias), and keeps
  the new path if it is shorter, has fewer vias or reaches the net class's width. A run may
  push other runs out of its way; the exchange is kept if the sum gets smaller. Locked
  copper stays, connections are neither opened nor made (checked at the end). On a
  six-layer board with 3.4 m of autorouted track it took 2 minutes and saved 70 mm and 10
  of 288 vias; with every run alone on the board the saving could be 17 % at most.
- `finish.route_stats(board)`: track length, vias and length below class width of the
  unlocked copper. `finish.runs_of()`, `finish.overlapping()`, `finish.in_the_way()`.

Fixed
- `prune_dangling()` crashed pcbnew in its second round on a reopened board (tracks removed
  with `Remove()`); it deletes them now.

## 0.7.1

Fixed
- `acute_angles()` no longer reports a piece shorter than the track is wide as a sharp
  joint with the track after it (a 0.1 mm piece between two 0.6 mm tracks read as 38
  degrees). The tracks on both sides of such a piece are still compared with each other.

## 0.7.0

Added
- `Board.reopen(pcb_path, ...)`: a saved board as a `Board`, for rework after routing
  (`rip_up`, `track`, `via`, `route_open`, `prune_dangling`, `fill_zones`, `unrouted`,
  `save`). Rework on the saved file starts from the same board every time and takes
  minutes where the building run takes the autorouter's time again.
- `Board.rip_up(x0, y0, x1, y1, nets=, keep=, layers=, vias=)`: removes the unlocked tracks
  and vias with an end in a rectangle. With `route_open(only=[...])` this is local rip-up
  and reroute: on a six-layer board, two connections that no order of whole-board routing
  closed were closed by clearing 7 x 5 mm around them and routing them first.

## 0.6.4

Fixed
- `route_open()` keeps the clearance of the net classes: to copper of another net it now
  keeps the larger of the two classes' clearances instead of the board's default. A 5 V
  track had been laid 0.25 mm from a relay contact's via whose class asks for 0.5 mm (DRC
  reported it).

## 0.6.3

Fixed
- `fan_to_planes()` and `route_open()` no longer add tracks of no length (a via in the
  middle of its own pad got one). KiCad's DRC reported one of them as "tracks crossing" with
  a track 2.5 mm away.

## 0.6.2

Fixed
- `route_open()`: a movable track lying in a rule area or at the board edge marked those
  cells as "free once the track is moved", so a new track could follow it in. A via of a
  floating rail landed 0.05 mm inside an isolation gap this way (1.7 mm instead of 2 mm to
  the other domain; the gap check caught it). What is fixed is now stamped last.

Added
- `route_open(hug=0.0)`: optional extra cost for steps away from all copper.

## 0.6.1

Changed
- `route_open()` joins an open connection to the nearest copper of the other part of its net,
  starting anywhere on the copper already joined to the first pad; the ratsnest names one pad
  pair, and on a rail that pad was often shut in while a track of the net ran 1 mm away
  (9 open connections became 5 on a 287-part board). Nets with a pour are still routed pad
  to pad: the module does not see what a plane joins.
- `route_open(depth=2)`: a track that is moved may move others in turn, all or nothing.
  `limit` now bounds one search at 400000 cells (a failing search over a whole six-layer
  board took half a minute, eight of them per connection).
- A new track ends exactly on the track or via it joins and no longer adds a short piece that
  could fold back; the same segment is not drawn twice.

Fixed
- `prune_dangling()` asks KiCad what dangles (`TestTrackEndpointDangling`) instead of its
  own geometric test, which took a track end 0.36 mm from the centre of a 0.6 mm via for
  loose where KiCad saw the copper overlap: it removed three whole connections of a fully
  routed board, vias included.

## 0.6.0

Added
- `Board.route_open(tmp_dir)` (module `finish`): connects what the autorouter left open, one
  connection at a time, with a grid search over all routing layers (0.1 mm grid, 45 degree
  steps, vias; a second try on a 0.05 mm grid in a window around the connection). Freerouting
  2.5 and 1.9 both left single connections open on a 287-part board for no visible reason
  (two neighbouring header pins of one net, a test point under a pad) and later passes did
  not recover them; a finisher that only needs a free path closes them, and DRC stays the
  judge. Where there is no free path it may cross unlocked tracks, but only if each crossed
  track finds another free way; otherwise nothing changes, so a call never leaves more open
  than it found. `only=[nets], locked=True` routes chosen nets before the autorouter.

Changed
- `Board.fan_to_planes(lane=1.0)`: the way straight out of every long pad of a part with five
  or more pads is kept free of other nets' fan-out vias and tracks. A ground pin's via sat
  0.25 mm in front of the next pin of a 0.5 mm pitch QFN; that pin was then unroutable on an
  otherwise empty four-layer board, for Freerouting and for `route_open()` alike.

## 0.5.1

Changed
- `Schematic.write_lib_tables(directory, fp_dirs=())`: project libraries outside the project
  directory are written relative to `${KIPRJMOD}` (with `..`), and footprint libraries are
  also looked up in `fp_dirs`. Two boards sharing one `lib/` directory got an absolute path in
  `sym-lib-table` and a `${KIPRJMOD}/lib` entry that pointed nowhere: ERC then warned that the
  footprint library was not configured.

## 0.5.0

Added
- `Board.fan_to_planes()`: before autorouting, every SMD pad of a plane's net gets a via into
  its plane (nearest legal spot, straight track on the pad's side, a nearby via of the same
  net used again). `autoroute()` then takes the pads that are on a plane off the router's
  list. Freerouting does not connect pins to a plane: on a 491-part board 273 of 465 open
  connections after a pass were on the three plane nets, and the count did not fall.

Fixed
- `via_near()` drew the track from a back-side pad to its via on the front.

## 0.4.0

Added
- `hwdesign.kicad.place.Placer`: placement by connections on both board sides. Parts fixed
  by hand, the rest around an anchor per group (large parts first, then each part where its
  pads are closest to the pads they connect to; a decoupling capacitor at a supply pin that
  has none yet). `rules` keep parts out of regions (ground domains, height limits), `keep()`
  reserves room for hand routes, `face()` and `find()` help fixing sense resistors.
  Coordinates are millimetres from the board's corner.
- `Board.autoroute(fanout=False, optimize=True)`. Freerouting 2.5 gives every SMD pin a via
  before routing unless told otherwise: 1168 vias on a 491-part board with parts on both
  sides. Off by default now; `optimize=False` skips the optimiser for a quick routability test.
- A test that runs Freerouting (skipped where the command is missing).
- Skill: section "Many parts, both sides".

## 0.3.2

Added
- `Board.place_near(..., back=True)` places on the back of the board.

Fixed
- Placement on both sides: `fits()` and `place_near()` compared a part's courtyard with every
  other part, whichever side it was on, so nothing could go under a part on the other side.
  Parts on opposite sides now only collide if one of them has through-hole pads (a pin header
  takes the space on both sides). `courtyard_overlaps()` uses the same rule: it used to miss
  an SMD part placed onto the pins of a through-hole part from the other side.
- `Board.place(ref, ..., back=False)` now brings a part back to the front if it was on the back.

## 0.3.1

Fixed
- Auto-wiring, two pins of one part tied together (a NAND gate used as an inverter): the
  drawing had a bar across the two pin ends, a 1.27 mm hook next to it and a jog at the
  driving output. Three causes: `finish()` joined adjacent signal pins along the pin ends
  before the router ran (it now leaves them to the router, which joins them in front of the
  pins; supply pins and pins on one spot are still joined); the router wired a pin again
  that a wire already ended on; and running between the 2.54 mm lines cost so much that a
  30 mm wire jogged right at a pin placed between the lines.

## 0.3.0

Added
- `Schematic(..., autowire=True)`: `finish()` connects the pins of each net inside a block
  with wires and uses a label only where a net leaves the block. A block is a `frame()` and
  the parts in it. A schematic connected by labels alone was rejected in review: it is a
  netlist, the circuit cannot be read from it. `hwdesign.kicad.autowire` holds the router
  (orthogonal, 1.27 mm grid, never along or over a foreign wire's end, pins entered
  straight); `Page.autowire()` runs it on one page. Pins it cannot reach keep a label and
  give a warning.

Changed
- `autoplace_fields()` tries above and below first for a part lying on its side. Left and
  right of it are its wires.
- The name of a power symbol that lies on its side (pin on the left or right of a part) is
  written beyond the symbol's tip. At the library's position it lay along the stub, struck
  through by the wire.

Fixed
- Pins of a part placed with `mirror=True` and `rot=90` or `270` were modelled at the
  opposite points: the library mirrored before rotating, KiCad rotates first. KiCad's ERC
  reported all pins of such FETs as not connected and parity failed. 0 and 180 degrees were
  not affected.
- Field justification on parts rotated by 90 degrees or mirrored: KiCad swaps left and right
  for 90 and 180 degrees and once more for a mirrored part; the library knew only 180.
  Texts moved by `autoplace_fields()` were drawn on the body of every resistor lying on its
  side while `overlaps()` reported nothing.

## 0.2.1

Added
- `Schematic(power_symbols={net: lib_id})`: supply nets beyond the six built-in names, each
  drawn with a power symbol of your choice. The symbol's value names the net, so a second
  ground (`{"GNDF": "power:GNDA"}`) or another rail gets its own net.

Changed
- `finish()` joins pins of one part that share a net and are stacked or neighbours (up to
  5.08 mm apart, no other pin between) with a wire and gives them one label or power symbol. A connector with eight ground pins on
  one spot got eight symbols on top of each other.
- `finish()` puts a pin's power symbol beyond the net labels of the pins right next to it.
  At the fixed 2.54 mm stub it lay on the neighbour's label.
- `overlaps()` reports parts, texts, labels and `frame()` rectangles that reach into the
  drawing sheet's title block. Nothing checked that corner, and frames drawn across it
  only showed up in the PDF.
- `overlaps()` accepts global labels on neighbouring pins: their boxes are 0.32 mm apart by
  construction, which the 0.4 mm rule reported for every bus on a multi-sheet design.

Fixed
- `power_flag()` on a pin marked the pin as connected, so `finish()` gave it no label and
  the pin ended up on a net with only the flag. The pin now keeps its label or power symbol.

## 0.2.0

Boards with more than two layers and schematics with more than one sheet. Existing scripts
run unchanged; what differs for them is listed under "Changed".

Added
- `Board(layers=2, planes=())`: 2, 4 or 6 copper layers; `planes` are inner layers for pours
  only (KiCad layer type "power", exported to Freerouting as `(type power)`).
  `Board.copper_layers` and `board.copper_layers(board)` list the enabled copper layers.
- `Board.zone(..., rect=, polygon=)`: a pour for one region, so two ground nets can share a
  layer.
- `board.min_gap(board, nets_a, nets_b)`: the smallest copper-to-copper distance between two
  groups of nets on any one layer, and where it is. The geometry is in the new
  `hwdesign.kicad.planar`, which needs no pcbnew.
- `Board.cutout(x, y, w, h, r)`: a slot inside the board or a notch in one edge. `fits()`,
  `place_near()`, `stitch()`, `via_near()`, `label_near()`, `place_refs()` and
  `courtyard_overlaps()` respect it.
- `Schematic.sheet(name, file)`: more pages with the same drawing API (`Page`). One
  reference namespace, design-wide `netlist()`, `overlaps()` and `finish()`; `save()` writes
  all files. With sheets, labels are written as global labels.
- `SymbolLib([dir, ...])` and `Schematic(sym_dirs=[...])`: project symbol libraries before
  KiCad's own. `write_lib_tables()` addresses libraries inside the project through
  `${KIPRJMOD}`.
- `fab.copper_layers(pcb)` reads the copper layers from the board file; `fab.export()`
  returns them as `copper`.
- `read_netlist()` returns each part's instance `path` and `sheet`; footprints of parts on a
  sheet get the full path.

Fixed
- `fab.export()` wrote only F.Cu and B.Cu: a four-layer board came out with two copper
  Gerbers. It now exports every copper layer of the board file.
- `path_length()` returned `None` for a route through an inner layer, and
  `prune_dangling()` removed a via (and then its track) that fed an inner plane: both only
  looked at F.Cu and B.Cu.
- `stitch()` only tested the layers that carry the net's pour. With a ground pour on the
  back alone it put 37 vias onto a front-layer track of another net (14 DRC shorts on a
  test board). On layers without the pour, a via must now clear foreign pads and tracks.
- A multi-unit part placed with `unit=2` replaced the entry of unit 1: its pins were missing
  from `netlist()` and were not labelled by `finish()`. Units are now merged.
- GitHub Copilot CLI support: agents list `execute` next to `Bash`, so they keep a shell on
  Windows; README shows the Copilot install.

Changed
- Schematics are written in the KiCad 10 format: `version 20260306`,
  `generator_version "10.0"`, `body_style` and `in_pos_files` on symbols, `hide` on the
  property. The header said 9.0 while the embedded library symbols were already KiCad 10's.
  `kicad-cli sch upgrade` (10.0.8) now rewrites a generated file without adding or dropping
  a token (it only reorders, and empties the project name when no project file is present).
- Placing a reference (and unit) twice raises `ValueError`; it used to give two symbols with
  one reference.
- `keepout()` and `foreign_copper()` default to all copper layers of the board instead of
  F.Cu and B.Cu. On two-layer boards that is the same.
- `outline()` and `outline_rounded()` replace an earlier outline instead of adding a second.

Not verified
- `autoroute()` on a four-layer board: Freerouting was not available when this was written.
  The DSN was checked, the routed result was not.

## 0.1.0

First release, extracted from [probe-wink](https://github.com/dwzg/probe-wink).

- `hwdesign.kicad`: schematic generator, ERC and netlist parity (`verify`), board builder with
  Freerouting and cleanup passes, layout checks, JLCPCB export, enclosure geometry export.
  Netlist reading moved to `netlist.py`, so parity checks work without `pcbnew`.
- `hwdesign.mcad`: solid probing, gasket and print estimates, f3d renders (from the
  probe-wink housing checks; the colour table is now a parameter).
- Skills: `pcb-as-code`, `pcb-review`, `jlcpcb-order`, `printed-enclosure`.
- Agents: `layout-reviewer`, `placement-preview-checker`.

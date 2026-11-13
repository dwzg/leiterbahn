//! Untangle a knot by trying who goes first.
//!
//! Negotiation ends in a few nets that need the same cells and go on changing places: on boards
//! of 158 nets, 2 to 5 nets on 10 to 40 cells from round 40 to round 200. What settles such a
//! place is an order: one net takes its way, the next goes round it by whatever it costs, a via
//! or a long way that no price for crossing ever made worth it. A handful of nets has few orders,
//! so all of them are tried, each net laid in the room those before it left. If none works, the
//! nets the knot would have to cross are taken up as well.
//!
//! Measured on five boards of 158 nets that ended with connections open: one came clean, in 60
//! rounds and 117 s instead of 182 rounds and 529 s. On the other four no order works: there the
//! knots are one tangle of 11 to 20 nets at the pins of a 0.5 mm pitch part, where a pin's only
//! way out is crossed by its neighbour's whichever goes first. Several ways per net that keep
//! off each other, and of those one each that fit together (tried, and taken out again), find
//! nothing there either: every way of a pin begins with the same few cells. Such a board wants
//! its parts moved.
use super::*;

impl Router<'_> {
    /// The nets that lie in another's way, grouped by place.
    fn knots(&mut self, order: &[usize]) -> Vec<Vec<usize>> {
        let mut found: Vec<(usize, [usize; 4])> = Vec::new();
        for &n in order {
            self.soft(n, -1);
            if let Some((_, b, _)) = self.where_stuck(n) {
                found.push((n, b));
            }
            self.soft(n, 1);
        }
        let reach = (1.0 / self.grids.step) as usize;
        let mut group: Vec<usize> = (0..found.len()).collect();
        loop {
            let mut changed = false;
            for a in 0..found.len() {
                for b in a + 1..found.len() {
                    let (p, q) = (found[a].1, found[b].1);
                    let near = p[0] <= q[2] + reach && q[0] <= p[2] + reach && p[1] <= q[3] + reach && q[1] <= p[3] + reach;
                    if near && group[a] != group[b] {
                        let (keep, drop) = (group[a].min(group[b]), group[a].max(group[b]));
                        group.iter_mut().filter(|g| **g == drop).for_each(|g| *g = keep);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut knots: Vec<Vec<usize>> = Vec::new();
        for g in 0..found.len() {
            let nets: Vec<usize> = (0..found.len()).filter(|&k| group[k] == g).map(|k| found[k].0).collect();
            if !nets.is_empty() {
                knots.push(nets);
            }
        }
        knots
    }

    fn take_out(&mut self, nets: &[usize]) -> Vec<Laid> {
        nets.iter()
            .map(|&n| {
                self.soft(n, -1);
                std::mem::take(&mut self.laid[n])
            })
            .collect()
    }

    fn put_back(&mut self, nets: &[usize], laid: Vec<Laid>) {
        for (&n, laid) in nets.iter().zip(laid) {
            self.laid[n] = laid;
            self.soft(n, 1);
        }
    }

    /// Lay the nets (all of them taken out) in up to `tries` orders, each in the room the ones
    /// before it leave. Returns the best: connections left open, cost, and how the nets lie then.
    /// The nets are out again afterwards. Ends at the first order that leaves nothing open.
    fn orders(&mut self, nets: &[usize], near: i32, tries: usize) -> (usize, f32, Vec<Laid>) {
        let mut best: (usize, f32, Vec<Laid>) = (usize::MAX, f32::MAX, Vec::new());
        let count: usize = (1..=nets.len()).product();
        let mut turn: Vec<usize> = (0..nets.len()).collect();
        let mut seed = 0x9e37_79b9u32;
        for k in 0..count.min(tries) {
            if k > 0 && count <= tries {
                // the next order of all there are
                let i = (0..turn.len() - 1).rev().find(|&i| turn[i] < turn[i + 1]).unwrap();
                let j = (i + 1..turn.len()).rev().find(|&j| turn[j] > turn[i]).unwrap();
                turn.swap(i, j);
                turn[i + 1..].reverse();
            } else if k == 1 {
                turn.reverse();
            } else if k > 1 {
                for i in (1..turn.len()).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    turn.swap(i, seed as usize % (i + 1));
                }
            }
            let (mut open, mut cost) = (0, 0.0);
            for &t in &turn {
                let n = nets[t];
                self.search.window = self.around(n, near);
                self.lay(n, None, 0.0);
                self.soft(n, 1);
                open += self.laid[n].open.len();
                cost += self.cost(n);
            }
            let laid = self.take_out(nets);
            if (open, cost) < (best.0, best.1) {
                best = (open, cost, laid);
            }
            if best.0 == 0 {
                break;
            }
        }
        best
    }

    /// Settle each knot by an order, see above. `thorough`: also with the nets in the knot's way.
    /// `last`: the rounds are over, so the best order is taken even if it leaves a connection
    /// open; else only one that leaves none. Returns the number of knots and how many are settled.
    pub(super) fn untangle(&mut self, order: &[usize], near: i32, thorough: bool, last: bool) -> (usize, usize) {
        let knots = self.knots(order);
        let (widen, step) = (self.widen, self.grids.step);
        self.widen = false; // an order that fails should fail fast
        let mut settled = 0;
        for knot in &knots {
            // All of it the first time the rounds stall and when they are over; in between only what is
            // cheap, a knot of up to four nets with its 24 orders. (All of it at every stall: boards
            // that stayed tangled took half as long again. Only the cheap part: the one board that
            // came clean did not, its knot wanted the nets in its way taken up.)
            if !thorough && knot.len() > 4 {
                continue;
            }
            // Not a knot with a net of many pads in it: every order lays its nets whole, and a ground
            // net of some hundred pads laid whole a hundred times took a board past 25 minutes over
            // four cells. (Such a net is laid again only where it is in the way: see the rounds.)
            if knot.iter().any(|&n| self.jobs[n].cells.len() > 24) {
                continue;
            }
            let before = self.take_out(knot);
            let none = (usize::MAX, f32::MAX, Vec::new());
            let first = if knot.len() <= 6 { self.orders(knot, near, 120) } else { none.clone() };
            if first.0 == 0 {
                settled += 1;
                self.put_back(knot, first.2);
                continue;
            }
            if !thorough {
                self.put_back(knot, before);
                continue;
            }
            // those the knot's nets would cross if they could, the ones crossed by most first
            let mut crossed: Vec<(usize, usize)> = Vec::new();
            for &n in knot {
                self.search.window = self.around(n, near);
                self.lay(n, Some(0.5), 0.0);
                let fresh = std::mem::take(&mut self.laid[n]);
                for &o in order {
                    if !knot.contains(&o) && fresh.items.iter().any(|a| self.laid[o].items.iter().any(|b| clash(a, b, step))) {
                        match crossed.iter_mut().find(|c| c.1 == o) {
                            Some(c) => c.0 += 1,
                            None => crossed.push((1, o)),
                        }
                    }
                }
            }
            crossed.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            let more: Vec<usize> = crossed.iter().map(|c| c.1).take(6usize.saturating_sub(knot.len())).collect();
            let all: Vec<usize> = knot.iter().chain(&more).copied().collect();
            let kept = self.take_out(&more);
            let second = if more.is_empty() { none } else { self.orders(&all, near, 60) };
            if second.0 == 0 || (last && second.0 < first.0) {
                settled += (second.0 == 0) as usize;
                self.put_back(&all, second.2);
                continue;
            }
            self.put_back(&more, kept);
            if last && first.0 < usize::MAX {
                self.put_back(knot, first.2);
            } else {
                self.put_back(knot, before);
            }
        }
        self.widen = widen;
        (knots.len(), settled)
    }
}

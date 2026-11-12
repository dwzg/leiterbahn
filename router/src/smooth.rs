//! Straighten what the search left crooked.
//!
//! On a grid with diagonals most ways have many shapes of the same length: 2 mm along and
//! 0.2 mm across can be one slanted piece and one straight one, or four steps aside spread over
//! the length. The search takes whichever it meets first, and a board of 158 nets came out with
//! 131 such steps and 27 tracks that turned back on themselves, in 2962 pieces for 2 m of track.
//! Here every stretch of a finished track is replaced by the way with one bend between its ends,
//! over as many of its bends as that way is free for, never longer and never nearer to copper it
//! is to keep away from than the stretch was.
use super::*;
use std::collections::HashSet;

impl Router<'_> {
    /// The cells from a to b on one layer with at most one bend: slanted first, or straight first.
    fn elbow(&self, a: u32, b: u32, slanted_first: bool) -> Vec<u32> {
        let (l, i0, j0) = self.grids.split(a);
        let (_, i1, j1) = self.grids.split(b);
        let (dx, dy) = (i1 as i64 - i0 as i64, j1 as i64 - j0 as i64);
        let slanted = dx.abs().min(dy.abs());
        let straight = dx.abs().max(dy.abs()) - slanted;
        let along = if dx.abs() > dy.abs() { (dx.signum(), 0) } else { (0, dy.signum()) };
        let mut steps = vec![(dx.signum(), dy.signum()); slanted as usize];
        if slanted_first {
            steps.extend(vec![along; straight as usize]);
        } else {
            steps.splice(0..0, vec![along; straight as usize]);
        }
        let (mut i, mut j) = (i0 as i64, j0 as i64);
        let mut cells = vec![a];
        for (di, dj) in steps {
            i += di;
            j += dj;
            cells.push(self.grids.node(l, i as usize, j as usize));
        }
        cells
    }

    /// May a track of this kind run along these cells as things lie? The first cell is where it
    /// is already. Not over the net's other tracks: that would close a loop.
    fn clear(&self, kind: usize, cells: &[u32], own: &HashSet<u32>) -> bool {
        let track = &self.grids.track[kind];
        let nx = self.grids.nx as u32;
        cells.windows(2).all(|w| {
            let (a, b) = (w[0], w[1]);
            let cell = track[b as usize];
            if cell.hard > 0 || cell.soft > 0 || own.contains(&b) {
                return false;
            }
            // a slanted step passes between two cells: as the search has it, neither may be closed,
            // and it may not cross a slanted piece of the net's own there
            let (lo, hi) = (a.min(b), a.max(b));
            let beside = match hi - lo {
                d if d == nx + 1 => [lo + 1, lo + nx],
                d if d == nx - 1 => [lo - 1, lo + nx],
                _ => return true,
            };
            beside.iter().all(|&c| track[c as usize].hard == 0) && !beside.iter().all(|c| own.contains(c))
        })
    }

    /// Which of the eight ways the step from a to b goes, counted round.
    fn heading(&self, a: u32, b: u32) -> i32 {
        let (p, q) = (self.grids.split(a), self.grids.split(b));
        match ((q.1 as i64 - p.1 as i64).signum(), (q.2 as i64 - p.2 as i64).signum()) {
            (1, 0) => 0,
            (1, 1) => 1,
            (0, 1) => 2,
            (-1, 1) => 3,
            (-1, 0) => 4,
            (-1, -1) => 5,
            (0, -1) => 6,
            _ => 7,
        }
    }

    /// By how many eighths of a turn the track bends at b on its way from a to c.
    fn turn(&self, a: u32, b: u32, c: u32) -> i32 {
        let d = (self.heading(a, b) - self.heading(b, c)).abs();
        d.min(8 - d)
    }

    /// What the search would have counted for these cells: their length, and more where they are
    /// near copper the net keeps away from.
    fn price(&self, n: usize, cells: &[u32]) -> f32 {
        let near = self.jobs[n].shy.filter(|_| self.grids.apart > 0.0).map(|a| &self.grids.near[a]);
        let nx = self.grids.nx as u32;
        let mut total = 0.0;
        for w in cells.windows(2) {
            let apart = w[0].abs_diff(w[1]);
            let len = if apart == 1 || apart == nx { 1.0 } else { DIAG };
            total += len;
            if near.is_some_and(|near| near[w[1] as usize].hard > 0) {
                total += len * self.search.shy_cost;
            }
        }
        total
    }

    /// Straighten the tracks of net n. Their ends, their vias and the places where they meet stay.
    /// Returns the number of bends before and after.
    pub(super) fn straighten(&mut self, n: usize) -> (usize, usize) {
        if self.laid[n].nodes.is_empty() {
            return (0, 0);
        }
        let plane = (self.grids.nx * self.grids.ny) as u32;
        self.soft(n, -1);
        self.own(n, 1);
        let ends: HashSet<u32> = self.laid[n].nodes.iter().flat_map(|p| [p[0], *p.last().unwrap()]).collect();
        let mut own: HashSet<u32> = HashSet::new();
        let mut joints: HashSet<u32> = HashSet::new(); // where two of the net's tracks meet
        for path in &self.laid[n].nodes {
            let cells: HashSet<u32> = path.iter().copied().collect();
            joints.extend(cells.intersection(&own));
            own.extend(cells);
        }
        let (mut before, mut after, mut changed) = (0, 0, false);
        for k in 0..self.laid[n].nodes.len() {
            let path = self.laid[n].nodes[k].clone();
            let kind = self.laid[n].kinds[k];
            let last = path.len() - 1;
            before += corners(&self.grids, &path).len();
            let stays =
                |i: usize| i == 0 || i == last || ends.contains(&path[i]) || path[i] / plane != path[i - 1] / plane || path[i] / plane != path[i + 1] / plane;
            let turns = |i: usize| {
                let (a, b, c) = (self.grids.split(path[i - 1]), self.grids.split(path[i]), self.grids.split(path[i + 1]));
                (b.1 as i64 - a.1 as i64, b.2 as i64 - a.2 as i64) != (c.1 as i64 - b.1 as i64, c.2 as i64 - b.2 as i64)
            };
            // the places a new way may start and end at: what stays, and every bend
            let marks: Vec<usize> = (0..=last).filter(|&i| stays(i) || turns(i)).collect();
            let mut out = vec![path[0]];
            let mut a = 0;
            while a + 1 < marks.len() {
                // as far as the next place that stays, and from there back to the next bend
                let stop = (a + 1..marks.len()).find(|&b| stays(marks[b])).unwrap();
                let mut next = a + 1;
                let mut cells = path[marks[a]..=marks[a + 1]].to_vec();
                if path[marks[a]] / plane == path[marks[stop]] / plane {
                    for b in (a + 2..=stop).rev() {
                        let old = &path[marks[a]..=marks[b]];
                        let was = self.price(n, old);
                        let first = old[0].abs_diff(old[1]) != 1 && old[0].abs_diff(old[1]) != self.grids.nx as u32;
                        let inner: HashSet<u32> = old.iter().copied().collect();
                        let others: HashSet<u32> = own.difference(&inner).copied().collect();
                        // no sharper at its two ends than the stretch was (or than a track may be), and
                        // where it meets another track of its net, the way it met it
                        let (i, j) = (marks[a], marks[b]);
                        let sharp = |new: &[u32]| {
                            let m = new.len() - 1;
                            let before = i > 0 && path[i - 1] / plane == path[i] / plane;
                            let behind = j < last && path[j + 1] / plane == path[j] / plane;
                            (before && self.turn(path[i - 1], new[0], new[1]) > self.turn(path[i - 1], path[i], path[i + 1]).max(1))
                                || (behind && self.turn(new[m - 1], new[m], path[j + 1]) > self.turn(path[j - 1], path[j], path[j + 1]).max(1))
                                || (joints.contains(&new[0]) && self.heading(new[0], new[1]) != self.heading(old[0], old[1]))
                                || (joints.contains(&new[m]) && self.heading(new[m - 1], new[m]) != self.heading(old[old.len() - 2], old[old.len() - 1]))
                        };
                        let way = [first, !first]
                            .into_iter()
                            .map(|slanted| self.elbow(old[0], old[old.len() - 1], slanted))
                            .find(|new| new.len() > 1 && !sharp(new) && self.clear(kind, new, &others) && self.price(n, new) <= was + 1e-3);
                        if let Some(new) = way {
                            (next, cells) = (b, new);
                            break;
                        }
                    }
                }
                for c in &path[marks[a]..=marks[next]] {
                    own.remove(c);
                }
                own.extend(cells.iter().copied());
                out.extend(&cells[1..]);
                a = next;
            }
            after += corners(&self.grids, &out).len();
            if out != path {
                self.laid[n].nodes[k] = out;
                changed = true;
            }
        }
        if changed {
            let (mut items, mut vias) = (Vec::new(), Vec::new());
            for k in 0..self.laid[n].nodes.len() {
                let bends = corners(&self.grids, &self.laid[n].nodes[k]);
                let half = self.grids.kinds[self.laid[n].kinds[k]].0;
                copper(&self.grids, &self.jobs[n], half, &bends, &mut items, &mut vias);
                self.laid[n].paths[k].cells = bends;
            }
            self.laid[n].items = items;
            self.laid[n].vias = vias;
        }
        self.own(n, -1);
        self.soft(n, 1);
        (before, after)
    }
}

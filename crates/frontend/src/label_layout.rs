//! Screen-space label placement for the 2D maps (pure geometry, no browser types).
//!
//! Labels are placed in priority order (lower number first). Each tries a ring of
//! candidate positions around its anchor; the first that stays inside the bounds and
//! does not overlap an already placed label or a marker wins. A label that had to move
//! away from its anchor gets a leader line. Low-priority labels that find no free spot
//! are hidden, unless `forced` (public geography such as zone codes).

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
impl Rect {
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && self.x + self.w > o.x && self.y < o.y + o.h && self.y + self.h > o.y
    }
    fn inside(&self, w: f64, h: f64) -> bool {
        self.x >= 0. && self.y >= 0. && self.x + self.w <= w && self.y + self.h <= h
    }
    /// Closest point of the rectangle to `(px, py)`.
    pub fn nearest(&self, px: f64, py: f64) -> (f64, f64) {
        (px.clamp(self.x, self.x + self.w), py.clamp(self.y, self.y + self.h))
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub w: f64,
    pub h: f64,
    /// Anchor point in screen pixels (the thing the label names).
    pub anchor: (f64, f64),
    /// Lower is more important.
    pub prio: u8,
    /// Always drawn (at the least bad position) even if it collides.
    pub forced: bool,
    /// Absolute top-left candidates; empty means a ring around the anchor.
    pub cands: Vec<(f64, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub rect: Rect,
    /// Anchor to connect with a leader line when the label sits away from it.
    pub leader: Option<(f64, f64)>,
}

/// Ring of top-left candidates around an anchor: right, left, above, below, then farther.
pub fn ring(anchor: (f64, f64), w: f64, h: f64) -> Vec<(f64, f64)> {
    let (ax, ay) = anchor;
    let g = 8.;
    let mut out = Vec::new();
    for r in [1., 2.2, 3.6] {
        let d = g * r;
        out.extend([
            (ax + d, ay - h / 2.),
            (ax - d - w, ay - h / 2.),
            (ax - w / 2., ay - d - h),
            (ax - w / 2., ay + d),
            (ax + d, ay - d - h),
            (ax + d, ay + d),
            (ax - d - w, ay - d - h),
            (ax - d - w, ay + d),
        ]);
    }
    out
}

/// Place `items` inside `bounds` (w, h). `markers` are drawn symbols labels must not hide.
/// Returns one entry per item, `None` when hidden.
pub fn place(items: &[Item], bounds: (f64, f64), markers: &[Rect]) -> Vec<Option<Placed>> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| items[i].prio);
    let mut taken: Vec<Rect> = Vec::new();
    let mut out = vec![None; items.len()];
    for i in order {
        let it = &items[i];
        let cands = if it.cands.is_empty() { ring(it.anchor, it.w, it.h) } else { it.cands.clone() };
        let rects: Vec<Rect> = cands.iter().map(|&(x, y)| Rect { x, y, w: it.w, h: it.h }).collect();
        let free = |r: &Rect| {
            r.inside(bounds.0, bounds.1)
                && !taken.iter().any(|t| t.overlaps(r))
                && !markers.iter().any(|m| m.overlaps(r))
        };
        let chosen = rects.iter().copied().find(free).or_else(|| {
            if it.forced {
                // Least bad: first in-bounds candidate that avoids other labels, else any in bounds.
                rects
                    .iter()
                    .copied()
                    .find(|r| r.inside(bounds.0, bounds.1) && !taken.iter().any(|t| t.overlaps(r)))
                    .or_else(|| rects.iter().copied().find(|r| r.inside(bounds.0, bounds.1)))
                    .or_else(|| rects.first().copied())
            } else {
                None
            }
        });
        if let Some(rect) = chosen {
            let (nx, ny) = rect.nearest(it.anchor.0, it.anchor.1);
            let far = ((nx - it.anchor.0).powi(2) + (ny - it.anchor.1).powi(2)).sqrt() > 10.;
            let leader = (far && it.cands.is_empty()).then_some(it.anchor);
            taken.push(rect);
            out[i] = Some(Placed { rect, leader });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(ax: f64, ay: f64, prio: u8) -> Item {
        Item { w: 60., h: 18., anchor: (ax, ay), prio, forced: false, cands: vec![] }
    }
    fn no_overlap(p: &[Option<Placed>]) -> bool {
        let r: Vec<_> = p.iter().flatten().collect();
        r.iter().enumerate().all(|(i, a)| r.iter().skip(i + 1).all(|b| !a.rect.overlaps(&b.rect)))
    }
    #[test]
    fn colliding_labels_are_separated() {
        let items: Vec<_> = (0..6).map(|i| item(200. + i as f64 * 3., 200., i)).collect();
        let p = place(&items, (600., 400.), &[]);
        assert!(no_overlap(&p));
        assert!(p.iter().all(|x| x.is_some()));
    }
    #[test]
    fn priority_wins_the_preferred_spot() {
        let items = vec![item(100., 100., 5), item(100., 100., 0)];
        let p = place(&items, (600., 400.), &[]);
        let hi = p[1].unwrap();
        assert_eq!(hi.rect.x, 108.);
        assert!(hi.leader.is_none());
        assert!(p[0].unwrap().leader.is_some() || p[0].unwrap().rect.x != 108.);
    }
    #[test]
    fn low_priority_hidden_when_no_room_but_forced_kept() {
        let mut first = item(30., 10., 0);
        first.cands = vec![(10., 0.)];
        let mut items = vec![first];
        let mut a = item(30., 10., 3);
        a.cands = vec![(10., 0.)];
        let mut f = a.clone();
        f.forced = true;
        items.push(a);
        items.push(f);
        let p = place(&items, (80., 20.), &[]);
        assert!(p[0].is_some());
        assert!(p[1].is_none());
        assert!(p[2].is_some());
    }
    #[test]
    fn markers_are_not_covered() {
        let m = Rect { x: 108., y: 91., w: 60., h: 18. };
        let p = place(&[item(100., 100., 0)], (600., 400.), &[m]);
        assert!(!p[0].unwrap().rect.overlaps(&m));
    }
    #[test]
    fn stays_inside_bounds() {
        let p = place(&[item(595., 5., 0)], (600., 400.), &[]);
        let r = p[0].unwrap().rect;
        assert!(r.x >= 0. && r.y >= 0. && r.x + r.w <= 600. && r.y + r.h <= 400.);
    }
}

//! Pure logic of the scenario editor's Timeline tab: injects on one
//! chronological axis divided into turn bands (their actions are edited inside
//! each inject), the time a drag-and-drop lands on, and consistency warnings. No DOM here, so it is
//! tested headless.
use scenario::{InjectTemplate, ScenarioEdits};

/// Turn length used when the scenario does not set one.
pub const DEFAULT_TURN_MINUTES: u32 = scenario::DEFAULT_TURN_MINUTES;
/// More consecutive empty turns than this collapse into one band.
const MAX_EMPTY_RUN: usize = 2;

/// An inject of `msel` by position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub idx: usize,
}

/// One row of the chronological list.
#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    pub key: Key,
    /// Effective time, simulated seconds from the start.
    pub at_s: i64,
    /// `false` for an inject placed by its turn only (no `at_s`).
    pub explicit: bool,
}

/// A span of consecutive turns (usually one) and the entries inside it.
#[derive(Clone, PartialEq, Debug)]
pub struct Band {
    pub first_turn: u32,
    pub last_turn: u32,
    pub start_s: i64,
    pub end_s: i64,
    /// Positions in the entries slice.
    pub items: Vec<usize>,
}

/// Where a dragged row is released.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// On the header (or body) of the band with this index.
    Band(usize),
    /// On the upper half of the entry at this position.
    Before(usize),
    /// On the lower half of the entry at this position.
    After(usize),
}

/// Result of a drop: the new time and, for equal times, where in the list
/// it goes (`(index, before)`).
#[derive(Clone, PartialEq, Debug)]
pub struct Drop {
    pub at_s: i64,
    pub anchor: Option<(usize, bool)>,
}

/// A problem with an inject's action.
#[derive(Clone, PartialEq, Debug)]
pub enum Warn {
    /// A pre-action (`id`) whose instant falls before the exercise starts
    /// (the instant, in seconds from the start): it would run at the first step.
    BeforeStart(String, i64),
}

fn turn_len_s(tm: u32) -> i64 {
    tm.clamp(1, 1440) as i64 * 60
}

/// Turn (from 1) containing the instant.
pub fn turn_of(at_s: i64, tm: u32) -> u32 {
    (at_s.max(0) / turn_len_s(tm)) as u32 + 1
}

/// Start of the turn containing the instant.
pub fn turn_start(at_s: i64, tm: u32) -> i64 {
    at_s.max(0) / turn_len_s(tm) * turn_len_s(tm)
}

/// Time of an inject without `at_s`: the start of its suggested turn.
pub fn implied_s(turn: u32, tm: u32) -> i64 {
    (turn.max(1) as i64 - 1) * turn_len_s(tm)
}

pub fn inject_s(m: &InjectTemplate, tm: u32) -> i64 {
    m.at_s.unwrap_or_else(|| implied_s(m.turn, tm)).max(0)
}

/// Injects by time; at equal times, list order.
pub fn sorted_entries(msel: &[InjectTemplate], tm: u32) -> Vec<Entry> {
    let mut v: Vec<Entry> = msel
        .iter()
        .enumerate()
        .map(|(idx, m)| Entry { key: Key { idx }, at_s: inject_s(m, tm), explicit: m.at_s.is_some() })
        .collect();
    v.sort_by_key(|e| (e.at_s, e.key.idx));
    v
}

/// Turn bands covering every entry plus one empty turn after the last, so an
/// item can always be dropped beyond the end. Long runs of empty turns
/// collapse into a single band.
pub fn bands(entries: &[Entry], tm: u32) -> Vec<Band> {
    let len = turn_len_s(tm);
    let last = entries.iter().map(|e| turn_of(e.at_s, tm)).max().unwrap_or(1) + 1;
    let mut singles: Vec<Band> = (1..=last)
        .map(|t| Band { first_turn: t, last_turn: t, start_s: (t as i64 - 1) * len, end_s: t as i64 * len, items: vec![] })
        .collect();
    for (pos, e) in entries.iter().enumerate() {
        singles[(turn_of(e.at_s, tm) - 1) as usize].items.push(pos);
    }
    let n = singles.len();
    let mut out: Vec<Band> = Vec::new();
    let mut k = 0;
    while k < n {
        let mut j = k;
        // The trailing turn stays on its own: it is the "drop beyond the end" target.
        if singles[k].items.is_empty() {
            while j + 1 < n - 1 && singles[j + 1].items.is_empty() {
                j += 1;
            }
        }
        if j - k + 1 > MAX_EMPTY_RUN {
            out.push(Band { first_turn: singles[k].first_turn, last_turn: singles[j].last_turn, start_s: singles[k].start_s, end_s: singles[j].end_s, items: vec![] });
        } else {
            out.extend(singles[k..=j].iter().cloned());
        }
        k = j + 1;
    }
    out
}

/// The time a drop gives, and where the item goes among equal times.
///
/// Rules: on a band header: the band's start. On the upper half of a row
/// ("between the previous row and this one"): the previous row's time when it
/// is in the same band, otherwise the band's start. On the lower half: the
/// row's own time, placed after it. Dropping a row on itself changes nothing.
pub fn resolve_drop(entries: &[Entry], bands: &[Band], dragged: Key, target: Target, tm: u32) -> Option<Drop> {
    let me = entries.iter().find(|e| e.key == dragged)?;
    match target {
        Target::Band(b) => Some(Drop { at_s: bands.get(b)?.start_s, anchor: None }),
        Target::Before(pos) => {
            let next = entries.get(pos)?;
            if next.key == dragged {
                return Some(Drop { at_s: me.at_s, anchor: None });
            }
            let prev = entries[..pos].iter().rev().find(|e| e.key != dragged);
            let band_start = turn_start(next.at_s, tm);
            let at_s = match prev {
                Some(p) if turn_start(p.at_s, tm) == band_start => p.at_s,
                _ => band_start,
            };
            let anchor = if next.at_s == at_s {
                Some((next.key.idx, true))
            } else {
                prev.filter(|p| p.at_s == at_s).map(|p| (p.key.idx, false))
            };
            Some(Drop { at_s, anchor })
        }
        Target::After(pos) => {
            let prev = entries.get(pos)?;
            if prev.key == dragged {
                return Some(Drop { at_s: me.at_s, anchor: None });
            }
            Some(Drop { at_s: prev.at_s, anchor: Some((prev.key.idx, false)) })
        }
    }
}

/// Moves `list[from]` next to `list[anchor]` (before or after it) and returns
/// its new index.
pub fn move_next_to<T>(list: &mut Vec<T>, from: usize, anchor: usize, before: bool) -> usize {
    if from >= list.len() || anchor >= list.len() || from == anchor {
        return from.min(list.len().saturating_sub(1));
    }
    let item = list.remove(from);
    let a = if anchor > from { anchor - 1 } else { anchor };
    let at = if before { a } else { a + 1 };
    list.insert(at, item);
    at
}

/// Sets the time of an inject: it becomes explicit and takes the turn that
/// contains the instant.
pub fn set_time(edits: &mut ScenarioEdits, key: Key, at_s: i64) {
    let tm = edits.turn_minutes.unwrap_or(DEFAULT_TURN_MINUTES);
    let at_s = at_s.max(0);
    if let Some(m) = edits.msel.as_mut().and_then(|l| l.get_mut(key.idx)) {
        m.at_s = Some(at_s);
        m.turn = turn_of(at_s, tm);
    }
}

/// Applies a drop: new time, then the position among equal times. Returns the
/// inject's new index.
pub fn apply_drop(edits: &mut ScenarioEdits, key: Key, drop: &Drop) -> usize {
    set_time(edits, key, drop.at_s);
    match drop.anchor {
        Some((a, before)) => edits.msel.as_mut().map_or(key.idx, |l| move_next_to(l, key.idx, a, before)),
        None => key.idx,
    }
}

/// Changes the scenario's turn length; injects with an explicit time keep
/// their instant and follow it to the turn that now contains it.
pub fn set_turn_minutes(edits: &mut ScenarioEdits, tm: u32) {
    let tm = tm.clamp(1, 1440);
    edits.turn_minutes = Some(tm);
    for m in edits.msel.iter_mut().flatten() {
        if let Some(t) = m.at_s {
            m.turn = turn_of(t, tm);
        }
    }
}

/// Gives every inject an explicit time (the one the turn implies).
pub fn make_explicit(edits: &mut ScenarioEdits) -> usize {
    let tm = edits.turn_minutes.unwrap_or(DEFAULT_TURN_MINUTES);
    let mut n = 0;
    for m in edits.msel.iter_mut().flatten() {
        if m.at_s.is_none() {
            m.at_s = Some(implied_s(m.turn, tm));
            n += 1;
        }
    }
    n
}

/// Sorts the injects by time (stable). Returns the new index of each old index.
pub fn sort_by_time(edits: &mut ScenarioEdits) -> Vec<usize> {
    let tm = edits.turn_minutes.unwrap_or(DEFAULT_TURN_MINUTES);
    let mut inj = vec![];
    if let Some(l) = edits.msel.as_mut() {
        let mut order: Vec<usize> = (0..l.len()).collect();
        order.sort_by_key(|&i| inject_s(&l[i], tm));
        let mut map = vec![0; l.len()];
        for (new, &old) in order.iter().enumerate() {
            map[old] = new;
        }
        let sorted: Vec<_> = order.iter().map(|&i| l[i].clone()).collect();
        *l = sorted;
        inj = map;
    }
    inj
}

/// Time for a new item: the start of the selected band, otherwise the time of
/// the last entry (the end of the timeline).
pub fn new_item_time(entries: &[Entry], bands: &[Band], band: Option<usize>) -> i64 {
    match band.and_then(|b| bands.get(b)) {
        Some(b) => b.start_s,
        None => entries.iter().map(|e| e.at_s).max().unwrap_or(0),
    }
}

/// Problems with the actions of an inject (none when it is fine).
pub fn inject_warnings(m: &InjectTemplate, tm: u32) -> Vec<Warn> {
    let t = inject_s(m, tm);
    m.actions
        .iter()
        .filter(|a| a.offset_s < 0 && t + a.offset_s < 0)
        .map(|a| Warn::BeforeStart(a.id.clone(), t + a.offset_s))
        .collect()
}

/// `HH:MM` of an instant of the scenario clock (`start_clock_s + at_s`).
pub fn hhmm(abs_s: i64) -> String {
    format!("{:02}:{:02}", abs_s.div_euclid(3600).rem_euclid(24), abs_s.div_euclid(60).rem_euclid(60))
}

/// Band title range, e.g. `13:30–14:00` (with the day once past day 1).
pub fn band_range(start_clock_s: i64, b: &Band) -> String {
    let day = |s: i64| s.div_euclid(86_400) + 1;
    let (a, z) = (start_clock_s + b.start_s, start_clock_s + b.end_s);
    let prefix = if day(a) > 1 { format!("G{} ", day(a)) } else { String::new() };
    let suffix = if day(z) != day(a) { format!(" (G{})", day(z)) } else { String::new() };
    format!("{prefix}{}–{}{suffix}", hhmm(a), hhmm(z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inj(id: &str, turn: u32, at: Option<i64>) -> InjectTemplate {
        InjectTemplate {
            id: id.into(),
            turn,
            title: id.into(),
            sender: String::new(),
            recipients: vec!["all".into()],
            text: "x".into(),
            objective: String::new(),
            event: String::new(),
            at_s: at,
            actions: vec![],
        }
    }

    fn edits(msel: Vec<InjectTemplate>, tm: u32) -> ScenarioEdits {
        ScenarioEdits { msel: Some(msel), turn_minutes: Some(tm), ..Default::default() }
    }

    fn k(idx: usize) -> Key {
        Key { idx }
    }

    #[test]
    fn turns_and_implied_times_follow_the_turn_length() {
        assert_eq!(turn_of(0, 30), 1);
        assert_eq!(turn_of(1799, 30), 1);
        assert_eq!(turn_of(1800, 30), 2);
        assert_eq!(turn_of(1800, 15), 3);
        assert_eq!(implied_s(3, 30), 3600);
        assert_eq!(implied_s(0, 30), 0);
        assert_eq!(turn_start(2000, 30), 1800);
    }

    #[test]
    fn entries_sort_by_time_then_list_order() {
        let m = vec![inj("I-02", 2, None), inj("I-01", 1, Some(0)), inj("I-03", 1, Some(0))];
        let e = sorted_entries(&m, 30);
        let order: Vec<_> = e.iter().map(|x| x.key.idx).collect();
        assert_eq!(order, vec![1, 2, 0]);
        assert!(!e[2].explicit && e[2].at_s == 1800);
    }

    #[test]
    fn bands_show_gaps_and_a_trailing_empty_turn() {
        let m = vec![inj("I-01", 1, Some(0)), inj("I-02", 1, Some(2400)), inj("I-03", 1, Some(2 * 1800))];
        let e = sorted_entries(&m, 30);
        let b = bands(&e, 30);
        // turns 1, 2, 3 and the empty 4th
        assert_eq!(b.len(), 4);
        assert_eq!(b[0].items.len(), 1);
        assert_eq!(b[1].items.len(), 1);
        assert_eq!((b[2].first_turn, b[2].items.len()), (3, 1));
        assert!(b[3].items.is_empty());
        assert_eq!(band_range(13 * 3600, &b[1]), "13:30–14:00");
    }

    #[test]
    fn long_empty_runs_collapse() {
        let m = vec![inj("I-01", 1, Some(0)), inj("I-02", 1, Some(10 * 1800))];
        let e = sorted_entries(&m, 30);
        let b = bands(&e, 30);
        // 1 | 2..10 collapsed | 11 | 12
        assert_eq!(b.len(), 4);
        assert_eq!((b[1].first_turn, b[1].last_turn), (2, 10));
        assert_eq!(b[1].start_s, 1800);
        assert_eq!(b[1].end_s, 10 * 1800);
        // Few empty turns stay separate.
        let m = vec![inj("I-01", 1, Some(0)), inj("I-02", 1, Some(2 * 1800))];
        let b = bands(&sorted_entries(&m, 30), 30);
        assert_eq!(b.len(), 4);
    }

    #[test]
    fn drop_rules() {
        // I-01 @0, I-02 @900, I-03 @1800 (band 2), I-04 @2400
        let m = vec![inj("I-01", 1, Some(0)), inj("I-02", 1, Some(900)), inj("I-03", 2, Some(1800)), inj("I-04", 2, Some(2400))];
        let e = sorted_entries(&m, 30);
        let b = bands(&e, 30);
        let pos = |idx| e.iter().position(|x| x.key == k(idx)).unwrap();
        let i04 = k(3);
        // On a band header: the band start.
        assert_eq!(resolve_drop(&e, &b, i04, Target::Band(0), 30).unwrap().at_s, 0);
        // First in its band: the band start.
        let d = resolve_drop(&e, &b, i04, Target::Before(pos(2)), 30).unwrap();
        assert_eq!((d.at_s, d.anchor), (1800, Some((2, true))), "I-03 is first in band 2: band start");
        let d = resolve_drop(&e, &b, k(0), Target::Before(pos(3)), 30).unwrap();
        assert_eq!((d.at_s, d.anchor), (1800, Some((2, false))), "previous row (I-03) is in the same band");
        // Lower half: the row's own time, after it.
        let d = resolve_drop(&e, &b, i04, Target::After(pos(1)), 30).unwrap();
        assert_eq!((d.at_s, d.anchor), (900, Some((1, false))));
        // First row of the timeline: band start.
        let d = resolve_drop(&e, &b, i04, Target::Before(0), 30).unwrap();
        assert_eq!(d.at_s, 0);
        // Dropping on itself is a no-op.
        let d = resolve_drop(&e, &b, i04, Target::Before(pos(3)), 30).unwrap();
        assert_eq!((d.at_s, d.anchor), (2400, None));
    }

    #[test]
    fn applying_a_drop_updates_time_turn_and_order() {
        let m = vec![inj("I-01", 1, Some(0)), inj("I-02", 1, Some(0)), inj("I-03", 1, None)];
        let mut ed = edits(m, 30);
        // I-03 (implicit, turn 1) dropped after I-01 → explicit 0, between I-01 and I-02.
        let drop = Drop { at_s: 0, anchor: Some((0, false)) };
        let new = apply_drop(&mut ed, k(2), &drop);
        assert_eq!(new, 1);
        let ids: Vec<_> = ed.msel.as_ref().unwrap().iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["I-01", "I-03", "I-02"]);
        assert_eq!(ed.msel.as_ref().unwrap()[1].at_s, Some(0));
        // Moving into the third turn re-derives the turn.
        set_time(&mut ed, k(0), 2 * 1800 + 60);
        let m0 = &ed.msel.as_ref().unwrap()[0];
        assert_eq!((m0.at_s, m0.turn), (Some(3660), 3));
    }

    #[test]
    fn move_next_to_handles_both_directions() {
        let mut v = vec!['a', 'b', 'c', 'd'];
        assert_eq!(move_next_to(&mut v, 0, 2, false), 2);
        assert_eq!(v, ['b', 'c', 'a', 'd']);
        assert_eq!(move_next_to(&mut v, 3, 0, true), 0);
        assert_eq!(v, ['d', 'b', 'c', 'a']);
        assert_eq!(move_next_to(&mut v, 1, 1, true), 1);
    }

    #[test]
    fn changing_turn_length_keeps_explicit_times_and_makes_times_explicit() {
        let m = vec![inj("I-01", 2, Some(2400)), inj("I-02", 3, None)];
        let mut ed = edits(m, 30);
        set_turn_minutes(&mut ed, 20);
        let l = ed.msel.as_ref().unwrap();
        assert_eq!((l[0].at_s, l[0].turn), (Some(2400), 3));
        assert_eq!((l[1].at_s, l[1].turn), (None, 3), "implicit ones follow the turn");
        assert_eq!(make_explicit(&mut ed), 1);
        assert_eq!(ed.msel.as_ref().unwrap()[1].at_s, Some(2 * 20 * 60));
        set_turn_minutes(&mut ed, 5000);
        assert_eq!(ed.turn_minutes, Some(1440));
    }

    #[test]
    fn sorting_by_time_reports_where_items_went() {
        let m = vec![inj("I-01", 3, None), inj("I-02", 1, Some(0)), inj("I-03", 2, Some(1900))];
        let mut ed = edits(m, 30);
        let im = sort_by_time(&mut ed);
        let ids: Vec<_> = ed.msel.as_ref().unwrap().iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["I-02", "I-03", "I-01"]);
        assert_eq!(im, vec![2, 0, 1]);
    }

    #[test]
    fn warns_when_a_pre_action_falls_before_the_start() {
        let act = |id: &str, off: i64| scenario::InjectAction { id: id.into(), title: id.into(), command: serde_json::json!({}), offset_s: off, note: String::new() };
        let mut m = inj("I-01", 1, Some(600));
        m.actions = vec![act("ok", -300), act("early", -900), act("post", 60)];
        assert_eq!(inject_warnings(&m, 30), vec![Warn::BeforeStart("early".into(), -300)]);
    }

    #[test]
    fn new_items_go_to_the_selected_band_or_the_end() {
        let m = vec![inj("I-01", 1, Some(100)), inj("I-02", 1, Some(2500))];
        let e = sorted_entries(&m, 30);
        let b = bands(&e, 30);
        assert_eq!(new_item_time(&e, &b, None), 2500);
        assert_eq!(new_item_time(&e, &b, Some(1)), 1800);
        assert_eq!(new_item_time(&[], &bands(&[], 30), None), 0);
    }

    #[test]
    fn clock_helpers_wrap_days() {
        assert_eq!(hhmm(13 * 3600 + 5 * 60), "13:05");
        let b = Band { first_turn: 1, last_turn: 1, start_s: 0, end_s: 1800, items: vec![] };
        assert_eq!(band_range(23 * 3600 + 45 * 60, &b), "23:45–00:15 (G2)");
    }
}

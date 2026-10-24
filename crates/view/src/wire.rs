//! The compact wire form of the view: differential and compressed.
//!
//! A viewer that already holds generation *g* asks for what changed since. The
//! sender keeps the last few states it has sent ([`FrameCache`]) and answers
//! with a [`ViewDelta`]: every small field in full, but the four grid arrays
//! (the bulk of the state) only where they differ from *g*, as gap-coded sparse
//! patches. A viewer whose base is unknown to the sender gets a full state.
//! Either is JSON deflated with zlib, which browsers inflate natively
//! (`DecompressionStream("deflate")`).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use serde::{Deserialize, Serialize};

use crate::{RoadSegment, ViewState};

/// A fixed-width grid element, compared and sent as its little-endian bytes
/// (so NaN is just another value).
trait Elem: Copy {
    const W: usize;
    fn put(self, out: &mut Vec<u8>);
    fn get(b: &[u8]) -> Self;
}
impl Elem for u8 {
    const W: usize = 1;
    fn put(self, out: &mut Vec<u8>) {
        out.push(self)
    }
    fn get(b: &[u8]) -> Self {
        b[0]
    }
}
impl Elem for i32 {
    const W: usize = 4;
    fn put(self, out: &mut Vec<u8>) {
        out.extend(self.to_le_bytes())
    }
    fn get(b: &[u8]) -> Self {
        i32::from_le_bytes(b.try_into().expect("width"))
    }
}
impl Elem for f32 {
    const W: usize = 4;
    fn put(self, out: &mut Vec<u8>) {
        out.extend(self.to_le_bytes())
    }
    fn get(b: &[u8]) -> Self {
        f32::from_le_bytes(b.try_into().expect("width"))
    }
}

fn bytes<T: Elem>(v: &[T]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * T::W);
    v.iter().for_each(|x| x.put(&mut out));
    out
}

/// One grid array: whole, or only its changes against the base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Patch {
    /// Base64 of all elements (no usable base, or the length changed).
    Full(String),
    /// Same length as the base. `gaps[i]` is the distance from the previous
    /// changed index (from 0 for the first); `data` holds the new elements.
    Sparse { gaps: Vec<u32>, data: String },
}

impl Patch {
    fn diff<T: Elem>(old: &[T], new: &[T]) -> Patch {
        if old.len() != new.len() {
            return Patch::Full(STANDARD.encode(bytes(new)));
        }
        let (old, new) = (bytes(old), bytes(new));
        let (mut gaps, mut data, mut prev) = (Vec::new(), Vec::new(), 0usize);
        for (i, (a, b)) in old.chunks_exact(T::W).zip(new.chunks_exact(T::W)).enumerate() {
            if a != b {
                gaps.push((i - prev) as u32);
                data.extend_from_slice(b);
                prev = i;
            }
        }
        Patch::Sparse {
            gaps,
            data: STANDARD.encode(data),
        }
    }

    fn apply<T: Elem>(&self, base: &[T]) -> Result<Vec<T>, String> {
        match self {
            Patch::Full(text) => {
                let raw = STANDARD.decode(text).map_err(|e| e.to_string())?;
                if raw.len() % T::W != 0 {
                    return Err("packed array has a partial element".into());
                }
                Ok(raw.chunks_exact(T::W).map(T::get).collect())
            }
            Patch::Sparse { gaps, data } => {
                let raw = STANDARD.decode(data).map_err(|e| e.to_string())?;
                if raw.len() != gaps.len() * T::W {
                    return Err("patch data does not match its indices".into());
                }
                let mut out = base.to_vec();
                let mut at = 0usize;
                for (gap, chunk) in gaps.iter().zip(raw.chunks_exact(T::W)) {
                    at += *gap as usize;
                    *out.get_mut(at).ok_or("patch index outside the grid")? = T::get(chunk);
                }
                Ok(out)
            }
        }
    }
}

/// What changed since generation `base`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewDelta {
    /// The generation this applies to.
    pub base: u64,
    /// The new state, with its grid arrays left empty (see the patches).
    pub state: ViewState,
    pub cells: Patch,
    pub arrival: Patch,
    pub intensity: Patch,
    pub probability: Patch,
    /// Road segment geometry never changes during an exercise, so it is sent
    /// only when it differs from the base (else `None` and `road_flags` apply).
    pub road_geometry: Option<Vec<RoadSegment>>,
    /// Per segment: bit 0 closed, bit 1 cut by fire.
    pub road_flags: Patch,
}

fn same_geometry(a: &[RoadSegment], b: &[RoadSegment]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.road == y.road && x.from == y.from && x.to == y.to)
}

fn flags(segments: &[RoadSegment]) -> Vec<u8> {
    segments
        .iter()
        .map(|s| s.closed as u8 | (s.cut_by_fire as u8) << 1)
        .collect()
}

pub fn diff(base: &ViewState, new: &ViewState) -> ViewDelta {
    ViewDelta {
        base: base.generation,
        cells: Patch::diff(&base.cells, &new.cells),
        arrival: Patch::diff(&base.arrival, &new.arrival),
        intensity: Patch::diff(&base.intensity, &new.intensity),
        probability: Patch::diff(&base.probability, &new.probability),
        road_geometry: (!same_geometry(&base.road_segments, &new.road_segments))
            .then(|| new.road_segments.clone()),
        road_flags: Patch::diff(&flags(&base.road_segments), &flags(&new.road_segments)),
        state: ViewState {
            road_segments: vec![],
            cells: vec![],
            arrival: vec![],
            intensity: vec![],
            probability: vec![],
            ..new.clone()
        },
    }
}

pub fn apply(base: &ViewState, d: &ViewDelta) -> Result<ViewState, String> {
    if base.generation != d.base {
        return Err(format!(
            "delta is against generation {}, not {}",
            d.base, base.generation
        ));
    }
    let mut road_segments = match &d.road_geometry {
        Some(segments) => segments.clone(),
        None => base.road_segments.clone(),
    };
    let road_flags = d.road_flags.apply::<u8>(&flags(&base.road_segments))?;
    if road_flags.len() != road_segments.len() {
        return Err("road flags do not match the segments".into());
    }
    for (segment, flag) in road_segments.iter_mut().zip(road_flags) {
        segment.closed = flag & 1 != 0;
        segment.cut_by_fire = flag & 2 != 0;
    }
    Ok(ViewState {
        road_segments,
        cells: d.cells.apply(&base.cells)?,
        arrival: d.arrival.apply(&base.arrival)?,
        intensity: d.intensity.apply(&base.intensity)?,
        probability: d.probability.apply(&base.probability)?,
        ..d.state.clone()
    })
}

/// A frame as sent: a whole state or a delta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    Full(Box<ViewState>),
    Delta(Box<ViewDelta>),
}

/// Zlib-deflate `raw`.
pub fn compress(raw: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
    z.write_all(raw).expect("in-memory write");
    z.finish().expect("in-memory write")
}

/// Inflate zlib `bytes`, refusing absurd sizes.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    ZlibDecoder::new(bytes)
        .take(MAX_FRAME)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    Ok(raw)
}

pub fn encode(frame: &Frame) -> Vec<u8> {
    compress(&serde_json::to_vec(frame).expect("view frames serialize"))
}

/// Largest inflated frame accepted, a guard against decompression bombs.
const MAX_FRAME: u64 = 512 * 1024 * 1024;

pub fn decode(bytes: &[u8]) -> Result<Frame, String> {
    serde_json::from_slice(&decompress(bytes)?).map_err(|e| e.to_string())
}

/// Sender side: remembers recent states so a viewer's generation can be diffed.
#[derive(Default)]
pub struct FrameCache {
    recent: VecDeque<Arc<ViewState>>,
}

impl FrameCache {
    /// How many sent states to keep as diff bases.
    const KEEP: usize = 16;

    /// The frame bringing a viewer at `have_generation` up to `new`.
    pub fn frame(&mut self, new: ViewState, have_generation: u64) -> Frame {
        let base = (have_generation != 0)
            .then(|| self.recent.iter().find(|s| s.generation == have_generation))
            .flatten();
        let frame = match base {
            Some(base) => Frame::Delta(Box::new(diff(base, &new))),
            None => Frame::Full(Box::new(new.clone())),
        };
        if !self.recent.iter().any(|s| s.generation == new.generation) {
            // Cues are per request, not part of what a later delta is built on.
            self.recent.push_back(Arc::new(ViewState {
                cues: vec![],
                ..new
            }));
            if self.recent.len() > Self::KEEP {
                self.recent.pop_front();
            }
        }
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(generation: u64, n: usize) -> ViewState {
        ViewState {
            generation,
            clock: format!("g{generation}"),
            cells: (0..n).map(|i| (i % 3) as u8).collect(),
            arrival: (0..n as i32).collect(),
            intensity: (0..n).map(|i| i as f32 * 0.5).collect(),
            probability: vec![f32::NAN; n],
            road_segments: (0..n.min(50))
                .map(|i| RoadSegment {
                    road: i % 3,
                    from: [i as f32, 0.0],
                    to: [i as f32, 9.0],
                    closed: generation % 2 == 0 && i == 3,
                    cut_by_fire: generation > 1 && i == 4,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn same(a: &ViewState, b: &ViewState) -> bool {
        // NaN != NaN: compare the packed form.
        serde_json::to_string(a).unwrap() == serde_json::to_string(b).unwrap()
    }

    #[test]
    fn delta_reproduces_the_new_state_and_is_smaller() {
        let base = state(1, 50_000);
        let mut new = state(2, 50_000);
        new.cells[10] = 1;
        new.cells[49_999] = 2;
        new.intensity[777] = 1234.5;
        let d = diff(&base, &new);
        assert!(same(&apply(&base, &d).unwrap(), &new));
        let full = encode(&Frame::Full(Box::new(new.clone())));
        let delta = encode(&Frame::Delta(Box::new(d)));
        assert!(delta.len() * 20 < full.len(), "{} vs {}", delta.len(), full.len());
        let Frame::Delta(back) = decode(&delta).unwrap() else {
            panic!("not a delta")
        };
        assert!(same(&apply(&base, &back).unwrap(), &new));
    }

    #[test]
    fn resized_grid_and_wrong_base_are_handled() {
        let (base, new) = (state(1, 10), state(2, 20));
        assert!(same(&apply(&base, &diff(&base, &new)).unwrap(), &new));
        assert!(apply(&state(9, 10), &diff(&base, &new)).is_err());
    }

    #[test]
    fn hostile_patches_are_errors() {
        let base = state(1, 4);
        let mut d = diff(&base, &state(2, 4));
        d.cells = Patch::Sparse {
            gaps: vec![99],
            data: STANDARD.encode([1u8]),
        };
        assert!(apply(&base, &d).is_err());
        d.cells = Patch::Sparse {
            gaps: vec![0, 1],
            data: STANDARD.encode([1u8]),
        };
        assert!(apply(&base, &d).is_err());
        assert!(decode(b"not zlib").is_err());
        assert_eq!(decompress(&compress(b"scene")).unwrap(), b"scene");
    }

    #[test]
    fn cache_sends_full_then_deltas_and_full_for_unknown_base() {
        let mut cache = FrameCache::default();
        assert!(matches!(cache.frame(state(1, 8), 0), Frame::Full(_)));
        assert!(matches!(cache.frame(state(2, 8), 1), Frame::Delta(_)));
        assert!(matches!(cache.frame(state(3, 8), 77), Frame::Full(_)));
        for g in 4..40 {
            cache.frame(state(g, 8), 0);
        }
        // Old bases fall out of the ring.
        assert!(matches!(cache.frame(state(40, 8), 1), Frame::Full(_)));
    }
}

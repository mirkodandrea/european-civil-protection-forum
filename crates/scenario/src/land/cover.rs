//! Land cover at the render resolution (5 m): what the ground *is* — road,
//! building, paved yard, irrigated plot — finer than the 20 m fuel raster can
//! say. Graphics only: the fire never reads it. Ported from `propagator_abm`.

use super::Pos;

/// Cover classes, as coded in `cover.u8` (the source's `tools/factory/fine.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CoverClass {
    Natural = 0,
    Road = 1,
    Track = 2,
    Building = 3,
    Yard = 4,
    Irrigated = 5,
    Water = 6,
}

impl CoverClass {
    fn from_u8(v: u8) -> CoverClass {
        match v {
            1 => CoverClass::Road,
            2 => CoverClass::Track,
            3 => CoverClass::Building,
            4 => CoverClass::Yard,
            5 => CoverClass::Irrigated,
            6 => CoverClass::Water,
            _ => CoverClass::Natural,
        }
    }
}

/// Row 0 is the north edge; cell (r, c) spans x in [c, c+1)·cell_m.
pub struct Cover {
    pub rows: usize,
    pub cols: usize,
    pub cell_m: f32,
    pub height_m: f32,
    classes: Vec<u8>,
}

impl Cover {
    pub fn new(rows: usize, cols: usize, cell_m: f32, height_m: f32, classes: Vec<u8>) -> Cover {
        Cover { rows, cols, cell_m, height_m, classes }
    }

    pub fn at(&self, p: Pos) -> CoverClass {
        let c = (p.x / self.cell_m).floor().clamp(0.0, (self.cols - 1) as f32) as usize;
        let r = ((self.height_m - p.y) / self.cell_m)
            .floor()
            .clamp(0.0, (self.rows - 1) as f32) as usize;
        CoverClass::from_u8(self.classes[r * self.cols + c])
    }

    pub fn is_natural(&self, p: Pos) -> bool {
        self.at(p) == CoverClass::Natural
    }
}

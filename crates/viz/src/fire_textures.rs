//! Procedural textures for the fire.
//!
//! Flat quads are what make particle fire look like cardboard: the silhouette
//! is a rectangle and the edges are hard lines. Everything here exists to give
//! the fire soft, irregular edges without shipping any image assets — the
//! textures are generated at startup from value noise, so the whole scenario
//! stays reproducible from code and data alone.
//!
//! All three are alpha masks; colour comes from the mesh's vertex colours, so
//! one texture serves flames of any temperature.

use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SIZE: u32 = 64;

/// Value noise on a lattice, smoothed — enough structure to break up an edge,
/// cheap enough to generate a handful of 64² textures at startup.
struct Noise(u64);

impl Noise {
    fn at(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        // Smoothstep the interpolant so the lattice does not show as a grid.
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let c = |ix: f32, iy: f32| {
            let mut h = self.0
                ^ (ix as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (iy as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
            h ^= h >> 29;
            h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            h ^= h >> 32;
            (h >> 40) as f32 / (1u32 << 24) as f32
        };
        let top = c(x0, y0) * (1.0 - sx) + c(x0 + 1.0, y0) * sx;
        let bot = c(x0, y0 + 1.0) * (1.0 - sx) + c(x0 + 1.0, y0 + 1.0) * sx;
        top * (1.0 - sy) + bot * sy
    }

    /// Two octaves is plenty at 64²; more just adds shimmer.
    fn fbm(&self, x: f32, y: f32) -> f32 {
        self.at(x, y) * 0.65 + self.at(x * 2.3, y * 2.3) * 0.35
    }
}

fn image(pixels: Vec<u8>) -> Image {
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A flame tongue: wide and solid at the base, tapering and dissolving toward
/// the tip, with a noisy edge so no two billboards share a silhouette.
pub fn flame_tongue() -> Image {
    let noise = Noise(0x51A3_7C11);
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        // Image row 0 is the top of the quad (uv.y = 0): v = 0 at the tip,
        // 1 at the base.
        let v = y as f32 / (SIZE - 1) as f32;
        for x in 0..SIZE {
            let u = x as f32 / (SIZE - 1) as f32 * 2.0 - 1.0;
            // The tongue narrows with height, and the noise wobbles the edge.
            // The geometry already tapers the ribbon; the texture only softens it.
            let width = 0.92;
            let edge = (1.0 - (u.abs() / width).powf(1.6)).clamp(0.0, 1.0);
            let turbulence = 0.55 + 0.9 * noise.fbm(u * 3.0 + 8.0, v * 5.0);
            // Fade the very base too, so a tongue never shows a cut-off line
            // where it meets the ground.
            let foot = (v * 6.0).clamp(0.0, 1.0);
            // The tip dissolves to nothing before the quad's top edge, so the
            // flame never ends in a straight cut.
            let tip = (v / 0.35).clamp(0.0, 1.0);
            let a = (edge * turbulence * foot * tip * (0.5 + 0.6 * v)).clamp(0.0, 1.0);
            // Hotter (whiter) toward the base and the core of the tongue.
            let heat = (a * (0.4 + 0.6 * v)).clamp(0.0, 1.0);
            pixels.extend_from_slice(&[
                255,
                (110.0 + 145.0 * heat) as u8,
                (30.0 + 90.0 * heat * heat) as u8,
                (a * 255.0) as u8,
            ]);
        }
    }
    image(pixels)
}

/// A soft irregular puff: smoke, and (scaled down) embers.
pub fn puff() -> Image {
    let noise = Noise(0x9C4D_2E77);
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        let v = y as f32 / (SIZE - 1) as f32 * 2.0 - 1.0;
        for x in 0..SIZE {
            let u = x as f32 / (SIZE - 1) as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            // Radial falloff, with the radius itself perturbed by noise so the
            // puff is a cloud rather than a disc.
            let lumpy = r * (0.75 + 0.5 * noise.fbm(u * 2.5 + 3.0, v * 2.5 + 7.0));
            let a = (1.0 - lumpy).clamp(0.0, 1.0).powf(1.8);
            let shade = (0.6 + 0.4 * noise.fbm(u * 4.0, v * 4.0)) * 255.0;
            pixels.extend_from_slice(&[shade as u8, shade as u8, shade as u8, (a * 255.0) as u8]);
        }
    }
    image(pixels)
}

/// A tight glowing dot for embers: same idea, much harder falloff.
pub fn spark() -> Image {
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        let v = y as f32 / (SIZE - 1) as f32 * 2.0 - 1.0;
        for x in 0..SIZE {
            let u = x as f32 / (SIZE - 1) as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let a = (1.0 - r).clamp(0.0, 1.0).powf(3.5);
            pixels.extend_from_slice(&[
                255,
                (140.0 + 115.0 * a) as u8,
                (40.0 * a) as u8,
                (a * 255.0) as u8,
            ]);
        }
    }
    image(pixels)
}

//! Rectangular TTX footprints dressed with the source game's masonry, eaves,
//! hipped roofs, glazing and shutters. This is presentation, not added premises.
use crate::materials::surface_alpha;
use bevy::prelude::*;
use bevy::render::{
    mesh::{Indices, PrimitiveTopology},
    render_asset::RenderAssetUsages,
};
#[derive(Default)]
struct Builder {
    p: Vec<[f32; 3]>,
    n: Vec<[f32; 3]>,
    c: Vec<[f32; 4]>,
    ix: Vec<u32>,
}
impl Builder {
    fn quad(&mut self, v: [Vec3; 4], colour: [f32; 3], surface: u8) {
        let n = (v[1] - v[0])
            .cross(v[2] - v[0])
            .normalize_or_zero()
            .to_array();
        let off = self.p.len() as u32;
        for p in v {
            self.p.push(p.to_array());
            self.n.push(n);
            self.c
                .push([colour[0], colour[1], colour[2], surface_alpha(surface)]);
        }
        self.ix
            .extend([off, off + 1, off + 2, off, off + 2, off + 3]);
    }
    fn mesh(self) -> Mesh {
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        let len = self.p.len();
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.p);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.n);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.c);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; len]);
        m.insert_indices(Indices::U32(self.ix));
        m
    }
}
fn linear(c: [f32; 3]) -> [f32; 3] {
    let c = Color::srgb(c[0], c[1], c[2]).to_linear();
    [c.red, c.green, c.blue]
}
pub fn building(size: [f32; 2], height: f32, seed: usize, house: bool) -> Mesh {
    let mut b = Builder::default();
    let (x, z) = (size[0] * 0.5, size[1] * 0.5);
    let wall = linear(if house {
        [
            [0.84, 0.76, 0.61],
            [0.88, 0.82, 0.70],
            [0.76, 0.66, 0.54],
            [0.88, 0.78, 0.66],
        ][seed % 4]
    } else {
        [0.78, 0.79, 0.74]
    });
    let roof = linear([[0.53, 0.25, 0.16], [0.63, 0.34, 0.23], [0.48, 0.27, 0.20]][seed % 3]);
    let stone = linear([0.64, 0.60, 0.52]);
    let glass = linear([0.12, 0.20, 0.23]);
    let shutter = linear([0.18, 0.27, 0.21]);
    let ring = [
        Vec3::new(-x, 0., z),
        Vec3::new(x, 0., z),
        Vec3::new(x, 0., -z),
        Vec3::new(-x, 0., -z),
    ];
    for i in 0..4 {
        let a = ring[i];
        let c = ring[(i + 1) % 4];
        let u = (c - a).normalize();
        let n = u.cross(Vec3::Y);
        let h = Vec3::Y * height;
        b.quad([a, c, c + h, a + h], wall, 0);
        b.quad(
            [
                a + n * 0.03,
                c + n * 0.03,
                c + Vec3::Y * 0.8 + n * 0.03,
                a + Vec3::Y * 0.8 + n * 0.03,
            ],
            stone,
            0,
        );
        b.quad(
            [
                a + h + n * 0.2,
                c + h + n * 0.2,
                c + h + Vec3::Y * 0.25 + n * 0.2,
                a + h + Vec3::Y * 0.25 + n * 0.2,
            ],
            stone,
            0,
        );
        let cols = ((c - a).length() / 4.).floor().max(1.) as usize;
        let rows = if house { 2 } else { 3 };
        for row in 0..rows {
            for col in 0..cols {
                let centre = a
                    + (c - a) * (col as f32 + 0.5) / cols as f32
                    + Vec3::Y * (1.8 + row as f32 * (height - 2.) / rows as f32)
                    + n * 0.06;
                let w = 0.65;
                let up = Vec3::Y * 0.85;
                b.quad(
                    [
                        centre - u * w - up,
                        centre + u * w - up,
                        centre + u * w + up,
                        centre - u * w + up,
                    ],
                    glass,
                    4,
                );
                if house {
                    for side in [-1., 1.] {
                        let q = centre + u * side * 1.05 + n * 0.04;
                        b.quad(
                            [
                                q - u * 0.35 - up,
                                q + u * 0.35 - up,
                                q + u * 0.35 + up,
                                q - u * 0.35 + up,
                            ],
                            shutter,
                            11,
                        );
                    }
                }
                let q = centre - Vec3::Y * 0.9 + n * 0.12;
                b.quad(
                    [
                        q - u * 0.82 - Vec3::Y * 0.08,
                        q + u * 0.82 - Vec3::Y * 0.08,
                        q + u * 0.82 + Vec3::Y * 0.08,
                        q - u * 0.82 + Vec3::Y * 0.08,
                    ],
                    stone,
                    0,
                );
            }
        }
        if i == 0 {
            let q = (a + c) * 0.5 + n * 0.10;
            let u0 = u * 0.7;
            b.quad(
                [
                    q - u0,
                    q + u0,
                    q + u0 + Vec3::Y * 2.1,
                    q - u0 + Vec3::Y * 2.1,
                ],
                linear([0.22, 0.14, 0.09]),
                10,
            );
        }
    }
    // Overhanging hip roof: separate faces keep the crease and tile finish.
    let e = 0.6;
    let y = height + 0.25;
    let h = if house { 2.4 } else { 0.8 };
    let corners = [
        Vec3::new(-x - e, y, z + e),
        Vec3::new(x + e, y, z + e),
        Vec3::new(x + e, y, -z - e),
        Vec3::new(-x - e, y, -z - e),
    ];
    let r0 = Vec3::new(-x * 0.55, y + h, 0.);
    let r1 = Vec3::new(x * 0.55, y + h, 0.);
    b.quad([corners[0], corners[1], r1, r0], roof, 0);
    b.quad([corners[2], corners[3], r0, r1], roof, 0);
    b.quad([corners[1], corners[2], r1, r1], roof, 0);
    b.quad([corners[3], corners[0], r0, r0], roof, 0);
    b.mesh()
}

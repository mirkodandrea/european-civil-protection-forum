"""Run with Blender --background --python scripts/build_vegetation_models.py.

Vegetation varieties, each with the burnt companion built from the SAME
structure. A variety has two halves that never overlap:

* live: trunk and boughs (`wood`) and the crown pads (`leaf`), as in
  build_models.py (same colours, same crown shading, same model space);
* remains: only what the fire adds or uncovers, namely the skeleton of fine
  twigs inside the crown (`skel`) and the ground debris (`debris`: ash,
  fallen limbs). The trunk and boughs of the live half stay where they are
  and are charred in place by the shader, so nothing is swapped.

Several varieties per species keep the silhouette family of their fuel group
(umbrella pine, round chestnut, macchia clump, gnarled olive): the player
reads the fuel from the silhouette.

Outputs (assets/models/): vegetation.json, vegetation_assets.blend and
vegetation_preview/ (a strip of burn stages per variety, overview images and
index.html).

Blender: metres, Z up, front -Y. Runtime: metres, Y up, front +Z.
"""
import bpy
import bmesh
import json
import math
import random
from pathlib import Path
from mathutils import Vector, noise

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'assets/models'
PREVIEW = OUT / 'vegetation_preview'
PREVIEW.mkdir(parents=True, exist_ok=True)

# Colours in sRGB, as the game uses them.
LEAF = (.19, .34, .095)
LEAF2 = (.29, .43, .15)
WOOD = (.20, .11, .045)
CHAR = (.045, .038, .034)
CHAR2 = (.11, .085, .07)
ASH = (.45, .44, .41)
ASH_DARK = (.24, .23, .22)
SCORCH = (.31, .17, .07)

# --- scene plumbing -----------------------------------------------------------
current = None
mats = {}


def lin(c):
    return [x ** 2.2 for x in c]


def material(color, glow=0.0, glow_color=(1.0, .28, .03)):
    key = (tuple(round(x, 3) for x in color), round(glow, 2))
    if key not in mats:
        m = bpy.data.materials.new('M')
        m.use_nodes = True
        b = m.node_tree.nodes['Principled BSDF']
        b.inputs['Base Color'].default_value = (*lin(color), 1)
        b.inputs['Roughness'].default_value = .85
        if glow > 0:
            b.inputs['Emission Color'].default_value = (*glow_color, 1)
            b.inputs['Emission Strength'].default_value = glow
        m.diffuse_color = (*lin(color), 1)
        m['srgb'] = list(color)
        mats[key] = m
    return mats[key]


def finish(o, name, kind, color):
    o.name = name
    for c in list(o.users_collection):
        c.objects.unlink(o)
    current.objects.link(o)
    o.data.materials.append(material(color))
    o['kind'] = kind
    o['color'] = list(color)
    return o


def rod(name, a, b, r0, r1, color, kind='wood', sides=4):
    a, b = Vector(a), Vector(b)
    if kind in ('skel', 'debris'):
        # budget: an open cone to a point is `sides` triangles (no caps)
        bpy.ops.mesh.primitive_cone_add(vertices=sides, radius1=r0, radius2=0, depth=(b - a).length, end_fill_type='NOTHING', location=(a + b) / 2)
    else:
        bpy.ops.mesh.primitive_cone_add(vertices=sides, radius1=r0, radius2=r1, depth=(b - a).length, location=(a + b) / 2)
    o = bpy.context.object
    o.rotation_euler = (b - a).to_track_quat('Z', 'Y').to_euler()
    return finish(o, name, kind, color)


def blob(name, pos, size, color, seed, jitter=.16, flat=.55):
    """A lumpy crown pad: a 12-vertex icosphere, each vertex pushed in or out
    and the underside flattened (the same as build_models.py)."""
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1, radius=1, location=pos)
    o = bpy.context.object
    k = seed * 7.31
    for v in o.data.vertices:
        r = 1 + jitter * math.sin(k + v.co.x * 5.1 + v.co.y * 3.7 + v.co.z * 4.3 + seed)
        v.co = v.co * r
        if v.co.z < 0:
            v.co.z *= flat
    o.scale = size
    return finish(o, name, 'leaf', color)


def disc(name, pos, rx, ry, h, color, rot=0.0):
    """Ground debris: a flat fan of seven triangles (the material is double sided)."""
    bpy.ops.mesh.primitive_circle_add(vertices=7, radius=1, fill_type='TRIFAN', location=(pos[0], pos[1], pos[2] + h))
    o = bpy.context.object
    o.scale = (rx, ry, 1)
    o.rotation_euler = (0, 0, rot)
    return finish(o, name, 'debris', color)


# --- shared structure ---------------------------------------------------------
def poly(name, pts, r0, r1, sides=4, color=WOOD, kind='wood'):
    """A tapering limb along a polyline."""
    n = len(pts) - 1
    for i in range(n):
        a = r0 + (r1 - r0) * i / n
        b = r0 + (r1 - r0) * (i + 1) / n
        rod(name, pts[i], pts[i + 1], a, b, color, kind, sides)


def sample_wood(trunks):
    out = []
    for pts in trunks:
        for a, b in zip(pts, pts[1:]):
            for k in range(6):
                out.append(tuple(Vector(a).lerp(Vector(b), k / 6)))
        out.append(tuple(pts[-1]))
    return out


def root_for(trunks, pos):
    """The point of the wood just under `pos` and closest to it: where a bough
    to that pad leaves the trunk."""
    best, bd = None, 1e9
    for p in sample_wood(trunks):
        if p[2] > pos[2] - .06:
            continue
        d = (Vector(p) - Vector(pos)).length
        if d < bd:
            best, bd = p, d
    return best or trunks[0][-1]


def crown_skeleton(trunks, pads, twigs=3, seed=0):
    twigs = min(twigs, 3)
    """The fine wood inside each pad: a bough from the trunk to the pad's
    centre and a fan of twigs that stop inside its skin, so that nothing of it
    shows until the leaves are gone."""
    rng = random.Random(seed)
    for pos, size, _col in pads:
        root = root_for(trunks, pos)
        mid = Vector(root).lerp(Vector(pos), .5)
        rod('Bough', root, pos, .017, .010, CHAR, 'skel', 3)
        for i in range(twigs):
            a = i * math.tau / twigs + rng.uniform(-.4, .4)
            reach = rng.uniform(.62, .90)
            end = (pos[0] + math.cos(a) * size[0] * reach,
                   pos[1] + math.sin(a) * size[1] * reach,
                   pos[2] + size[2] * rng.uniform(-.25, .55))
            rod('Twig', pos, end, .010, .004, CHAR2 if i % 2 else CHAR, 'skel', 3)
            if i == 0:
                sub = Vector(pos).lerp(Vector(end), .6)
                rod('Sprig', tuple(sub), (sub.x - math.sin(a) * size[0] * .25, sub.y + math.cos(a) * size[1] * .25, sub.z + size[2] * .25),
                    .006, .003, CHAR, 'skel', 3)
        # one dead bough that stays bare past the crown, like the real thing
        a = rng.uniform(0, math.tau)
        end = (pos[0] + math.cos(a) * size[0] * 1.02, pos[1] + math.sin(a) * size[1] * 1.02, pos[2] + size[2] * .1)
        rod('Dead bough', tuple(mid), end, .012, .004, CHAR, 'skel', 3)


def shrub_skeleton(blobs, stems=3, seed=0):
    """Woody stems of a clump: leaning from near the foot, forking twice, kept
    inside the blob's volume so the clump stays a clump."""
    rng = random.Random(seed)
    for pos, size, _col in blobs:
        foot = (pos[0] * .35, pos[1] * .35, .02)
        n = 3 if size[0] > .3 else 2
        for i in range(n):
            a = i * math.tau / n + rng.uniform(-.35, .35)
            reach = rng.uniform(.15, .40)
            fork = (pos[0] * .6 + math.cos(a) * size[0] * reach * .6,
                    pos[1] * .6 + math.sin(a) * size[1] * reach * .6,
                    pos[2] * .75 + size[2] * .05)
            rod('Stem', foot, fork, .028, .017, CHAR, 'skel', 3)
            for k in (-1, 1):
                b = a + k * rng.uniform(.5, .9)
                top = (pos[0] + math.cos(b) * size[0] * rng.uniform(.30, .55),
                       pos[1] + math.sin(b) * size[1] * rng.uniform(.30, .55),
                       pos[2] + size[2] * rng.uniform(.10, .50))
                rod('Branch', fork, top, .015, .005, CHAR2 if k > 0 else CHAR, 'skel', 3)


def debris(radius, fallen=None, ash_seed=0):
    # no ash disc: the terrain under a burnt plant is already darkened by the fire
    if fallen:
        a, b, r = fallen
        rod('Fallen limb', a, b, r, r * .45, CHAR, 'debris', 4)


# --- varieties ----------------------------------------------------------------
VARIETIES = {}


def variety(species, name, text):
    def deco(fn):
        VARIETIES[name] = dict(species=species, text=text, build=fn)
        return fn
    return deco


def umbrella(trunks, pads, limbs=(), fallen=None, twigs=5, seed=0, ash=.5):
    for pts, r0, r1 in trunks:
        poly('Trunk', pts, r0, r1, 4)
    for pts, r0, r1 in limbs:
        poly('Limb', pts, r0, r1, 4)
    for i, (pos, size, col) in enumerate(pads):
        blob('Pad', pos, size, col, 7 + i, .12, .3)
    wood = [t[0] for t in trunks] + [l[0] for l in limbs]
    crown_skeleton(wood, pads, twigs, seed)
    debris(ash, fallen, seed)


@variety('pine', 'pine_1', 'Classico: tronco inclinato e ombrello a tre cuscini.')
def pine_1():
    umbrella([([(0, 0, 0), (.07, .02, .82)], .038, .02)],
             [((.08, .02, .88), (.36, .32, .09), LEAF), ((.26, .10, .80), (.20, .17, .07), LEAF2), ((-.15, .08, .92), (.22, .19, .07), LEAF2)], seed=1)


@variety('pine', 'pine_2', 'Gemello: due fusti che partono dalla stessa base.')
def pine_2():
    umbrella([([(0, 0, 0), (.02, 0, .30)], .045, .035)],
             [((.18, .05, .86), (.30, .27, .08), LEAF), ((.34, .10, .78), (.16, .14, .06), LEAF2),
              ((-.17, -.05, .80), (.26, .23, .08), LEAF2), ((-.32, -.10, .72), (.14, .12, .055), LEAF)],
             limbs=[([(.02, 0, .30), (.16, .04, .80)], .03, .016), ([(.02, 0, .30), (-.14, -.04, .74)], .026, .014)],
             fallen=((.2, -.2, .03), (.62, -.34, .035), .026), seed=2)


@variety('pine', 'pine_3', 'Slanciato: fusto alto e ombrello stretto, un ramo basso.')
def pine_3():
    umbrella([([(0, 0, 0), (.03, .01, .5), (.06, .02, .90)], .03, .016)],
             [((.07, .02, .94), (.26, .23, .075), LEAF), ((.20, .08, .88), (.15, .13, .055), LEAF2),
              ((-.10, .05, .90), (.16, .14, .055), LEAF2), ((-.17, -.05, .68), (.14, .12, .05), LEAF)],
             limbs=[([(.035, .011, .56), (-.12, -.04, .64)], .012, .007)], seed=3, ash=.4)


@variety('pine', 'pine_4', 'Piegato dal vento: fusto curvo e chioma spostata sottovento.')
def pine_4():
    umbrella([([(0, 0, 0), (-.05, 0, .30), (.04, .03, .62), (.20, .07, .78)], .042, .02)],
             [((.26, .08, .84), (.34, .28, .08), LEAF), ((.46, .14, .78), (.20, .16, .06), LEAF2), ((.10, .14, .86), (.18, .15, .06), LEAF2)],
             fallen=((-.1, .2, .03), (-.5, .38, .035), .024), seed=4)


@variety('chestnut', 'chestnut_1', 'Classico: tronco tozzo e chioma tonda a lobi.')
def chestnut_1():
    umbrella([([(0, 0, 0), (0, 0, .40)], .08, .05)],
             [((0, 0, .66), (.46, .44, .34), LEAF2), ((.30, .16, .56), (.30, .28, .24), LEAF), ((-.28, -.18, .60), (.30, .28, .25), LEAF)],
             seed=5, ash=.6, twigs=6)


@variety('chestnut', 'chestnut_2', 'Alto: tronco lungo e chioma raccolta.')
def chestnut_2():
    umbrella([([(0, 0, 0), (.02, 0, .50)], .07, .045)],
             [((.02, 0, .72), (.34, .33, .28), LEAF2), ((.22, .12, .64), (.24, .22, .22), LEAF),
              ((-.20, -.12, .68), (.24, .22, .22), LEAF), ((0, .02, .84), (.20, .19, .14), LEAF2)],
             fallen=((.1, .1, .03), (.56, .3, .035), .03), seed=6, ash=.5, twigs=6)


@variety('chestnut', 'chestnut_3', 'Largo e basso: chioma ampia su un fusto corto.')
def chestnut_3():
    umbrella([([(0, 0, 0), (0, 0, .26)], .10, .07)],
             [((0, 0, .50), (.58, .52, .28), LEAF2), ((.40, .20, .42), (.34, .30, .20), LEAF),
              ((-.36, -.22, .44), (.34, .30, .21), LEAF), ((0, .38, .40), (.28, .26, .18), LEAF2)],
             seed=7, ash=.7, twigs=6)


@variety('chestnut', 'chestnut_4', 'Ceduo: tre polloni che si aprono dalla ceppaia.')
def chestnut_4():
    umbrella([([(0, 0, 0), (.12, .03, .36)], .05, .03), ([(0, 0, 0), (-.10, .06, .40)], .045, .028), ([(0, 0, 0), (-.02, -.12, .34)], .04, .025)],
             [((.16, .04, .62), (.30, .28, .26), LEAF2), ((-.16, .08, .68), (.28, .26, .25), LEAF),
              ((-.04, -.17, .58), (.26, .24, .22), LEAF2), ((0, 0, .80), (.20, .19, .15), LEAF)],
             seed=8, ash=.55, twigs=5)


def clump(blobs, skeleton_seed, ash, stems=6, fallen=None):
    for i, (pos, size, col) in enumerate(blobs):
        blob('Macchia', pos, size, col, 1 + i, .16, .55)
    shrub_skeleton(blobs, stems, skeleton_seed)
    debris(ash, fallen, skeleton_seed)


@variety('bush', 'bush_1', 'Classico: cuore tondo e due lobi.')
def bush_1():
    clump([((0, 0, .34), (.78, .72, .40), LEAF2), ((.42, .22, .24), (.48, .44, .30), LEAF), ((-.36, -.30, .22), (.46, .42, .28), LEAF)], 11, .66)


@variety('bush', 'bush_2', 'Alto (lentisco): un mazzo compatto e più slanciato.')
def bush_2():
    clump([((0, 0, .42), (.55, .52, .44), LEAF), ((.30, .12, .30), (.36, .34, .30), LEAF2), ((-.26, -.10, .34), (.34, .32, .30), LEAF2),
           ((.02, -.10, .70), (.30, .28, .24), LEAF)], 12, .58, 5)


@variety('bush', 'bush_3', 'Tappeto basso (cisto): largo e piatto, quattro lobi.')
def bush_3():
    clump([((0, 0, .20), (.85, .80, .22), LEAF2), ((.50, .30, .16), (.45, .40, .16), LEAF), ((-.45, -.25, .15), (.42, .38, .15), LEAF),
           ((.10, -.50, .14), (.40, .36, .14), LEAF2)], 13, .86, 4)


@variety('bush', 'bush_4', 'Rado: tre ciuffi staccati con spazi tra uno e l\'altro.')
def bush_4():
    clump([((-.40, .10, .30), (.38, .36, .30), LEAF), ((.38, -.10, .28), (.40, .37, .28), LEAF2), ((.05, .42, .20), (.30, .28, .20), LEAF),
           ((0, -.20, .14), (.22, .20, .10), LEAF2)], 14, .7, 5)


@variety('olive', 'olive_1', 'Classico: tronco nodoso e tre cime argentate.')
def olive_1():
    umbrella([([(0, 0, 0), (.06, 0, .46)], .07, .035)],
             [((.06, 0, .62), (.40, .36, .25), LEAF2), ((.28, .10, .52), (.26, .24, .19), LEAF), ((-.20, -.12, .56), (.26, .24, .20), LEAF)],
             seed=15, ash=.5)


@variety('olive', 'olive_2', 'Contorto: tronco a S con un grosso ramo laterale.')
def olive_2():
    umbrella([([(0, 0, 0), (-.06, .04, .20), (.04, 0, .42)], .08, .04)],
             [((.06, 0, .62), (.34, .30, .22), LEAF2), ((.30, .14, .58), (.24, .22, .18), LEAF), ((-.16, .10, .60), (.24, .22, .18), LEAF)],
             limbs=[([(0, .02, .34), (.26, .12, .50)], .03, .016)], fallen=((.1, -.2, .03), (.5, -.3, .035), .024), seed=16, ash=.5)


@variety('olive', 'olive_3', 'Vecchio a due fusti: ceppaia divisa in due.')
def olive_3():
    umbrella([([(0, 0, 0), (-.07, 0, .44)], .06, .032), ([(0, 0, 0), (.12, .04, .38)], .055, .03)],
             [((-.08, 0, .62), (.30, .28, .21), LEAF2), ((.16, .05, .55), (.28, .26, .20), LEAF), ((0, -.12, .66), (.22, .20, .17), LEAF2)],
             seed=17, ash=.5)


SPECIES = [('pine', 'Pino marittimo', 'conifere'), ('chestnut', 'Castagno', 'latifoglie'), ('bush', 'Macchia', 'arbusti'), ('olive', 'Ulivo', 'alberi isolati nei prati')]


# --- build / export -----------------------------------------------------------
def build(name):
    global current
    current = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(current)
    VARIETIES[name]['build']()
    bpy.context.view_layer.update()
    return list(current.objects)


def triangles(objs, keep, shade_crown):
    lookup, ps, ns, cs, idx, flags = {}, [], [], [], [], []
    for o in objs:
        if o['kind'] not in keep:
            continue
        mesh = o.to_mesh()
        mesh.calc_loop_triangles()
        color = tuple(o['color'])
        zs = [v.co.z for v in mesh.vertices]
        z0, z1 = min(zs), max(zs)
        nm = o.matrix_world.to_3x3().inverted().transposed()
        flag = o['kind'] == 'wood' if 'wood' in keep else o['kind'] == 'debris'
        for tri in mesh.loop_triangles:
            n = (nm @ tri.normal).normalized()
            for vi in tri.vertices:
                p = o.matrix_world @ mesh.vertices[vi].co
                pp = [round(p.x, 5), round(p.z, 5), round(-p.y, 5)]
                nn = [round(n.x, 4), round(n.z, 4), round(-n.y, 4)]
                c = list(color)
                if shade_crown and o['kind'] == 'leaf':
                    t = (mesh.vertices[vi].co.z - z0) / max(z1 - z0, 1e-6)
                    c = [ch * (.78 + .40 * t) for ch in color]
                c = [round(x, 5) for x in c]
                key = (*pp, *c, flag)
                if key not in lookup:
                    lookup[key] = len(ps)
                    ps.append(pp); ns.append(nn); cs.append(c); flags.append(flag)
                idx.append(lookup[key])
        o.to_mesh_clear()
    return ps, ns, cs, idx, flags


def low_remains(objs):
    """A few cones standing for the skeleton, for chunks beyond the LOD distance."""
    skel = [o for o in objs if o['kind'] == 'skel']
    pts = [o.matrix_world @ Vector(c) for o in skel for c in o.bound_box]
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    centre = (lo + hi) / 2
    reach = max(hi.x - lo.x, hi.y - lo.y) / 2 * .85
    global current
    current = bpy.data.collections.new('low')
    bpy.context.scene.collection.children.link(current)
    rod('Bare crown', (centre.x, centre.y, lo.z), (centre.x, centre.y, hi.z), reach, 0, CHAR2, 'skel', 5)
    out = list(current.objects)
    bpy.context.view_layer.update()
    return out


def export():
    baked = {}
    for name in VARIETIES:
        objs = build(name)
        live = triangles(objs, ('wood', 'leaf'), True)
        rem = triangles(objs, ('skel', 'debris'), False)
        low = triangles(low_remains(objs), ('skel', 'debris'), False)
        baked[name] = dict(
            live=dict(positions=live[0], normals=live[1], colors=[[*c, 1] for c in live[2]], indices=live[3], wood=live[4]),
            remains=dict(positions=rem[0], normals=rem[1], colors=rem[2], indices=rem[3], debris=rem[4]),
            low=dict(positions=low[0], normals=low[1], colors=low[2], indices=low[3], debris=low[4]),
        )
        print(f'{name}: live {len(live[3]) // 3} tris / {len(live[0])} verts, remains {len(rem[3]) // 3} / {len(rem[0])}, low {len(low[3]) // 3} / {len(low[0])}')
    (OUT / 'vegetation.json').write_text(json.dumps(baked, separators=(',', ':')) + '\n')


# --- preview: the stages of a burn -------------------------------------------
# The preview follows the rules of the foliage shader (retro.wgsl, burn_plant):
# a front crosses the crown in patches, l = progress * 1.6 - noise * 0.9, and
# a leaf is green for l < 0, a hot glow just after, then char, then gone; wood
# chars in place; the skeleton shows where the leaves are gone; ash comes last.
STAGES = [0.0, 0.14, 0.26, 0.40, 0.62, 1.0]
STAGE_NAMES = ['Vivo', 'Scottatura', 'Fiamme', 'Chioma consumata', 'Scheletro', 'Bruciato']
SIZE = 12.0  # metres a plant stands in the game, for the noise scale


def smooth(a, b, x):
    t = min(max((x - a) / (b - a), 0), 1)
    return t * t * (3 - 2 * t)


def burn_l(p, s):
    q = Vector(p) * SIZE
    # mathutils noise is narrow around 0; stretch it to the spread of the shader's value noise
    def v(x, y, z):
        return min(max(.5 + 1.25 * noise.noise(Vector((x, y, z))), 0), 1)
    n = .55 * v(q.x * .8 + q.z * .5, q.y * .8, 3.0) + .45 * v(q.x * 2.7 - q.z * 1.9, q.y * 2.7, 7.0)
    return s * 1.6 - n * .9


def mix(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


def stage_plant(objs, s, dx):
    rng = random.Random(5)
    for o in list(objs):
        o.location.x += dx
        bpy.context.view_layer.update()
        centre = o.matrix_world @ (sum((Vector(c) for c in o.bound_box), Vector()) / 8)
        centre_local = Vector((centre.x - dx, centre.y, centre.z))
        l = burn_l(centre_local, s)
        kind = o['kind']
        base = tuple(o['color'])
        if kind == 'wood':
            charred = smooth(.0, .42, l)
            hot = smooth(0, .07, l) * (1 - smooth(.1, .36, l))
            ember = smooth(.08, .2, l) * (1 - smooth(.3, .7, l))
            col = mix(mix(base, (base[0] * .8, base[1] * .55, base[2] * .3), smooth(-.35, 0, l) * .65), CHAR, charred)
            o.data.materials.clear()
            o.data.materials.append(material(col, hot * .5 + ember * .3))
        elif kind == 'skel':
            if l < .30:
                bpy.data.objects.remove(o)
                continue
            o.data.materials.clear()
            o.data.materials.append(material(base, smooth(.3, .45, l) * (1 - smooth(.45, .9, l)) * .9))
        elif kind == 'debris':
            if s < .65:
                bpy.data.objects.remove(o)
                continue
        elif kind == 'leaf':
            me = o.data
            bm = bmesh.new()
            bm.from_mesh(me)
            bmesh.ops.subdivide_edges(bm, edges=bm.edges[:], cuts=3, use_grid_fill=True)
            lv = {}
            slots = []

            def slot(m):
                if m.name not in lv:
                    lv[m.name] = len(slots)
                    slots.append(m)
                return lv[m.name]
            gone = []
            for f in bm.faces:
                p = o.matrix_world @ f.calc_center_median()
                fl = burn_l(Vector((p.x - dx, p.y, p.z)), s)
                hole = rng.random()
                if hole < smooth(.34, .62, fl):
                    gone.append(f)
                    continue
                if fl < 0:
                    sc = smooth(-.35, 0, fl) * .65
                    m = material(mix(base, (base[0] * .8, base[1] * .55, base[2] * .3), sc))
                elif fl < .08:
                    m = material((.16, .045, .008), .45)
                elif fl < .36:
                    ember = rng.random() < .22 * (1 - smooth(.2, .36, fl))
                    m = material((.05, .04, .035), 1.6 if ember else 0, (1.0, .15, .02))
                else:
                    m = material((.05, .04, .035))
                f.material_index = slot(m)
            if gone:
                bmesh.ops.delete(bm, geom=gone, context='FACES')
            bm.to_mesh(me)
            bm.free()
            me.materials.clear()
            for m in slots:
                me.materials.append(m)
            if not slots:
                bpy.data.objects.remove(o)


def scene_setup(width_m, height_m, res):
    sc = bpy.context.scene
    sc.render.engine = 'CYCLES'
    sc.cycles.samples = 28
    sc.cycles.use_denoising = True
    sc.render.resolution_x, sc.render.resolution_y = res
    sc.render.film_transparent = False
    world = sc.world or bpy.data.worlds.new('W')
    sc.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes['Background']
    bg.inputs['Color'].default_value = (.46, .52, .60, 1)
    bg.inputs['Strength'].default_value = 1.3
    for o in [o for o in bpy.data.objects if o.type in ('LIGHT', 'CAMERA')]:
        bpy.data.objects.remove(o)
    bpy.ops.object.light_add(type='SUN', location=(0, 0, 10))
    sun = bpy.context.object
    sun.rotation_euler = (math.radians(48), 0, math.radians(-35))
    sun.data.energy = 5.5
    sun.data.angle = math.radians(6)
    bpy.ops.object.camera_add()
    cam = bpy.context.object
    cam.data.type = 'ORTHO'
    cam.data.ortho_scale = width_m
    cam.rotation_euler = (math.radians(58), 0, 0)
    sc.camera = cam
    return cam


def place_camera(cam, cx, cy, dist=14):
    # looks 32 degrees below the horizon (rotation x = 58); step back along the view
    view = cam.rotation_euler.to_quaternion() @ Vector((0, 0, -1))
    cam.location = Vector((cx, cy, .4)) - view * dist


def ground(cx, cy, w, h):
    bpy.ops.mesh.primitive_plane_add(size=1, location=(cx, cy, -.005))
    g = bpy.context.object
    g.scale = (w, h, 1)
    g.data.materials.append(material((.36, .35, .17)))
    return g


def reset_scene():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o)
    for c in list(bpy.data.collections):
        bpy.data.collections.remove(c)


def render(path):
    bpy.context.scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)


def strip(name):
    global current
    reset_scene()
    cam = scene_setup(STAGE_GAP * len(STAGES) + .6, 0, (1800, 520))
    ground(STAGE_GAP * len(STAGES) / 2 - STAGE_GAP / 2, 0, 60, 60)
    for i, s in enumerate(STAGES):
        # the burnt patch under the plant darkens with the fire
        objs = build(name)
        bpy.ops.mesh.primitive_cylinder_add(vertices=24, radius=.78, depth=.004, location=(i * STAGE_GAP, 0, 0))
        patch = bpy.context.object
        patch.data.materials.append(material(mix((.36, .35, .17), (.10, .09, .08), smooth(.15, .8, s))))
        stage_plant(objs, s, i * STAGE_GAP)
    place_camera(cam, STAGE_GAP * len(STAGES) / 2 - STAGE_GAP / 2, 0)
    render(PREVIEW / f'{name}.png')


def overview(s, path):
    global current
    reset_scene()
    names = list(VARIETIES)
    cols = 4
    gap = 1.8
    rowgap = 2.4
    cam = scene_setup(gap * cols + .6, 0, (1800, 1300))
    ground(0, 0, 80, 80)
    rows = {'pine': 0, 'chestnut': 1, 'bush': 2, 'olive': 3}
    count = {}
    for name in names:
        sp = VARIETIES[name]['species']
        i = count.get(sp, 0)
        count[sp] = i + 1
        objs = build(name)
        bpy.ops.mesh.primitive_cylinder_add(vertices=24, radius=.78, depth=.004, location=(i * gap, -rows[sp] * rowgap, 0))
        patch = bpy.context.object
        patch.data.materials.append(material(mix((.36, .35, .17), (.10, .09, .08), smooth(.15, .8, s))))
        for o in objs:
            o.location.y -= rows[sp] * rowgap
        # the stage function moves along x only; offset y was applied above
        stage_plant(objs, s, i * gap)
    view_c = Vector((gap * (cols - 1) / 2, -rowgap * 1.5 - .3, .4))
    place_camera(cam, view_c.x, view_c.y, 14)
    render(path)


STAGE_GAP = 1.5


def html():
    rows = []
    for key, title, note in SPECIES:
        cards = []
        for name, v in VARIETIES.items():
            if v['species'] != key:
                continue
            cards.append(f'''<figure><img src="{name}.png" alt="{name}" loading="lazy">
<figcaption><b>{name}</b> &mdash; {v["text"]}</figcaption></figure>''')
        rows.append(f'<section><h2>{title} <small>{note}</small></h2>{"".join(cards)}</section>')
    labels = ''.join(f'<span>{n}</span>' for n in STAGE_NAMES)
    page = f'''<!doctype html>
<html lang="it"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Vegetazione: varietà e bruciatura</title>
<style>
:root{{--bg:#14171c;--fg:#e8eaed;--mut:#9aa3ad;--line:#2a2f37}}
@media (prefers-color-scheme:light){{:root{{--bg:#f4f1ea;--fg:#1d2024;--mut:#5e6670;--line:#d9d3c6}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif}}
main{{max-width:1240px;margin:0 auto;padding:24px 16px 64px}}
h1{{font-size:1.6rem;margin:.2em 0}} h2{{margin:2em 0 .4em;font-size:1.25rem}} h2 small{{color:var(--mut);font-weight:400;font-size:.8em}}
p{{color:var(--mut);max-width:70ch}}
img{{width:100%;height:auto;display:block;border-radius:8px;border:1px solid var(--line)}}
figure{{margin:0 0 18px}} figcaption{{padding:6px 2px;color:var(--mut)}} figcaption b{{color:var(--fg)}}
.stages{{display:grid;grid-template-columns:repeat(6,1fr);text-align:center;color:var(--mut);font-size:.8rem;margin:0 0 4px;position:sticky;top:0;background:var(--bg);padding:6px 0;z-index:2;border-bottom:1px solid var(--line)}}
.two{{display:grid;grid-template-columns:1fr 1fr;gap:14px}}
@media (max-width:760px){{.two{{grid-template-columns:1fr}} .stages{{font-size:.62rem}}}}
</style></head><body><main>
<h1>Vegetazione: varietà e bruciatura</h1>
<p>Ogni varietà ha due metà che non si sovrappongono: la pianta viva (tronco, rami, chioma) e quello che l&rsquo;incendio aggiunge o scopre (scheletro dei rametti, cenere, legna caduta). Il legno della pianta viva si carbonizza sul posto: nessuno scambio di modelli. Le immagini mostrano sei fasi dell&rsquo;incendio, seguendo le regole dello shader.</p>
<div class="two"><figure><img src="overview_vivo.png" alt="Tutte le varietà, vive"><figcaption>Le 15 varietà, vive.</figcaption></figure>
<figure><img src="overview_bruciato.png" alt="Tutte le varietà, bruciate"><figcaption>Le stesse, a incendio finito.</figcaption></figure></div>
<div class="stages">{labels}</div>
{"".join(rows)}
<p>Generato da <code>scripts/build_vegetation_models.py</code> (Blender). Le immagini sono un&rsquo;anteprima dell&rsquo;idea, non il rendering del gioco.</p>
</main></body></html>'''
    (PREVIEW / 'index.html').write_text(page, encoding='utf-8')


if __name__ == '__main__':
    reset_scene()
    export()
    for name in VARIETIES:
        strip(name)
    overview(0.0, PREVIEW / 'overview_vivo.png')
    overview(1.0, PREVIEW / 'overview_bruciato.png')
    html()
    reset_scene()
    for name in VARIETIES:
        build(name)
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT / 'vegetation_assets.blend'))

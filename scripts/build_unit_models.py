"""Run with: Blender --background --python scripts/build_unit_models.py

The people and vehicles drawn for the player's teams: a firefighter on foot,
the engine (autobotte) and the air tanker. Bakes `assets/models/units.json`
(positions, normals, colours, indices, surfaces) and writes `units_assets.blend` and a
contact sheet `units_preview.png`.

Blender: metres, Z up, front -Y. Runtime: metres, Y up, front +Z; origins at
ground level (the tanker's origin is its centre). Colours are baked as neutral
greys: the game multiplies them by the live status tint of the unit, so paint
and clothing are bright and everything else is darker, never coloured.
"""
import json
import math
import os
from contextlib import contextmanager
from pathlib import Path

import bpy
import bmesh
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets/models"

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)



# Real colours, absolute sRGB: what is baked is what the player sees (the
# game no longer tints these with the team or status colour).
COLOURS = {
    # fire engine
    "red": (.74, .05, .04),
    "white": (.93, .94, .95),
    "dark": (.045, .047, .055),
    "metal": (.58, .60, .64),
    "alu": (.74, .76, .79),
    "glass": (.11, .19, .25),
    "light": (.98, .95, .78),
    "beacon": (.08, .34, 1.0),
    "tail": (.85, .08, .04),
    "yellow": (.96, .78, .08),
    "amber": (1.0, .55, .06),
    "tyre": (.055, .055, .06),
    "rim": (.80, .82, .86),
    "chrome": (.88, .90, .93),
    "grille": (.07, .075, .09),
    "plate": (.95, .94, .86),
    # firefighter
    "jacket": (.95, .40, .04),
    "trousers": (.13, .16, .17),
    "boot": (.08, .06, .05),
    "glove": (.50, .36, .22),
    "skin": (.80, .58, .45),
    "pack": (.88, .66, .08),
    "wood": (.52, .36, .20),
    "reflect": (.90, .92, .55),
    "helmet": (.96, .95, .90),
    "visor": (.05, .08, .12),
    "strap": (.10, .10, .11),
}
# What each tone is made of, for the game's PBR (`materials/structure.rs`,
# `Surface`; ids as `SURFACE_*` in `structure.wgsl`): baked per vertex.
SURFACES = {"paint": 1, "metal": 2, "rubber": 3, "glass": 4, "lamp": 5, "skin": 6,
            "cloth": 7, "reflective": 8, "leather": 9, "wood": 10}
TONE_SURFACE = {
    "red": "paint", "white": "paint", "yellow": "paint", "plate": "paint", "helmet": "paint",
    "dark": "rubber", "tyre": "rubber", "strap": "cloth",
    "metal": "metal", "alu": "metal", "rim": "metal", "chrome": "metal", "grille": "metal",
    "glass": "glass", "visor": "glass",
    "light": "lamp", "beacon": "lamp", "tail": "lamp", "amber": "lamp",
    "jacket": "cloth", "trousers": "cloth", "pack": "cloth",
    "boot": "leather", "glove": "leather", "skin": "skin", "wood": "wood",
    "reflect": "reflective",
}
materials = {}
current = None
PART = None          # name of the animated part being built, None for the body
RIGS = {}            # model -> part -> rig description (Blender coordinates)
SHARP = math.radians(50)   # edges sharper than this stay hard, the rest are smooth


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def mat(tone):
    if tone not in materials:
        rgb = COLOURS[tone]
        m = bpy.data.materials.new(tone)
        m.diffuse_color = (*[srgb_to_linear(c) for c in rgb], 1)
        m["srgb"] = list(rgb)
        m["surface"] = SURFACES.get(TONE_SURFACE.get(tone), 0)
        materials[tone] = m
    return materials[tone]


@contextmanager
def part(name, pivot, axis, kind, phase=0.0):
    """Everything built inside moves as one animated part about `pivot`.

    `kind` tells the game how: wheel / prop (spin), swing (walking), tool
    (working), blink (a beacon, `phase` 0 or 1 alternate). Pivot and axis are in
    Blender coordinates here and baked in runtime coordinates.
    """
    global PART
    PART = name
    RIGS.setdefault(current.name, {})[name] = dict(pivot=pivot, axis=axis, kind=kind, phase=phase)
    try:
        yield
    finally:
        PART = None


def activate(o):
    bpy.context.view_layer.objects.active = o
    o.select_set(True)


def bevelled(o, width, seg):
    activate(o)
    mod = o.modifiers.new("bevel", "BEVEL")
    mod.width = width
    mod.segments = seg
    bpy.ops.object.modifier_apply(modifier=mod.name)


def finish(o, name, tone):
    """Curved where the angle between faces is gentle, hard on panel edges."""
    o.name = name
    for c in list(o.users_collection):
        c.objects.unlink(o)
    current.objects.link(o)
    me = o.data
    bm = bmesh.new()
    bm.from_mesh(me)
    for e in bm.edges:
        e.smooth = not (len(e.link_faces) == 2 and e.calc_face_angle(0.0) > SHARP)
    for f in bm.faces:
        f.smooth = True
    bm.to_mesh(me)
    bm.free()
    activate(o)
    wn = o.modifiers.new("wn", "WEIGHTED_NORMAL")  # a big flat panel keeps its own normal
    wn.keep_sharp = True
    bpy.ops.object.modifier_apply(modifier=wn.name)
    me.materials.append(mat(tone))
    o["part"] = PART or ""
    return o


def box(name, pos, size, tone, bevel=0.0, seg=1):
    bpy.ops.mesh.primitive_cube_add(size=1, location=pos)
    o = bpy.context.object
    o.scale = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel and min(size) >= .1:   # thin parts read the same without it
        bevelled(o, min(bevel, min(size) * .4), seg)
    return finish(o, name, tone)


def ico(name, pos, size, tone, sub=2):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=sub, radius=1, location=pos)
    o = bpy.context.object
    o.scale = size
    return finish(o, name, tone)


def rod(name, a, b, r1, tone, r2=None, sides=8, bevel=0.0):
    a, b = Vector(a), Vector(b)
    bpy.ops.mesh.primitive_cone_add(vertices=sides, radius1=r1, radius2=r1 if r2 is None else r2,
                                    depth=(b - a).length, location=(a + b) / 2)
    o = bpy.context.object
    o.rotation_euler = (b - a).to_track_quat("Z", "Y").to_euler()
    if bevel:
        bevelled(o, bevel, 2)
    return finish(o, name, tone)


def solid(name, verts, faces, tone, bevel=0.0, seg=2, outward=True):
    """A mesh from raw faces; closed shapes get their normals pointing outwards."""
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(v) for v in verts], [], faces)
    me.update()
    if outward:
        bm = bmesh.new()
        bm.from_mesh(me)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        bm.to_mesh(me)
        bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(o)
    if bevel:
        bevelled(o, bevel, seg)
    return finish(o, name, tone)


def extrude(name, pts, axis, a, b, tone, bevel=0.0, seg=2):
    """A 2D profile pushed along an axis. 'x': profile in (y, z), x from a to b;
    'y': profile in (x, z), y from a to b."""
    n = len(pts)
    verts = []
    for c in (a, b):
        for u, v in pts:
            verts.append((c, u, v) if axis == "x" else (u, c, v))
    faces = [list(range(n)), list(range(2 * n - 1, n - 1, -1))]
    for i in range(n):
        j = (i + 1) % n
        faces.append([i, j, n + j, n + i])
    return solid(name, verts, faces, tone, bevel, seg)


def ring_pts(n, expo, a0=0.0, a1=2 * math.pi, closed=True):
    out = []
    m = n if closed else n + 1
    for k in range(m):
        a = a0 + (a1 - a0) * k / n
        c, s = math.cos(a), math.sin(a)
        out.append((math.copysign(abs(c) ** (2 / expo), c), math.copysign(abs(s) ** (2 / expo), s)))
    return out


def loft(name, sections, tone, cx=0.0, n=14, expo=2.4, a0=0.0, a1=2 * math.pi, bevel=0.0):
    """A body of revolution-ish along Y. sections: (y, cz, half width, half height).
    With a partial angle range it is an open skin (a stripe), else closed with end caps."""
    closed = abs((a1 - a0) - 2 * math.pi) < 1e-6
    pts = ring_pts(n, expo, a0, a1, closed)
    m = len(pts)
    verts = []
    for (y, cz, hw, hh) in sections:
        for (c, s) in pts:
            verts.append((cx + c * hw, y, cz + s * hh))
    faces = []
    for i in range(len(sections) - 1):
        for k in range(m if closed else m - 1):
            k2 = (k + 1) % m
            faces.append([i * m + k, i * m + k2, (i + 1) * m + k2, (i + 1) * m + k])
    if closed:
        faces.append(list(range(m)))
        last = (len(sections) - 1) * m
        faces.append(list(range(last + m - 1, last - 1, -1)))
        return solid(name, verts, faces, tone, bevel)
    # open skin: make the winding point away from the axis
    mid = Vector(verts[0]) * 0
    f = faces[0]
    p = [Vector(verts[i]) for i in f]
    nrm = (p[1] - p[0]).cross(p[2] - p[0])
    axis_pt = Vector((cx, p[0].y, sections[0][1]))
    if nrm.dot(sum(p, mid) / 4 - axis_pt) < 0:
        faces = [list(reversed(f)) for f in faces]
    return solid(name, verts, faces, tone, bevel, outward=False)


AIRFOIL = [(0, 0), (.03, .55), (.15, .88), (.38, 1), (.68, .66), (1, .08),
           (1, -.08), (.68, -.4), (.3, -.5), (.07, -.38)]


def wing(name, stations, zc, tone):
    """stations: (x, y of leading edge, chord, thickness), the aerofoil at each."""
    n = len(AIRFOIL)
    verts = []
    for (x, yle, chord, th) in stations:
        for (t, v) in AIRFOIL:
            verts.append((x, yle + t * chord, zc + v * th / 2))
    faces = [list(range(n)), list(range(len(stations) * n - 1, (len(stations) - 1) * n - 1, -1))]
    for i in range(len(stations) - 1):
        for k in range(n):
            k2 = (k + 1) % n
            faces.append([i * n + k, i * n + k2, (i + 1) * n + k2, (i + 1) * n + k])
    return solid(name, verts, faces, tone)


def arch(name, cx, cy, r, w, tone, thick=.1):
    """The upper half of a ring in the YZ plane (a wheel-arch lip), x from cx-w/2 to cx+w/2."""
    pts = []
    for k in range(7):
        a = math.radians(8 + k * 27.5)
        pts.append((cy + math.cos(a) * (r + thick), math.sin(a) * (r + thick)))
    inner = []
    for k in range(7):
        a = math.radians(8 + k * 27.5)
        inner.append((cy + math.cos(a) * r, math.sin(a) * r))
    poly = pts + inner[::-1]
    return extrude(name, [(u, v + WR) for u, v in poly], "x", cx - w / 2, cx + w / 2, tone)


WR = .55   # engine wheel radius


# --- firefighter on foot ------------------------------------------------------

def firefighter():
    # Slightly stylised (big head and helmet so it still reads from the game
    # camera, where a person is a few pixels tall) but properly proportioned:
    # high-visibility orange AIB jacket with reflective bands, white helmet
    # with a visor, boots, gloves, a backpack pump. Legs and arms swing when it
    # walks; the tool arm chops when it works.
    for s, nm in ((-1, "l"), (1, "r")):
        x = s * .105
        with part(f"leg_{nm}", (x, 0, .92), (1, 0, 0), "swing", 0.0 if s < 0 else math.pi):
            rod("Thigh", (x, 0, .94), (x, -.02, .52), .098, "trousers", r2=.082, sides=10)
            rod("Shin", (x, -.02, .52), (x, 0, .20), .082, "trousers", r2=.072, sides=10)
            box("Knee pad", (x, -.085, .52), (.115, .05, .13), "dark", .02)
            rod("Shin band", (x, 0, .33), (x, 0, .38), .078, "reflect", sides=10)
            rod("Boot shaft", (x, 0, .03), (x, 0, .25), .082, "boot", r2=.078, sides=10)
            box("Boot toe", (x, -.085, .055), (.125, .22, .11), "boot", .04)
            box("Boot sole", (x, -.06, .015), (.13, .30, .03), "dark", .01)
    # arms held a little forward, as if carrying the tool
    for s, nm, kind, ph in ((-1, "l", "swing", math.pi), (1, "r", "tool", 0.0)):
        sx = s * .255
        with part(f"arm_{nm}", (sx, 0, 1.38), (1, 0, 0), kind, ph):
            ico("Shoulder", (sx, 0, 1.37), (.075, .075, .075), "jacket", 1)
            rod("Upper arm", (sx, 0, 1.37), (s * .30, -.06, 1.12), .066, "jacket", r2=.058, sides=10)
            rod("Forearm", (s * .30, -.06, 1.12), (s * .31, -.19, 1.03), .058, "jacket", r2=.052, sides=10)
            rod("Cuff band", (s * .308, -.165, 1.045), (s * .311, -.185, 1.03), .056, "reflect", sides=10)
            ico("Glove", (s * .31, -.21, 1.0), (.058, .07, .062), "glove", 1)
            if kind == "tool":
                # the hand tool: a Pulaski, planted in the ground like a staff
                rod("Tool shaft", (s * .31, -.21, 0.0), (s * .31, -.21, 1.45), .02, "wood", sides=8)
                box("Tool head", (s * .31, -.27, 1.46), (.04, .22, .08), "metal", .01)
                box("Tool adze", (s * .31, -.17, 1.47), (.04, .06, .12), "metal", .005)
    # torso
    box("Pelvis", (0, 0, .93), (.33, .21, .16), "trousers", .05)
    box("Belt", (0, 0, 1.0), (.345, .225, .05), "strap", .015)
    box("Buckle", (0, -.116, 1.0), (.06, .012, .04), "metal")
    box("Jacket", (0, 0, 1.2), (.43, .26, .50), "jacket", .08, 3)
    box("Jacket skirt", (0, 0, 1.03), (.42, .25, .12), "jacket", .04)
    box("Chest band", (0, 0, 1.13), (.445, .275, .05), "reflect", .012)
    box("Waist band", (0, 0, 1.0 + .08), (.44, .27, .035), "reflect", .008)
    for sx in (-.1, .1):
        box("Front stripe", (sx, -.133, 1.25), (.045, .012, .36), "reflect")
        box("Back stripe", (sx, .133, 1.25), (.045, .012, .36), "reflect")
    box("Yoke", (0, 0, 1.4), (.45, .25, .06), "jacket", .03)
    rod("Collar", (0, 0, 1.43), (0, 0, 1.49), .095, "jacket", sides=12)
    rod("Neck", (0, 0, 1.46), (0, 0, 1.54), .06, "skin", sides=10)
    box("Radio", (.14, -.145, 1.3), (.06, .035, .11), "dark", .01)
    rod("Antenna", (.14, -.145, 1.35), (.14, -.145, 1.5), .006, "dark", sides=4)
    # head and helmet
    ico("Head", (0, -.005, 1.63), (.112, .115, .135), "skin")
    ico("Nose", (0, -.108, 1.62), (.02, .025, .03), "skin", 1)
    for s in (-1, 1):
        ico("Ear", (s * .103, 0, 1.625), (.015, .025, .035), "skin", 1)
    ico("Helmet", (0, .005, 1.70), (.13, .145, .105), "helmet")
    box("Helmet neck flap", (0, .135, 1.64), (.25, .09, .035), "helmet")
    box("Helmet peak", (0, -.15, 1.715), (.22, .07, .035), "helmet")
    box("Visor", (0, -.125, 1.665), (.15, .02, .05), "visor")
    box("Helmet crest", (0, .0, 1.795), (.03, .2, .03), "red")
    box("Chin strap", (0, -.088, 1.58), (.14, .015, .015), "strap")
    # backpack pump
    box("Backpack pump", (0, .185, 1.22), (.27, .15, .40), "pack", .06, 3)
    rod("Pump tank", (0, .20, 1.40), (0, .20, 1.50), .075, "pack", sides=12)
    rod("Pump cap", (0, .20, 1.50), (0, .20, 1.53), .035, "dark", sides=8)
    for sx in (-.1, .1):
        box("Strap", (sx, -.008, 1.37), (.04, .27, .02), "strap")
        box("Strap front", (sx, -.135, 1.30), (.04, .012, .20), "strap")
    rod("Pump hose", (-.1, .20, 1.02), (-.14, .08, .86), .018, "dark", sides=6)
    rod("Nozzle", (-.14, .08, .86), (-.14, -.02, .84), .014, "metal", sides=6)


# --- fire engine (autobotte) --------------------------------------------------

def wheel(s, cx, y):
    # outward direction is s along x
    out = lambda d: cx + s * d
    rod("Tyre", (out(-.16), y, WR), (out(.16), y, WR), WR, "tyre", sides=18)
    rod("Rim", (out(.15), y, WR), (out(.19), y, WR), .36, "rim", sides=14)
    rod("Rim dish", (out(.19), y, WR), (out(.195), y, WR), .27, "dark", sides=10)
    rod("Hub cap", (out(.19), y, WR), (out(.235), y, WR), .125, "chrome", r2=.10, sides=8)
    for k in range(5):  # rim slots: what shows the wheel turning
        a = k * 2 * math.pi / 5
        px, pz = y + math.cos(a + .6) * .31, WR + math.sin(a + .6) * .31
        rod("Rim slot", (out(.19), px, pz), (out(.2), px, pz), .035, "dark", sides=4)


def engine():
    w = 2.5
    hw = w / 2
    front_y, rear_y = -2.3, 2.1
    for y, end in ((front_y, "f"), (rear_y, "r")):
        for s, side in ((-1, "l"), (1, "r")):
            cx = s * 1.12
            with part(f"wheel_{end}{side}", (cx, y, WR), (1, 0, 0), "wheel"):
                wheel(s, cx, y)
            arch(f"Wheel arch {end}{side}", s * 1.18, y, WR + .05, .26, "dark", .09)
    # chassis, tanks and running gear
    box("Chassis", (0, 0.1, .70), (1.9, 6.9, .30), "dark", .04)
    box("Axle front", (0, front_y, WR), (2.0, .16, .14), "dark")
    box("Axle rear", (0, rear_y, WR), (2.0, .16, .14), "dark")
    for s in (-1, 1):
        box("Side skirt", (s * 1.2, .5, .66), (.08, 3.0, .26), "dark", .03)
        box("Mud flap", (s * 1.12, rear_y + .72, .5), (.46, .04, .42), "dark", .01)
        box("Mud flap", (s * 1.12, front_y + .72, .5), (.46, .04, .36), "dark", .01)
    # cab: a raked, rounded shell
    extrude("Cab", [(-3.35, .78), (-3.35, 1.78), (-3.12, 2.72), (-1.15, 2.72), (-1.15, .78)], "x",
            -1.23, 1.23, "red", .1, 2)
    box("Cab roof visor", (0, -3.07, 2.78), (2.2, .34, .05), "red", .02)
    # windscreen: raked glass in a dark frame
    rake = math.atan2(.23, .95)
    for nm, sz, dy, tone in (("Windscreen frame", (2.1, .03, .84), -.012, "dark"),
                             ("Windscreen", (1.98, .03, .72), -.02, "glass")):
        g = box(nm, (0, -3.223 + dy - .01, 2.27), sz, tone, .012)
        g.rotation_euler = (-rake, 0, 0)
    box("Dash band", (0, -3.355, 1.55), (2.3, .03, .06), "white")
    box("Grille", (0, -3.375, 1.15), (1.3, .05, .46), "grille", .02)
    for z in (.98, 1.07, 1.16, 1.25, 1.34):
        box("Grille slat", (0, -3.405, z), (1.22, .018, .03), "alu")
    box("Badge", (0, -3.41, 1.62), (.22, .015, .1), "chrome", .005)
    for s in (-1, 1):
        box("Headlamp", (s * .92, -3.375, 1.27), (.34, .06, .2), "light", .04)
        box("Headlamp bezel", (s * .92, -3.36, 1.27), (.40, .03, .26), "dark", .02)
        box("Indicator", (s * .92, -3.375, 1.0), (.3, .05, .09), "amber", .02)
        box("Fog lamp", (s * .5, -3.55, .67), (.16, .04, .1), "light", .02)
    box("Front bumper", (0, -3.5, .68), (w, .3, .30), "white", .06)
    box("Bumper rubber", (0, -3.65, .60), (w * .96, .04, .12), "dark", .015)
    box("Number plate", (0, -3.665, .76), (.52, .02, .12), "plate")
    for s in (-1, 1):
        box("Tow eye", (s * .62, -3.68, .58), (.1, .08, .1), "metal", .02)
        # side of the cab: window, door, mirror, step
        box("Window frame", (s * 1.235, -2.25, 2.29), (.03, 1.28, .66), "dark", .012)
        box("Side window", (s * 1.245, -2.25, 2.29), (.03, 1.16, .54), "glass", .01)
        box("Door line", (s * 1.237, -2.98, 1.6), (.03, .03, 1.65), "dark")
        box("Door line", (s * 1.237, -1.35, 1.65), (.03, .03, 1.65), "dark")
        box("Door line", (s * 1.237, -2.16, .98), (.03, 1.6, .03), "dark")
        box("Door handle", (s * 1.25, -1.62, 1.75), (.04, .2, .05), "chrome", .015)
        box("Door panel", (s * 1.238, -2.25, 1.4), (.025, 1.2, .5), "white", .01)
        box("Cab step", (s * 1.2, -2.25, .6), (.2, 1.1, .08), "metal", .02)
        rod("Mirror arm", (s * 1.22, -3.0, 2.28), (s * 1.52, -3.05, 2.36), .02, "chrome", sides=6)
        box("Mirror", (s * 1.58, -3.05, 2.36), (.07, .18, .42), "dark", .03)
        ico("Mirror glass", (s * 1.545, -3.06, 2.36), (.01, .07, .17), "glass", 1)
        box("Wheel-arch trim", (s * 1.24, front_y, 1.12), (.06, 1.5, .08), "dark", .02)
    # roof: light bar with the blinking beacons
    box("Light bar", (0, -2.7, 2.79), (1.7, .22, .09), "dark", .03)
    for s, nm in ((-1, "l"), (1, "r")):
        with part(f"beacon_{nm}", (s * .38, -2.7, 2.88), (0, 0, 1), "blink", 0.0 if s < 0 else 1.0):
            box("Beacon", (s * .38, -2.7, 2.90), (.58, .2, .15), "beacon", .05)
    for s in (-1, 1):
        box("Corner light", (s * .86, -2.7, 2.89), (.2, .19, .12), "beacon", .04)
        box("Roof marker", (s * .95, -3.15, 2.82), (.12, .08, .06), "amber", .02)
    ico("Roof horn", (0.0, -2.2, 2.78), (.2, .12, .06), "alu", 1)
    # body: equipment compartments, tank, roof ladders
    box("Equipment body", (0, 1.16, 1.78), (w, 4.5, 1.62), "red", .09, 2)
    box("Body base", (0, 1.16, 1.0), (w * .96, 4.5, .12), "dark")
    comps = [(-.35, 1.0), (.82, 1.0), (1.98, 1.0), (2.95, .55)]
    for s in (-1, 1):
        for cy, cl in comps:
            box("Locker frame", (s * 1.255, cy, 1.74), (.03, cl + .08, 1.0), "dark", .01)
            box("Roller shutter", (s * 1.268, cy, 1.74), (.03, cl, .92), "alu", .01)
            for z in (1.5, 1.74, 1.98):
                box("Shutter rib", (s * 1.285, cy, z), (.02, cl * .94, .022), "metal")
            box("Shutter handle", (s * 1.29, cy, 1.31), (.03, min(cl * .5, .5), .04), "dark")
        box("Reflective stripe", (s * 1.262, .96, 2.45), (.026, 4.4, .09), "white")
        box("Reflective stripe", (s * 1.262, .96, 2.36), (.026, 4.4, .035), "yellow")
        box("Water inlet", (s * 1.3, 0.2, 1.12), (.12, .14, .12), "metal", .03)
    # tank dome and hatch
    prof = [(.72 * math.cos(math.pi * k / 10), 2.58 + .26 * math.sin(math.pi * k / 10)) for k in range(11)]
    extrude("Water tank dome", prof, "y", -.85, 3.0, "red", 0)
    rod("Manhole", (0, 1.1, 2.80), (0, 1.1, 2.86), .24, "metal", sides=14)
    rod("Manhole cap", (0, 1.1, 2.86), (0, 1.1, 2.90), .14, "alu", sides=10)
    # roof ladders on their racks
    for s in (-1, 1):
        for y in (-.5, 1.1, 2.7):
            box("Ladder post", (s * .98, y, 2.64), (.46, .06, .15), "dark")
        for dx in (-.18, .18):
            box("Ladder rail", (s * .98 + dx, 1.1, 2.76), (.04, 3.9, .07), "alu")
        for i in range(9):
            box("Ladder rung", (s * .98, -.7 + i * .44, 2.76), (.36, .03, .03), "alu")
    # rear: hose reel, step, lamps, conspicuity bars
    rod("Hose reel drum", (-.5, 3.72, 1.8), (.5, 3.72, 1.8), .24, "dark", sides=14)
    for s in (-1, 1):
        rod("Hose reel flange", (s * .5, 3.72, 1.8), (s * .54, 3.72, 1.8), .36, "metal", sides=14)
        box("Tail lamp", (s * 1.0, 3.77, 1.5), (.26, .05, .34), "tail", .02)
        box("Reverse lamp", (s * 1.0, 3.77, 1.18), (.22, .05, .1), "light", .02)
    rod("Reel crank", (.56, 3.72, 1.8), (.7, 3.72, 1.8), .02, "chrome", sides=5)
    box("Reel nozzle", (0, 3.8, 1.5), (.08, .1, .08), "alu", .02)
    box("Rear bumper", (0, 3.62, .72), (w * .94, .28, .24), "dark", .05)
    for i in range(8):
        box("Rear chevron", (-1.05 + i * .3, 3.78, .98), (.2, .03, .24), "red" if i % 2 else "white")
    box("Rear step", (0, 3.78, .58), (2.0, .3, .06), "metal", .015)
    box("Rear number plate", (0, 3.78, 1.8 - .95), (.5, .02, .12), "plate")
    # exhaust stack behind the cab and a spare hard-suction hose on the left
    rod("Exhaust", (1.0, -1.02, .95), (1.0, -1.02, 2.95), .055, "chrome", sides=8)
    box("Exhaust cap", (1.0, -1.02, 2.98), (.12, .12, .05), "dark", .015)


# --- air tanker ---------------------------------------------------------------

def tanker():
    # Amphibious water bomber (CL-415 style): boat hull, high wing, two
    # turboprops, wingtip floats, T-tail. Nose -Y. White with red markings and
    # yellow wing tips; the propellers turn.
    hull = [(-7.7, .12, .14, .14), (-7.2, .08, .55, .62), (-6.3, .1, .95, 1.05), (-4.8, .1, 1.2, 1.28),
            (-2.0, .1, 1.3, 1.4), (1.8, .05, 1.3, 1.4), (4.0, .12, 1.12, 1.2), (5.6, .38, .78, .82),
            (6.8, .55, .45, .5), (7.4, .62, .15, .18)]
    loft("Fuselage", hull, "white", n=16, expo=2.5)
    belly = [(y, cz, hw * 1.012, hh * 1.012) for (y, cz, hw, hh) in hull[1:-1]]
    loft("Belly stripe", belly, "red", n=14, expo=2.5, a0=math.radians(200), a1=math.radians(340))
    for a in (-12, 168):  # cheat lines along both sides
        loft("Cheat line", [(y, cz + hh * .12, hw * 1.012, hh * 1.012) for (y, cz, hw, hh) in hull[2:-3]], "red",
             n=2, expo=2.5, a0=math.radians(a), a1=math.radians(a + 24))
    box("Hull keel", (0, 1.2, -1.42), (.5, 3.4, .16), "metal", .04)
    for s in (-1, 1):
        box("Scoop probe", (s * .6, 1.5, -1.52), (.08, .5, .34), "dark", .02)
        ico("Cockpit side window", (s * .88, -5.75, .78), (.1, .55, .26), "glass", 1)
    ico("Windscreen", (0, -6.15, .88), (.62, .75, .4), "glass", 2)
    box("Windscreen frame", (0, -5.35, .98), (1.3, .08, .06), "dark", .02)
    for k in range(5):
        ico("Cabin window", (1.0, -2.6 + k * .9, .75), (.05, .3, .22), "glass", 1)
        ico("Cabin window", (-1.0, -2.6 + k * .9, .75), (.05, .3, .22), "glass", 1)
    # high wing, tapered, with yellow tips and red aileron panels
    def st(x):
        a = abs(x)
        if a < 1.2:
            return (x, -1.9, 2.7, .48)
        if a < 4.0:
            t = (a - 1.2) / 2.8
            return (x, -1.9 + t * .4, 2.7 - t * .5, .48 - t * .13)
        t = (a - 4.0) / 4.6
        return (x, -1.5 + t * .5, 2.2 - t * .7, .35 - t * .15)
    wing("Wing", [st(x) for x in (-7.8, -4.0, -1.2, 1.2, 4.0, 7.8)], 1.7, "white")
    for s in (-1, 1):
        wing("Wing tip", [st(s * 7.8), st(s * 8.7)], 1.7, "yellow")
        box("Aileron", (s * 6.2, -.12, 1.62), (2.4, .5, .05), "red", .01)
        box("Flap", (s * 2.9, .07, 1.5), (2.8, .5, .05), "red", .01)
        # engines: nacelle on the leading edge, intake, exhaust, propeller
        cx = s * 3.6
        nac = [(-3.45, 1.78, .3, .3), (-3.1, 1.78, .52, .52), (-2.3, 1.8, .6, .6), (-.4, 1.76, .58, .56),
               (.8, 1.72, .42, .4), (1.5, 1.7, .16, .16)]
        loft("Engine nacelle", nac, "white", cx=cx, n=14, expo=2.2)
        box("Engine intake", (cx, -3.05, 1.35), (.6, .5, .2), "dark", .05)
        rod("Exhaust stub", (cx + s * .56, -1.7, 1.7), (cx + s * .76, -1.9, 1.78), .09, "dark", sides=8)
        with part("prop_" + ("l" if s < 0 else "r"), (cx, -3.5, 1.78), (0, 1, 0), "prop"):
            rod("Spinner", (cx, -3.45, 1.78), (cx, -3.95, 1.78), .27, "alu", r2=.02, sides=14)
            rod("Propeller hub", (cx, -3.4, 1.78), (cx, -3.5, 1.78), .3, "dark", sides=14)
            for k in range(4):
                a = k * math.pi / 2 + .4
                r = .95
                for nm, rr, size, tone in (("Propeller blade", .95, (.17, .04, 1.45), "dark"),
                                           ("Propeller tip", 1.65, (.15, .045, .25), "yellow")):
                    b = box(nm, (cx + math.sin(a) * rr, -3.48, 1.78 + math.cos(a) * rr), size, tone)
                    b.rotation_euler = (0, a, 0)
        # wingtip floats on a pylon
        fl = [(-1.3, .45, .06, .06), (-.9, .5, .17, .17), (.2, .5, .2, .2), (1.0, .5, .1, .1), (1.3, .5, .03, .03)]
        loft("Float", fl, "red", cx=s * 7.6, n=10, expo=2.2)
        box("Float strut", (s * 7.6, -.1, 1.05), (.08, .6, .95), "metal", .02)
    # T-tail
    extrude("Tail fin", [(4.3, .85), (5.6, 4.15), (6.9, 4.15), (7.3, .55)], "x", -.12, .12, "white", .03)
    extrude("Rudder stripe", [(5.9, 3.9), (6.9, 3.9), (6.95, 2.9), (6.15, 2.9)], "x", -.135, .135, "red")
    wing("Tailplane", [(-2.9, 6.1, 1.0, .16), (-.4, 5.7, 1.5, .26), (.4, 5.7, 1.5, .26), (2.9, 6.1, 1.0, .16)],
         4.15, "white")
    box("Tail light", (0, 7.42, 4.1), (.1, .08, .1), "tail", .02)


def drone():
    # Monitoring quadcopter (Matrice class, ~1.3 m tip to tip): dark carbon
    # body, four folding arms with motors, red/white rotor tips so the spin
    # reads from far away, landing skids, a gimbal camera under the nose and
    # navigation lights. Origin at the skids; the rotors turn about Z.
    box("Body", (0, 0, .42), (.36, .5, .16), "dark", .05)
    box("Top shell", (0, -.02, .53), (.3, .4, .07), "white", .03)
    box("Battery", (0, .12, .6), (.2, .22, .06), "metal", .02)
    rod("GPS mast", (0, .18, .63), (0, .18, .76), .015, "metal")
    ico("GPS puck", (0, .18, .78), (.06, .06, .02), "white", 1)
    arm_r = .62
    for k, (sx, sy) in enumerate(((-1, -1), (1, -1), (1, 1), (-1, 1))):
        tip = Vector((sx * arm_r * .707, sy * arm_r * .707, .46))
        rod("Arm", (sx * .12, sy * .16, .44), tip, .022, "dark", sides=8)
        rod("Motor", tip, tip + Vector((0, 0, .08)), .045, "metal", sides=12)
        # front arms carry red lights, rear arms green (aviation convention: red ahead)
        box("Arm light", tip - Vector((0, 0, .03)), (.04, .04, .025), "tail" if sy < 0 else "beacon", .01)
        pivot = tuple(tip + Vector((0, 0, .09)))
        with part(f"rotor_{k}", pivot, (0, 0, 1), "prop"):
            rod("Rotor hub", tip + Vector((0, 0, .08)), tip + Vector((0, 0, .11)), .03, "alu", sides=10)
            for a in (0.3 + k * .4, math.pi + 0.3 + k * .4):
                c, s = math.cos(a), math.sin(a)
                blade = box("Rotor blade", tip + Vector((c * .14, s * .14, .1)), (.28, .035, .008), "dark")
                blade.rotation_euler = (0, 0, a)
                cap = box("Rotor tip", tip + Vector((c * .3, s * .3, .1)), (.05, .037, .009), "red")
                cap.rotation_euler = (0, 0, a)
    for sx in (-1, 1):
        rod("Skid leg", (sx * .12, -.12, .36), (sx * .2, -.14, .03), .012, "dark")
        rod("Skid leg", (sx * .12, .14, .36), (sx * .2, .16, .03), .012, "dark")
        rod("Skid", (sx * .2, -.26, .02), (sx * .2, .28, .02), .016, "dark", sides=8)
    # gimbal camera under the nose (front is -Y)
    rod("Gimbal stem", (0, -.18, .34), (0, -.18, .28), .02, "metal")
    ico("Camera", (0, -.2, .23), (.07, .07, .06), "dark", 2)
    ico("Lens", (0, -.27, .23), (.035, .015, .035), "glass", 1)
    with part("beacon", (0, .04, .5), (0, 0, 1), "blink", 0.0):
        box("Strobe", (0, .22, .5), (.05, .03, .03), "light", .01)


def rt(v):
    """Blender (Z up, front -Y) to runtime (Y up, front +Z)."""
    return [round(v[0], 5), round(v[2], 5), round(-v[1], 5)]


BUILDERS = {"firefighter": firefighter, "engine": engine, "tanker": tanker, "drone": drone}
baked = {}
for name, build in BUILDERS.items():
    current = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(current)
    build()
    bpy.context.view_layer.update()
    groups = {}  # "" = body, otherwise the animated part
    counts = {}
    for o in current.objects:
        pos, nrm, col, idx, srf = groups.setdefault(o["part"], ([], [], [], [], []))
        mesh = o.to_mesh()
        mesh.calc_loop_triangles()
        corner = mesh.corner_normals
        colour = [srgb_to_linear(c) for c in o.data.materials[0]["srgb"]]
        surface = o.data.materials[0]["surface"]
        nm = o.matrix_world.to_3x3().inverted().transposed()
        for tri in mesh.loop_triangles:
            for vi, li in zip(tri.vertices, tri.loops):
                p = o.matrix_world @ mesh.vertices[vi].co
                n = (nm @ Vector(corner[li].vector)).normalized()
                pos.append(rt(p))
                nrm.append(rt(n))
                col.append([round(c, 5) for c in colour])
                srf.append(surface)
                idx.append(len(idx))
        counts[o.name] = counts.get(o.name, 0) + len(mesh.loop_triangles)
        o.to_mesh_clear()
    if os.environ.get("UNIT_DEBUG"):
        print(name, sorted(counts.items(), key=lambda kv: -kv[1])[:25])
    total = 0
    for part_name, (pos, nrm, col, idx, srf) in groups.items():
        key = name if not part_name else f"{name}.{part_name}"
        entry = dict(positions=pos, normals=nrm, colors=col, indices=idx, surfaces=srf)
        if part_name:
            r = RIGS[name][part_name]
            entry["rig"] = dict(pivot=rt(r["pivot"]), axis=rt(r["axis"]), kind=r["kind"], phase=round(r["phase"], 5))
        baked[key] = entry
        total += len(idx) // 3
        print(f"{key}: {len(idx) // 3} triangles")
    print(f"== {name}: {total} triangles in all")

(OUT / "units.json").write_text(json.dumps(baked, separators=(",", ":")) + "\n")
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=str(OUT / "units_assets.blend"))

# --- contact sheet (after saving the source) ---------------------------------
layout = {"firefighter": (-5, 0, 0), "engine": (0, 0, 0), "tanker": (0, 16, 2.4), "drone": (6, 0, 0)}
for name, pos in layout.items():
    for o in bpy.data.collections[name].objects:
        o.location = Vector(pos) + o.location
for tone, m in materials.items():
    m.use_nodes = True
    sh = m.node_tree.nodes.get("Principled BSDF")
    sh.inputs["Base Color"].default_value = m.diffuse_color
    sh.inputs["Roughness"].default_value = .6
current = bpy.data.collections.new("studio")
bpy.context.scene.collection.children.link(current)
PART = None
COLOURS["ground"] = (.30, .34, .22)
box("Ground", (0, 8, -.2), (50, 50, .3), "ground")
bpy.ops.object.light_add(type="SUN", location=(0, 0, 20))
bpy.context.object.data.energy = 3.5
bpy.context.object.rotation_euler = (math.radians(40), 0, math.radians(35))
bpy.ops.object.camera_add(location=(14, -16, 14))
cam = bpy.context.object
cam.rotation_euler = (Vector((-1, 7, 1.5)) - cam.location).to_track_quat("-Z", "Y").to_euler()
cam.data.type = "ORTHO"
cam.data.ortho_scale = 30
sc = bpy.context.scene
sc.camera = cam
sc.render.engine = "BLENDER_EEVEE"
sc.world = bpy.data.worlds.new("w")
sc.world.color = (.45, .5, .55)
sc.render.resolution_x, sc.render.resolution_y = 1400, 1000
sc.render.filepath = str(OUT / "units_preview.png")
bpy.ops.render.render(write_still=True)

# Optional close-ups for checking details: UNIT_CLOSEUPS=<dir> renders each model
# from the front-left and the rear-right.
if os.environ.get("UNIT_CLOSEUPS"):
    outdir = Path(os.environ["UNIT_CLOSEUPS"])
    views = {"firefighter": ((-5, 0, 1.0), 2.4), "engine": ((0, 0, 1.5), 9.0), "tanker": ((0, 16, 2.4), 22.0), "drone": ((6, 0, .4), 1.8)}
    sc.render.resolution_x, sc.render.resolution_y = 1000, 800
    for name, (centre, scale) in views.items():
        for tag, ang in (("a", -35), ("b", 145)):
            d = Vector((math.sin(math.radians(ang)), -math.cos(math.radians(ang)), .6)).normalized() * 30
            cam.location = Vector(centre) + d
            cam.rotation_euler = (Vector(centre) - cam.location).to_track_quat("-Z", "Y").to_euler()
            cam.data.ortho_scale = scale
            sc.render.filepath = str(outdir / f"{name}_{tag}.png")
            bpy.ops.render.render(write_still=True)

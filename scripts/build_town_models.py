"""Run with:
    /Applications/Blender.app/Contents/MacOS/Blender --background --python scripts/build_town_models.py

The town kit for the kiosk diorama: landmarks (church, town hall, school, fire
station, utilities, farms, lighthouse), street and beach props, and the
vehicle and people variants. Chunky, bevel-free toy shapes in a pastel
palette, one flat colour per part, baked to `assets/models/town.json` the way
`build_models.py` bakes `meshes.json` (positions, normals, colours, indices).

Blender: metres, Z up, front -Y. Runtime: metres, Y up, front +Z.
Every model has its origin at the centre of its footprint, on the ground.
Landmarks are authored at the footprint the town generator gives them
(`scripts/generate_demo_scenarios.py`, LANDMARKS) and scaled to it at runtime.

Parts whose colour is the *paint* of a car or the *clothes* of a person are
pure white, so the game's material tint (status colour, paint colour) shows
through; everything else carries its own colour.

A contact sheet is rendered to `assets/models/town_preview.png`.
"""
import bpy
import bmesh
import json
import math
from pathlib import Path
from mathutils import Vector, Matrix

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets/models"
OUT.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)

materials = {}


def mat(color):
    key = tuple(round(c, 3) for c in color)
    if key not in materials:
        m = bpy.data.materials.new(f"c{len(materials)}")
        m.diffuse_color = (*color, 1)
        m.use_nodes = True
        bsdf = m.node_tree.nodes.get("Principled BSDF")
        bsdf.inputs["Base Color"].default_value = (*color, 1)
        bsdf.inputs["Roughness"].default_value = .7
        materials[key] = m
    return materials[key]


# --- palette: pastel toy town ------------------------------------------------
CREAM = (.94, .89, .77)
OCHRE = (.92, .74, .46)
ROSE = (.90, .64, .56)
WHITE = (.96, .96, .94)
TINT = (1.0, 1.0, 1.0)          # takes the game's material tint
TERRACOTTA = (.82, .40, .25)
STONE = (.80, .76, .68)
STONE_D = (.62, .58, .52)
WOOD = (.45, .29, .16)
WOOD_D = (.30, .19, .11)
GLASS = (.16, .24, .33)
DARK = (.10, .10, .12)
RED = (.86, .14, .12)
GREEN_IT = (.0, .57, .28)
BLUE_PC = (.12, .36, .78)
YELLOW = (.99, .82, .28)
METAL = (.62, .65, .68)
ASPHALT = (.32, .33, .35)
LEAF = (.22, .45, .20)
LEAF_D = (.12, .30, .14)
SKIN = (.98, .82, .68)
HAIR = (.25, .17, .10)
GREY_HAIR = (.82, .82, .80)
SEA = (.35, .65, .88)
SAND = (.95, .87, .64)
ORANGE = (.98, .55, .16)
LIME = (.62, .85, .30)

current = None


def link(o, color):
    for c in list(o.users_collection):
        c.objects.unlink(o)
    current.objects.link(o)
    o.data.materials.clear()
    o.data.materials.append(mat(color))
    return o


def box(pos, size, color):
    bpy.ops.mesh.primitive_cube_add(size=1, location=pos)
    o = bpy.context.object
    o.scale = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return link(o, color)


def cyl(base, radius, height, color, sides=12, top=None):
    """Upright cylinder (or frustum) standing on `base`."""
    bpy.ops.mesh.primitive_cone_add(vertices=sides, radius1=radius, radius2=radius if top is None else top, depth=height,
                                    location=(base[0], base[1], base[2] + height / 2))
    return link(bpy.context.object, color)


def rod(a, b, radius, color, sides=6):
    a, b = Vector(a), Vector(b)
    bpy.ops.mesh.primitive_cylinder_add(vertices=sides, radius=radius, depth=(b - a).length, location=(a + b) / 2)
    o = bpy.context.object
    o.rotation_euler = (b - a).to_track_quat("Z", "Y").to_euler()
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=False)
    return link(o, color)


def ball(pos, size, color, subdiv=2):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=subdiv, radius=1, location=pos)
    o = bpy.context.object
    o.scale = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return link(o, color)


def from_pydata(verts, faces, color):
    me = bpy.data.meshes.new("m")
    me.from_pydata(verts, [], faces)
    me.update()
    o = bpy.data.objects.new("o", me)
    bpy.context.scene.collection.objects.link(o)
    return link(o, color)


def gable(c, sx, sy, h, color, overhang=.4):
    """Gable roof whose ridge runs along X, eaves at height c[2]."""
    x0, x1 = c[0] - sx / 2 - overhang, c[0] + sx / 2 + overhang
    y0, y1 = c[1] - sy / 2 - overhang, c[1] + sy / 2 + overhang
    z0, z1 = c[2], c[2] + h
    v = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0), (x0, c[1], z1), (x1, c[1], z1)]
    f = [(0, 1, 5, 4), (2, 3, 4, 5), (0, 4, 3), (1, 2, 5), (0, 3, 2, 1)]
    return from_pydata(v, f, color)


def gable_y(c, sx, sy, h, color, overhang=.4):
    """Gable roof whose ridge runs along Y (front to back)."""
    x0, x1 = c[0] - sx / 2 - overhang, c[0] + sx / 2 + overhang
    y0, y1 = c[1] - sy / 2 - overhang, c[1] + sy / 2 + overhang
    z0, z1 = c[2], c[2] + h
    v = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0), (c[0], y0, z1), (c[0], y1, z1)]
    f = [(1, 2, 5, 4), (3, 0, 4, 5), (0, 1, 4), (2, 3, 5), (0, 3, 2, 1)]
    return from_pydata(v, f, color)


def pyramid(c, sx, sy, h, color, top=0.0):
    x0, x1, y0, y1 = c[0] - sx / 2, c[0] + sx / 2, c[1] - sy / 2, c[1] + sy / 2
    tx, ty = sx * top / 2, sy * top / 2
    z0, z1 = c[2], c[2] + h
    v = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
         (c[0] - tx, c[1] - ty, z1), (c[0] + tx, c[1] - ty, z1), (c[0] + tx, c[1] + ty, z1), (c[0] - tx, c[1] + ty, z1)]
    f = [(0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7), (4, 5, 6, 7), (0, 3, 2, 1)]
    return from_pydata(v, f, color)


def windows_front(x0, x1, y, z0, rows, every, color=GLASS, w=1.3, h=1.5, storey=3.2):
    """A row of windows on a face at depth `y` (negative = front)."""
    for r in range(rows):
        z = z0 + 1.6 + r * storey
        x = x0 + every / 2
        while x < x1 - every / 3:
            box((x, y, z), (w, .12, h), color)
            x += every


# --- landmarks -----------------------------------------------------------------
def church():
    box((-3, 0, 5.5), (22, 14, 11), CREAM)
    gable((-3, 0, 11), 22, 14, 5, TERRACOTTA)
    box((-3, -7.05, 1.9), (3, .2, 3.8), WOOD_D)           # door
    cyl((-3, -7.1, 7.0), 1.6, .25, GLASS, 16)              # rose window, flat on the facade
    o = bpy.context.object
    o.rotation_euler = (math.pi / 2, 0, 0)
    for x in (-11, -6, 0, 5):
        box((x, -7.05, 6), (1.0, .15, 3.4), GLASS)
    box((11, 0, 12), (6, 6, 24), OCHRE)                    # campanile
    for side in (-1, 1):
        box((11, side * 3.05, 20), (2.6, .2, 3.6), DARK)  # belfry openings
        box((11 + side * 3.05, 0, 20), (.2, 2.6, 3.6), DARK)
    pyramid((11, 0, 24), 6.8, 6.8, 5, TERRACOTTA)
    box((11, 0, 30.2), (.35, .35, 2.4), METAL)             # cross
    box((11, 0, 30.6), (1.4, .3, .3), METAL)
    box((-3, -7.6, .15), (8, 1.4, .3), STONE)              # steps


def chapel():
    box((0, 0, 3.5), (10, 8, 7), WHITE)
    gable_y((0, 0, 7), 10, 8, 3, TERRACOTTA)
    box((0, -4.05, 1.6), (2, .2, 3.2), WOOD_D)
    box((0, -4.0, 10.6), (2.2, .6, 2.6), WHITE)            # bell gable
    box((0, -4.3, 10.4), (1.0, .2, 1.2), DARK)


def townhall():
    box((0, 0, 6), (24, 14, 12), OCHRE)
    pyramid((0, 0, 12), 25, 15, 3, TERRACOTTA, top=.4)
    box((0, 0, .5), (25, 15, 1), STONE)                    # base course
    windows_front(-12, 12, -7.05, 0, 3, 3.6)
    box((0, -7.1, 2.0), (3.2, .25, 4.0), WOOD_D)           # door
    box((0, -8.0, 5.0), (6, 1.8, .35), STONE)              # balcony
    box((0, -8.8, 5.7), (6, .15, 1.0), METAL)
    cyl((0, -7.2, 10.0), 1.4, .3, WHITE, 16)               # clock
    bpy.context.object.rotation_euler = (math.pi / 2, 0, 0)
    rod((7, -7.5, 12), (7, -7.5, 19), .12, METAL)          # flag pole and tricolore
    for i, col in enumerate((GREEN_IT, WHITE, RED)):
        box((7.8 + i * .9, -7.5, 18.2), (.9, .1, 1.4), col)


def school():
    box((0, 0, 4.5), (38, 18, 9), YELLOW)
    gable((0, 0, 9), 38, 18, 3, TERRACOTTA, .6)
    windows_front(-19, 19, -9.05, 0, 2, 3.2, w=2.0)
    box((0, -10.5, 3.6), (8, 3, .4), WHITE)                # entrance canopy
    for x in (-3.6, 3.6):
        rod((x, -11.8, 0), (x, -11.8, 3.5), .15, WHITE)
    box((0, -9.05, 1.6), (3, .2, 3.2), GLASS)
    rod((-16, -11, 0), (-16, -11, 8), .1, METAL)
    for i, col in enumerate((GREEN_IT, WHITE, RED)):
        box((-15.2 + i * .8, -11, 7.3), (.8, .1, 1.2), col)


def fire_station():
    box((-2, 0, 4), (22, 16, 8), WHITE)
    box((-2, -8.02, 6.8), (22, .2, 1.0), RED)              # red band
    for x in (-9, -2, 5):
        box((x, -8.05, 2.5), (5, .2, 5), RED)               # bay doors
        for z in (1.0, 2.0, 3.0, 4.0):
            box((x, -8.18, z), (4.8, .1, .12), (.70, .08, .07))
    box((-2, 0, 8.3), (22.4, 16.4, .6), STONE_D)           # parapet
    box((11, 2, 8), (4, 4, 16), RED)                       # hose tower
    pyramid((11, 2, 16), 4.6, 4.6, 1.6, STONE_D)
    for z in (5, 9, 13):
        box((11, -.05, z), (1.6, .2, 1.8), GLASS)


def fuel():
    box((0, 1, 5.2), (16, 10, .8), WHITE)                  # canopy
    box((0, 1, 4.6), (16.2, 10.2, .4), RED)
    for x in (-6, 6):
        for y in (-2.5, 4.5):
            rod((x, y, 0), (x, y, 4.6), .25, WHITE)
    for x in (-3, 3):
        box((x, 1, .25), (2.4, 4.8, .5), STONE)
        box((x, 1, 1.2), (1.0, .7, 1.8), RED)
        box((x, .62, 1.5), (.7, .1, .6), DARK)
    box((0, 8.5, 1.8), (8, 4, 3.6), CREAM)                 # kiosk
    box((0, 6.45, 1.5), (3, .1, 2.2), GLASS)
    rod((9.5, -5, 0), (9.5, -5, 9), .3, METAL)             # price sign
    box((9.5, -5, 8.5), (3.4, .5, 3), YELLOW)
    box((9.5, -5.3, 8.5), (2.8, .1, 2.2), BLUE_PC)


def water_tower():
    cyl((0, 0, 0), 1.7, 18, STONE)
    cyl((0, 0, 18), 4.6, 6, WHITE, 20)
    cyl((0, 0, 20.4), 4.65, 1.2, BLUE_PC, 20)
    cyl((0, 0, 24), 4.6, 2.2, STONE_D, 20, top=.6)
    for a in range(4):
        x, y = math.cos(a * math.pi / 2) * 4, math.sin(a * math.pi / 2) * 4
        rod((x, y, 0), (x * .3, y * .3, 18), .25, METAL)


def substation():
    box((0, 0, .15), (26, 18, .3), STONE_D)
    for x in (-13, 13):
        for y in range(-9, 10, 3):
            rod((x, y, 0), (x, y, 2.4), .08, METAL)
    for y in (-9, 9):
        for x in range(-13, 14, 3):
            rod((x, y, 0), (x, y, 2.4), .08, METAL)
    for x in (-13, 13):
        rod((x, -9, 2.3), (x, 9, 2.3), .05, METAL)
    for y in (-9, 9):
        rod((-13, y, 2.3), (13, y, 2.3), .05, METAL)
    for i, x in enumerate((-7, 0, 7)):
        box((x, -2, 1.6), (4, 3, 3.2), (.55, .62, .55))
        for dx in (-1, 0, 1):
            rod((x + dx, -2, 3.2), (x + dx, -2, 5.0), .18, (.75, .55, .35))
    # a lattice pylon
    for dx in (-1.6, 1.6):
        for dy in (-1.6, 1.6):
            rod((dx + 4, dy + 5, 0), (dx * .3 + 4, dy * .3 + 5, 20), .14, METAL)
    for z in (6, 12, 18):
        box((4, 5, z), (7, .3, .3), METAL)


def farm():
    box((-3, 0, 3.5), (16, 10, 7), OCHRE)
    gable((-3, 0, 7), 16, 10, 3.2, TERRACOTTA)
    box((-3, -5.05, 1.4), (1.6, .2, 2.8), WOOD_D)
    windows_front(-11, 5, -5.05, 0, 2, 3.4, w=1.0, h=1.3)
    box((-3, -6.2, 3.0), (6, 2.2, .3), TERRACOTTA)          # porch roof
    for x in (-5.6, -.4):
        rod((x, -7.1, 0), (x, -7.1, 3.0), .15, WOOD)
    box((8.5, 0, 1.0), (7, 9, 2), STONE)                    # yard wall
    cyl((8, -2, 0), .9, 1.0, YELLOW, 10)                     # hay
    cyl((9.5, 2, 0), .9, 1.0, YELLOW, 10)


def barn():
    box((0, 0, 3.5), (16, 11, 7), (.74, .22, .16))
    gable_y((0, 0, 7), 16, 11, 3.5, STONE_D)
    box((0, -5.55, 2.6), (5, .15, 5.2), WOOD_D)
    box((0, -5.62, 2.6), (5.2, .1, .3), WHITE)
    for x in (-6, 6):
        cyl((x, -7.5, 0), 1.0, 1.1, YELLOW, 10)
        o = bpy.context.object
        o.rotation_euler = (0, math.pi / 2, 0)
        o.location.z = 1.0


def silo():
    cyl((0, 0, 0), 3.5, 14, METAL, 18)
    cyl((0, 0, 14), 3.5, 2.5, METAL, 18, top=.4)
    for z in (3, 7, 11):
        cyl((0, 0, z), 3.55, .25, STONE_D, 18)


def mill():
    box((0, 0, 4.5), (14, 10, 9), STONE)
    gable((0, 0, 9), 14, 10, 3.5, TERRACOTTA)
    box((0, -5.05, 1.6), (2, .2, 3.2), WOOD_D)
    windows_front(-7, 7, -5.05, 0, 2, 3.5, w=1.0, h=1.2)
    # the water wheel, on the east wall
    bpy.ops.mesh.primitive_cylinder_add(vertices=16, radius=4.2, depth=1.2, location=(7.8, 0, 4.2))
    o = bpy.context.object
    o.rotation_euler = (0, math.pi / 2, 0)
    link(o, WOOD)
    for a in range(8):
        ang = a * math.pi / 8
        rod((7.8, math.cos(ang) * 4.4, 4.2 + math.sin(ang) * 4.4), (7.8, -math.cos(ang) * 4.4, 4.2 - math.sin(ang) * 4.4), .18, WOOD_D)
    box((10, 0, .3), (3, 14, .6), SEA)                      # the race


def industrial():
    box((0, 0, 4), (32, 18, 8), (.70, .76, .82))
    for i in range(4):
        x = -12 + i * 8
        v = [(x - 4, -9, 8), (x + 4, -9, 8), (x + 4, 9, 8), (x - 4, 9, 8), (x + 4, -9, 11), (x + 4, 9, 11)]
        from_pydata(v, [(0, 1, 4), (3, 5, 2), (0, 4, 5, 3), (1, 2, 5, 4)], (.55, .60, .66))
        box((x + 3.9, 0, 9.5), (.2, 18, 2.6), GLASS)
    for x in (-8, 4):
        box((x, -9.05, 2.6), (6, .2, 5.2), (.85, .70, .25))
    box((0, -9.06, 6.6), (32, .1, .5), YELLOW)


def lighthouse():
    cyl((0, 0, 0), 4.0, 4, WHITE, 18)
    for i in range(5):
        cyl((0, 0, 4 + i * 4), 2.8 - i * .12, 4, RED if i % 2 == 0 else WHITE, 16, top=2.8 - (i + 1) * .12)
    cyl((0, 0, 24), 2.6, .5, DARK, 16)
    cyl((0, 0, 24.5), 1.6, 2.4, (1.0, .95, .6), 12)
    cyl((0, 0, 26.9), 1.9, 1.6, RED, 12, top=.2)


# --- props -----------------------------------------------------------------------
def cypress():
    cyl((0, 0, 0), .25, 1.5, WOOD, 6)
    cyl((0, 0, 1.0), 1.3, 9.5, LEAF_D, 8, top=.1)


def street_tree():
    cyl((0, 0, 0), .25, 2.6, WOOD, 6)
    ball((0, 0, 4.0), (2.4, 2.4, 2.0), LEAF, 1)
    ball((.8, .5, 4.9), (1.5, 1.5, 1.3), (.30, .55, .25), 1)


def fountain():
    cyl((0, 0, 0), 4.0, .8, STONE, 20)
    cyl((0, 0, .8), 3.5, .05, SEA, 20)
    cyl((0, 0, 0), .8, 2.6, STONE, 10)
    cyl((0, 0, 2.6), 1.6, .4, STONE, 14)
    cyl((0, 0, 3.0), .3, 1.0, SEA, 8, top=.05)


def bench():
    box((0, 0, .45), (2.0, .5, .08), WOOD)
    box((0, .25, .75), (2.0, .08, .5), WOOD)
    for x in (-.8, .8):
        box((x, 0, .22), (.1, .5, .45), DARK)


def umbrella_a():
    rod((0, 0, 0), (0, 0, 2.4), .05, WHITE)
    cyl((0, 0, 2.0), 1.4, .7, (.95, .35, .30), 8, top=.05)


def umbrella_b():
    rod((0, 0, 0), (0, 0, 2.4), .05, WHITE)
    cyl((0, 0, 2.0), 1.4, .7, (.25, .55, .90), 8, top=.05)


def lounger():
    box((0, 0, .35), (.7, 1.9, .1), WHITE)
    box((0, .8, .6), (.7, .5, .1), WHITE)


def beach_hut():
    box((0, 0, 1.3), (5, 3.5, 2.6), (.30, .62, .85))
    for x in (-2, -1, 0, 1, 2):
        box((x, -1.78, 1.3), (.45, .06, 2.4), WHITE)
    gable((0, 0, 2.6), 5, 3.5, 1.0, WHITE, .3)


def boat():
    v = [(-1.1, -3.2, .2), (1.1, -3.2, .2), (1.1, 2.4, .2), (-1.1, 2.4, .2),
         (-1.3, -3.4, 1.0), (1.3, -3.4, 1.0), (1.3, 2.6, 1.0), (-1.3, 2.6, 1.0), (0, 4.0, 1.0)]
    from_pydata(v, [(0, 1, 5, 4), (1, 2, 6, 5), (3, 0, 4, 7), (2, 3, 8), (6, 2, 8), (3, 7, 8), (0, 3, 2, 1), (4, 5, 6, 8, 7)], WHITE)
    box((0, -1.4, 1.5), (1.6, 1.8, 1.0), (.20, .50, .80))
    box((0, -1.4, 2.05), (1.8, 2.0, .12), WHITE)
    box((0, 0, .45), (2.62, 6.8, .25), (.20, .50, .80))


def sailboat():
    v = [(-.9, -2.8, .2), (.9, -2.8, .2), (.9, 2.2, .2), (-.9, 2.2, .2),
         (-1.0, -3.0, .9), (1.0, -3.0, .9), (1.0, 2.4, .9), (-1.0, 2.4, .9), (0, 3.6, .9)]
    from_pydata(v, [(0, 1, 5, 4), (1, 2, 6, 5), (3, 0, 4, 7), (2, 3, 8), (6, 2, 8), (3, 7, 8), (0, 3, 2, 1), (4, 5, 6, 8, 7)], WHITE)
    rod((0, 0, .9), (0, 0, 9), .08, METAL)
    from_pydata([(0, .2, 1.6), (0, .2, 8.6), (0, 3.0, 1.6)], [(0, 1, 2), (0, 2, 1)], WHITE)


def pier():
    box((0, 0, .6), (1, 1, .3), WOOD)
    for x in (-.45, .45):
        for y in (-.45, .45):
            rod((x, y, -2), (x, y, .5), .04, WOOD_D)


def pc_tent():
    box((0, 0, 1.2), (6, 4, 2.4), BLUE_PC)
    gable_y((0, 0, 2.4), 6, 4, 1.2, BLUE_PC, .1)
    box((0, -2.05, 1.0), (1.4, .1, 2.0), (.08, .22, .55))
    box((0, -2.08, 2.0), (2.6, .05, .5), WHITE)


def camp_tent():
    v = [(-1.2, -1.6, 0), (1.2, -1.6, 0), (1.2, 1.6, 0), (-1.2, 1.6, 0), (0, -1.6, 1.6), (0, 1.6, 1.6)]
    from_pydata(v, [(1, 2, 5, 4), (3, 0, 4, 5), (0, 1, 4), (2, 3, 5)], ORANGE)


def camp_tent_b():
    v = [(-1.2, -1.6, 0), (1.2, -1.6, 0), (1.2, 1.6, 0), (-1.2, 1.6, 0), (0, -1.6, 1.6), (0, 1.6, 1.6)]
    from_pydata(v, [(1, 2, 5, 4), (3, 0, 4, 5), (0, 1, 4), (2, 3, 5)], LIME)


def caravan():
    box((0, 0, 1.5), (2.3, 6, 2.2), WHITE)
    box((0, 0, 1.1), (2.32, 6.02, .3), (.30, .55, .80))
    box((1.17, -1, 1.8), (.05, 1.4, .7), GLASS)
    box((-1.17, 1, 1.8), (.05, 1.4, .7), GLASS)
    for y in (-.5,):
        for x in (-1.15, 1.15):
            rod((x, y, .35), (x * 1.08, y, .35), .35, DARK, 10)
    rod((0, -3.0, .5), (0, -3.9, .45), .06, METAL)


def goal():
    for x in (-3.6, 3.6):
        rod((x, 0, 0), (x, 0, 2.4), .08, WHITE)
        rod((x, 0, 2.4), (x, 1.5, 0), .04, WHITE)
    rod((-3.6, 0, 2.4), (3.6, 0, 2.4), .08, WHITE)


def assembly_sign():
    for x in (-1.6, 1.6):
        rod((x, 0, 0), (x, 0, 4.2), .1, METAL)
    box((0, 0, 3.4), (4.2, .15, 2.2), (.10, .60, .30))
    box((0, -.09, 3.4), (3.8, .05, 1.9), WHITE)
    box((0, -.11, 3.4), (3.5, .05, 1.6), (.10, .60, .30))
    # the standard pictogram: a family of three, white
    for i, (x, s) in enumerate(((-.9, 1.0), (0, .8), (.8, .6))):
        ball((x, -.15, 3.55 + .25 * s), (.18 * s, .05, .18 * s), WHITE, 1)
        box((x, -.15, 3.0 + .2 * s), (.28 * s, .05, .55 * s), WHITE)


def streetlight():
    rod((0, 0, 0), (0, 0, 6), .1, DARK)
    rod((0, 0, 6), (0, -1.2, 6.3), .06, DARK)
    box((0, -1.3, 6.2), (.4, .7, .2), (1.0, .95, .7))


def parked_car():
    vehicle_body("hatch", paint=True)


# --- vehicles ----------------------------------------------------------------------
def tyres(l, w, r, axles=2):
    ys = [-l * .31, l * .31] if axles == 2 else [-l * .30, 0, l * .30]
    for y in ys:
        for side in (-1, 1):
            rod((side * (w / 2 - .12), y, r), (side * (w / 2 + .06), y, r), r, DARK, 10)


def vehicle_body(kind, paint=False):
    P = TINT
    if kind == "hatch":
        tyres(3.9, 1.75, .33)
        box((0, 0, .65), (1.75, 3.9, .7), P)
        box((0, .35, 1.25), (1.5, 2.0, .6), GLASS)
        box((0, .4, 1.6), (1.45, 1.8, .12), P)
    elif kind == "sedan":
        tyres(4.6, 1.8, .34)
        box((0, 0, .66), (1.8, 4.6, .66), P)
        box((0, .1, 1.24), (1.55, 2.3, .55), GLASS)
        box((0, .1, 1.55), (1.5, 2.1, .1), P)
    elif kind == "suv":
        tyres(4.5, 1.95, .42)
        box((0, 0, .9), (1.95, 4.5, .95), P)
        box((0, .3, 1.7), (1.8, 3.0, .7), GLASS)
        box((0, .35, 2.1), (1.75, 2.8, .14), P)
        box((0, .4, 2.25), (1.4, 2.0, .1), DARK)          # roof rack
    elif kind == "van":
        tyres(5.0, 2.0, .38)
        box((0, .45, 1.35), (2.0, 4.1, 2.2), P)
        box((0, -2.05, .95), (1.95, 1.0, 1.4), P)
        box((0, -1.6, 1.85), (1.8, .5, .8), GLASS)
    elif kind == "ape":
        # the three-wheeled Piaggio, the farm truck of every Italian village
        rod((0, -1.2, .3), (0, -1.0, .3), .3, DARK, 10)
        for side in (-1, 1):
            rod((side * .55, .9, .3), (side * .75, .9, .3), .3, DARK, 10)
        box((0, -.9, 1.1), (1.25, 1.1, 1.4), P)
        box((0, -1.42, 1.35), (1.1, .1, .6), GLASS)
        box((0, .8, .75), (1.4, 1.8, .4), WOOD)
        box((0, .8, 1.05), (1.4, 1.8, .1), WOOD_D)
    elif kind == "bus":
        tyres(10.5, 2.5, .5)
        box((0, 0, 1.8), (2.5, 10.5, 2.8), P)
        box((0, .2, 2.35), (2.52, 9.4, .9), GLASS)
        box((0, -5.26, 2.3), (2.2, .05, 1.4), GLASS)
        box((0, 0, 3.25), (2.3, 9.8, .15), WHITE)
    elif kind == "camper":
        tyres(6.0, 2.2, .4)
        box((0, .5, 1.6), (2.2, 4.9, 2.4), P)
        box((0, -2.4, 1.05), (2.1, 1.2, 1.3), P)
        box((0, -1.9, 2.5), (2.15, 1.2, .9), P)
        box((0, -2.9, 1.5), (1.9, .4, .6), GLASS)
        box((1.11, .8, 2.0), (.05, 1.6, .7), GLASS)
    # lights
    l = {"hatch": 3.9, "sedan": 4.6, "suv": 4.5, "van": 5.0, "ape": 2.6, "bus": 10.5, "camper": 6.0}[kind]
    for side in (-1, 1):
        box((side * .55, -l / 2 - .02, .8), (.35, .06, .18), (1.0, .96, .75))
        box((side * .6, l / 2 + .02, .8), (.3, .06, .18), RED)


# --- people ----------------------------------------------------------------------
def figure(kind):
    """Chunky bean figures about 1.8 m (child 1.1 m), facing -Y, clothes white."""
    s = .62 if kind == "child" else 1.0
    if kind == "woman":
        cyl((0, 0, 0), .5 * s, 1.1 * s, TINT, 10, top=.32 * s)
        ball((0, 0, 1.12 * s), (.36 * s, .32 * s, .34 * s), TINT)
    else:
        ball((0, 0, .62 * s), (.44 * s, .38 * s, .62 * s), TINT)
        box((0, 0, .1 * s), (.6 * s, .5 * s, .2 * s), (.25, .22, .20))
    head = 1.50 * s if kind != "elder" else 1.42
    ball((0, -.04 if kind == "elder" else 0, head), (.34 * s, .32 * s, .34 * s), SKIN)
    hair = GREY_HAIR if kind == "elder" else HAIR
    ball((0, .06, head + .16 * s), (.33 * s, .30 * s, .2 * s), hair, 1)
    if kind == "elder":
        rod((.5, -.3, 0), (.42, -.25, 1.0), .05, WOOD)
    if kind == "child":
        box((0, .3, .75 * s), (.4 * s, .2 * s, .45 * s), (.95, .55, .20))   # school bag


BUILDERS = {
    # landmarks (authored at their generator footprint, metres)
    "church": (church, (30, 18)), "chapel": (chapel, (14, 10)), "townhall": (townhall, (26, 16)),
    "school": (school, (40, 22)), "fire_station": (fire_station, (28, 18)), "fuel": (fuel, (22, 14)),
    "water_tower": (water_tower, (10, 10)), "substation": (substation, (26, 18)), "farm": (farm, (26, 14)),
    "barn": (barn, (18, 12)), "silo": (silo, (8, 8)), "mill": (mill, (22, 16)), "industrial": (industrial, (34, 20)),
    "lighthouse": (lighthouse, (10, 10)),
    # props
    "cypress": (cypress, None), "street_tree": (street_tree, None), "fountain": (fountain, None), "bench": (bench, None),
    "umbrella_a": (umbrella_a, None), "umbrella_b": (umbrella_b, None), "lounger": (lounger, None), "beach_hut": (beach_hut, None),
    "boat": (boat, None), "sailboat": (sailboat, None), "pier": (pier, None), "pc_tent": (pc_tent, None),
    "camp_tent": (camp_tent, None), "camp_tent_b": (camp_tent_b, None), "caravan": (caravan, None), "goal": (goal, None),
    "assembly_sign": (assembly_sign, None), "streetlight": (streetlight, None),
    # vehicles (body takes the paint tint)
    "car_hatch": (lambda: vehicle_body("hatch"), None), "car_sedan": (lambda: vehicle_body("sedan"), None),
    "car_suv": (lambda: vehicle_body("suv"), None), "car_van": (lambda: vehicle_body("van"), None),
    "car_ape": (lambda: vehicle_body("ape"), None), "car_bus": (lambda: vehicle_body("bus"), None),
    "car_camper": (lambda: vehicle_body("camper"), None),
    # people (clothes take the status tint)
    "person_man": (lambda: figure("man"), None), "person_woman": (lambda: figure("woman"), None),
    "person_child": (lambda: figure("child"), None), "person_elder": (lambda: figure("elder"), None),
}

baked = {}
for name, (build, footprint) in BUILDERS.items():
    current = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(current)
    build()
    bpy.context.view_layer.update()
    positions, normals, colors, indices = [], [], [], []
    deps = bpy.context.evaluated_depsgraph_get()
    for o in current.objects:
        eo = o.evaluated_get(deps)
        mesh = eo.to_mesh()
        mesh.calc_loop_triangles()
        color = o.data.materials[0].diffuse_color[:]
        mw = o.matrix_world
        nm = mw.to_3x3().inverted().transposed()
        for tri in mesh.loop_triangles:
            if tri.area < 1e-7:
                continue
            n = (nm @ tri.normal).normalized()
            for vi in tri.vertices:
                p = mw @ mesh.vertices[vi].co
                positions.append([round(p.x, 4), round(p.z, 4), round(-p.y, 4)])
                normals.append([round(n.x, 4), round(n.z, 4), round(-n.y, 4)])
                colors.append([round(c, 4) for c in color[:3]])
                indices.append(len(indices))
        eo.to_mesh_clear()
    baked[name] = dict(positions=positions, normals=normals, colors=colors, indices=indices,
                       footprint=list(footprint) if footprint else None)
    print(f"{name}: {len(indices) // 3} triangles")

(OUT / "town.json").write_text(json.dumps(baked, separators=(",", ":")) + "\n")

# --- contact sheet -------------------------------------------------------------------
names = list(BUILDERS)
cols = 8
for i, name in enumerate(names):
    col = bpy.data.collections[name]
    x, y = (i % cols) * 42.0, -(i // cols) * 40.0
    k = 1.0 if BUILDERS[name][1] else (4.0 if name.startswith(("person", "car", "bench", "lounger", "umbrella")) else 2.0)
    for o in col.objects:
        o.matrix_world = Matrix.Translation((x, y, 0)) @ Matrix.Scale(k, 4) @ o.matrix_world
current = bpy.data.collections.new("studio")
bpy.context.scene.collection.children.link(current)
box((150, -80, -.3), (420, 260, .5), (.78, .80, .74))
bpy.ops.object.light_add(type="SUN", location=(0, 0, 50))
bpy.context.object.data.energy = 4
bpy.context.object.rotation_euler = (math.radians(40), math.radians(10), math.radians(-35))
bpy.ops.object.camera_add(location=(150, -330, 260))
cam = bpy.context.object
cam.rotation_euler = (Vector((150, -85, 0)) - cam.location).to_track_quat("-Z", "Y").to_euler()
cam.data.type = "ORTHO"
cam.data.ortho_scale = 360
scene = bpy.context.scene
scene.camera = cam
scene.world = bpy.data.worlds.new("w")
scene.world.color = (.6, .6, .62)
scene.render.engine = "BLENDER_EEVEE" if "BLENDER_EEVEE" in [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items] else "BLENDER_EEVEE_NEXT"
scene.render.resolution_x, scene.render.resolution_y = 2000, 1300
scene.render.filepath = str(OUT / "town_preview.png")
bpy.ops.render.render(write_still=True)
bpy.ops.wm.save_as_mainfile(filepath=str(OUT / "town_assets.blend"))

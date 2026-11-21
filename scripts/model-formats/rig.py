# SPDX-License-Identifier: GPL-3.0-or-later
"""Writes one rigged model as Blender writes one, in text and binary fbx: five limbs under
an armature, a `Null` model that is moved, turned, scaled and animated; a mesh model with a
geometric transform; a bind pose that is not the limbs' own; and a vertex of five weights.

    python3 scripts/model-formats/rig.py crates/qs-mdl/src/mesh/fixtures

`blender.py` records what Blender makes of the binary file."""
import math, os, struct, sys
from fbxbin import hexed, named, p70, record, text

out = sys.argv[1]
os.makedirs(out, exist_ok=True)

# A pose is three rows of four: a matrix and, last in each row, the move.
def turn(degrees):
    """A turn about x, then y, then z."""
    x, y, z = (math.radians(a) for a in degrees)
    cx, sx, cy, sy, cz, sz = math.cos(x), math.sin(x), math.cos(y), math.sin(y), math.cos(z), math.sin(z)
    rx = [[1, 0, 0], [0, cx, -sx], [0, sx, cx]]
    ry = [[cy, 0, sy], [0, 1, 0], [-sy, 0, cy]]
    rz = [[cz, -sz, 0], [sz, cz, 0], [0, 0, 1]]
    mul = lambda a, b: [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    return mul(rz, mul(ry, rx))
def pose(move, degrees, scale):
    r = turn(degrees)
    return [[r[i][j] * scale[j] for j in range(3)] + [move[i]] for i in range(3)]
def after(a, b):
    """`a` after `b`."""
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)]
            + [sum(a[i][k] * b[k][3] for k in range(3)) + a[i][3]] for i in range(3)]
def inverse(a):
    m = [row[:3] for row in a]
    cross = lambda u, v: [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    det = sum(m[0][i] * cross(m[1], m[2])[i] for i in range(3))
    rows = [cross(m[1], m[2]), cross(m[2], m[0]), cross(m[0], m[1])]
    inv = [[rows[j][i] / det for j in range(3)] for i in range(3)]
    return [inv[i] + [-sum(inv[i][k] * a[k][3] for k in range(3))] for i in range(3)]
def columns(a):
    """A pose as a file writes a matrix: four columns of four."""
    return [float(a[i][j]) if i < 3 else (1.0 if j == 3 else 0.0) for j in range(4) for i in range(4)]

SEC = 46186158000
TIMES = [k * SEC // 10 for k in range(5)]                      # a key every tenth of a second
V = [(0, 0, 0), (0, 4, 0), (0, 0, 4), (16, 0, 0), (8, 2, 0)]
UV = [(0.1, 0.2), (0.9, 0.2), (0.1, 0.8), (0.9, 0.8), (0.5, 0.5)]
# The polygons: a quad and three triangles.
POLY = [(0, 1, 4, 3), (0, 2, 1), (0, 3, 2), (1, 2, 3)]
# Each vertex's weights, by limb: the last follows all five, the lightest by a twentieth.
BL = [[(0, 1.0)], [(0, 1.0)], [(0, 1.0)], [(1, 1.0)], [(0, 0.3), (1, 0.3), (2, 0.2), (3, 0.15), (4, 0.05)]]
ARMATURE = ((3.0, 0.0, -2.0), (-90.0, 0.0, 0.0), (2.0, 2.0, 2.0))
# Each limb: its name, its parent among the limbs, where it stands, how it is turned at
# rest, how it was turned when the mesh was bound, and its turn about z at each key.
LIMBS = [
    ("root", None, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), None),
    ("arm", 0, (8.0, 0.0, 0.0), (0.0, 0.0, 10.0), (0.0, 0.0, -20.0), [10, 20, 45, 70, 90]),
    ("first", 1, (4.0, 1.0, 0.0), (0.0, 0.0, 0.0), (0.0, 15.0, 0.0), [0, 30, 60, 30, 0]),
    ("second", 1, (4.0, 0.0, 1.0), (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), [0, -20, -40, -60, -80]),
    ("third", 1, (4.0, -1.0, 0.0), (20.0, 0.0, 0.0), (0.0, 0.0, 0.0), [0, 45, 0, -45, 0]),
]
MESH = ((1.0, -1.0, 0.5), (0.0, 0.0, 30.0), (2.0, 1.5, 2.0))
GEOMETRIC = ((1.0, 0.0, 0.0), (0.0, 10.0, 0.0), (1.0, 1.0, 0.5))
# The armature rises along its own y from its first key to its last: two keys, where the
# limbs' curves have five, so the animation is sampled by its keys' times.
RISE = [0.0, 3.0]

ID = 2 ** 32                                                    # a 64-bit number, as programs write
def lcl(move, degrees, scale):
    return [("Lcl Translation", "Lcl Translation", "", "A", *move), ("Lcl Rotation", "Lcl Rotation", "", "A", *degrees),
            ("Lcl Scaling", "Lcl Scaling", "", "A", *scale)]
def model(number, name, kind, lines):
    return ("Model", [ID + number, named(name, "Model"), kind], [("Version", [232], []), p70(*lines)])
def curve_node(number, values):
    return ("AnimationCurveNode", [ID + number, named("n", "AnimCurveNode"), ""],
            [p70(*[("d|" + axis, "Number", "", "A", float(v)) for axis, v in zip("XYZ", values)])])
def curve(number, times, values):
    return ("AnimationCurve", [ID + number, named("", "AnimCurve"), ""], [
        ("Default", [0.0], []), ("KeyVer", [4008], []), ("KeyTime", [("l", times)], []),
        ("KeyValueFloat", [("f", [float(v) for v in values])], []),
        # Each key is joined to the next by a straight line.
        ("KeyAttrFlags", [("i", [1 << 2])], []), ("KeyAttrDataFloat", [("f", [0.0, 0.0, 0.0, 0.0])], []),
        ("KeyAttrRefCount", [("i", [len(times)])], [])])

# Where each limb stood when the mesh was bound, and where the mesh did.
armature = pose(*ARMATURE)
bound = []
for name, parent, move, rest, bind, keys in LIMBS:
    above = armature if parent is None else bound[parent]
    bound.append(after(above, pose(move, bind, (1.0, 1.0, 1.0))))
mesh = pose(*MESH)

corners, uv_index = [], []
for polygon in POLY:
    corners += list(polygon[:-1]) + [-(polygon[-1] + 1)]
    uv_index += list(polygon)
objects = [
    ("Geometry", [ID + 100, named("rig", "Geometry"), "Mesh"], [
        ("Vertices", [("d", [float(x) for p in V for x in p])], []),
        ("PolygonVertexIndex", [("i", corners)], []),
        ("LayerElementUV", [0], [
            ("Version", [101], []), ("Name", ["UVMap"], []), ("MappingInformationType", ["ByPolygonVertex"], []),
            ("ReferenceInformationType", ["IndexToDirect"], []),
            ("UV", [("d", [x for uv in UV for x in (uv[0], 1 - uv[1])])], []), ("UVIndex", [("i", uv_index)], [])]),
        ("Layer", [0], [("Version", [100], []), ("LayerElement", [], [("Type", ["LayerElementUV"], []), ("TypedIndex", [0], [])])]),
    ]),
    model(200, "rig", "Mesh", lcl(*MESH) + [
        ("GeometricTranslation", "Vector3D", "Vector", "", *GEOMETRIC[0]), ("GeometricRotation", "Vector3D", "Vector", "", *GEOMETRIC[1]),
        ("GeometricScaling", "Vector3D", "Vector", "", *GEOMETRIC[2])]),
    ("NodeAttribute", [ID + 240, named("Armature", "NodeAttribute"), "Null"], [("TypeFlags", ["Null"], [])]),
    model(250, "Armature", "Null", lcl(*ARMATURE)),
    ("Deformer", [ID + 400, named("rig", "Deformer"), "Skin"], [("Version", [101], [])]),
    ("AnimationStack", [ID + 500, named("wave", "AnimStack"), ""], [
        p70(("LocalStop", "KTime", "Time", "", TIMES[-1]), ("ReferenceStop", "KTime", "Time", "", TIMES[-1]))]),
    ("AnimationLayer", [ID + 501, named("base", "AnimLayer"), ""], []),
    curve_node(590, ARMATURE[0]), curve(591, [TIMES[0], TIMES[-1]], RISE),
]
joins = [("OO", 100, 200), ("OO", 200, 0), ("OO", 240, 250), ("OO", 250, 0), ("OO", 400, 100), ("OO", 501, 500),
         ("OO", 590, 501), ("OP", 590, 250, "Lcl Translation"), ("OP", 591, 590, "d|Y")]
for at, (name, parent, move, rest, bind, keys) in enumerate(LIMBS):
    limb, attribute, cluster, node = 300 + at, 350 + at, 410 + at, 510 + 2 * at
    pairs = [(i, weight) for i, blends in enumerate(BL) for j, weight in blends if j == at]
    objects += [
        ("NodeAttribute", [ID + attribute, named(name, "NodeAttribute"), "LimbNode"], [("TypeFlags", ["Skeleton"], [])]),
        model(limb, name, "LimbNode", lcl(move, rest, (1.0, 1.0, 1.0))),
        ("Deformer", [ID + cluster, named(name, "SubDeformer"), "Cluster"], [
            ("Version", [100], []), ("Indexes", [("i", [i for i, _ in pairs])], []), ("Weights", [("d", [w for _, w in pairs])], []),
            # The mesh in the limb's space, and the limb in the file's, when the mesh was bound.
            ("Transform", [("d", columns(after(inverse(bound[at]), mesh)))], []),
            ("TransformLink", [("d", columns(bound[at]))], [])]),
    ]
    joins += [("OO", attribute, limb), ("OO", limb, 250 if parent is None else 300 + parent), ("OO", cluster, 400), ("OO", limb, cluster)]
    if keys:
        objects += [curve_node(node, rest), curve(node + 1, TIMES, keys)]
        joins += [("OO", node, 501), ("OP", node, limb, "Lcl Rotation"), ("OP", node + 1, node, "d|Z")]

settings = [("UpAxis", "int", "Integer", "", 1), ("UpAxisSign", "int", "Integer", "", 1), ("FrontAxis", "int", "Integer", "", 2),
            ("FrontAxisSign", "int", "Integer", "", 1), ("CoordAxis", "int", "Integer", "", 0), ("CoordAxisSign", "int", "Integer", "", 1),
            ("UnitScaleFactor", "double", "Number", "", 1.0),
            # Thirty frames a second, so a key every tenth of a second is on a frame.
            ("TimeMode", "enum", "", "", 6), ("TimeSpanStart", "KTime", "Time", "", 0), ("TimeSpanStop", "KTime", "Time", "", TIMES[-1])]
top = [
    ("FBXHeaderExtension", [], [("FBXHeaderVersion", [1003], []), ("FBXVersion", [7400], []), ("Creator", ["rig.py"], [])]),
    ("GlobalSettings", [], [("Version", [1000], []), p70(*settings)]),
    ("Objects", [], objects),
    ("Connections", [], [("C", [j[0], ID + j[1], ID + j[2] if j[2] else 0] + list(j[3:]), []) for j in joins]),
]

open(f"{out}/rig.fbx.txt", "w").write("; FBX 7.4.0 project file\n" + "".join(text(node) for node in top))
data = b"Kaydara FBX Binary  \x00\x1a\x00" + struct.pack("<I", 7400)
for node in top:
    data += record(*node, False, len(data))
data += bytes(13) + b"\xfa\xbc\xab\x09 a footer the readers pass over"
open(f"{out}/rig-7400.fbx.hex", "w").write(hexed(data))

# SPDX-License-Identifier: GPL-3.0-or-later
"""Writes one small boned model in md5, smd and fbx: two joints, five vertices, five
triangles, an animation of three frames."""
import math, os, sys
out = sys.argv[1]
HARD = len(sys.argv) > 2
S45 = math.sin(math.radians(45))
os.makedirs(out, exist_ok=True)
V = [(0, 0, 0), (0, 4, 0), (0, 0, 4), (16, 0, 0), (8, 2, 0)]
UV = [(0.1, 0.2), (0.9, 0.2), (0.1, 0.8), (0.9, 0.8), (0.5, 0.5)]
BL = [[(0, 1.0)], [(0, 1.0)], [(0, 1.0)], [(1, 1.0)], [(0, 0.5), (1, 0.5)]]
TRI = [(0, 1, 2), (0, 2, 3), (0, 3, 1), (1, 3, 2), (1, 4, 3)]   # as a model has them
JOINT = [(0, 0, 0), (8, 0, 0)]                                    # each in the model's space
ANGLES = [0, 45, 90]
LIFT = [0, 1, 2]

# ---- md5
w, verts = [], []
for i, (p, uv, bl) in enumerate(zip(V, UV, BL)):
    verts.append(f"\tvert {i} ( {uv[0]} {uv[1]} ) {len(w)} {len(bl)}")
    for j, bias in bl:
        rel = [p[k] - JOINT[j][k] for k in range(3)]
        if HARD and j == 1:
            rel = [rel[1], -rel[0], rel[2]]
        w.append(f"\tweight {len(w)} {j} {bias} ( {rel[0]} {rel[1]} {rel[2]} )")
mesh = ['MD5Version 10', 'commandline ""', '', 'numJoints 2', 'numMeshes 1', '', 'joints {',
        '\t"root" -1 ( 0 0 0 ) ( 0 0 0 )', ('\t"arm" 0 ( 8 0 0 ) ( 0 0 %.9f )' % -S45 if HARD else '\t"arm" 0 ( 8 0 0 ) ( 0 0 0 )'), '}', '', 'mesh {', '\tshader "skin"',
        f'\tnumverts {len(V)}'] + verts + [f'\tnumtris {len(TRI)}'] + [f"\ttri {i} {a} {b} {c}" for i, (a, b, c) in enumerate(TRI)] + [f'\tnumweights {len(w)}'] + w + ['}', '']
open(f"{out}/arm.md5mesh.txt", "w").write("\n".join(mesh))
anim = ['MD5Version 10', 'commandline ""', '', 'numFrames 3', 'numJoints 2', 'frameRate 10', 'numAnimatedComponents 2', '',
        'hierarchy {', '\t"root" -1 4 0', '\t"arm" 0 32 1', '}', '', 'bounds {'] + ['\t( -1 -1 -1 ) ( 20 20 20 )'] * 3 + ['}', '',
       'baseframe {', '\t( 0 0 0 ) ( 0 0 0 )', ('\t( 8 0 0 ) ( 0 0 %.9f )' % -S45 if HARD else '\t( 8 0 0 ) ( 0 0 0 )'), '}', '']
for f, (a, z) in enumerate(zip(ANGLES, LIFT)):
    anim += [f'frame {f} {{', f'\t{z} {-math.sin(math.radians(a + (90 if HARD else 0)) / 2):.9f}', '}', '']
open(f"{out}/wave.md5anim.txt", "w").write("\n".join(anim))

# ---- smd
def smd_vert(i):
    p, uv, bl = V[i], UV[i], BL[i]
    links = f" {len(bl)} " + " ".join(f"{j} {b}" for j, b in bl) if len(bl) > 1 else ""
    return f"{bl[0][0]} {p[0]} {p[1]} {p[2]} 0 0 1 {uv[0]} {1 - uv[1]}{links}"
ref = ['version 1', 'nodes', '0 "root" -1', '1 "arm" 0', 'end', 'skeleton', 'time 0', '0 0 0 0 0 0 0', ('1 8 0 0 0 0 1.570796327' if HARD else '1 8 0 0 0 0 0'), 'end', 'triangles']
for a, b, c in TRI:
    ref += ['skin.bmp', smd_vert(c), smd_vert(b), smd_vert(a)]
ref += ['end', '']
open(f"{out}/arm.smd.txt", "w").write("\n".join(ref))
ani = ['version 1', 'nodes', '0 "root" -1', '1 "arm" 0', 'end', 'skeleton']
for f, (a, z) in enumerate(zip(ANGLES, LIFT)):
    if HARD and f == 1:
        continue
    ani += [f'time {f}', f'0 0 0 {z} 0 0 0'] + ([] if HARD and f == 2 else [f'1 8 0 0 0 0 {math.radians(a + (90 if HARD else 0)):.9f}'])
ani += ['end', '']
open(f"{out}/wave.smd.txt", "w").write("\n".join(ani))

# ---- fbx
def arr(name, vals):
    return f"\t\t{name}: *{len(vals)} {{\n\t\t\ta: {','.join(str(v) for v in vals)}\n\t\t}}"
poly, uvi = [], []
for a, b, c in TRI:
    poly += [b, a, -(c + 1)]
    uvi += [b, a, c]
def cluster(idn, joint):
    pairs = [(i, wt) for i, bl in enumerate(BL) for j, wt in bl if j == joint]
    return f'\tDeformer: {idn}, "SubDeformer::", "Cluster" {{\n' + arr("Indexes", [i for i, _ in pairs]) + "\n" + arr("Weights", [wt for _, wt in pairs]) + "\n\t}"
def limb(idn, name, t):
    pre = '\n\t\t\tP: "PreRotation", "Vector3D", "Vector", "",0,0,90' if HARD and name == "arm" else ""
    return f'\tModel: {idn}, "Model::{name}", "LimbNode" {{\n\t\tProperties70:  {{\n\t\t\tP: "Lcl Translation", "Lcl Translation", "", "A",{t[0]},{t[1]},{t[2]}{pre}\n\t\t}}\n\t}}'
def node(idn):
    return f'\tAnimationCurveNode: {idn}, "AnimCurveNode::n", "" {{\n\t\tProperties70:  {{\n\t\t\tP: "d|X", "Number", "", "A",0\n\t\t\tP: "d|Y", "Number", "", "A",0\n\t\t\tP: "d|Z", "Number", "", "A",0\n\t\t}}\n\t}}'
def curve(idn, vals):
    return f'\tAnimationCurve: {idn}, "AnimCurve::", "" {{\n\t\tDefault: 0\n\t\tKeyVer: 4008\n' + arr("KeyTime", [0, 1, 2]) + "\n" + arr("KeyValueFloat", vals) + "\n\t}"
fbx = ['; FBX 7.3.0 project file', 'FBXHeaderExtension:  {', '\tFBXHeaderVersion: 1003', '\tCreator: "samples.py"', '}', 'Objects:  {',
       '\tGeometry: 100, "Geometry::arm", "Mesh" {', arr("Vertices", [x for p in V for x in p]), arr("PolygonVertexIndex", poly),
       '\t\tLayerElementNormal: 0 {\n\t\t\tVersion: 101\n\t\t\tMappingInformationType: "ByPolygonVertex"', arr("Normals", [x for _ in poly for x in (0, 0, 1)]), '\t\t}',
       '\t\tLayerElementUV: 0 {\n\t\t\tVersion: 101\n\t\t\tName: "UVMap"\n\t\t\tMappingInformationType: "ByPolygonVertex"',
       arr("UV", [x for uv in UV for x in (uv[0], 1 - uv[1])]), arr("UVIndex", uvi), '\t\t}', '\t}',
       '\tModel: 200, "Model::arm", "Mesh" {\n\t\tVersion: 232\n\t\tProperties70:  {\n\t\t\tP: "Lcl Scaling", "Lcl Scaling", "", "A",1,1,1\n\t\t}\n\t\tShading: Y\n\t}' if not HARD else '\tModel: 200, "Model::arm", "Mesh" {\n\t\tVersion: 232\n\t\tProperties70:  {\n\t\t\tP: "GeometricTranslation", "Vector3D", "Vector", "",1,0,0\n\t\t\tP: "Lcl Translation", "Lcl Translation", "", "A",0,3,0\n\t\t\tP: "Lcl Rotation", "Lcl Rotation", "", "A",0,0,30\n\t\t\tP: "Lcl Scaling", "Lcl Scaling", "", "A",2,2,2\n\t\t}\n\t\tShading: Y\n\t}',
       limb(300, "root", (0, 0, 0)), limb(301, "arm", (8, 0, 0)),
       '\tDeformer: 400, "Deformer::", "Skin" {\n\t\tVersion: 101\n\t}', cluster(401, 0), cluster(402, 1),
       '\tAnimationStack: 500, "AnimStack::wave", "" {\n\t}', '\tAnimationLayer: 501, "AnimLayer::base", "" {\n\t}',
       node(510), node(511), curve(520, ANGLES), curve(521, LIFT), '}', 'Connections:  {',
       '\tC: "OO",100,200', '\tC: "OO",301,300', '\tC: "OO",400,100', '\tC: "OO",401,400', '\tC: "OO",402,400',
       '\tC: "OO",300,401', '\tC: "OO",301,402', '\tC: "OO",501,500', '\tC: "OO",510,501', '\tC: "OO",511,501',
       '\tC: "OP",510,301, "Lcl Rotation"', '\tC: "OP",511,300, "Lcl Translation"', '\tC: "OP",520,510, "d|Z"', '\tC: "OP",521,511, "d|Z"', '}', '']
open(f"{out}/arm.fbx.txt", "w").write("\n".join(fbx))

# ---- fbx, binary: the same tree as the text file, as records
import struct
from fbxbin import hexed, named, p70, record
def binary(wide, ids):
    """`ids` is added to every object's number: a great one is past what a double holds."""
    def limb_node(idn, name, t):
        lines = [("Lcl Translation", "Lcl Translation", "", "A", float(t[0]), float(t[1]), float(t[2]))]
        if HARD and name == "arm":
            lines.append(("PreRotation", "Vector3D", "Vector", "", 0.0, 0.0, 90.0))
        return ("Model", [ids + idn, named(name, "Model"), "LimbNode"], [p70(*lines)])
    def cluster_node(idn, joint):
        pairs = [(i, wt) for i, bl in enumerate(BL) for j, wt in bl if j == joint]
        return ("Deformer", [ids + idn, named("", "SubDeformer"), "Cluster"],
                [("Indexes", [("i", [i for i, _ in pairs])], []), ("Weights", [("d", [wt for _, wt in pairs])], [])])
    def curve_node(idn):
        return ("AnimationCurveNode", [ids + idn, named("n", "AnimCurveNode"), ""],
                [p70(("d|X", "Number", "", "A", 0.0), ("d|Y", "Number", "", "A", 0.0), ("d|Z", "Number", "", "A", 0.0))])
    def curve_values(idn, vals):
        return ("AnimationCurve", [ids + idn, named("", "AnimCurve"), ""],
                [("Default", [0.0], []), ("KeyTime", [("l", [0, 1, 2])], []), ("KeyValueFloat", [("f", [float(v) for v in vals])], [])])
    mesh_lines = [("Lcl Scaling", "Lcl Scaling", "", "A", 1.0, 1.0, 1.0)]
    if HARD:
        mesh_lines = [("GeometricTranslation", "Vector3D", "Vector", "", 1.0, 0.0, 0.0), ("Lcl Translation", "Lcl Translation", "", "A", 0.0, 3.0, 0.0),
                      ("Lcl Rotation", "Lcl Rotation", "", "A", 0.0, 0.0, 30.0), ("Lcl Scaling", "Lcl Scaling", "", "A", 2.0, 2.0, 2.0)]
    objects = [
        ("Geometry", [ids + 100, named("arm", "Geometry"), "Mesh"], [
            ("Vertices", [("d", [float(x) for p in V for x in p])], []),
            ("PolygonVertexIndex", [("i", poly)], []),
            ("LayerElementNormal", [0], [("Version", [101], []), ("Normals", [("d", [x for _ in poly for x in (0.0, 0.0, 1.0)])], [])]),
            ("LayerElementUV", [0], [("Version", [101], []), ("Name", ["UVMap"], []),
                                     ("UV", [("d", [x for uv in UV for x in (uv[0], 1 - uv[1])])], []), ("UVIndex", [("i", uvi)], [])]),
        ]),
        ("Model", [ids + 200, named("arm", "Model"), "Mesh"], [("Version", [232], []), p70(*mesh_lines), ("Shading", [True], [])]),
        limb_node(300, "root", (0, 0, 0)), limb_node(301, "arm", (8, 0, 0)),
        ("Deformer", [ids + 400, named("", "Deformer"), "Skin"], [("Version", [101], [])]),
        cluster_node(401, 0), cluster_node(402, 1),
        ("AnimationStack", [ids + 500, named("wave", "AnimStack"), ""], []),
        ("AnimationLayer", [ids + 501, named("base", "AnimLayer"), ""], []),
        curve_node(510), curve_node(511), curve_values(520, ANGLES), curve_values(521, LIFT),
    ]
    joins = [("OO", 100, 200), ("OO", 301, 300), ("OO", 400, 100), ("OO", 401, 400), ("OO", 402, 400), ("OO", 300, 401), ("OO", 301, 402),
             ("OO", 501, 500), ("OO", 510, 501), ("OO", 511, 501), ("OP", 510, 301, "Lcl Rotation"), ("OP", 511, 300, "Lcl Translation"),
             ("OP", 520, 510, "d|Z"), ("OP", 521, 511, "d|Z")]
    top = [("FBXHeaderExtension", [], [("FBXHeaderVersion", [1003], []), ("Creator", ["samples.py"], [])]),
           ("Objects", [], objects),
           ("Connections", [], [("C", [j[0], ids + j[1], ids + j[2]] + list(j[3:]), []) for j in joins])]
    out = b"Kaydara FBX Binary  \x00\x1a\x00" + struct.pack("<I", 7500 if wide else 7400)
    for node in top:
        out += record(*node, wide, len(out))
    out += bytes(25 if wide else 13) + b"\xfa\xbc\xab\x09 a footer the readers pass over"
    return out
# Version 7.4 with 32-bit counts, and 7.5 with 64-bit counts and object numbers past 2 to the 53rd.
open(f"{out}/arm-7400.fbx.hex", "w").write(hexed(binary(False, 0)))
open(f"{out}/arm-7500.fbx.hex", "w").write(hexed(binary(True, 2 ** 62)))

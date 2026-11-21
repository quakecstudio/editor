# SPDX-License-Identifier: GPL-3.0-or-later
"""Poses an IQM file: for each frame, each triangle's three corner positions (and uv)."""
import json, struct, sys
b = open(sys.argv[1], "rb").read()
assert b[:16] == b"INTERQUAKEMODEL\0"
h = struct.unpack("<27I", b[16:16 + 108])
(version, filesize, flags, num_text, ofs_text, num_meshes, ofs_meshes, num_va, num_verts, ofs_va, num_tris, ofs_tris, ofs_adj,
 num_joints, ofs_joints, num_poses, ofs_poses, num_anims, ofs_anims, num_frames, num_fc, ofs_frames, ofs_bounds, num_comment, ofs_comment, num_ext, ofs_ext) = h
arrays = {}
for i in range(num_va):
    t, fl, fmt, size, off = struct.unpack("<5I", b[ofs_va + 20 * i: ofs_va + 20 * i + 20])
    code = {1: "B", 7: "f"}[fmt]
    width = {1: 1, 7: 4}[fmt]
    arrays[t] = [struct.unpack("<%d%s" % (size, code), b[off + v * size * width: off + (v + 1) * size * width]) for v in range(num_verts)]
tris = [struct.unpack("<3I", b[ofs_tris + 12 * i: ofs_tris + 12 * i + 12]) for i in range(num_tris)]
def mat(t, q, s):
    x, y, z, w = q
    n = (x * x + y * y + z * z + w * w) ** 0.5
    x, y, z, w = x / n, y / n, z / n, w / n
    r = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
         [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
         [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
    return [[r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t[i]] for i in range(3)]
def mul(a, c):
    return [[sum(a[i][k] * c[k][j] for k in range(3)) + (a[i][3] if j == 3 else 0) for j in range(4)] for i in range(3)]
def app(m, p):
    return [sum(m[i][k] * p[k] for k in range(3)) + m[i][3] for i in range(3)]
def inv(m):
    a, c, d = [m[0][:3], m[1][:3], m[2][:3]]
    cr = lambda u, v: [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    det = sum(a[i] * cr(c, d)[i] for i in range(3))
    cols = [cr(c, d), cr(d, a), cr(a, c)]
    r = [[cols[j][i] / det for j in range(3)] for i in range(3)]
    t = [m[i][3] for i in range(3)]
    return [r[i] + [-sum(r[i][k] * t[k] for k in range(3))] for i in range(3)]
joints = []
for i in range(num_joints):
    name, parent = struct.unpack("<Ii", b[ofs_joints + 48 * i: ofs_joints + 48 * i + 8])
    v = struct.unpack("<10f", b[ofs_joints + 48 * i + 8: ofs_joints + 48 * i + 48])
    joints.append((parent, mat(v[0:3], v[3:7], v[7:10])))
def world(local):
    out = []
    for (parent, _), m in zip(joints, local):
        out.append(mul(out[parent], m) if parent >= 0 else m)
    return out
base = world([m for _, m in joints])
unbind = [inv(m) for m in base]
poses = []
for i in range(num_poses):
    parent, mask = struct.unpack("<iI", b[ofs_poses + 88 * i: ofs_poses + 88 * i + 8])
    v = struct.unpack("<20f", b[ofs_poses + 88 * i + 8: ofs_poses + 88 * i + 88])
    poses.append((mask, v[:10], v[10:]))
fd = struct.unpack("<%dH" % (num_frames * num_fc), b[ofs_frames: ofs_frames + 2 * num_frames * num_fc]) if num_frames else ()
pos, uv = arrays[0], arrays.get(1)
bi, bw = arrays.get(4), arrays.get(5)
def frame_tris(skin):
    out = []
    for t in tris:
        corners = []
        for v in t:
            p = pos[v]
            if bi and skin:
                q = [0, 0, 0]
                for j, w in zip(bi[v], bw[v]):
                    if w:
                        r = app(skin[j], p)
                        q = [q[k] + r[k] * w / 255 for k in range(3)]
                p = q
            corners.append(list(p) + (list(uv[v]) if uv else []))
        out.append(corners)
    return out
frames = []
at = 0
for f in range(num_frames):
    local = []
    for mask, off, sc in poses:
        ch = list(off)
        for c in range(10):
            if mask & (1 << c):
                ch[c] += fd[at] * sc[c]
                at += 1
        local.append(mat(ch[0:3], ch[3:7], ch[7:10]))
    w = world(local)
    frames.append(frame_tris([mul(w[j], unbind[j]) for j in range(num_joints)]))
if not frames:
    frames.append(frame_tris(None))
with open(sys.argv[2], "w") as out:
    out.write("# FTEQW's converter's poses of the model beside this file: a frame line, then a line for each\n# triangle, its three corners each x y z u v. Written by scripts/model-formats/iqm.py.\n")
    for frame in frames:
        out.write("frame\n")
        for tri in frame:
            out.write(" ".join("%.6f" % (x + 0.0) for corner in tri for x in corner) + "\n")
print(sys.argv[1], "verts", num_verts, "tris", num_tris, "joints", num_joints, "frames", num_frames)

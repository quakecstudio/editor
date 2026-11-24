#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Measure how a game's levels use their textures, one line for each texture named.

    scripts/measure-textures.py <pak>... -- <texture>...

For each `maps/*.bsp` in the PAK files that is no box model (`maps/b_*`), the area of each
texture on the world model's faces, split by the way a face looks: up (a floor), down (a
ceiling) or neither (a wall), a face looking up or down when its normal is within about 45
degrees of upright. It prints, for each texture named, its share of its own area on each, in
percent, as the rows of `MEASURED` in crates/qs-map-gen/src/theme/tests.rs, and a note for a
texture no level uses. It reads BSP29 (BSP-1) and prints nothing of a level's own data.
"""
import collections
import struct
import sys

ENTRY = struct.Struct("<56sii")
BSP_VERSION = 29
LUMP_PLANES, LUMP_TEXTURES, LUMP_VERTEXES, LUMP_TEXINFO, LUMP_FACES = 1, 2, 3, 6, 7
LUMP_EDGES, LUMP_SURFEDGES, LUMP_MODELS = 12, 13, 14
# A face's normal past this, up or down, makes it a floor or a ceiling.
LEVEL = 0.7
FACES = ("floor", "wall", "ceiling")


def pak_files(path):
    """The files in a PAK: (name, bytes)."""
    with open(path, "rb") as pak:
        data = pak.read()
    _, offset, length = struct.unpack_from("<4sii", data, 0)
    for at in range(offset, offset + length, ENTRY.size):
        name, start, size = ENTRY.unpack_from(data, at)
        yield name.split(b"\0")[0].decode(), data[start:start + size]


def lump(bsp, index):
    start, size = struct.unpack_from("<ii", bsp, 4 + 8 * index)
    return bsp[start:start + size]


def texture_names(bsp):
    textures = lump(bsp, LUMP_TEXTURES)
    count = struct.unpack_from("<i", textures, 0)[0]
    names = []
    for k in range(count):
        at = struct.unpack_from("<i", textures, 4 + 4 * k)[0]
        names.append(textures[at:at + 16].split(b"\0")[0].decode() if at >= 0 else "")
    return names


def face_area(points):
    """The area of a convex polygon in 3D."""
    total = [0.0, 0.0, 0.0]
    first = points[0]
    for b, c in zip(points[1:], points[2:]):
        u = [b[j] - first[j] for j in range(3)]
        v = [c[j] - first[j] for j in range(3)]
        total[0] += u[1] * v[2] - u[2] * v[1]
        total[1] += u[2] * v[0] - u[0] * v[2]
        total[2] += u[0] * v[1] - u[1] * v[0]
    return sum(x * x for x in total) ** 0.5 / 2


def measure(bsp, used):
    """Adds the world model's faces of `bsp` to `used`, by texture and kind of face."""
    names = texture_names(bsp)
    planes, vertexes = lump(bsp, LUMP_PLANES), lump(bsp, LUMP_VERTEXES)
    texinfo, faces = lump(bsp, LUMP_TEXINFO), lump(bsp, LUMP_FACES)
    edges, surfedges = lump(bsp, LUMP_EDGES), lump(bsp, LUMP_SURFEDGES)
    first, count = struct.unpack_from("<ii", lump(bsp, LUMP_MODELS), 56)
    for face in range(first, first + count):
        plane, side, first_edge, edge_count, info = struct.unpack_from("<hhihh", faces, 20 * face)
        up = struct.unpack_from("<f", planes, 20 * plane + 8)[0] * (-1 if side else 1)
        texture = struct.unpack_from("<i", texinfo, 40 * info + 32)[0]
        points = []
        for k in range(edge_count):
            edge = struct.unpack_from("<i", surfedges, 4 * (first_edge + k))[0]
            vertex = struct.unpack_from("<HH", edges, 4 * abs(edge))[1 if edge < 0 else 0]
            points.append(struct.unpack_from("<fff", vertexes, 12 * vertex))
        kind = "floor" if up > LEVEL else "ceiling" if up < -LEVEL else "wall"
        used[names[texture]][kind] += face_area(points)


def main(args):
    if "--" not in args:
        sys.exit(__doc__)
    split = args.index("--")
    paks, wanted = args[:split], args[split + 1:]
    used = collections.defaultdict(lambda: collections.defaultdict(float))
    for pak in paks:
        for name, data in pak_files(pak):
            level = name.startswith("maps/") and name.endswith(".bsp")
            if not level or name.startswith("maps/b_"):
                continue
            if struct.unpack_from("<i", data, 0)[0] != BSP_VERSION:
                continue
            measure(data, used)
    for texture in wanted:
        area = sum(used[texture].values())
        if not area:
            print(f"    // {texture}: on no level's faces")
            continue
        floor, wall, ceiling = (round(100 * used[texture][kind] / area) for kind in FACES)
        print(f'    ("{texture}", Use::of({floor}, {wall}, {ceiling})),')


if __name__ == "__main__":
    main(sys.argv[1:])

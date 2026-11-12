# SPDX-License-Identifier: GPL-3.0-or-later
"""Writes the keep fixture's made-up palette and WAD, no id data: python3 generate.py <keep dir>."""
import struct, sys, pathlib
out = pathlib.Path(sys.argv[1])
# Palette: a 6x6x6 colour cube at 0..215, greys at 216..223, bright colours at 224..255.
pal = []
for i in range(256):
    if i < 216:
        pal += [(i // 36) * 51, (i // 6 % 6) * 51, (i % 6) * 51]
    elif i < 224:
        g = (i - 216) * 32 + 16; pal += [g, g, g]
    else:
        pal += [255, (i - 224) * 8, 255 - (i - 224) * 8]
def cube(r, g, b): return r * 36 + g * 6 + b
def grey(n): return 216 + n
S = 64
def ceiling(x, y): return grey(2) if (x % 32 == 0 or y % 32 == 0) else grey(4)
def door(x, y): return cube(3, 2, 1) if (x // 8) % 2 else cube(2, 1, 0)
def floor(x, y): return cube(2, 2, 1) if ((x // 16) + (y // 16)) % 2 else cube(1, 1, 0)
def wall(x, y):
    row = y // 16; shift = 16 if row % 2 else 0
    if y % 16 == 0 or (x + shift) % 32 == 0: return grey(1)
    return cube(3, 1, 0)
def miptex(name, f):
    level0 = bytes(f(x, y) for y in range(S) for x in range(S))
    levels = [level0]
    for k in (2, 4, 8):
        levels.append(bytes(f(x * k, y * k) for y in range(S // k) for x in range(S // k)))
    offs = []; o = 40
    for lv in levels: offs.append(o); o += len(lv)
    head = name.encode().ljust(16, b"\0") + struct.pack("<II", S, S) + struct.pack("<4I", *offs)
    return head + b"".join(levels)
lumps = [(n, miptex(n, f)) for n, f in [("ceiling", ceiling), ("door", door), ("floor", floor), ("wall", wall)]]
data = b""; dirs = b""; pos = 12
for name, body in lumps:
    dirs += struct.pack("<iiiBBH", pos, len(body), len(body), 68, 0, 0) + name.encode().ljust(16, b"\0")
    data += body; pos += len(body)
wad = b"WAD2" + struct.pack("<ii", len(lumps), pos) + data + dirs
(out / "gfx").mkdir(parents=True, exist_ok=True)
(out / "gfx/keep.wad").write_bytes(wad)
(out / "gfx/palette.lmp").write_bytes(bytes(pal))

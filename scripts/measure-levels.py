#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Measure the levels in a game's PAK files, one JSON object per level, by its name.

    scripts/measure-levels.py <pak>...

For each `maps/*.bsp` that is no box model: its extent, its faces and how many are slanted
(on a plane that is on none of the three axes), the height under each monster, item and player
start (from the floor below it to the ceiling above it, in the player's hull, at its
quarter, half and three quarters), its lights and their middle brightness, and how many
monsters, doors, lifts, buttons, triggers and keys it has. The figures of
docs/agent-reference/maps.md come from it. It reads BSP29 (BSP-1) and prints nothing of a
level's own data.
"""
import collections
import json
import re
import struct
import sys

LUMPS = ["entities", "planes", "textures", "vertexes", "visibility", "nodes", "texinfo",
         "faces", "lighting", "clipnodes", "leafs", "marksurfaces", "edges", "surfedges",
         "models"]
BSP_VERSION = 29
CONTENTS_SOLID = -2
# The player's box reaches 24 below and 32 above its origin, which hull 1 is grown by.
PLAYER_HEIGHT = 56
STEP = 8
FAR = 2048
STANDING = ("monster_", "info_player", "item_", "weapon_")


def pak_files(path):
    """Each file of the PAK at `path`: its name and its bytes."""
    with open(path, "rb") as file:
        data = file.read()
    magic, start, length = struct.unpack_from("<4sii", data, 0)
    if magic != b"PACK":
        raise SystemExit(f"{path}: no PAK")
    for entry in range(length // 64):
        name, at, size = struct.unpack_from("<56sii", data, start + entry * 64)
        yield name.split(b"\0")[0].decode("latin1"), data[at:at + size]


def lumps(bsp):
    """The lumps of a BSP29 by name; None for another version."""
    if struct.unpack_from("<i", bsp, 0)[0] != BSP_VERSION:
        return None
    out = {}
    for index, name in enumerate(LUMPS):
        at, size = struct.unpack_from("<ii", bsp, 4 + index * 8)
        out[name] = bsp[at:at + size]
    return out


def entities(text):
    """The entities of an entity lump, each its keys and values."""
    blocks = re.findall(r"\{([^{}]*)\}", text)
    return [dict(re.findall(r'"([^"]*)"\s+"([^"]*)"', block)) for block in blocks]


def records(lump, layout):
    """The records of `lump`, each unpacked by `layout`."""
    size = struct.calcsize(layout)
    return [struct.unpack_from(layout, lump, at * size) for at in range(len(lump) // size)]


def measure(name, bsp):
    """The figures of the level `name`; None for a BSP this does not read."""
    held = lumps(bsp)
    if held is None:
        return None
    planes = records(held["planes"], "<4fi")
    faces = records(held["faces"], "<hhihhBBBBi")
    clipnodes = records(held["clipnodes"], "<ihh")
    world = records(held["models"], "<9f4i3i")[0]
    first, count = world[14], world[15]
    slanted = sum(1 for face in faces[first:first + count] if planes[face[0]][4] >= 3)
    head = world[10]

    def solid(point):
        node = head
        while node >= 0:
            plane, front, back = clipnodes[node]
            a, b, c, dist, _ = planes[plane]
            side = a * point[0] + b * point[1] + c * point[2] - dist
            node = front if side >= 0 else back
        return node == CONTENTS_SOLID

    held_entities = entities(held["entities"].split(b"\0")[0].decode("latin1"))
    heights = []
    for entity in held_entities:
        if not entity.get("classname", "").startswith(STANDING):
            continue
        try:
            x, y, z = map(float, entity["origin"].split())
        except (KeyError, ValueError):
            continue
        if solid((x, y, z)):
            continue
        low = high = z
        while not solid((x, y, low - STEP)) and low > z - FAR:
            low -= STEP
        while not solid((x, y, high + STEP)) and high < z + FAR:
            high += STEP
        if high < z + FAR:
            heights.append(high - low + PLAYER_HEIGHT)
    heights.sort()
    classes = collections.Counter(entity.get("classname", "") for entity in held_entities)
    lights = sorted(
        float(entity.get("light") or 300)
        for entity in held_entities
        if entity.get("classname", "").startswith("light")
    )

    def starting(start):
        return sum(many for classname, many in classes.items() if classname.startswith(start))

    def part(share):
        return heights[int(len(heights) * share)] if heights else None

    return {
        "level": name,
        "title": held_entities[0].get("message", "") if held_entities else "",
        "size": [round(world[3 + axis] - world[axis]) for axis in range(3)],
        "faces": count,
        "slanted_pct": round(100 * slanted / max(count, 1)),
        "heights": [part(0.25), part(0.5), part(0.75)],
        "highest": heights[-1] if heights else None,
        "lights": len(lights),
        "light_middle": lights[len(lights) // 2] if lights else None,
        "monsters": starting("monster_"),
        "doors": classes["func_door"] + classes["func_door_secret"],
        "lifts": classes["func_plat"] + classes["func_train"],
        "buttons": classes["func_button"],
        "triggers": starting("trigger_"),
        "keys": classes["item_key1"] + classes["item_key2"],
    }


def main(paks):
    rows = []
    for pak in paks:
        for name, data in pak_files(pak):
            level = re.fullmatch(r"maps/([a-z0-9_]+)\.bsp", name)
            if level and not level.group(1).startswith("b_"):
                row = measure(level.group(1), data)
                if row:
                    rows.append(row)
    for row in sorted(rows, key=lambda row: row["level"]):
        print(json.dumps(row))


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    main(sys.argv[1:])

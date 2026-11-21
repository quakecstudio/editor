# SPDX-License-Identifier: GPL-3.0-or-later
"""Records what Blender makes of an fbx file, for the readers' tests to hold a mesh to: run
inside Blender, it imports the file, poses it ten times a second from its animation's first
key and at its last, and writes each pose's triangles.

    blender -b --python scripts/model-formats/blender.py -- model.fbx out.blender.txt [ufbx|legacy]

`ufbx` is Blender's importer built on ufbx (`wm.fbx_import`), `legacy` its older one in
Python (`import_scene.fbx`). A line `frame` starts a pose; each line after is a triangle,
three corners of `x y z u v`. A polygon is a fan from its first corner and a triangle of
corners `a b c` is written `b a c`, as the readers keep one (MDL-10.6); `v` is measured down
the picture.

Positions are written in the file's own space. Blender stands a file whose y is up on its
own z, `(x, y, z)` to `(x, -z, y)`, and takes a file's unit of a centimetre to its metre, a
hundredth: both are undone here, so a reader told the file's z is up, which turns nothing,
gives these numbers."""
import bpy, math, sys

path, out, importer = (sys.argv[sys.argv.index("--") + 1:] + ["ufbx"])[:3]
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
if importer == "ufbx":
    bpy.ops.wm.fbx_import(filepath=path, use_anim=True, anim_offset=0.0)
else:
    bpy.ops.import_scene.fbx(filepath=path, use_anim=True, anim_offset=0.0)
ranges = [tuple(action.frame_range) for action in bpy.data.actions]
start = min((r[0] for r in ranges), default=0.0)
end = max((r[1] for r in ranges), default=0.0)
step = scene.render.fps / scene.render.fps_base / 10.0
times, k = [], 0
while start + k * step < end:
    times.append(start + k * step)
    k += 1
times.append(end)
UNIT = 100.0
lines = ["# %s as Blender %s poses it (%s): %d poses" % (path.rsplit("/", 1)[-1], bpy.app.version_string, importer, len(times))]
for time in times:
    whole = math.floor(time)
    scene.frame_set(int(whole), subframe=time - whole)
    graph = bpy.context.evaluated_depsgraph_get()
    lines.append("frame")
    for thing in scene.objects:
        if thing.type != "MESH":
            continue
        posed = thing.evaluated_get(graph)
        mesh = posed.to_mesh()
        uvs = mesh.uv_layers.active.data if mesh.uv_layers.active else None
        def corner(loop):
            x, y, z = posed.matrix_world @ mesh.vertices[mesh.loops[loop].vertex_index].co
            u, v = uvs[loop].uv if uvs else (0.0, 1.0)
            return "%.6f %.6f %.6f %.6f %.6f" % (x * UNIT, z * UNIT, -y * UNIT, u, 1.0 - v)
        for polygon in mesh.polygons:
            loops = list(polygon.loop_indices)
            for second, third in zip(loops[1:], loops[2:]):
                lines.append(" ".join(corner(loop) for loop in (second, loops[0], third)))
        posed.to_mesh_clear()
open(out, "w").write("\n".join(lines) + "\n")
print("POSED", importer, len(times), "poses, rate", scene.render.fps, "range", start, end)

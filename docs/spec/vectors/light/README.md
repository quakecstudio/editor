# light test vectors

Vectors for [`../../light.md`](../../light.md). Each map is a BSP compiled by `qs-map-bsp` from
a `.map` of boxes written for these vectors (no game data), stored as an `xxd` hex dump
(`<map>.bsp.txt`; `xxd -r` rebuilds it). [`results.tsv`](results.tsv) gives, for each map in
the fast mode and with `-extra`, what id's light printed and wrote: `lightdatasize`, the number
of `WARNING` lines, the number of switchable light styles, and the sha256 of the whole file it
wrote.

| Map | What it holds |
| --- | --- |
| `room` | A room with a pillar and a wedge with a slanted, turned and scaled face; a plain light, one of style 5, a named (switchable) light and a spotlight with a cone (LIGHT-2, LIGHT-4.4) |
| `hall` | A hall under a sky with a pool of water and a door model; five lights of five styles on one wall (LIGHT-4.5), a light too dim to start a lightmap before them (LIGHT-4.7), lights set by `_light` and `lightning`, and a target that names nothing (LIGHT-2.5) |
| `scatter3`, `scatter7` | A room 1024 by 1024 by 256 with 24 boxes turned at random, each face with a random texture shift, rotation and scale, and 16 lights of four styles at random |

**Checked:** on 2026-11-13 every map was run through id's light: `qutils/LIGHT/LIGHT.C`,
`LIGHT.H`, `LTFACE.C`, `TRACE.C`, `ENTITIES.C`, `ENTITIES.H`, `THREADS.C`, `THREADS.H` and
`qutils/COMMON/CMDLIB.C`, `MATHLIB.C`, `BSPFILE.C` and their headers, copied unchanged from
id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`, built with the system C
compiler (Apple clang, arm64) with the reference build's flags (`-std=gnu99 -O2
-fno-strict-aliasing -ffp-contract=off -fsigned-char`, `docs/spec/vm.md`) and
`oracles/maps/compat.h` included first. A build that contracts floating point (clang's default
on arm64) differs from these by one in a few bytes. light is used here as an oracle only
([`../../../porting.md`](../../../porting.md#upstreams)). `qs-map-light` writes every file
exactly (`every_vector_matches_id_light`).

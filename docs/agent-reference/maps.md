# Maps for the agent

## The .map format

Sources: [`docs/spec/map.md`](../spec/map.md) (MAP-1 to MAP-6).

- A `.map` is text: entities in `{ }`, each a list of `"key" "value"` pairs, one to a line,
  and for a brush entity its brushes, each in `{ }` of its own (MAP-2.1, MAP-2.2). A key
  holds at most 30 bytes and a value, in practice, at most 127 (MAP-2.4, MAP-1.6); a map
  holds at most 1024 entities (MAP-2.8).
- A brush is a convex solid: one face per line, three points `( x y z )` on its plane, then
  the texture name and its shifts, rotation and scales (MAP-3.2, MAP-4.1). The plane's
  normal is `(p0 - p1) x (p2 - p1)` and must point out of the brush (MAP-3.4). qbsp reads
  coordinates as integers (MAP-3.3), so keep them whole. Valve 220 faces (`[ ... ]` axes) need ericw-tools; id's qbsp does not
  read them (MAP-5.5).
- The first entity is the world, `worldspawn`, which holds the level's own brushes
  (MAP-6.1). Every later entity with brushes is a brush entity, given the model `*1`, `*2`,
  and so on in file order (MAP-6.2). The rest are point entities placed by `origin`, three
  numbers (MAP-2.7).
- Keys starting with `_` are for the compile tools, and the game ignores them (MAP-6.7).

## Units and the player

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc`
(`defs.qc`, `client.qc`, `subs.qc`); [`docs/spec/engine/physics.md`](../spec/engine/physics.md);
[`docs/spec/bsp.md`](../spec/bsp.md) (BSP-11.3).

- Distances are in Quake units, and the z axis points up. The player's box runs from
  `'-16 -16 -24'` to `'16 16 32'` (`VEC_HULL_MIN`, `VEC_HULL_MAX`), 32 units wide and 56
  tall, with the eyes 22 units above the origin. A gap the player walks through must be at
  least 32 wide and 56 tall. Large monsters such as the shambler use hull 2, `'-32 -32 -24'`
  to `'32 32 64'`.
- Walking climbs a step of up to 18 units (`STEPSIZE`, PHYS-8.3). Use stairs of 16 or less.
- A jump adds 270 to the vertical speed (`PlayerJump`: `self.velocity_z = self.velocity_z +
  270`), and gravity is 800 (`sv_gravity`, PHYS-6.4). That makes the peak 270² / (2 · 800),
  about 45 units: a ledge of 40 is a safe jump.
- An `info_player_start` is required: `PutClientInServer` stops with "no
  info_player_start on level" without one (`client.qc`). Put its origin 24 units above the
  floor, where the box's bottom rests.

## The entities a map needs

Sources: the same `client.qc`; `qutils/LIGHT/ENTITIES.C` and `ENTITIES.H` at the same commit.

- `worldspawn` with `wad`, the texture WAD's path relative to the game directory; qbsp reads
  `_wad` first, and ericw-tools takes several paths separated by `;` (MAP-6.3). `message`
  is the level's title (MAP-6.5).
- `info_player_start`, as above, and `info_player_deathmatch` for deathmatch.
- `light` entities. The `light` tool lights only from them; `light` (or `_light`) is the
  brightness, 300 when not given (`DEFAULTLIGHTLEVEL`).
- The classes a mod spawns are its QuakeC functions of the same name. The brief lists the
  open map's classes, and `project.entities` describes each class the project defines.

## Textures

Sources: [`docs/spec/map.md`](../spec/map.md) (MAP-4.5, MAP-4.6, MAP-6.3).

- A face's texture names a texture in the world's WAD. Use only names that WAD holds, and
  read the WAD rather than guessing. A name holds at most 15 bytes (MAP-4.6).
- A name starting with `*` is a liquid (water, slime or lava), and one starting with `sky`
  is the sky; neither gets a lightmap (MAP-4.5). Trigger brushes are hidden whatever their
  texture, because `InitTrigger` (`subs.qc`) clears their model; `trigger` is the usual name
  for them.

## Leaks

Source: `qutils/QBSP/OUTSIDE.C` (`FillOutside`) and `QBSP.C` at the same commit.

- The level must be sealed: no gap from any point entity to the void outside it. qbsp
  floods the outside from the map's edge. When the flood reaches an entity, it prints
  "reached occupant at" with the entity's origin, writes the path to `<map>.pts` and does
  no filling. The map still compiles, but every face outside is kept, so vis and light are
  slow and wrong.
- Seal the world with brushes on every side. Each entity's origin, other than the world's,
  must lie inside, and an entity at `'0 0 0'` is not counted (`FillOutside` skips it).

# Entities and the engine

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc/defs.qc`;
[`docs/spec/engine/physics.md`](../spec/engine/physics.md) (PHYS-1 the frame, PHYS-2 thinking,
PHYS-5 stepping monsters, PHYS-6 contact); [`docs/spec/language.md`](../spec/language.md)
(QC-12, QC-15, QC-19). A mod may add to `defs.qc`; read the project's own.

## What the engine calls

The program never runs by itself: the engine calls into it.

- **A map entity's spawn function**, named by its `classname`, once when the map loads, with
  `self` the entity and its keys already set as fields.
- **`StartFrame`** at the start of every server frame (PHYS-1.1).
- **`think`** of each entity whose `nextthink` has come: `nextthink` is set to 0 first, so a
  think that should repeat sets `self.nextthink` again (PHYS-2.1).
- **`touch`** of both entities when one moves into the other, with `self` the one whose
  function it is and `other` the other (PHYS-6.1). An entity with `SOLID_NOT` is not touched.
- **`use`**, never by the engine: the mod's own `SUB_UseTargets` (`subs.qc`) calls it on each
  entity whose `targetname` is the caller's `target`.
- **`PlayerPreThink`** and **`PlayerPostThink`** around each player's move, **`ClientConnect`**,
  **`PutClientInServer`**, **`ClientDisconnect`**, **`ClientKill`**, **`SetNewParms`** and
  **`SetChangeParms`** (`defs.qc`, "required prog functions").

The globals the engine sets (`defs.qc`): `self`, `other`, `world`, `time`, `frametime`,
`mapname`, `deathmatch`, `coop`, `teamplay`, `serverflags`, the counts `total_secrets`,
`total_monsters`, `found_secrets` and `killed_monsters`, and `parm1` to `parm16`.

## How an entity moves and collides

Three fields decide it, set in the spawn function before `setmodel` and `setsize`:

| `movetype` | Value | What moves it |
| --- | --- | --- |
| `MOVETYPE_NONE` | 0 | nothing; it only thinks (PHYS-2.2) |
| `MOVETYPE_WALK` | 3 | the player's own movement |
| `MOVETYPE_STEP` | 4 | a monster: it falls, and moves only when its code calls `walkmove` or `movetogoal` (PHYS-5) |
| `MOVETYPE_FLY` | 5 | its `velocity`, no gravity |
| `MOVETYPE_TOSS` | 6 | its `velocity` and gravity |
| `MOVETYPE_PUSH` | 7 | a door or a platform: it pushes what is in its way |
| `MOVETYPE_NOCLIP` | 8 | its `velocity`, through everything (PHYS-2.3) |
| `MOVETYPE_FLYMISSILE` | 9 | as `FLY`, a little larger against monsters |
| `MOVETYPE_BOUNCE` | 10 | as `TOSS`, bouncing |

| `solid` | Value | What it does to others |
| --- | --- | --- |
| `SOLID_NOT` | 0 | nothing touches it |
| `SOLID_TRIGGER` | 1 | touched, but blocks nothing |
| `SOLID_BBOX` | 2 | touched, and blocks |
| `SOLID_SLIDEBOX` | 3 | touched, and blocks: players and monsters |
| `SOLID_BSP` | 4 | blocks by its brushes: doors and walls, with `MOVETYPE_PUSH` |

`flags` holds bits the engine and the mod both read: `FL_FLY` 1, `FL_SWIM` 2, `FL_CLIENT` 8,
`FL_INWATER` 16, `FL_MONSTER` 32, `FL_GODMODE` 64, `FL_NOTARGET` 128, `FL_ITEM` 256,
`FL_ONGROUND` 512, `FL_PARTIALGROUND` 1024. `takedamage` is `DAMAGE_NO` 0, `DAMAGE_YES` 1 or
`DAMAGE_AIM` 2.

- Move an entity with `setorigin (e, org)`, never by writing `e.origin`: the engine keeps its
  own record of where each entity is for collision, and only the builtin updates it.
- `setmodel (e, "progs/x.mdl")` shows a model and sets the size from it; `setsize (e, mins,
  maxs)` sets the box. A model or a sound must be precached, in a spawn function, before
  either names it.
- `spawn()` makes an entity during play; `remove (e)` frees one. A removed entity's fields
  read as nothing afterwards: do not keep using it.

## Finding and testing things

- `find (start, field, "value")` gives the next entity after `start` whose string field holds
  the value, or `world`; loop until it gives `world`. `findradius (org, radius)` gives a
  chain through `.chain`.
- `traceline (from, to, nomonsters, ignore)` sets `trace_fraction` (1 when nothing was hit),
  `trace_endpos`, `trace_ent`, `trace_plane_normal`, `trace_inopen` and `trace_inwater`.
- `makevectors (angles)` sets `v_forward`, `v_right` and `v_up`. They are globals: another
  `makevectors`, or a call that makes one, replaces them.
- `sound (e, channel, "path.wav", volume, attenuation)` with a channel `CHAN_AUTO` 0,
  `CHAN_WEAPON` 1, `CHAN_VOICE` 2, `CHAN_ITEM` 3 or `CHAN_BODY` 4 and an attenuation
  `ATTN_NONE` 0, `ATTN_NORM` 1, `ATTN_IDLE` 2 or `ATTN_STATIC` 3.
- `engine.builtins` lists every builtin the project's engine has, with its number and its
  signature. Declare one the project does not declare yet as `type name = #n;`.

## What the language does that C does not

In the vanilla dialect, from the language spec:

- `&&` and `||` evaluate both sides (QC-15.10): `if (e && e.health > 0)` reads `e.health`
  when `e` is the world too. That is safe, the world's fields read as 0, but a call on the
  right side always runs.
- `&` and `|` bind as tightly as `*` and `/`, and `!` takes the whole comparison after it:
  `!a == b` is `!(a == b)` (QC-15.2, QC-15.6). Use parentheses.
- `=` binds tighter than `&&`: `a = b && c` stores `b` (QC-15.3).
- An `if` tests only the first part of its value: a vector is true when its x is not 0, and
  an empty string literal is true where the null string is false (QC-19.3, QC-19.4). Test a
  vector with `!= '0 0 0'`.
- There are two scopes, the file's and the function's; a block opens none, and a local
  cannot hide a global of the same name: it is the global (QC-12.1, QC-12.3).
- A name must be declared before it is used, in the order `progs.src` lists the files
  (QC-12.5): declare a function ahead as `void() name;`.
- There is no `for`, `switch`, `break`, `continue`, array, struct, `int` or cast in the
  vanilla dialect. The FTEQCC dialect has `for`, `switch`, `break` and `continue`, `++` and
  `--`, compound assignment, `%`, the ternary operator, `const`, arrays, `typedef struct` and
  `enum` ([`docs/spec/dialect-fteqcc-rerelease.md`](../spec/dialect-fteqcc-rerelease.md)).
  `project.info` says which dialect a project is in; write what the files around yours write.

## Debugging

- `project.compile` gives each error with its file and line. Fix the first: the later ones
  often follow from it.
- `vm.run` calls one function in a fresh VM with a mock engine, for logic with no map.
- In the running game: `engine.inspect` reads any entity's fields and the globals;
  `preview.breakpoint` stops at a file and line, `preview.step` and `preview.step_by` go on,
  `preview.watch` evaluates an expression where it stopped, and `preview.set_field` changes a
  field.
- `dprint` lines show in the console (`preview.console`) when the `developer` setting is 1:
  `engine.console` with `developer 1`.
- A QuakeC error stops the game and reports the function, file and line; `preview.poll`
  reads it.

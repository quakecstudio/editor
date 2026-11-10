# QuakeC for the agent

## The language

Sources: [`docs/spec/language.md`](../spec/language.md) (QC-10 Types, QC-8 frame macros,
QC-13 functions); id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73
`qcc/v101qc`.

- Types are `void`, `float`, `vector` (three floats), `string` and `entity`; `.T` is a field
  of type `T`; `T(T1 a, T2 b)` is a function type (QC-10.1 to QC-10.3). In vanilla QuakeC
  there is no `int`, array, struct, pointer or `const` (QC-10.9), and no casts. Numbers are
  floats: test bits with `&` and `|` on whole-number floats, as `self.spawnflags & 1`.
- A function is declared by its type and given a body with `=`, as in
  `void() SUB_Remove = {remove(self);};` (`subs.qc`).
- A frame function `void() name = [$frame, next] { ... };` sets the frame and makes `next`
  think a tenth of a second later (QC-13); `$frame` and `$cd` lines name the model's frames (QC-8).
- FTEQCC projects accept more (the dialect in the brief says which):
  [`docs/spec/dialect-fteqcc.md`](../spec/dialect-fteqcc.md). Do not use FTEQCC syntax in a
  vanilla project; the compiler refuses it.

## How a mod runs

Sources: id's `qcc/v101qc` (`world.qc`, `client.qc`, `subs.qc`, `combat.qc`).

- `progs.src` names the output and lists the source files in order; a name must be declared
  before it is used, so `defs.qc` comes first.
- The engine spawns each entity of the map by calling the function named by its
  `classname` with `self` the new entity (`info_player_start` in `client.qc`); the world is
  `worldspawn` (`world.qc`), which precaches models and sounds.
- Time-based behaviour is `self.nextthink = time + delay; self.think = function;`: the engine
  calls `think` with `self` that entity once `time` reaches `nextthink`.
- `self` is the entity being run and `other` the one touched or touching; save and restore
  `self` around a call made on another entity's behalf.
- Damage goes through `T_Damage (targ, inflictor, attacker, damage)` (`combat.qc`); weapons
  are in `weapons.qc` (`W_FireAxe` and the others), the player's frames in `player.qc`.
- A builtin is declared `type name = #n;` with the engine's number; the brief lists the
  target engine's builtins with their numbers and signatures. Call only those.

## Pitfalls

- Every model and sound used must be precached in `worldspawn` or a spawn function before it
  is used.
- Strings from builtins such as `ftos` and `vtos` share one buffer in id's engine: use one
  before calling the next ([`docs/spec/builtins.md`](../spec/builtins.md)).
- Keep the project's existing style and file order; add new files to `progs.src`.

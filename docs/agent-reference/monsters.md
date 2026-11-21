# Writing a monster

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc`:
`monsters.qc`, `ai.qc`, `fight.qc`, `combat.qc`, `soldier.qc`, `defs.qc`. A mod's own code may
differ: read its `monsters.qc`, `ai.qc` and one whole monster before you write another, and
follow what it does.

## What a monster is

A monster is a spawn function named for its classname, a set of frame functions that chain
into each other, and seven fields that name the first function of each chain. The shared AI
in `ai.qc` runs the chains: you write the animation and the attacks, not the behaviour.

The soldier's spawn function, whole (`soldier.qc`):

```c
void() monster_army =
{
    if (deathmatch)
    {
        remove(self);
        return;
    }
    precache_model ("progs/soldier.mdl");
    precache_sound ("soldier/death1.wav");
    // ... every model and sound the monster uses ...

    self.solid = SOLID_SLIDEBOX;
    self.movetype = MOVETYPE_STEP;

    setmodel (self, "progs/soldier.mdl");

    setsize (self, '-16 -16 -24', '16 16 40');
    self.health = 30;

    self.th_stand = army_stand1;
    self.th_walk = army_walk1;
    self.th_run = army_run1;
    self.th_missile = army_atk1;
    self.th_pain = army_pain;
    self.th_die = army_die;

    walkmonster_start ();
};
```

- `walkmonster_start` (`monsters.qc`) counts the monster in `total_monsters` and, a moment
  later, drops it to the floor, sets `takedamage`, `FL_MONSTER`, `view_ofs` and `use`, and
  calls `th_stand`, or `th_walk` when its `target` is a `path_corner`. A flying monster ends
  with `flymonster_start`, a swimming one with `swimmonster_start`.
- The soldier's size is `'-16 -16 -24'` to `'16 16 40'`; id's large monsters use
  `VEC_HULL2_MIN` and `VEC_HULL2_MAX`, `'-32 -32 -24'` to `'32 32 64'` (`defs.qc`). Use the
  size of the monster whose model you reuse.
- `setmodel` before `setsize`: `setmodel` sets the size from the model.

## The seven fields

Declared in `defs.qc`:

| Field | Called | It must |
| --- | --- | --- |
| `th_stand` | when it has nothing to do | loop frames that call `ai_stand()` |
| `th_walk` | along a path of `path_corner`s | loop frames that call `ai_walk(dist)` |
| `th_run` | once it has an enemy | loop frames that call `ai_run(dist)` |
| `th_melee` | by `CheckAttack` when the enemy is in melee range | play the attack and return to the run |
| `th_missile` | by `CheckAttack`, by chance, at range | play the attack and return to the run |
| `th_pain` | by `T_Damage`, with the attacker and the damage | play a pain chain, or return at once |
| `th_die` | by `Killed` | play a death chain that ends on its last frame |

A monster needs `th_stand`, `th_walk`, `th_run`, `th_pain`, `th_die` and one or both of the
attacks. `CheckAttack` (`fight.qc`) tests `if (self.th_melee)` and `if (!self.th_missile)`, so
leave unset the one it does not have.

## Frame functions

The model's frames are named at the top of the monster's file, in the model's own order, and
each function shows one frame and names the next:

```c
$frame stand1 stand2 stand3 stand4 stand5 stand6 stand7 stand8
$frame run1 run2 run3 run4 run5 run6 run7 run8

void() army_stand1 =[ $stand1, army_stand2 ] {ai_stand();};
// ...
void() army_stand8 =[ $stand8, army_stand1 ] {ai_stand();};

void() army_run1 =[ $run1, army_run2 ] {ai_run(11);};
```

`void() name =[ $frame, next ] { code };` sets `self.frame`, makes `next` the think a tenth
of a second later, and runs `code` (`monsters.qc`'s opening comment). A chain that loops ends
by naming its first function; a death chain ends by naming itself.

- `$frame` names are numbered from 0 in the order written, and they are the file's own: a
  new file that uses a model must list that model's frames again, all of them, in the same
  order as the file it copies them from. A name left out shifts every frame after it.
- A function named as a `next` before it is defined needs no declaration (the header
  declares it); a function called in a body before its definition does, as
  `void() army_fire;` is.

## What the AI does for you

From `ai.qc` and `fight.qc`:

- `ai_stand()` looks for a player (`FindTarget`) and, when it sees one, plays the sight sound
  and switches to `th_run`.
- `ai_walk(dist)` moves `dist` units toward the path and looks for a player.
- `ai_run(dist)` moves `dist` units toward the enemy and calls `CheckAnyAttack`, which starts
  `th_melee` or `th_missile`; when the enemy dies it goes back to `th_walk` or `th_stand`.
- `ai_face()` turns toward the enemy; call it in each frame of an attack.
- `ai_charge(dist)` faces and moves toward the enemy; `ai_melee()` hurts an enemy within 60
  units by a random amount scaled by 3 (`fight.qc`).
- `ai_pain(dist)` and `ai_back(dist)` step back; `ai_painforward(dist)` and
  `ai_forward(dist)` step forward.
- `SUB_CheckRefire(first)` starts the attack again when the enemy is still in sight.

The distances are the units moved in that tenth of a second: copy a similar monster's
numbers so the feet match the floor.

## Attacks

A ranged attack is a chain of frames that face the enemy, call a fire function on one frame,
and return to the run (`soldier.qc`):

```c
void() army_atk5 =[ $shoot5, army_atk6 ] {ai_face();army_fire();
self.effects = self.effects | EF_MUZZLEFLASH;};
// ...
void() army_atk9 =[ $shoot9, army_run1 ] {ai_face();};

void() army_fire =
{
    local vector dir;
    local entity en;

    ai_face();
    sound (self, CHAN_WEAPON, "soldier/sattck1.wav", 1, ATTN_NORM);
    en = self.enemy;
    dir = en.origin - en.velocity*0.2;
    dir = normalize (dir - self.origin);
    FireBullets (4, dir, '0.1 0.1 0');
};
```

`FireBullets (count, dir, spread)` is in `weapons.qc`. For a projectile, read how the ogre
(`ogre.qc`) or the enforcer (`enforcer.qc`) makes one: `spawn()`, a `movetype`, a `solid`,
a `velocity`, a `touch` function that calls `T_Damage`, and a `think` that removes it.

## Pain and death

```c
void(entity attacker, float damage) army_pain =
{
    if (self.pain_finished > time)
        return;
    self.pain_finished = time + 0.6;
    army_pain1 ();
    sound (self, CHAN_VOICE, "soldier/pain1.wav", 1, ATTN_NORM);
};

void() army_die =
{
    if (self.health < -35)
    {
        sound (self, CHAN_VOICE, "player/udeath.wav", 1, ATTN_NORM);
        ThrowHead ("progs/h_guard.mdl", self.health);
        ThrowGib ("progs/gib1.mdl", self.health);
        return;
    }
    sound (self, CHAN_VOICE, "soldier/death1.wav", 1, ATTN_NORM);
    army_die1 ();
};
```

- A pain chain ends by naming the run chain's first function.
- One frame of a death chain sets `self.solid = SOLID_NOT;` so the body does not block.
- `Killed` (`combat.qc`) has already counted the kill and fired the monster's targets before
  it calls `th_die`.

## Where a new monster must be added

id's code names monsters in four places beyond their own file. Search the mod for each and
add yours where its neighbours are:

1. **`progs.src`**: the monster's file, after `ai.qc`, `fight.qc` and `monsters.qc`.
2. **`SightSound` (`ai.qc`)**: a chain of `if (self.classname == "monster_...")`. A classname
   not in it makes no sound when it first sees the player.
3. **`CheckAnyAttack` (`ai.qc`)**: a classname not in it uses `CheckAttack`, which is right
   for most. Add yours only when it needs its own rule.
4. **`ClientObituary` (`client.qc`)**: the line printed when the monster kills the player.
   A classname not in it prints no cause.

A model must also be in the game's files: reuse a model the game ships, with its frames, or
the map shows nothing.

## Trying it

1. `project.compile` must be clean.
2. Put one in a map (`map.place_entity` with the classname) near the player start, compile
   and play it paused (`map.play_from_here` with `start` paused), then `engine.inspect` the
   entities: the monster is there with its `health` and its `model`.
3. `preview.resume`, let it run, and `engine.inspect` again: its `enemy` is the player and
   its `origin` has moved. `engine.screenshot` shows it. If it stands still, its `th_run`
   chain is not calling `ai_run`, or its classname's spawn function did not finish.
4. A scenario (`tests/*.scenario`) that starts the map, runs frames and expects the player's
   `health` below 100 proves the attack.

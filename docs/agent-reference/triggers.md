# Writing a trigger

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc`:
`triggers.qc`, `subs.qc`, `defs.qc`; how a map's keys become fields is
[`docs/spec/edicts.md`](../spec/edicts.md) (EDICT-8). A mod's own code may differ: read its `triggers.qc` and
`SUB_UseTargets` in its `subs.qc` before you write another.

## What a trigger is

A trigger is a brush entity with no picture: a volume in the map that calls its `touch`
when something moves into it, or a point entity with only a `use`. What it does is nearly
always "fire my targets", and that part is already written: `SUB_UseTargets`.

The mapper links entities by two keys, which arrive as fields: the trigger's `target` and
the other entity's `targetname`. Firing a target calls the `use` of every entity whose
`targetname` is the trigger's `target`.

## InitTrigger

Every trigger volume's spawn function calls it (`subs.qc`):

```c
void() InitTrigger =
{
// trigger angles are used for one-way touches.  An angle of 0 is assumed
// to mean no restrictions, so use a yaw of 360 instead.
    if (self.angles != '0 0 0')
        SetMovedir ();
    self.solid = SOLID_TRIGGER;
    setmodel (self, self.model);    // set size and link into world
    self.movetype = MOVETYPE_NONE;
    self.modelindex = 0;
    self.model = "";
};
```

- A brush entity arrives with `model` naming its own brushes in the compiled map:
  `setmodel (self, self.model)` gives the entity their size and links it into the world,
  and clearing `modelindex` and `model` after makes it invisible. Do not precache or name a
  model.
- `SOLID_TRIGGER`: touched, and blocks nothing.
- The mapper's `angle` key `v` arrives as `angles` `'0 v 0'` (EDICT-8). `SetMovedir` turns it
  into `self.movedir`, a direction, and clears `angles`; an `angles` of `'0 -1 0'` is up and
  `'0 -2 0'` down.

## The smallest trigger

`trigger_hurt`, whole:

```c
void() hurt_on =
{
    self.solid = SOLID_TRIGGER;
    self.nextthink = -1;
};

void() hurt_touch =
{
    if (other.takedamage)
    {
        self.solid = SOLID_NOT;
        T_Damage (other, self, self, self.dmg);
        self.think = hurt_on;
        self.nextthink = time + 1;
    }

    return;
};

/*QUAKED trigger_hurt (.5 .5 .5) ?
Any object touching this will be hurt
set dmg to damage amount
defalt dmg = 5
*/
void() trigger_hurt =
{
    InitTrigger ();
    self.touch = hurt_touch;
    if (!self.dmg)
        self.dmg = 5;
};
```

- **Keys are fields with defaults.** A key the mapper did not set is 0 or the null string:
  `if (!self.dmg) self.dmg = 5;`. A key must be a field the program declares (`dmg`, `wait`,
  `delay`, `speed`, `count`, `message`, `sounds`, `spawnflags` and the rest are in
  `defs.qc`); the engine ignores a key that names none, with the message `'<key>' is not a
  field` (EDICT-8). Declare a new one, `.float charge;`, before the spawn function.
- **`touch` runs every frame the thing stays inside**, so a trigger that should not fire
  each frame holds itself off. id's turns itself off, `self.solid = SOLID_NOT`, and a think
  turns it back on a second later; another way, which a mod may use in its place, is a time
  in a field of the trigger that `touch` tests against `time` and sets again. Use the one
  the project's `trigger_hurt` uses.
- In `touch`, `self` is the trigger and `other` what touched it. Test `other` first: a
  monster, a grenade and a corpse all touch.
- The `/*QUAKED` comment is the map editor's, and `project.entities` reads it: id writes `?`
  in place of a box for a brush entity, and after it the names of its spawnflags in the
  order of their bits (`notouch` is `SPAWNFLAG_NOTOUCH`, 1). A line under it that starts with
  a key in quotes, `"wait" time between fires`, is what the map editor shows beside that key
  when a mapper adds it; a bare `"sounds"` line takes the lines under it, up to a blank line.

## Firing targets

`trigger_multiple` is the pattern for a trigger that fires. Three ways in, one way out:

```c
void() multi_killed =
{
    self.enemy = damage_attacker;
    multi_trigger();
};

void() multi_use =
{
    self.enemy = activator;
    multi_trigger();
};

void() multi_touch =
{
    if (other.classname != "player")
        return;

// if the trigger has an angles field, check player's facing direction
    if (self.movedir != '0 0 0')
    {
        makevectors (other.angles);
        if (v_forward * self.movedir < 0)
            return;     // not facing the right way
    }

    self.enemy = other;
    multi_trigger ();
};
```

and `multi_trigger`, cut to what every firing trigger does:

```c
    if (self.nextthink > time)
    {
        return;     // allready been triggered
    }
    // ...
    if (self.noise)
        sound (self, CHAN_VOICE, self.noise, 1, ATTN_NORM);

// don't trigger again until reset
    self.takedamage = DAMAGE_NO;

    activator = self.enemy;

    SUB_UseTargets();

    if (self.wait > 0)
    {
        self.think = multi_wait;
        self.nextthink = time + self.wait;
    }
    else
    {   // we can't just remove (self) here, because this is a touch function
        // called wheil C code is looping through area links...
        self.touch = SUB_Null;
        self.nextthink = time + 0.1;
        self.think = SUB_Remove;
    }
```

- **Set `activator` before `SUB_UseTargets ()`.** It is a global: who set this off.
  `SUB_UseTargets` prints the trigger's `message` to it, and each target's `use` may read
  it, as `counter_use` below does. A trigger keeps who
  touched it in a field, here `enemy`, because a delay may pass before it fires.
- **`SUB_UseTargets`** does four things with `self`'s fields, in this order: with a `delay`,
  spawn an entity that fires later and return; `centerprint` the `message` to a player
  activator; `remove` every entity whose `targetname` is `killtarget`, and return if there
  was a `killtarget`; then call the `use` of every entity whose `targetname` is `target`,
  with `self` that entity and `other` the trigger, putting both back after.
- **Do not `remove (self)` in a touch function.** The engine is walking its list of
  entities; id's comment above says so. Clear `touch` and let a think remove it: `self.think
  = SUB_Remove; self.nextthink = time + 0.1;`.
- **Repeat or not** is `wait`: `trigger_multiple` defaults it to 0.2, and `trigger_once` is
  `self.wait = -1; trigger_multiple ();`. Reuse a spawn function like that where you can.
- A trigger with `health` is shot, not touched: `takedamage = DAMAGE_YES`, `solid =
  SOLID_BBOX`, and `th_die` set to the function that fires.

## A trigger with no volume

`trigger_relay` is one line, `self.use = SUB_UseTargets;`: it passes a firing on, with its
own `delay`, `message` and `killtarget`. `trigger_counter` is the pattern for one that keeps
state between uses:

```c
void() counter_use =
{
    self.count = self.count - 1;
    if (self.count < 0)
        return;

    if (self.count != 0)
    {
        // ... "Only 2 more to go..." to a player activator ...
        return;
    }
    // ...
    self.enemy = activator;
    multi_trigger ();
};

void() trigger_counter =
{
    self.wait = -1;
    if (!self.count)
        self.count = 2;

    self.use = counter_use;
};
```

A `use` function runs with `self` the entity used, `other` the one that fired it and
`activator` who began the chain. A point entity like this needs no `InitTrigger`, no model
and no `solid`.

## Puzzles from id's entities

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc`:
`doors.qc`, `buttons.qc`, `triggers.qc`, `plats.qc`, `monsters.qc`, `combat.qc`, `ai.qc`,
`items.qc`. What `map.generate` builds them into is in [`maps.md`](maps.md).

- **Keys.** A `func_door` with `spawnflags` 16 (`DOOR_SILVER_KEY`) needs `item_key1`, with 8
  (`DOOR_GOLD_KEY`) `item_key2`. "Key doors are allways wait -1", and `door_touch` takes the
  key away: `other.items = other.items - self.items`. One key opens one door.
- **A door opened from elsewhere.** A door with a `targetname` spawns no touch field ("a
  remote button or trigger field activates the door"). Give it `wait` -1 to stay open;
  the default is 3, after which it closes.
- **A button.** `func_button` fires its `target` when touched, or shot when it has
  `health`; `wait` -1 "never return". `angle` is the way it moves: into its wall.
- **An arena.** A monster fires its `target` when it dies: `Killed` calls
  `monster_death_use`, which does `activator = self.enemy; SUB_UseTargets ();`. Point every
  monster of a room at one `trigger_counter` with `count` the number of monsters and its
  `target` the door. A counter with no `count` takes 2 (`if (!self.count) self.count =
  2;`), so a counter whose monsters never spawn never fires: give it monsters. Its messages,
  "Only 2 more to go...", are silenced by `spawnflags` 1.
- **A secret door.** `func_door_secret` with no `targetname` opens when shot (`health
  10000`, `th_pain = fd_secret_use`). `spawnflags` 1 (`SECRET_OPEN_ONCE`) keeps it open, 4
  (`SECRET_1ST_DOWN`) moves it down first; otherwise it closes after `wait`, 5 seconds.
  Doors that touch are linked by `LinkDoors`, which finds them by `classname` "door", which
  a secret door's is too: keep a secret door from touching another.
- **A secret.** `trigger_secret` counts toward `total_secrets` and, touched by the player,
  prints "You found a secret area!".
- **A lift.** `func_plat` is "always drawn in the extended position": build it at the top,
  and its `height` is how far it moves down (`pos2_z = self.origin_z - self.height`). It
  rises when a player stands on it at the bottom, waits 3 seconds at the top and goes down;
  a player standing on it keeps it up.
- **A teleporter.** `trigger_teleport` sends what touches it to the
  `info_teleport_destination` whose `targetname` is its `target`, facing that entity's
  `angle`; the destination raises its own origin by 27 (`self.origin = self.origin + '0 0
  27'`), so put it on the floor.
- **An ambush.** A monster with `spawnflags` 1 "will only wake up on really seeing the
  player, not another monster getting angry" (`FindTarget`). The zombie uses that bit for
  crucified, so leave it off zombies.

## What a model gets wrong

- Testing `other.classname == "player"` and then reading `other.health`: a dead player
  still touches. id's teleporter tests `other.health <= 0 || other.solid != SOLID_SLIDEBOX`.
- Firing every frame: no `wait`, no `solid = SOLID_NOT`, and a message printed on each.
- Calling a target's function directly. Go through `SUB_UseTargets`, so `delay`,
  `killtarget` and `message` work as mappers expect.
- Giving the trigger a model, or precaching one: its brushes are its model.
- A new key with no field: the engine ignores it, and the default is always used.
- `remove (self)` in `touch`.

## Where a new trigger must be added

Its file, or `triggers.qc`, in `progs.src` after `subs.qc`, with its `/*QUAKED` comment.
Nothing else names it.

## Trying it

`project.compile`. In a map, `map.brush_create` a brush where the player will walk and
`map.make_brush_entity` it into your classname, with its keys set by `map.set_key`; give it
a `target` and something with that `targetname` whose `use` you can see, a door or a
`trigger_relay` with a `message`, or a `trigger_counter` whose `count` falls by one each
time it is fired. `preview.play`, walk the player in, and read the target with
`engine.inspect`, or the `message` in `preview.console`. To hurt the player for a test,
`give h 50` at `engine.console` sets its health once frames run.

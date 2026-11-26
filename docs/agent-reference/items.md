# Writing an item

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc`:
`items.qc`, `subs.qc`, `client.qc`, `defs.qc`. A mod's own code may differ: read its
`items.qc`, and one item of the kind you are adding from its spawn function to the end of its
touch function, before you write another.

## What an item is

An item is an entity a map places, with a spawn function named for its `classname` and a
touch function that gives the player something and takes the item out of the world. It
does not think or move after it has settled. id's are health, armour, weapons, ammo, keys,
powerups and the backpack a dead player drops.

The health box, whole but for two of its three sizes (`items.qc`):

```c
void() item_health =
{
    self.touch = health_touch;

    if (self.spawnflags & H_ROTTEN)
    {
        // ... the small box ...
    }
    else
    if (self.spawnflags & H_MEGA)
    {
        // ... the large box ...
    }
    else
    {
        precache_model("maps/b_bh25.bsp");
        precache_sound("items/health1.wav");
        setmodel(self, "maps/b_bh25.bsp");
        self.noise = "items/health1.wav";
        self.healamount = 25;
        self.healtype = 1;
    }
    setsize (self, '0 0 0', '32 32 56');
    StartItem ();
};
```

In order: the touch function; a precache of every model and sound it uses, here in the
spawn function, which runs while the map loads; `setmodel`; what it gives, in fields of its
own (`.float healamount, healtype;` is declared just above); `setsize`; and `StartItem ()`.

- The model is a small `.bsp` (`maps/b_bh25.bsp`) or a `.mdl` (`progs/armor.mdl`); either
  works with `setmodel`. For one of your own see the `models` topic.
- `setsize` comes after `setmodel`, which sets a size of its own. id's boxes are 32 wide and
  56 tall from the entity's corner; a model centred on its origin takes `'-16 -16 0', '16
  16 56'` (the weapons) or `'-16 -16 -24', '16 16 32'` (the powerups).
- `StartItem` only sets a think: `self.nextthink = time + 0.2; self.think = PlaceItem;`,
  "items start after other solids". `PlaceItem` then makes it an item and drops it to the
  floor:

  ```c
  self.mdl = self.model;      // so it can be restored on respawn
  self.flags = FL_ITEM;       // make extra wide
  self.solid = SOLID_TRIGGER;
  self.movetype = MOVETYPE_TOSS;
  self.velocity = '0 0 0';
  self.origin_z = self.origin_z + 6;
  oldz = self.origin_z;
  if (!droptofloor())
  {
      dprint ("Bonus item fell out of level at ");
      // ...
      remove(self);
      return;
  }
  ```

  So do not set `solid`, `movetype` or `flags` yourself, and place the item above a floor:
  one with nothing under it is removed, with the `dprint` to tell you.

## The touch function

`health_touch`, cut to its skeleton:

```c
void() health_touch =
{
    if (other.classname != "player")
        return;

    if (!T_Heal(other, self.healamount, 0))
        return;

    sprint(other, "You receive ");
    // ...
    sound(other, CHAN_ITEM, self.noise, 1, ATTN_NORM);
    stuffcmd (other, "bf\n");

    self.model = string_null;
    self.solid = SOLID_NOT;

    if (deathmatch != 2)
    {
        if (deathmatch)
            self.nextthink = time + 20;
        self.think = SUB_regen;
    }

    activator = other;
    SUB_UseTargets();               // fire all targets / killtargets
};
```

Every item's touch has these parts, in this order:

1. **Who may take it.** `self` is the item and `other` what touched it. Return unless it is
   a player (`other.classname != "player"`, or `!(other.flags & FL_CLIENT)` in
   `weapon_touch`), and unless it is alive (`other.health <= 0` in `ammo_touch` and
   `powerup_touch`): a corpse still touches.
2. **Whether it is needed.** Return, leaving the item, when the player is full: `T_Heal`
   returns 0 at full health, `ammo_touch` returns when `other.ammo_shells >= 100`. `T_Heal
   (e, amount, ignore)` stops at `max_health` with `ignore` 0 and only at 250 with it set:
   for a limit of your own, test and write `other.health` yourself.
3. **Give it**, by writing the player's fields: `other.health`, `other.ammo_nails`,
   `other.items = other.items | self.items`, `other.armorvalue`. After ammo, call
   `bound_other_ammo ()`, which holds shells and rockets and cells to 100 and nails to 200.
4. **Tell the player**: `sprint (other, "You got the ")` and the rest, a sound on `CHAN_ITEM`,
   and `stuffcmd (other, "bf\n")`, the flash of the screen. `sprint` takes one string: print
   a sentence in pieces, and a number through `ftos`, each used before the next is made.
5. **Take it out of the world**: `self.model = string_null; self.solid = SOLID_NOT;`. It is
   not removed: in deathmatch a think brings it back, `self.think = SUB_regen` with
   `nextthink` 20 or 30 seconds on, and `SUB_regen` restores `self.model = self.mdl` and
   `self.solid = SOLID_TRIGGER`. In single player `nextthink` stays unset and it never
   returns.
6. **Fire its targets**: `activator = other; SUB_UseTargets ();`, so a map can make an item
   open a door. Set `activator` first: `SUB_UseTargets` prints the item's `message` to it.

When the item changes what the player holds, the weapon functions run with `self` as the
player, and `self` is put back after (`ammo_touch`):

```c
    stemp = self;
    self = other;
    W_SetCurrentAmmo();
    self = stemp;
```

Forgetting to restore `self` leaves the rest of the touch function working on the player.

## A powerup, and a thing that lasts

`item_artifact_invulnerability` sets `self.items = IT_INVULNERABILITY`, a `netname` for the
message and a `noise`, and `powerup_touch` gives the bit and starts a clock on the player:

```c
    other.items = other.items | self.items;
    // ...
    if (self.classname == "item_artifact_invulnerability")
    {
        other.invincible_time = 1;
        other.invincible_finished = time + 30;
    }
```

The item only starts it. What the powerup does is elsewhere: `T_Damage` (`combat.qc`) reads
`invincible_finished`, and `CheckPowerups` (`client.qc`), run each frame from
`PlayerPostThink`, warns as it runs out and clears the bit when the time has passed. A new
effect that lasts needs the same three: a field for when it ends, the code that reads it
where the effect applies, and the end of it in `CheckPowerups`. `SetChangeParms` strips the
keys and powerups from `items` at a level's end; anything else in `items` is carried on.

## Where a new item must be added

- Its file, or `items.qc`, in `progs.src` after the files it uses.
- A `/*QUAKED classname (r g b) (mins) (maxs) flag1 flag2` comment above the spawn function,
  as id's have: the map editor reads it for the entity's name, box and spawnflags, and a
  line starting with a quoted key, `"wait" seconds before it respawns`, for what that key does.
- Nothing else for a plain item: unlike a monster or a weapon it is named nowhere but in
  its own functions. A weapon's pickup also needs its branch in `weapon_touch`, and ammo in
  `ammo_touch` (the `weapons` topic).

id's forms above (`deathmatch != 2`, `sprint` with two arguments, strings written in
place) are what a mod most often changes: one may have its own helpers for the deathmatch
rules, a `sprint` that takes a print level, or names for its messages. Write what its own
items write.

## Trying it

`project.compile`; place it in a map with `map.place_entity` a little above a floor, and
`preview.play`. `engine.inspect` on the item shows `solid` 1 and `flags` 768 once it has
settled (`FL_ITEM` 256 and `FL_ONGROUND` 512, which landing sets); walk the player onto it, or move the player with `preview.set_field` on `origin`,
and read the player's field it gives and the item's `model`, which is empty once taken.
To test the deathmatch return, the map needs an `info_player_deathmatch`, and `deathmatch`
is set at the console before a `map` command starts the level.

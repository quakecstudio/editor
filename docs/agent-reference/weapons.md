# Writing a weapon

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc`:
`weapons.qc`, `player.qc`, `items.qc`, `client.qc`, `world.qc`, `combat.qc`, `defs.qc`; the
status bar is
[`docs/spec/engine/hud.md`](../spec/engine/hud.md) (HUD-1.2, HUD-1.4). A mod's own code may
differ: read its `weapons.qc` whole, and one weapon through every function below, before you
add another.

## What a weapon is

A weapon is not an entity. It is a bit in the player's `items`, the number of that bit in
the player's `weapon` while it is held, and a branch for it in each of six functions. The
thing that lies in a map is an item that gives the bit (the `items` topic); what flies out
of the gun is an entity the fire function spawns.

The fields (`defs.qc`): `.float weapon` ("one of the IT_SHOTGUN, etc flags"), `.float items`
(the bits), `.string weaponmodel` and `.float weaponframe` (the gun the player sees),
`.float currentammo` and `ammo_shells`, `ammo_nails`, `ammo_rockets`, `ammo_cells`, `.float
impulse` (the key pressed), `.float button0` (fire held) and `.float attack_finished` (the
time before which it cannot fire again).

The bits (`defs.qc`): `IT_SHOTGUN` 1, `IT_SUPER_SHOTGUN` 2, `IT_NAILGUN` 4,
`IT_SUPER_NAILGUN` 8, `IT_GRENADE_LAUNCHER` 16, `IT_ROCKET_LAUNCHER` 32, `IT_LIGHTNING` 64,
`IT_EXTRA_WEAPON` 128, `IT_AXE` 4096; the ammo kinds `IT_SHELLS` 256, `IT_NAILS` 512,
`IT_ROCKETS` 1024, `IT_CELLS` 2048. Every bit to 4194304 is taken; `IT_EXTRA_WEAPON` is the
one id left for a ninth weapon. The status bar draws a weapon's icon for the seven bits from
`IT_SHOTGUN` to `IT_LIGHTNING` only (HUD-1.4), so a weapon on `IT_EXTRA_WEAPON` has no icon,
and the ammo icon is chosen by which of the four ammo bits is set (HUD-1.2). A mod that
needs more than one new weapon keeps its own field of bits; look for one before adding one.

## Each frame

`PlayerPostThink` (`client.qc`) calls `W_WeaponFrame`:

```c
void() W_WeaponFrame =
{
    if (time < self.attack_finished)
        return;

    ImpulseCommands ();

// check for attack
    if (self.button0)
    {
        SuperDamageSound ();
        W_Attack ();
    }
};
```

So nothing happens until `attack_finished`; then a key's impulse is handled, then fire.

## The six places

1. **`W_Precache`** (called from `worldspawn`): `precache_sound` for every sound the weapon
   makes. The models are precached in `worldspawn` itself, in `world.qc`; add the weapon's
   `v_` model, the model of what it fires, and the pickup's model there or in the pickup's
   spawn function.
2. **`W_SetCurrentAmmo`**: for `self.weapon == IT_X`, set `currentammo` from the ammo field,
   `weaponmodel` to the `v_` model, `weaponframe` to 0, and the ammo's bit in `items`:

   ```c
   else if (self.weapon == IT_ROCKET_LAUNCHER)
   {
       self.currentammo = self.ammo_rockets;
       self.weaponmodel = "progs/v_rock2.mdl";
       self.weaponframe = 0;
       self.items = self.items | IT_ROCKETS;
   }
   ```

   Without a branch the weapon falls to the last `else`: no model and no ammo.
3. **`W_Attack`**: for `self.weapon == IT_X`, start the player's attack frames, fire, and set
   how soon it may fire again:

   ```c
   else if (self.weapon == IT_ROCKET_LAUNCHER)
   {
       player_rocket1();
       W_FireRocket();
       self.attack_finished = time + 0.8;
   }
   ```

   `W_Attack` first calls `W_CheckNoAmmo`, which changes to `W_BestWeapon ()` and returns
   FALSE when `currentammo` is 0 and the weapon is not the axe: a weapon that uses no ammo
   must be allowed there as the axe is.
4. **`W_ChangeWeapon`**: the impulse that selects it. Impulses 1 to 8 are the weapons, and
   id's `ImpulseCommands` takes 9 for the cheat, 10 to cycle, 11 and 255; a mod may take
   more, so read its `ImpulseCommands` and pick a number it does not use. `ImpulseCommands`
   sends only 1 to 8 to `W_ChangeWeapon` ("`if (self.impulse >= 1 && self.impulse <= 8)`"),
   so a ninth weapon needs its own impulse added there, and a branch in `W_ChangeWeapon`
   that sets `fl` to its bit and `am` when its ammo is short.
5. **`CycleWeaponCommand`** (impulse 10): the chain from one weapon to the next; put the new
   one between two.
6. **`W_BestWeapon`**: which weapon is chosen when ammo runs out or one is picked up. A
   weapon that hurts its user, as the rocket launcher does, is left out by id.

id names its weapons in two more places, and a mod may in others: search the project for
one weapon's bit, `IT_SUPER_NAILGUN`, and give the new one a branch wherever it has one.
id's two are `RankForWeapon` (`items.qc`), the order deathmatch uses to decide whether a
pickup replaces the weapon held, and `CheatCommand` (impulse 9), which gives every weapon.

And the **pickup**, in `items.qc` (the `items` topic): `weapon_touch` has a branch for each
`classname`, and ends in `objerror ("weapon_touch: unknown classname")` for one it does not
know. In single player the pickup also selects the weapon ("`if (!deathmatch) self.weapon =
new;`"). A weapon carries to the next level by itself: `SetChangeParms` saves `self.items` and
`self.weapon` in `parm1` and `parm8`, and `DecodeLevelParms` gives them back.

## Firing: a trace

`W_FireShotgun`, and `FireBullets` under it:

```c
void() W_FireShotgun =
{
    local vector dir;

    sound (self, CHAN_WEAPON, "weapons/guncock.wav", 1, ATTN_NORM);

    self.punchangle_x = -2;

    self.currentammo = self.ammo_shells = self.ammo_shells - 1;
    dir = aim (self, 100000);
    FireBullets (6, dir, '0.04 0.04 0');
};
```

- Take the ammo from both `currentammo` and the ammo field.
- `aim (self, speed)` gives the direction the player looks, turned toward a target with
  `DAMAGE_AIM` near the crosshair; it reads `v_forward` and ignores `speed`
  ([`docs/spec/builtins.md`](../spec/builtins.md), BUILTIN-14.6). So `makevectors
  (self.v_angle)` comes first: it gives `v_forward`, `v_right` and `v_up` for the player's
  view, and `W_Attack` has called it. `self.angles` is the body's, not the view's.
- `FireBullets` traces each pellet with `traceline (src, src + direction*2048, FALSE, self)`
  from `self.origin + v_forward*10` at seven tenths of the player's height, and calls
  `TraceAttack (4, direction)` when `trace_fraction != 1.0`. It gathers the damage between
  `ClearMultiDamage ()` and `ApplyMultiDamage ()`, so one monster hit by six pellets is hurt
  once: use the pair around any loop of `TraceAttack`.
- `self.punchangle_x = -2` kicks the view up.

## Firing: a missile

`W_FireRocket`:

```c
    missile = spawn ();
    missile.owner = self;
    missile.movetype = MOVETYPE_FLYMISSILE;
    missile.solid = SOLID_BBOX;

    makevectors (self.v_angle);
    missile.velocity = aim(self, 1000);
    missile.velocity = missile.velocity * 1000;
    missile.angles = vectoangles(missile.velocity);

    missile.touch = T_MissileTouch;

    missile.nextthink = time + 5;
    missile.think = SUB_Remove;

    setmodel (missile, "progs/missile.mdl");
    setsize (missile, '0 0 0', '0 0 0');
    setorigin (missile, self.origin + v_forward*8 + '0 0 16');
```

- `owner` is who fired: an entity and its owner do not collide
  ([`docs/spec/engine/world.md`](../spec/engine/world.md), WORLD-7.3), so the missile leaves
  the player it starts inside, and the touch function gives the owner as the attacker.
- A `think` that removes it, so one that hits nothing does not fly for ever.
- A size of nothing, so it hits what its point hits.

Its touch, `T_MissileTouch`: return when `other == self.owner`; remove it and return when
`pointcontents (self.origin) == CONTENT_SKY`; `T_Damage (other, self, self.owner, damg)`
when `other.health`; `T_RadiusDamage (self, self.owner, 120, other)` for everything near
but the one hit; then the explosion, a `TE_EXPLOSION` temp entity written with `WriteByte`
and `WriteCoord`, and `BecomeExplosion ()`. In a touch function `self` is the missile and
`other` what it hit.

`T_Damage (target, inflictor, attacker, damage)` is the one way to hurt: it handles armour,
the quad, pain and death (`combat.qc`). Never write `health` yourself.

## The player's frames

`player_shot1` (`player.qc`) is where the gun animates:

```c
void() player_shot1 = [$shotatt1, player_shot2 ] {self.weaponframe=1;
self.effects = self.effects | EF_MUZZLEFLASH;};
void() player_shot2 = [$shotatt2, player_shot3 ] {self.weaponframe=2;};
// ... to player_shot6, whose next is player_run
```

Each is the player's body frame and the gun's `weaponframe`; the last goes back to
`player_run`. A new weapon on an existing `v_` model may reuse a chain that only animates,
as `player_shot1` and `player_rocket1` do, with the fire function called from `W_Attack`.
The nailgun's and the lightning gun's chains fire for themselves (`player_nail1` calls
`W_FireSpikes (4)` and sets `attack_finished`), so they fit no other weapon. A model of
your own needs its own chain, as many frames as the model has (`model.inspect`).

## Trying it

`project.compile`, then `preview.play` a map with the pickup in it, or give the weapon to
the player with `preview.set_field` on its `items` and `weapon`. Press the weapon's
impulse with `engine.console` (`impulse` and its number); since the pickup selects the
weapon in single player, select another first to test the impulse. For one shot, pause,
send `+attack`, `preview.step` one frame, and send `-attack`: `+attack` and `-attack` in
one console line fire nothing, and held while the game runs it fires again and again.
`engine.inspect` reads `currentammo`, `weaponmodel` and the health of what you shot.

## What a unit of ammo can do

`map.balance` counts the damage a map's ammunition can do with this table (`ID_WEAPONS` in
`crates/qs-workbench/src/map_balance.rs`, which a test checks against these rows). It is the
most one shot does to one target with every pellet or nail hitting, a random part at its
least, quoted from `weapons.qc` of id 1.01 (`id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73`,
`qcc/v101qc`) and of the rerelease
(`id-Software/quake-rerelease-qc@634eefab09a77eb7b5f5ca7078ba3d8784a91142`, `quakec`), which
give each weapon the same numbers. The lines are id 1.01's, then the rerelease's.

| Weapon | Ammo | Units a shot | Damage a shot |
| --- | --- | --- | --- |
| `shotgun` | `ammo_shells` | 1 | 24 |
| `super_shotgun` | `ammo_shells` | 2 | 56 |
| `nailgun` | `ammo_nails` | 1 | 9 |
| `super_nailgun` | `ammo_nails` | 2 | 18 |
| `grenade_launcher` | `ammo_rockets` | 1 | 120 |
| `rocket_launcher` | `ammo_rockets` | 1 | 100 |
| `lightning` | `ammo_cells` | 1 | 30 |

- **Shotgun**: `self.ammo_shells - 1` and `FireBullets (6, dir, '0.04 0.04 0')` (lines 290
  and 292; 292 and 294), each pellet `TraceAttack (4, direction)` (270; 271), which calls
  `AddMultiDamage (trace_ent, damage)` (233; 234): six pellets of 4.
- **Super shotgun**: `self.ammo_shells - 2` and `FireBullets (14, dir, '0.14 0.08 0')` (315
  and 317; 318 and 320): fourteen pellets of 4 for two shells.
- **Nailgun**: `self.ammo_nails - 1` (679; 690), and `spike_touch`'s `T_Damage (other, self,
  self.owner, 9)` (708; 715).
- **Super nailgun**: `self.ammo_nails - 2` (648; 658), and `superspike_touch`'s `T_Damage
  (other, self, self.owner, 18)` (748; 757).
- **Grenade launcher**: `self.ammo_rockets - 1` (567; 576), and `GrenadeExplode`'s `T_RadiusDamage (self, self.owner, 120, world)`
  (533; 540). `T_RadiusDamage` (`combat.qc`) takes half the distance from the blast to the
  target's centre from the 120, so 120 is a blast at the target's centre.
- **Rocket launcher**: `self.ammo_rockets - 1` (395; 397), and `T_MissileTouch`'s `damg = 100 + random()*20` (359; 362) to what it
  hits, halved for `monster_shambler` ("mostly immune"); its `T_RadiusDamage (self,
  self.owner, 120, other)` passes over the one hit, so 100 is the least a hit does.
- **Thunderbolt**: `self.ammo_cells - 1` and `LightningDamage (self.origin, trace_endpos +
  v_forward*4, self, 30)` (508 and 524; 516 and 531). Fired under water it instead
  discharges every cell at once in `T_RadiusDamage (self, self, 35*self.ammo_cells, world)`
  (495; 504, where it is `35*cells`).

A kind of ammunition is counted with the most any weapon does with one unit: a shell 28 (the
super shotgun), a nail 9, a rocket 120 (the grenade launcher's blast), a cell 30. A mod's
own kind, a field the engine does not define, is counted in units and with no damage, as is
any weapon a mod changes: read its `weapons.qc` for what it does.

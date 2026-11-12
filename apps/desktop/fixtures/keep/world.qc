// SPDX-License-Identifier: GPL-3.0-or-later

void() worldspawn = {};

// Fires the entities the keys name, so the map editor links them.
void() SUB_UseTargets = {
	local entity t;
	t = find(world, targetname, self.target);
	t = find(world, targetname, self.killtarget);
};

float DOOR_START_OPEN = 1;

/*QUAKED info_player_start (1 0 0) (-16 -16 -24) (16 16 32)
Where the player starts.
*/
void() info_player_start = {};

/*QUAKED monster_ogre (1 0 0) (-32 -32 -24) (32 32 64)
An ogre; fires its target when it dies.
*/
void() monster_ogre = { if (!self.speed) self.speed = 120; if (self.target) SUB_UseTargets(); };

/*QUAKED light (0 1 0) (-8 -8 -8) (8 8 8)
A light.
*/
void() light = { if (self.style) self.wait = 1; };

/*QUAKED func_door (0 .5 .8) ?
A door.
*/
void() func_door = { if (!self.speed) self.speed = 100; if (!self.wait) self.wait = 3; if (self.spawnflags & DOOR_START_OPEN) self.wait = 4; setmodel(self, self.model); };

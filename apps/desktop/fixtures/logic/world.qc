// SPDX-License-Identifier: GPL-3.0-or-later

// A button that fires its targets as id's SUB_UseTargets does, and a door (L-76).
entity(entity start, .string fld, string match) find = #18;
string(string s) precache_model = #20;
void() worldspawn = { precache_model("progs/player.mdl"); };
void() button_use = {
	local entity t, stemp;
	t = find(world, targetname, self.target);
	while (t) {
		stemp = self;
		self = t;
		if (self.use) self.use();
		self = stemp;
		t = find(t, targetname, self.target);
	}
};
void() func_button = { self.use = button_use; };
void() door_use = { self.frame = 1; };
void() func_door = { self.use = door_use; };

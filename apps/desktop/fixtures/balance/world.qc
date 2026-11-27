// SPDX-License-Identifier: GPL-3.0-or-later

// The Balance tab's snapshot fixture (L-78): monsters whose health reads the skill, counted
// as id's walkmonster_start counts them, and pickups whose touch gives as id's items.qc does.
string(string s) precache_model = #20;
void(entity e) remove = #15;
float(string s) cvar = #45;
void() worldspawn = { precache_model("progs/player.mdl"); };
void() info_player_start = {};
void() monster_army = { self.health = 30; total_monsters = total_monsters + 1; };
void() monster_ogre = { self.health = 200 + cvar("skill") * 25; total_monsters = total_monsters + 1; };
void() trigger_secret = { total_secrets = total_secrets + 1; };
void() shells_touch = { other.ammo_shells = other.ammo_shells + 20; remove(self); };
void() item_shells = { self.touch = shells_touch; };
void() rockets_touch = { other.ammo_rockets = other.ammo_rockets + 5; remove(self); };
void() item_rockets = { self.touch = rockets_touch; };
void() health_touch = { other.health = other.health + 25; remove(self); };
void() item_health = { self.touch = health_touch; };
void() armor_touch = { other.armorvalue = 100; remove(self); };
void() item_armor1 = { self.touch = armor_touch; };
void() ssg_touch = { other.items = other.items | 2; other.ammo_shells = other.ammo_shells + 5; };
void() weapon_supershotgun = { self.touch = ssg_touch; };

// SPDX-License-Identifier: GPL-3.0-or-later

entity self, world;
float time;
.string classname, model, target, killtarget, targetname, message;
.float speed, wait, angle, light_lev, style, spawnflags;
.vector origin, angles;
.void() think;
void(entity e, string m) setmodel = #3;
void(entity e, vector min, vector max) setsize = #4;
entity(entity start, .string fld, string match) find = #18;

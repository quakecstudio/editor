// SPDX-License-Identifier: GPL-3.0-or-later

entity self;
float time;
.string classname, model, target, targetname, message;
.float speed, wait, angle, light_lev, style;
.vector origin, angles;
.void() think;
void(entity e, string m) setmodel = #3;
void(entity e, vector min, vector max) setsize = #4;

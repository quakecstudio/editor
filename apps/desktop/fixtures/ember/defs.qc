// SPDX-License-Identifier: GPL-3.0-or-later
// A small mod for the desktop app's tests: the Project, Symbols and Problems panels.

entity self;
float time;
.float health;
.float bounces;
.vector velocity;
float BOUNCE_SPEED = 0.7;
void(entity e) remove = #15;

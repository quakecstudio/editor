// SPDX-License-Identifier: GPL-3.0-or-later

// A trap whose comment numbers its sounds, and which precaches the sound its noise key names
// (L-83).
string(string s) precache_sound = #19;
void() worldspawn = {};
void() info_player_start = {};

/*QUAKED trap_buzzer (1 0 0) (-8 -8 -8) (8 8 8)
"sounds"
1) buzz
2) click
3) hum
"noise" the sound it makes when it fires
*/
void() trap_buzzer = { if (!self.sounds) self.sounds = 1; precache_sound (self.noise); };

// SPDX-License-Identifier: GPL-3.0-or-later
// Every construct of vanilla QuakeC (docs/spec/language.md), written for the parser's tests.

/*
	System definitions, as a defs file declares them.
*/
entity self;
entity world;
float time, frametime;
vector v_forward, v_up;
string mapname;
void end_sys_globals;

.float health, frame;
.vector origin, velocity;
.string classname;
.entity enemy;
.void() think;
.float nextthink;
..float fieldref;
.void end_sys_fields;

float FALSE = 0;
float TRUE = 1;
float SOLID_BBOX = 2;
vector VEC_ORIGIN = '0 0 0';
vector VEC_HULL_MIN = '-16 -16 -24';
string NAME = "sample \"one\"\n";

void(vector ang) makevectors = #1;
void(entity e, vector o) setorigin = #2;
float() random = #7;
void(string s) dprint = #25;
void(...) bprint = #23;
entity(entity start, .string fld, string match) find = #18;
void(float(float x) fn, .float fld) apply;

$cd /raid/quake/id1/models/sample
$origin 0 0 24
$base base
$skin skin
$flags 8
$scale 1

$frame stand1 stand2 stand3
$frame walk1 walk2

void() sample_stand2;
void() sample_stand1 = [$stand1, sample_stand2] { self.nextthink = time + 0.1; };
void() sample_stand2 = [$stand2, sample_stand1] {};
void() sample_walk = [1, sample_stand1] { };

float(float a, float b) max_of =
{
	if (a > b)
		return a;
	return b;
};

void() control =
{
	local float i, j = 4;
	local vector dir;
	local entity e;

	i = 0;
	while (i < 10)
	{
		i = i + 1;
		if (i == 5)
			j = j - -1;
		else if (!i || i >= 7 && i <= 9)
			j = j * 2 / 3;
		else
			j = j & 3 | 8;
	}
	do
		i = i - 1;
	while (i != 0);

	dir = v_forward * 2 + v_up;
	j = dir * dir;
	j = dir_x + dir_y - dir_z;
	e = find (world, classname, "player");
	if (e)
		e.origin = e.origin + '0 0 1';
	e.enemy.health = e.health = 100;
	self.think = sample_walk;
	self.think ();
	setorigin (self, (self.origin + VEC_HULL_MIN) * 0.5);
	bprint ("a", "b", ftos (j));
	if (e != world && e.classname == "player")
		dprint (NAME);
	j = max_of (i, j);
	self.frame = $walk2;
	return;
};

void(float a, float b, float c, float d, float e, float f, float g, float h) eight = {};

// SPDX-License-Identifier: GPL-3.0-or-later
// FTEQCC constructs (docs/spec/dialect-fteqcc-rerelease.md, docs/spec/dialect-fteqcc.md),
// written for the parser's tests under the FTEQCC profile.

#pragma noref 1
float time;
entity self, world;
.float health, frame;
.void() think;
#pragma noref 0

const float LIMIT = 10;
var float counter = 0;
nosave float scratch;
int count_i = 0x10;
unsigned int mask = 0xffi;
float half = .5;
float third = 1.0f / 3;
vector up = '0 0 1';
vector flat = '8 8';
float letter = 'a';
string greeting = "hello\tworld";
float table[4] = { 1, 2, 3, 4 };
float *cursor;

enum { RED, GREEN = 4, BLUE };
enum class Mode : float { OFF, ON };
enumflags { FLAG_A, FLAG_B };

typedef struct { float x; float y; } point_t;
typedef float vec2_t[2];
point_t corners[2] = { { 0, 0 }, { 1, 1 } };

struct node_s { float value; struct node_s *next; };
union bits_u { float f; int i; };

class Thing : entity
{
	float size;
	virtual void Think(void);
	nonvirtual float Count(void) { return size; };
public:
	void Thing(void);
};

accessor handle : float
{
	get float value = handle_value;
	set float value = handle_set;
};

float handle_value(handle h) = #0:handle_value;
void handle_set(handle h, float v) = #0:handle_set;
void(string s, ...) bprint = #23;
float(float a, float b = 2) add = { return a + b; };

void Thing::Think(void)
{
	size += 1;
	super::Think();
};

float(float n) fact =
{
	if (n <= 1)
		return 1;
	return n * fact(n - 1);
}

void() worker =
{
	float i, total = 0;
	for (i = 0; i < LIMIT; i++)
		total += i;
	for (int j = 0; j < 3; j++) { }
	switch (total)
	{
	case 1:
		total--;
		break;
	case 2 .. 5:
		total = total % 3;
		break;
	default:
		total = total ? total : 1;
	}
	if not (total)
		total = 1;
	if !(total)
		total = 2;
	while (total > 0)
	{
		total = total - 1;
		if (total == 3)
			continue;
	}
	do total++; __until (total >= 3);
	total = (total & 3) | (total ^ 1) << 2 >> 1;
	total &~= 4;
	total |= ~total;
	total = -total;
	cursor = &total;
	*cursor = 1;
	self->health = self.health - 1;
	self.frame = $walk1;
	total = sizeof(float) + sizeof total;
	total = (float)self.health;
	up = [1, 2, total];
	i = Mode::ON;
	greeting = "joined " "strings";
	return;
};

$frame walk1 walk2

void() run1 = [$walk1, run2] { self.frame = 1; };
void() run2 = [++ 1 .. 4] { };

void() jumps =
{
	float n = 0;
again:
	n++;
	if (n < 3)
		goto again;
};

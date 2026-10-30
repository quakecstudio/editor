// SPDX-License-Identifier: GPL-3.0-or-later

void() Ember_Touch =
{
	self.bounces = self.bounces - 1;
	self.velocity = self.velocity * BOUNCE_SPEED;
	if (self.bounces <= 0)
		Ember_Explode ();
};

void() W_FireEmber =
{
	local entity ember;
	ember.bounces = 2;
	ember.speed = 3;
};

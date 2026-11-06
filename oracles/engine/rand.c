// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// rand, as the VM oracle's (DIFF-6.2): a 32-bit state from 1, so a run's random numbers are
// the same on every OS. id's files call it through oracles/engine/rand.h; the engine and view
// oracles both link it.

#include "rand.h"

static unsigned random_state = 1;

int qs_rand(void) {
    random_state = random_state * 1103515245u + 12345u;
    return (int)((random_state >> 16) & 0x7fff);
}

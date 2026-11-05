// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// What the functions cargo xtask oracles copies out of id's host.c and view.c read at their
// files' scope rather than through a header: host.c's hunk level, and view.c's roll settings
// and the three vectors V_CalcRoll fills (oracles/engine/engine.c defines the settings). It
// comes after id's quakedef.h, which has no include guard.

#ifndef QS_EXTRACTED_H
#define QS_EXTRACTED_H

extern int host_hunklevel;
extern cvar_t cl_rollspeed;
extern cvar_t cl_rollangle;

#ifdef QS_EXTRACTED_VIEW
// view.c: `vec3_t forward, right, up;`, file-level and used by V_CalcRoll alone here.
static vec3_t forward, right, up;
#endif

#endif

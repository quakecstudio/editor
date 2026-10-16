// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The mock engine around id's code: the parts of the server that differential.md says the
// mock keeps (settings, message buffer sizes, model bounds, the random state, linking without
// an area tree) and the world queries it answers as empty space (DIFF-2.4, DIFF-5.5, DIFF-5.7,
// DIFF-6, DIFF-7). Written from the spec; nothing here is id's code.

#include "oracle.h"

// --- Settings (DIFF-2.4, BUILTIN-11.7, BUILTIN-11.8) ---

cvar_t deathmatch = {"deathmatch", NULL, 0};
cvar_t coop = {"coop", NULL, 0};
cvar_t teamplay = {"teamplay", NULL, 0};
cvar_t skill = {"skill", NULL, 0};
int current_skill;

#define QS_MAX_CVARS 256
static cvar_t *cvars[QS_MAX_CVARS];
static int num_cvars;

static cvar_t *find_cvar(const char *name) {
    for (int i = 0; i < num_cvars; i++)
        if (strcmp(cvars[i]->name, name) == 0)
            return cvars[i];
    return NULL;
}

static char *copy_text(const char *text) {
    size_t n = strlen(text) + 1;
    char *p = malloc(n);
    if (!p)
        Sys_Error("vm-oracle: out of memory");
    memcpy(p, text, n);
    return p;
}

static void set_text(cvar_t *var, const char *text) {
    free(var->string);
    var->string = copy_text(text);
    var->value = Q_atof(var->string);
}

// Sets a setting from the scenario, creating it if it is not one of the four; a later line
// for a name replaces an earlier one.
void qs_set_cvar(const char *name, const char *text) {
    if (num_cvars == 0) {
        cvars[num_cvars++] = &skill;
        cvars[num_cvars++] = &deathmatch;
        cvars[num_cvars++] = &coop;
        cvars[num_cvars++] = &teamplay;
        set_text(&skill, "1");
        set_text(&deathmatch, "0");
        set_text(&coop, "0");
        set_text(&teamplay, "0");
    }
    if (!name)
        return;
    cvar_t *var = find_cvar(name);
    if (!var) {
        if (num_cvars == QS_MAX_CVARS)
            Sys_Error("vm-oracle: too many settings");
        var = calloc(1, sizeof(*var));
        if (!var)
            Sys_Error("vm-oracle: out of memory");
        var->name = copy_text(name);
        cvars[num_cvars++] = var;
    }
    set_text(var, text);
}

float Cvar_VariableValue(char *var_name) {
    cvar_t *var = find_cvar(var_name);
    return var ? var->value : 0;
}

// What a broadcast of the change would send is not kept (DIFF-6.5).
void Cvar_Set(char *var_name, char *value) {
    cvar_t *var = find_cvar(var_name);
    if (!var) {
        Con_Printf("Cvar_Set: variable %s not found\n", var_name);
        return;
    }
    set_text(var, value);
}

// --- Output (DIFF-6.4, DIFF-6.5) ---

static int command_buffer_size;

// BUILTIN-11.5: the command buffer is never run; only its size is kept.
void Cbuf_AddText(char *text) {
    int length = (int)strlen(text);
    if (command_buffer_size + length >= 8192) {
        Con_Printf("Cbuf_AddText: overflow\n");
        return;
    }
    command_buffer_size += length;
}

void Host_ClientCommands(char *fmt, ...) {
    (void)fmt;
}

void SV_BroadcastPrintf(char *fmt, ...) {
    (void)fmt;
}

static byte datagram_buf[MAX_DATAGRAM];
static byte reliable_buf[MAX_DATAGRAM];
static byte signon_buf[8192];
static byte *sink;
static int sink_size;

// The unreliable and reliable datagrams and the signon buffer keep id's sizes and overflow
// error; a client's reliable message is a sink that never overflows (DIFF-6.5).
void qs_init_buffers(void) {
    sv.datagram.data = datagram_buf;
    sv.datagram.maxsize = (int)sizeof(datagram_buf);
    sv.reliable_datagram.data = reliable_buf;
    sv.reliable_datagram.maxsize = (int)sizeof(reliable_buf);
    sv.signon.data = signon_buf;
    sv.signon.maxsize = (int)sizeof(signon_buf);
    for (int i = 0; i < svs.maxclients; i++) {
        svs.clients[i].message.data = NULL;
        svs.clients[i].message.maxsize = 0;
        svs.clients[i].message.allowoverflow = true;
    }
}

void SZ_Clear(sizebuf_t *buf) {
    buf->cursize = 0;
}

// BUILTIN-13.3, BUILTIN-13.4.
void *SZ_GetSpace(sizebuf_t *buf, int length) {
    if (!buf->data) {
        if (length > sink_size) {
            byte *grown = realloc(sink, (size_t)length);
            if (!grown)
                Sys_Error("vm-oracle: out of memory");
            sink = grown;
            sink_size = length;
        }
        return sink;
    }
    if (buf->cursize + length > buf->maxsize) {
        if (!buf->allowoverflow)
            Sys_Error("SZ_GetSpace: overflow without allowoverflow set");
        if (length > buf->maxsize)
            Sys_Error("SZ_GetSpace: %i is > full buffer size", length);
        buf->overflowed = true;
        Con_Printf("SZ_GetSpace: overflow");
        SZ_Clear(buf);
    }
    void *data = buf->data + buf->cursize;
    buf->cursize += length;
    return data;
}

// --- Models (DIFF-6.3) ---

typedef struct {
    char *name;
    vec3_t mins, maxs;
} bounds_t;

static bounds_t *model_bounds;
static int num_model_bounds;

void qs_add_model_bounds(const char *name, const float *mins, const float *maxs) {
    bounds_t *grown = realloc(model_bounds, (size_t)(num_model_bounds + 1) * sizeof(*grown));
    if (!grown)
        Sys_Error("vm-oracle: out of memory");
    model_bounds = grown;
    bounds_t *b = &model_bounds[num_model_bounds++];
    b->name = copy_text(name);
    VectorCopy(mins, b->mins);
    VectorCopy(maxs, b->maxs);
}

// precache_model never fails to load a model; the bounds are a model directive's, '-16 -16
// -16' to '16 16 16' for any other .mdl, and zero for everything else.
model_t *Mod_ForName(char *name, qboolean crash) {
    (void)crash;
    model_t *mod = calloc(1, sizeof(*mod));
    if (!mod)
        Sys_Error("vm-oracle: out of memory");
    for (int i = 0; i < num_model_bounds; i++) {
        if (strcmp(model_bounds[i].name, name) == 0) {
            VectorCopy(model_bounds[i].mins, mod->mins);
            VectorCopy(model_bounds[i].maxs, mod->maxs);
            return mod;
        }
    }
    size_t length = strlen(name);
    if (length >= 4 && memcmp(name + length - 4, ".mdl", 4) == 0) {
        for (int i = 0; i < 3; i++) {
            mod->mins[i] = -16;
            mod->maxs[i] = 16;
        }
    }
    return mod;
}

// --- The world as empty space (DIFF-5.5, DIFF-5.7, DIFF-7) ---

// DIFF-5.7 and EDICT-12.2: linking writes absmin and absmax, and touches nothing.
void SV_LinkEdict(edict_t *ent, qboolean touch_triggers) {
    (void)touch_triggers;
    if (ent == sv.edicts || ent->free)
        return;
    VectorAdd(ent->v.origin, ent->v.mins, ent->v.absmin);
    VectorAdd(ent->v.origin, ent->v.maxs, ent->v.absmax);
    if (qs_ftoi(ent->v.flags) & FL_ITEM) {
        ent->v.absmin[0] -= 15;
        ent->v.absmin[1] -= 15;
        ent->v.absmax[0] += 15;
        ent->v.absmax[1] += 15;
    } else {
        ent->v.absmin[0] -= 1;
        ent->v.absmin[1] -= 1;
        ent->v.absmin[2] -= 1;
        ent->v.absmax[0] += 1;
        ent->v.absmax[1] += 1;
        ent->v.absmax[2] += 1;
    }
}

void SV_UnlinkEdict(edict_t *ent) {
    (void)ent;
}

// DIFF-7.1: no trace hits anything, so id's PF_traceline writes the globals DIFF-7.1 lists.
trace_t SV_Move(vec3_t start, vec3_t mins, vec3_t maxs, vec3_t end, int type, edict_t *passedict) {
    (void)start;
    (void)mins;
    (void)maxs;
    (void)type;
    (void)passedict;
    trace_t trace;
    memset(&trace, 0, sizeof(trace));
    trace.fraction = 1;
    trace.inopen = true;
    VectorCopy(end, trace.endpos);
    return trace;
}

// DIFF-7.2.
int SV_PointContents(vec3_t p) {
    (void)p;
    return CONTENTS_EMPTY;
}

// DIFF-7.4.
qboolean SV_CheckBottom(edict_t *ent) {
    (void)ent;
    return true;
}

// DIFF-5.5: a pusher's move is the zero-velocity branch of id's SV_PushMove, whatever the
// velocity.
void SV_PushMove(edict_t *pusher, float movetime) {
    pusher->v.ltime += movetime;
}

// --- Map keys (EDICT-8.2, EDICT-8.3) ---

char com_token[1024];

// id's ED_ParseEpair, except that a vector value with fewer than three space-separated
// components gets 0 for the missing ones, where id's reads past the end of the value
// (EDICT-8.3), and a value of any length is read (id copies it into 128 bytes).
qboolean ED_ParseEpair(void *base, ddef_t *key, char *s) {
    void *d = (void *)((int *)base + key->ofs);
    switch (key->type & ~DEF_SAVEGLOBAL) {
    case ev_string:
        *(string_t *)d = (string_t)(ED_NewString(s) - pr_strings);
        break;
    case ev_float:
        *(float *)d = (float)atof(s);
        break;
    case ev_vector: {
        const char *w = s;
        for (int i = 0; i < 3; i++) {
            if (!w) {
                ((float *)d)[i] = 0;
                continue;
            }
            const char *space = strchr(w, ' ');
            size_t n = space ? (size_t)(space - w) : strlen(w);
            char *component = malloc(n + 1);
            if (!component)
                Sys_Error("vm-oracle: out of memory");
            memcpy(component, w, n);
            component[n] = 0;
            ((float *)d)[i] = (float)atof(component);
            free(component);
            w = space ? space + 1 : NULL;
        }
        break;
    }
    case ev_entity:
        *(int *)d = (int)EDICT_TO_PROG(EDICT_NUM(atoi(s)));
        break;
    case ev_field: {
        ddef_t *def = ED_FindField(s);
        if (!def) {
            Con_Printf("Can't find field %s\n", s);
            return false;
        }
        *(int *)d = G_INT(def->ofs);
        break;
    }
    case ev_function: {
        dfunction_t *func = ED_FindFunction(s);
        if (!func) {
            Con_Printf("Can't find function %s\n", s);
            return false;
        }
        *(func_t *)d = (func_t)(func - pr_functions);
        break;
    }
    default:
        break;
    }
    return true;
}

// --- Random numbers (DIFF-6.2) ---

static unsigned random_state = 1;

void qs_set_random_state(unsigned state) {
    random_state = state;
}

int qs_rand(void) {
    random_state = random_state * 1103515245u + 12345u;
    return (int)((random_state >> 16) & 0x7fff);
}

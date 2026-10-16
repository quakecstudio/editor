// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The builtin tables. The scenario table is id's pr_builtin (builtins.md BUILTIN-5) with id's
// PF_ functions where they are compiled from the pinned source, and this file's own where
// builtins.md fixes an answer id's C leaves undefined (BUILTIN-7.5, BUILTIN-7.6, BUILTIN-12.6,
// BUILTIN-16.3), where the error message is differential.md's (DIFF-8.2), or where DIFF-7
// mocks a world builtin. The vector table holds the stubs of docs/spec/vectors/vm/README.md.

#include "oracle.h"

void PF_Fixme(void);

// --- Strings (BUILTIN-7) ---

// BUILTIN-7.3 and BUILTIN-7.5: the text of every string parameter from `first` to the last
// one the call supplied, never truncated.
char *PF_VarString(int first) {
    static char *out;
    static size_t size;
    size_t used = 0;
    if (!out) {
        size = 256;
        out = malloc(size);
        if (!out)
            Sys_Error("vm-oracle: out of memory");
    }
    out[0] = 0;
    for (int i = first; i < pr_argc; i++) {
        const char *s = G_STRING(OFS_PARM0 + i * 3);
        size_t n = strlen(s);
        if (used + n + 1 > size) {
            while (used + n + 1 > size)
                size *= 2;
            char *grown = realloc(out, size);
            if (!grown)
                Sys_Error("vm-oracle: out of memory");
            out = grown;
        }
        memcpy(out + used, s, n + 1);
        used += n;
    }
    return out;
}

// The one buffer ftos and vtos write (BUILTIN-7.4), large enough for any text they make.
static char *string_temp;

static char *temp_buffer(void) {
    if (!string_temp) {
        string_temp = qs_alloc(512);
        qs_engine_string_register(string_temp);
    }
    return string_temp;
}

// BUILTIN-7.6: a NaN or an infinity is printed the same way by every C library.
static int format_float(char *out, size_t size, float v) {
    if (v != v)
        return snprintf(out, size, "  nan");
    if (isinf(v))
        return snprintf(out, size, v > 0 ? "  inf" : " -inf");
    return snprintf(out, size, "%5.1f", v);
}

// BUILTIN-7.1.
void PF_ftos(void) {
    char *buf = temp_buffer();
    float v = G_FLOAT(OFS_PARM0);
    int i = qs_ftoi(v);
    if (v == (float)i)
        snprintf(buf, 512, "%d", i);
    else
        format_float(buf, 512, v);
    qs_engine_string_touched(buf);
    G_INT(OFS_RETURN) = (int)(buf - pr_strings);
}

// BUILTIN-7.2.
void PF_vtos(void) {
    char *buf = temp_buffer();
    char parts[3][160];
    for (int i = 0; i < 3; i++)
        format_float(parts[i], sizeof(parts[i]), G_VECTOR(OFS_PARM0)[i]);
    snprintf(buf, 512, "'%s %s %s'", parts[0], parts[1], parts[2]);
    qs_engine_string_touched(buf);
    G_INT(OFS_RETURN) = (int)(buf - pr_strings);
}

void PF_dprint(void) {
    Con_DPrintf("%s", PF_VarString(0));
}

// --- Errors and debugging (BUILTIN-16, DIFF-8.2) ---

// BUILTIN-16.1: the message, then self is printed (read with BUILTIN-2.3's check), then a
// host error whose record carries the joined text.
void PF_error(void) {
    char *s = PF_VarString(0);
    Con_Printf("======SERVER ERROR in %s:\n%s\n", pr_strings + pr_xfunction->s_name, s);
    edict_t *ed = PROG_TO_EDICT(pr_global_struct->self);
    ED_Print(ed);
    qs_host_error(s);
}

// BUILTIN-16.2: as error, and self is freed first.
void PF_objerror(void) {
    char *s = PF_VarString(0);
    Con_Printf("======OBJECT ERROR in %s:\n%s\n", pr_strings + pr_xfunction->s_name, s);
    edict_t *ed = PROG_TO_EDICT(pr_global_struct->self);
    ED_Print(ed);
    ED_Free(ed);
    qs_host_error(s);
}

// BUILTIN-16.3.
void PF_break(void) {
    Con_Printf("break statement\n");
    Sys_Error("break statement");
}

// BUILTIN-16.4: the flag changes nothing the trace records (DIFF-6.7).
void PF_traceon(void) {
    pr_trace = true;
}

void PF_traceoff(void) {
    pr_trace = false;
}

// BUILTIN-16.5: the entity goes through NUM_FOR_EDICT; nothing is printed.
void PF_eprint(void) {
    (void)G_EDICTNUM(OFS_PARM0);
}

void PF_coredump(void) {
}

// BUILTIN-12.6: a style outside 0 to 63 is a runtime error, after the value is read.
static void qs_lightstyle(void) {
    int style = qs_ftoi(G_FLOAT(OFS_PARM0));
    (void)G_STRING(OFS_PARM1);
    if (style < 0 || style >= MAX_LIGHTSTYLES)
        PR_RunError("invalid lightstyle");
    PF_lightstyle();
}

// --- World builtins (DIFF-7) ---

// DIFF-7.3, with PF_newcheckclient's cycle and no PVS.
static int new_check_client(int check) {
    if (check < 1)
        check = 1;
    if (check > svs.maxclients)
        check = svs.maxclients;
    int i = check == svs.maxclients ? 1 : check + 1;
    for (;; i++) {
        if (i == svs.maxclients + 1)
            i = 1;
        edict_t *ent = EDICT_NUM(i);
        if (i == check)
            break;
        if (ent->free)
            continue;
        if (ent->v.health <= 0)
            continue;
        if (qs_ftoi(ent->v.flags) & FL_NOTARGET)
            continue;
        break;
    }
    return i;
}

void PF_checkclient(void) {
    if (sv.time - sv.lastchecktime >= 0.1) {
        sv.lastcheck = new_check_client(sv.lastcheck);
        sv.lastchecktime = sv.time;
    }
    edict_t *ent = EDICT_NUM(sv.lastcheck);
    if (ent->free || ent->v.health <= 0)
        G_INT(OFS_RETURN) = 0;
    else
        G_INT(OFS_RETURN) = (int)EDICT_TO_PROG(ent);
}

// DIFF-7.4.
void PF_aim(void) {
    (void)G_EDICT(OFS_PARM0);
    VectorCopy(pr_global_struct->v_forward, G_VECTOR(OFS_RETURN));
}

// DIFF-7.5.
void PF_droptofloor(void) {
    edict_t *ent = PROG_TO_EDICT(pr_global_struct->self);
    SV_LinkEdict(ent, false);
    ent->v.flags = (float)(qs_ftoi(ent->v.flags) | FL_ONGROUND);
    ent->v.groundentity = 0;
    G_FLOAT(OFS_RETURN) = 1;
}

static qboolean can_move(edict_t *ent) {
    return (qs_ftoi(ent->v.flags) & (FL_ONGROUND | FL_FLY | FL_SWIM)) != 0;
}

// DIFF-7.6: the step along a yaw, with id's expressions from PF_walkmove and
// SV_StepDirection.
static void step(edict_t *ent, float yaw, float dist) {
    vec3_t move;
    yaw = (float)(yaw * M_PI * 2 / 360);
    move[0] = (float)(cos(yaw) * dist);
    move[1] = (float)(sin(yaw) * dist);
    ent->v.origin[0] = ent->v.origin[0] + move[0];
    ent->v.origin[1] = ent->v.origin[1] + move[1];
}

// DIFF-7.7.
void PF_walkmove(void) {
    edict_t *ent = PROG_TO_EDICT(pr_global_struct->self);
    float yaw = G_FLOAT(OFS_PARM0);
    float dist = G_FLOAT(OFS_PARM1);
    if (!can_move(ent)) {
        G_FLOAT(OFS_RETURN) = 0;
        return;
    }
    step(ent, yaw, dist);
    SV_LinkEdict(ent, true);
    G_FLOAT(OFS_RETURN) = 1;
}

// DIFF-7.8.
void PF_movetogoal(void) {
    edict_t *ent = PROG_TO_EDICT(pr_global_struct->self);
    float dist = G_FLOAT(OFS_PARM0);
    if (!can_move(ent)) {
        G_FLOAT(OFS_RETURN) = 0;
        return;
    }
    float yaw = ent->v.ideal_yaw;
    PF_changeyaw();
    vec3_t oldorigin;
    VectorCopy(ent->v.origin, oldorigin);
    step(ent, yaw, dist);
    float delta = ent->v.angles[YAW] - ent->v.ideal_yaw;
    if (delta > 45 && delta < 315)
        VectorCopy(oldorigin, ent->v.origin);
    SV_LinkEdict(ent, true);
}

// --- The tables ---

builtin_t qs_scenario_builtins[QS_NUM_BUILTINS] = {
    PF_Fixme,          // 0
    PF_makevectors,    // 1
    PF_setorigin,      // 2
    PF_setmodel,       // 3
    PF_setsize,        // 4
    PF_Fixme,          // 5
    PF_break,          // 6
    PF_random,         // 7
    PF_sound,          // 8
    PF_normalize,      // 9
    PF_error,          // 10
    PF_objerror,       // 11
    PF_vlen,           // 12
    PF_vectoyaw,       // 13
    PF_Spawn,          // 14
    PF_Remove,         // 15
    PF_traceline,      // 16
    PF_checkclient,    // 17
    PF_Find,           // 18
    PF_precache_sound, // 19
    PF_precache_model, // 20
    PF_stuffcmd,       // 21
    PF_findradius,     // 22
    PF_bprint,         // 23
    PF_sprint,         // 24
    PF_dprint,         // 25
    PF_ftos,           // 26
    PF_vtos,           // 27
    PF_coredump,       // 28
    PF_traceon,        // 29
    PF_traceoff,       // 30
    PF_eprint,         // 31
    PF_walkmove,       // 32
    PF_Fixme,          // 33
    PF_droptofloor,    // 34
    qs_lightstyle,     // 35
    PF_rint,           // 36
    PF_floor,          // 37
    PF_ceil,           // 38
    PF_Fixme,          // 39
    PF_checkbottom,    // 40
    PF_pointcontents,  // 41
    PF_Fixme,          // 42
    PF_fabs,           // 43
    PF_aim,            // 44
    PF_cvar,           // 45
    PF_localcmd,       // 46
    PF_nextent,        // 47
    PF_particle,       // 48
    PF_changeyaw,      // 49
    PF_Fixme,          // 50
    PF_vectoangles,    // 51
    PF_WriteByte,      // 52
    PF_WriteChar,      // 53
    PF_WriteShort,     // 54
    PF_WriteLong,      // 55
    PF_WriteCoord,     // 56
    PF_WriteAngle,     // 57
    PF_WriteString,    // 58
    PF_WriteEntity,    // 59
    PF_Fixme,          // 60
    PF_Fixme,          // 61
    PF_Fixme,          // 62
    PF_Fixme,          // 63
    PF_Fixme,          // 64
    PF_Fixme,          // 65
    PF_Fixme,          // 66
    PF_movetogoal,     // 67
    PF_precache_file,  // 68
    PF_makestatic,     // 69
    PF_changelevel,    // 70
    PF_Fixme,          // 71
    PF_cvar_set,       // 72
    PF_centerprint,    // 73
    PF_ambientsound,   // 74
    PF_precache_model, // 75
    PF_precache_sound, // 76
    PF_precache_file,  // 77
    PF_setspawnparms,  // 78
};

// The vectors' environment: builtin 2 runs the function in OFS_PARM0 as a nested run; every
// other number writes the argument count as a float to slot 1 and copies slot 4 to slot 2.
static void vector_stub(void) {
    G_FLOAT(OFS_RETURN) = (float)pr_argc;
    G_INT(OFS_RETURN + 1) = G_INT(OFS_PARM0);
}

static void vector_nested(void) {
    qs_run(G_INT(OFS_PARM0));
}

builtin_t qs_vector_builtins[QS_NUM_BUILTINS] = {
    vector_stub, vector_stub, vector_nested, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub, vector_stub, vector_stub, vector_stub, vector_stub, vector_stub,
    vector_stub,
};

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The environment the VM oracle compiles id's code in. id's WinQuake/pr_exec.c and the engine
// functions cargo xtask oracles extracts from the pinned sources include "quakedef.h"; this
// file stands in for WinQuake's, declaring only what those functions use, with the names and
// types they expect. id's own headers (mathlib.h, pr_comp.h, progdefs.h, progdefs.q1, progs.h)
// are fetched by checksum and included unchanged. The values below are the ones docs/spec/
// states for the WinQuake build without QUAKE2 (edicts.md, builtins.md, limits.md).

#ifndef QS_QUAKEDEF_H
#define QS_QUAKEDEF_H

#include <math.h>
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef unsigned char byte;
typedef enum { false, true } qboolean;

// limits.md
#define MAX_EDICTS 600
#define MAX_MODELS 256
#define MAX_SOUNDS 256
#define MAX_LIGHTSTYLES 64
#define MAX_DATAGRAM 1024
#define MAX_MSGLEN 8000
#define MAX_QPATH 64
#define NUM_SPAWN_PARMS 16

#define PITCH 0
#define YAW 1
#define ROLL 2

// builtins.md BUILTIN-12 and BUILTIN-13 constants
#define svc_sound 6
#define svc_print 8
#define svc_stufftext 9
#define svc_lightstyle 12
#define svc_particle 18
#define svc_spawnstatic 20
#define svc_centerprint 26
#define svc_spawnstaticsound 29
#define MSG_BROADCAST 0
#define MSG_ONE 1
#define MSG_ALL 2
#define MSG_INIT 3
#define SND_VOLUME 1
#define SND_ATTENUATION 2
#define DEFAULT_SOUND_PACKET_VOLUME 255
#define DEFAULT_SOUND_PACKET_ATTENUATION 1.0

// edicts.md EDICT-3 to EDICT-12 constants
#define MOVETYPE_NONE 0
#define MOVETYPE_WALK 3
#define MOVETYPE_STEP 4
#define MOVETYPE_FLY 5
#define MOVETYPE_TOSS 6
#define MOVETYPE_PUSH 7
#define MOVETYPE_NOCLIP 8
#define MOVETYPE_FLYMISSILE 9
#define MOVETYPE_BOUNCE 10
#define SOLID_NOT 0
#define SOLID_TRIGGER 1
#define SOLID_BSP 4
#define FL_FLY 1
#define FL_SWIM 2
#define FL_NOTARGET 128
#define FL_ITEM 256
#define FL_ONGROUND 512
#define DAMAGE_AIM 2
#define EF_MUZZLEFLASH 2
#define SPAWNFLAG_NOT_EASY 256
#define SPAWNFLAG_NOT_MEDIUM 512
#define SPAWNFLAG_NOT_HARD 1024
#define SPAWNFLAG_NOT_DEATHMATCH 2048
#define CONTENTS_EMPTY -1

#include "mathlib.h"

typedef struct link_s {
    struct link_s *prev, *next;
} link_t;

typedef struct {
    vec3_t origin;
    vec3_t angles;
    int modelindex;
    int frame;
    int colormap;
    int skin;
    int effects;
} entity_state_t;

// pr_exec.c is built with QS_PR_EXEC, which redirects two of its variables and one C library
// call, and nothing else (oracles/README.md, "How the VM is observed"):
// - its check of the statement trace flag, made after each statement becomes current and
//   before it runs, calls the oracle's hook, which records the statement and reads as false;
// - its call stack is storage the oracle owns, with one spare frame: on a stack overflow
//   PR_StackTrace writes one frame past the 32 (pr_stack[pr_depth] with pr_depth 32), which in
//   id's 32-bit build lands in an unused local stack slot and here would land on whatever the
//   linker put next;
// - strcmp returns the difference of the first differing bytes, as glibc's and macOS's do and
//   vm.md VM-7.2 fixes for NE_S; C fixes only its sign, and the Windows C runtime returns -1
//   and 1.
#ifdef QS_PR_EXEC
qboolean *qs_statement_hook(void);
#define pr_trace (*qs_statement_hook())
#define pr_stack (*qs_pr_stack())
int qs_strcmp(const char *a, const char *b);
#define strcmp qs_strcmp
#endif

#include "progs.h"

// What pr_cmds.c defines at the top of the file, before the functions the oracle extracts.
#define RETURN_EDICT(e) (((int *)pr_globals)[OFS_RETURN] = EDICT_TO_PROG(e))

typedef struct sizebuf_s {
    qboolean allowoverflow;
    qboolean overflowed;
    byte *data;
    int maxsize;
    int cursize;
} sizebuf_t;

typedef struct model_s {
    char name[MAX_QPATH];
    vec3_t mins, maxs;
} model_t;

typedef enum { ss_loading, ss_active } server_state_t;

typedef struct {
    qboolean active;
    qboolean paused;
    double time;
    int lastcheck;
    double lastchecktime;
    char *model_precache[MAX_MODELS + 1];
    struct model_s *models[MAX_MODELS];
    char *sound_precache[MAX_SOUNDS + 1];
    char *lightstyles[MAX_LIGHTSTYLES];
    int num_edicts;
    int max_edicts;
    edict_t *edicts;
    server_state_t state;
    sizebuf_t datagram;
    sizebuf_t reliable_datagram;
    sizebuf_t signon;
} server_t;

typedef struct client_s {
    qboolean active;
    qboolean spawned;
    sizebuf_t message;
    edict_t *edict;
    float spawn_parms[NUM_SPAWN_PARMS];
} client_t;

typedef struct {
    int maxclients;
    struct client_s *clients;
    int serverflags;
    qboolean changelevel_issued;
} server_static_t;

typedef struct {
    vec3_t normal;
    float dist;
} plane_t;

typedef struct {
    qboolean allsolid;
    qboolean startsolid;
    qboolean inopen, inwater;
    float fraction;
    vec3_t endpos;
    plane_t plane;
    edict_t *ent;
} trace_t;

typedef struct cvar_s {
    char *name;
    char *string;
    float value;
} cvar_t;

extern server_t sv;
extern server_static_t svs;
extern client_t *host_client;
extern double host_frametime;
extern int current_skill;
extern cvar_t deathmatch, coop, teamplay, skill;
extern char com_token[1024];

#define Q_memcpy memcpy
#define Q_strlen(s) ((int)strlen(s))

void Con_Printf(char *fmt, ...);
void Con_DPrintf(char *fmt, ...);
void Sys_Error(char *error, ...);
void Host_Error(char *error, ...);
void Host_ClientCommands(char *fmt, ...);
void SV_BroadcastPrintf(char *fmt, ...);
char *va(char *format, ...);
void *Hunk_Alloc(int size);
void Cbuf_AddText(char *text);
float Cvar_VariableValue(char *var_name);
void Cvar_Set(char *var_name, char *value);

char *COM_Parse(char *data);
float Q_atof(char *str);

void SZ_Clear(sizebuf_t *buf);
void *SZ_GetSpace(sizebuf_t *buf, int length);
void SZ_Write(sizebuf_t *buf, void *data, int length);
void MSG_WriteChar(sizebuf_t *sb, int c);
void MSG_WriteByte(sizebuf_t *sb, int c);
void MSG_WriteShort(sizebuf_t *sb, int c);
void MSG_WriteLong(sizebuf_t *sb, int c);
void MSG_WriteString(sizebuf_t *sb, char *s);
void MSG_WriteCoord(sizebuf_t *sb, float f);
void MSG_WriteAngle(sizebuf_t *sb, float f);

model_t *Mod_ForName(char *name, qboolean crash);
void SV_LinkEdict(edict_t *ent, qboolean touch_triggers);
void SV_UnlinkEdict(edict_t *ent);
trace_t SV_Move(vec3_t start, vec3_t mins, vec3_t maxs, vec3_t end, int type, edict_t *passedict);
int SV_PointContents(vec3_t p);
qboolean SV_CheckBottom(edict_t *ent);
void SV_StartParticle(vec3_t org, vec3_t dir, int color, int count);
void SV_StartSound(edict_t *entity, int channel, char *sample, int volume, float attenuation);
int SV_ModelIndex(char *name);
qboolean SV_RunThink(edict_t *ent);
void SV_PushMove(edict_t *pusher, float movetime);
void SV_Physics_Pusher(edict_t *ent);

ddef_t *ED_FindField(char *name);
dfunction_t *ED_FindFunction(char *name);
void ED_ClearEdict(edict_t *e);
qboolean ED_ParseEpair(void *base, ddef_t *key, char *s);

char *PF_VarString(int first);
void SetMinMaxSize(edict_t *e, float *min, float *max, qboolean rotate);
sizebuf_t *WriteDest(void);
void PR_CheckEmptyString(char *s);

// The builtins, for the table in builtins.c. Those marked "ours" are written in this oracle
// because builtins.md fixes an answer id's C leaves undefined, or differential.md DIFF-7 mocks
// them; the rest are id's, extracted unchanged.
void PF_makevectors(void);
void PF_setorigin(void);
void PF_setmodel(void);
void PF_setsize(void);
void PF_break(void);
void PF_random(void);
void PF_sound(void);
void PF_normalize(void);
void PF_error(void);
void PF_objerror(void);
void PF_vlen(void);
void PF_vectoyaw(void);
void PF_Spawn(void);
void PF_Remove(void);
void PF_traceline(void);
void PF_checkclient(void);
void PF_Find(void);
void PF_precache_sound(void);
void PF_precache_model(void);
void PF_stuffcmd(void);
void PF_findradius(void);
void PF_bprint(void);
void PF_sprint(void);
void PF_dprint(void);
void PF_ftos(void);
void PF_vtos(void);
void PF_coredump(void);
void PF_traceon(void);
void PF_traceoff(void);
void PF_eprint(void);
void PF_walkmove(void);
void PF_droptofloor(void);
void PF_lightstyle(void);
void PF_rint(void);
void PF_floor(void);
void PF_ceil(void);
void PF_checkbottom(void);
void PF_pointcontents(void);
void PF_fabs(void);
void PF_aim(void);
void PF_cvar(void);
void PF_localcmd(void);
void PF_nextent(void);
void PF_particle(void);
void PF_changeyaw(void);
void PF_vectoangles(void);
void PF_WriteByte(void);
void PF_WriteChar(void);
void PF_WriteShort(void);
void PF_WriteLong(void);
void PF_WriteCoord(void);
void PF_WriteAngle(void);
void PF_WriteString(void);
void PF_WriteEntity(void);
void PF_movetogoal(void);
void PF_precache_file(void);
void PF_makestatic(void);
void PF_changelevel(void);
void PF_cvar_set(void);
void PF_centerprint(void);
void PF_ambientsound(void);
void PF_setspawnparms(void);

// Every translation unit except pr_exec.c is built with QS_ENGINE. In them a call into QuakeC
// goes through the oracle's run wrapper, which writes the trace's run and exit records, and
// the argument accessors check entity and string values as builtins.md BUILTIN-2.7, BUILTIN-2.5
// and BUILTIN-2.8 fix (and field indexes, BUILTIN-8.3), at the point id's code reads them
// (BUILTIN-2.9). PF_makestatic reads its entity's model as `pr_strings + ent->v.model`, with
// no accessor; in the file of id's pr_cmds.c functions that text is checked as it is passed
// to SV_ModelIndex, which is where id's code reads it.
#ifdef QS_ENGINE
void qs_run(func_t fnum);
edict_t *qs_edict(int value);
char *qs_string(int value);
char *qs_field_string(edict_t *ed, int field);
char *qs_checked_text(char *text);
#ifdef QS_EXTRACTED_PR_CMDS
#define SV_ModelIndex(name) SV_ModelIndex(qs_checked_text(name))
#endif
#define PR_ExecuteProgram qs_run
#undef G_EDICT
#define G_EDICT(o) qs_edict(G_INT(o))
#undef PROG_TO_EDICT
#define PROG_TO_EDICT(e) qs_edict(e)
#undef G_STRING
#define G_STRING(o) qs_string(G_INT(o))
#undef E_STRING
#define E_STRING(e, o) qs_field_string(e, o)
#define rand qs_rand
int qs_rand(void);
#endif

#endif

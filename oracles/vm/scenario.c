// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// Scenario mode: differential.md DIFF-2 (the scenario file), DIFF-3 (setting up the map),
// DIFF-4 (clients), DIFF-5 (server frames) and DIFF-8 (errors and the end of a scenario).
// Map spawning and thinking are id's ED_LoadFromFile, SV_RunThink and SV_Physics_Pusher,
// compiled from the pinned source; the loop around them is this file's.

#include <errno.h>
#include <limits.h>

#include "oracle.h"

void ED_LoadFromFile(char *data);

// --- The scenario (DIFF-2) ---

#define QS_MAX_CLIENTS 16

typedef enum { INPUT_BUTTON0, INPUT_BUTTON2, INPUT_IMPULSE, INPUT_V_ANGLE } input_field_t;

typedef struct {
    long frame;
    long edict;
    input_field_t field;
    float values[3];
} input_t;

typedef struct {
    char *progs;
    char *entities;
    char map[64];
    long maxclients;
    long clients;
    long submodels;
    long frames;
    double frametime;
    unsigned long seed;
    input_t *inputs;
    int num_inputs;
} scenario_t;

static scenario_t scenario;
static qboolean entered[QS_MAX_CLIENTS];
static client_t clients[QS_MAX_CLIENTS];

static int refuse(const char *path, int line, const char *why) {
    if (line > 0)
        fprintf(stderr, "vm-oracle: %s:%d: %s\n", path, line, why);
    else
        fprintf(stderr, "vm-oracle: %s: %s\n", path, why);
    return QS_EXIT_REFUSED;
}

// DIFF-2.3: decimal digits with an optional leading `-`, in range.
static qboolean parse_integer(const char *text, long long min, long long max, long long *out) {
    const char *p = text;
    if (*p == '-')
        p++;
    if (!*p)
        return false;
    for (const char *q = p; *q; q++)
        if (*q < '0' || *q > '9')
            return false;
    if (strlen(p) > 18)
        return false;
    long long value = strtoll(text, NULL, 10);
    if (value < min || value > max)
        return false;
    *out = value;
    return true;
}

// A decimal number: an optional `-`, digits with at most one `.`, and an optional exponent.
static qboolean decimal_syntax(const char *text) {
    const char *p = text;
    int digits = 0;
    if (*p == '-')
        p++;
    while (*p >= '0' && *p <= '9') {
        p++;
        digits++;
    }
    if (*p == '.') {
        p++;
        while (*p >= '0' && *p <= '9') {
            p++;
            digits++;
        }
    }
    if (digits == 0)
        return false;
    if (*p == 'e' || *p == 'E') {
        p++;
        if (*p == '-' || *p == '+')
            p++;
        if (*p < '0' || *p > '9')
            return false;
        while (*p >= '0' && *p <= '9')
            p++;
    }
    return *p == 0;
}

static qboolean parse_float(const char *text, float *out) {
    if (!decimal_syntax(text))
        return false;
    *out = strtof(text, NULL);
    return true;
}

static char *join_path(const char *dir, const char *name) {
    size_t a = strlen(dir);
    size_t b = strlen(name);
    char *path = malloc(a + b + 2);
    if (!path) {
        fprintf(stderr, "vm-oracle: out of memory\n");
        exit(QS_EXIT_IO);
    }
    memcpy(path, dir, a);
    path[a] = '/';
    memcpy(path + a + 1, name, b + 1);
    return path;
}

static char *read_file(const char *path, long *size_out) {
    FILE *f = fopen(path, "rb");
    if (!f)
        return NULL;
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return NULL;
    }
    long size = ftell(f);
    if (size < 0) {
        fclose(f);
        return NULL;
    }
    rewind(f);
    char *data = malloc((size_t)size + 1);
    if (!data || fread(data, 1, (size_t)size, f) != (size_t)size) {
        fclose(f);
        free(data);
        return NULL;
    }
    fclose(f);
    data[size] = 0;
    *size_out = size;
    return data;
}

static qboolean valid_map_name(const char *name) {
    size_t n = strlen(name);
    if (n < 1 || n > 63)
        return false;
    for (size_t i = 0; i < n; i++) {
        char c = name[i];
        if (!((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_'))
            return false;
    }
    return true;
}

static int parse_scenario(const char *path) {
    long size;
    char *text = read_file(path, &size);
    if (!text)
        return refuse(path, 0, "cannot read the scenario");
    char *dir = malloc(strlen(path) + 2);
    if (!dir)
        return refuse(path, 0, "out of memory");
    strcpy(dir, path);
    char *slash = strrchr(dir, '/');
    char *backslash = strrchr(dir, '\\');
    if (backslash && (!slash || backslash > slash))
        slash = backslash;
    if (slash)
        *slash = 0;
    else
        strcpy(dir, ".");

    scenario.maxclients = 1;
    scenario.clients = 0;
    scenario.submodels = 0;
    scenario.frames = 100;
    scenario.frametime = 0.1;
    scenario.seed = 1;
    strcpy(scenario.map, "test");
    qs_set_cvar(NULL, NULL);

    const char *once[] = {"progs", "entities", "map", "maxclients", "clients",
                          "submodels", "frames", "frametime", "seed"};
    qboolean seen[9] = {false};
    char **model_names = NULL;
    int num_models = 0;

    int line = 0;
    char *next = text;
    while (next && *next) {
        char *start = next;
        char *end = strchr(start, '\n');
        if (end) {
            *end = 0;
            next = end + 1;
        } else {
            next = NULL;
        }
        line++;
        size_t n = strlen(start);
        if (n > 0 && start[n - 1] == '\r')
            start[n - 1] = 0;
        char *hash = strchr(start, '#');
        if (hash)
            *hash = 0;
        char *words[16];
        int count = 0;
        for (char *w = strtok(start, " \t"); w; w = strtok(NULL, " \t")) {
            if (count == 16)
                return refuse(path, line, "too many arguments");
            words[count++] = w;
        }
        if (count == 0)
            continue;
        const char *key = words[0];
        int args = count - 1;
        long long v;
        for (int i = 0; i < 9; i++) {
            if (strcmp(key, once[i]) == 0) {
                if (seen[i])
                    return refuse(path, line, "a key that may appear once is repeated");
                seen[i] = true;
            }
        }
        if (strcmp(key, "progs") == 0) {
            if (args != 1)
                return refuse(path, line, "progs takes a path");
            scenario.progs = join_path(dir, words[1]);
        } else if (strcmp(key, "entities") == 0) {
            if (args != 1)
                return refuse(path, line, "entities takes a path");
            scenario.entities = join_path(dir, words[1]);
        } else if (strcmp(key, "map") == 0) {
            if (args != 1 || !valid_map_name(words[1]))
                return refuse(path, line, "map takes a name of 1 to 63 bytes from a-z, 0-9, _");
            strcpy(scenario.map, words[1]);
        } else if (strcmp(key, "maxclients") == 0) {
            if (args != 1 || !parse_integer(words[1], 1, QS_MAX_CLIENTS, &v))
                return refuse(path, line, "maxclients takes an integer from 1 to 16");
            scenario.maxclients = (long)v;
        } else if (strcmp(key, "clients") == 0) {
            if (args != 1 || !parse_integer(words[1], 0, QS_MAX_CLIENTS, &v))
                return refuse(path, line, "clients takes an integer from 0 to maxclients");
            scenario.clients = (long)v;
        } else if (strcmp(key, "submodels") == 0) {
            if (args != 1 || !parse_integer(words[1], 0, 254, &v))
                return refuse(path, line, "submodels takes an integer from 0 to 254");
            scenario.submodels = (long)v;
        } else if (strcmp(key, "model") == 0) {
            float b[6];
            if (args != 7)
                return refuse(path, line, "model takes a name and six numbers");
            for (int i = 0; i < 6; i++)
                if (!parse_float(words[2 + i], &b[i]))
                    return refuse(path, line, "model takes a name and six numbers");
            for (int i = 0; i < num_models; i++)
                if (strcmp(model_names[i], words[1]) == 0)
                    return refuse(path, line, "a model name is repeated");
            char **grown = realloc(model_names, (size_t)(num_models + 1) * sizeof(*grown));
            if (!grown)
                return refuse(path, line, "out of memory");
            model_names = grown;
            model_names[num_models++] = words[1];
            qs_add_model_bounds(words[1], b, b + 3);
        } else if (strcmp(key, "cvar") == 0) {
            if (args != 2)
                return refuse(path, line, "cvar takes a name and a text");
            qs_set_cvar(words[1], words[2]);
        } else if (strcmp(key, "frames") == 0) {
            if (args != 1 || !parse_integer(words[1], 0, 1000000, &v))
                return refuse(path, line, "frames takes an integer from 0 to 1000000");
            scenario.frames = (long)v;
        } else if (strcmp(key, "frametime") == 0) {
            if (args != 1 || !decimal_syntax(words[1]))
                return refuse(path, line, "frametime takes a number");
            scenario.frametime = strtod(words[1], NULL);
            if (!(scenario.frametime > 0 && scenario.frametime <= 1))
                return refuse(path, line, "frametime is greater than 0 and at most 1");
        } else if (strcmp(key, "seed") == 0) {
            if (args != 1 || !parse_integer(words[1], 0, 4294967295LL, &v))
                return refuse(path, line, "seed takes an integer from 0 to 4294967295");
            scenario.seed = (unsigned long)v;
        } else if (strcmp(key, "input") == 0) {
            input_t in;
            memset(&in, 0, sizeof(in));
            if (args < 4 || !parse_integer(words[1], 1, 1000000, &v))
                return refuse(path, line, "input takes a frame, an edict, a field and values");
            in.frame = (long)v;
            if (!parse_integer(words[2], 1, QS_MAX_CLIENTS, &v))
                return refuse(path, line, "input takes an edict from 1 to clients");
            in.edict = (long)v;
            const char *field = words[3];
            int values = args - 3;
            if (strcmp(field, "button0") == 0 || strcmp(field, "button2") == 0) {
                if (values != 1 || !parse_integer(words[4], 0, 1, &v))
                    return refuse(path, line, "a button takes 0 or 1");
                in.field = field[6] == '0' ? INPUT_BUTTON0 : INPUT_BUTTON2;
                in.values[0] = (float)v;
            } else if (strcmp(field, "impulse") == 0) {
                if (values != 1 || !parse_integer(words[4], 1, 255, &v))
                    return refuse(path, line, "impulse takes an integer from 1 to 255");
                in.field = INPUT_IMPULSE;
                in.values[0] = (float)v;
            } else if (strcmp(field, "v_angle") == 0) {
                if (values != 3)
                    return refuse(path, line, "v_angle takes three numbers");
                for (int i = 0; i < 3; i++)
                    if (!parse_float(words[4 + i], &in.values[i]))
                        return refuse(path, line, "v_angle takes three numbers");
                in.field = INPUT_V_ANGLE;
            } else {
                return refuse(path, line, "input field is button0, button2, impulse or v_angle");
            }
            input_t *grown =
                realloc(scenario.inputs, (size_t)(scenario.num_inputs + 1) * sizeof(*grown));
            if (!grown)
                return refuse(path, line, "out of memory");
            scenario.inputs = grown;
            scenario.inputs[scenario.num_inputs++] = in;
        } else {
            return refuse(path, line, "unknown key");
        }
    }
    if (!scenario.progs)
        return refuse(path, 0, "no progs");
    if (scenario.clients > scenario.maxclients)
        return refuse(path, 0, "clients is more than maxclients");
    for (int i = 0; i < scenario.num_inputs; i++) {
        if (scenario.inputs[i].frame > scenario.frames)
            return refuse(path, 0, "an input's frame is after the last frame");
        if (scenario.inputs[i].edict > scenario.clients)
            return refuse(path, 0, "an input's edict is not a client that enters");
    }
    free(model_names);
    return QS_EXIT_TRACED;
}

// --- Frames (DIFF-5) ---

static edict_t *edict_num(int n) {
    return (edict_t *)((byte *)sv.edicts + (size_t)n * (size_t)pr_edict_size);
}

// DIFF-5.4: a client's turn without the move.
static void physics_client(edict_t *ent, int num) {
    if (!svs.clients[num - 1].active)
        return;
    pr_global_struct->time = (float)sv.time;
    pr_global_struct->self = (int)EDICT_TO_PROG(ent);
    qs_run(pr_global_struct->PlayerPreThink);
    switch (qs_ftoi(ent->v.movetype)) {
    case MOVETYPE_NONE:
    case MOVETYPE_WALK:
    case MOVETYPE_FLY:
    case MOVETYPE_NOCLIP:
        if (!SV_RunThink(ent))
            return;
        break;
    case MOVETYPE_TOSS:
    case MOVETYPE_BOUNCE:
        SV_RunThink(ent);
        break;
    default:
        Sys_Error("SV_Physics_client: bad movetype %i", qs_ftoi(ent->v.movetype));
    }
    SV_LinkEdict(ent, true);
    pr_global_struct->time = (float)sv.time;
    pr_global_struct->self = (int)EDICT_TO_PROG(ent);
    qs_run(pr_global_struct->PlayerPostThink);
}

// DIFF-5.3: SV_Physics without the movement.
static void physics(double frametime) {
    host_frametime = frametime;
    pr_global_struct->self = (int)EDICT_TO_PROG(sv.edicts);
    pr_global_struct->other = (int)EDICT_TO_PROG(sv.edicts);
    pr_global_struct->time = (float)sv.time;
    qs_run(pr_global_struct->StartFrame);
    for (int i = 0; i < sv.num_edicts; i++) {
        edict_t *ent = edict_num(i);
        if (ent->free)
            continue;
        if (pr_global_struct->force_retouch)
            SV_LinkEdict(ent, true);
        float movetype = ent->v.movetype;
        if (i > 0 && i <= svs.maxclients)
            physics_client(ent, i);
        else if (movetype == MOVETYPE_PUSH)
            SV_Physics_Pusher(ent);
        else if (movetype == MOVETYPE_NONE || movetype == MOVETYPE_NOCLIP ||
                 movetype == MOVETYPE_STEP || movetype == MOVETYPE_TOSS ||
                 movetype == MOVETYPE_BOUNCE || movetype == MOVETYPE_FLY ||
                 movetype == MOVETYPE_FLYMISSILE)
            SV_RunThink(ent);
        else
            Sys_Error("SV_Physics: bad movetype %i", qs_ftoi(movetype));
    }
    if (pr_global_struct->force_retouch)
        pr_global_struct->force_retouch--;
    sv.time += host_frametime;
}

// DIFF-4.3.
static void apply_inputs(long frame) {
    for (int i = 0; i < scenario.num_inputs; i++) {
        input_t *in = &scenario.inputs[i];
        if (in->frame != frame)
            continue;
        edict_t *ent = edict_num((int)in->edict);
        switch (in->field) {
        case INPUT_BUTTON0:
            ent->v.button0 = in->values[0];
            break;
        case INPUT_BUTTON2:
            ent->v.button2 = in->values[0];
            break;
        case INPUT_IMPULSE:
            ent->v.impulse = in->values[0];
            break;
        case INPUT_V_ANGLE:
            VectorCopy(in->values, ent->v.v_angle);
            break;
        }
    }
}

// DIFF-5.1.
static void server_frame(long frame) {
    pr_global_struct->frametime = (float)scenario.frametime;
    SZ_Clear(&sv.datagram);
    apply_inputs(frame);
    physics(scenario.frametime);
    SZ_Clear(&sv.reliable_datagram);
    for (int i = 1; i < sv.num_edicts; i++) {
        edict_t *ent = edict_num(i);
        ent->v.effects = (float)(qs_ftoi(ent->v.effects) & ~EF_MUZZLEFLASH);
    }
}

// DIFF-4.1: SV_ConnectClient, then Host_Spawn_f.
static void client_enters(int slot) {
    client_t *client = &svs.clients[slot];
    edict_t *ent = client->edict;
    qs_run(pr_global_struct->SetNewParms);
    for (int i = 0; i < NUM_SPAWN_PARMS; i++)
        client->spawn_parms[i] = (&pr_global_struct->parm1)[i];
    memset(&ent->v, 0, (size_t)progs->entityfields * 4);
    ent->v.colormap = (float)(slot + 1);
    ent->v.team = 1;
    ent->v.netname = (string_t)(qs_engine_string("player") - pr_strings);
    for (int i = 0; i < NUM_SPAWN_PARMS; i++)
        (&pr_global_struct->parm1)[i] = client->spawn_parms[i];
    pr_global_struct->time = (float)sv.time;
    pr_global_struct->self = (int)EDICT_TO_PROG(ent);
    qs_run(pr_global_struct->ClientConnect);
    qs_run(pr_global_struct->PutClientInServer);
    client->active = true;
    client->spawned = true;
    entered[slot] = true;
}

// --- Setting up the map (DIFF-3) ---

static char *entities_text;

static void start_map(void) {
    if (coop.value)
        Cvar_Set("deathmatch", "0");
    current_skill = (int)(skill.value + 0.5);
    if (current_skill < 0)
        current_skill = 0;
    if (current_skill > 3)
        current_skill = 3;
    char text[16];
    snprintf(text, sizeof(text), "%d", current_skill);
    Cvar_Set("skill", text);

    sv.num_edicts = svs.maxclients + 1;
    for (int i = 0; i < svs.maxclients; i++)
        svs.clients[i].edict = edict_num(i + 1);
    sv.time = 1.0;
    sv.state = ss_loading;
    sv.active = false;
    svs.changelevel_issued = false;
    sv.lastcheck = 0;
    sv.lastchecktime = 0;
    qs_set_random_state((unsigned)scenario.seed);

    // DIFF-3.3.
    static char world_model[80];
    static char submodel_names[255][8];
    snprintf(world_model, sizeof(world_model), "maps/%s.bsp", scenario.map);
    sv.sound_precache[0] = pr_strings;
    sv.model_precache[0] = pr_strings;
    sv.model_precache[1] = world_model;
    sv.models[1] = Mod_ForName(world_model, true);
    for (long i = 1; i <= scenario.submodels; i++) {
        snprintf(submodel_names[i], sizeof(submodel_names[i]), "*%ld", i);
        sv.model_precache[1 + i] = submodel_names[i];
        sv.models[1 + i] = Mod_ForName(submodel_names[i], true);
    }

    // DIFF-3.4, EDICT-4.1, EDICT-11.4.
    edict_t *world = sv.edicts;
    memset(&world->v, 0, (size_t)progs->entityfields * 4);
    world->free = false;
    world->v.model = (string_t)(qs_engine_string(world_model) - pr_strings);
    world->v.modelindex = 1;
    world->v.solid = SOLID_BSP;
    world->v.movetype = MOVETYPE_PUSH;
    if (coop.value)
        pr_global_struct->coop = coop.value;
    else
        pr_global_struct->deathmatch = deathmatch.value;
    pr_global_struct->mapname = (string_t)(qs_engine_string(scenario.map) - pr_strings);
    pr_global_struct->serverflags = (float)svs.serverflags;

    // DIFF-3.5: id's ED_LoadFromFile sets time to the clock and runs each spawn function.
    ED_LoadFromFile(entities_text);

    // DIFF-3.6: the two settle frames.
    sv.active = true;
    sv.state = ss_active;
    physics(0.1);
    physics(0.1);
}

static void run_scenario(void) {
    start_map();
    for (int slot = 0; slot < scenario.clients; slot++)
        client_enters(slot);
    for (long frame = 1; frame <= scenario.frames; frame++)
        server_frame(frame);
}

// DIFF-8.3 to DIFF-8.5: Host_ShutdownServer's ClientDisconnect runs.
static void shut_down(void) {
    jmp_buf catch_point;
    if (setjmp(catch_point))
        return;
    qs_catch = &catch_point;
    for (int slot = 0; slot < svs.maxclients; slot++) {
        if (!entered[slot])
            continue;
        int saved = pr_global_struct->self;
        pr_global_struct->self = (int)EDICT_TO_PROG(svs.clients[slot].edict);
        qs_run(pr_global_struct->ClientDisconnect);
        pr_global_struct->self = saved;
    }
}

int qs_scenario_main(const char *scenario_path, const char *trace_path) {
    int parsed = parse_scenario(scenario_path);
    if (parsed != QS_EXIT_TRACED)
        return parsed;
    if (scenario.entities) {
        long size;
        entities_text = read_file(scenario.entities, &size);
        if (!entities_text)
            return refuse(scenario.entities, 0, "cannot read the entities");
    } else {
        entities_text = "{\n\"classname\" \"worldspawn\"\n}\n";
    }
    qs_progs_file_t file;
    const char *refused = qs_load_progs(scenario.progs, true, &file);
    if (refused)
        return refuse(scenario.progs, 0, refused);

    svs.maxclients = (int)scenario.maxclients;
    svs.clients = clients;
    qs_alloc_edicts(MAX_EDICTS);
    qs_init_buffers();
    pr_builtins = qs_scenario_builtins;
    pr_numbuiltins = QS_NUM_BUILTINS;

    if (!qs_trace_open(trace_path)) {
        fprintf(stderr, "vm-oracle: cannot write %s\n", trace_path);
        return QS_EXIT_IO;
    }
    qs_trace_header(pr_crc, progs->numstrings);

    jmp_buf catch_point;
    int kind = setjmp(catch_point);
    if (kind == 0) {
        qs_catch = &catch_point;
        run_scenario();
    } else if (kind == QS_ERROR_HOST && sv.active) {
        shut_down();
    }
    qs_catch = NULL;
    if (!qs_trace_close()) {
        fprintf(stderr, "vm-oracle: cannot write %s\n", trace_path);
        return QS_EXIT_IO;
    }
    return QS_EXIT_TRACED;
}

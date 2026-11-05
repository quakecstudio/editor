// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors
//
// The engine oracle's harness: id's server (WinQuake's sv_*, world, model, pr_*, common, cvar,
// cmd and zone, compiled whole from the pinned commit) driven headless. It plays the host: the
// memory, the clock, the console, the command line, the network as one local client whose
// messages come from a script, and the client's side of the connection. Everything here is this
// project's code; what id's host.c and view.c do that the server relies on is either copied
// unchanged by cargo xtask oracles (the [[extract]] entries of oracles/sources.toml) or written
// out below with the function it stands in for named.
//
//   engine-oracle [--console] <script> <trace> [<engine word>...]
//
// The script is described in oracles/README.md ("The engine oracle").

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "oracle.h"

// One block holds the hunk, so the progs' strings, the strings ED_NewString makes and id's
// static string buffers are all within 32 bits of pr_strings on a 64-bit host (EDICT-13.1).
#define QS_MEMORY (64 * 1024 * 1024)
static byte qs_memory[QS_MEMORY];

// ---- host.c's globals, as host.c defines them ----

quakeparms_t host_parms;
qboolean host_initialized;
double host_frametime;
double host_time;
double realtime;
double oldrealtime;
int host_framecount;
int host_hunklevel;
int minimum_memory;
client_t *host_client;
jmp_buf host_abortserver;
byte *host_basepal;
byte *host_colormap;

cvar_t host_framerate = {.name = "host_framerate", .string = "0"};
cvar_t host_speeds = {.name = "host_speeds", .string = "0"};
cvar_t sys_ticrate = {.name = "sys_ticrate", .string = "0.05"};
cvar_t serverprofile = {.name = "serverprofile", .string = "0"};
cvar_t fraglimit = {.name = "fraglimit", .string = "0", .archive = false, .server = true};
cvar_t timelimit = {.name = "timelimit", .string = "0", .archive = false, .server = true};
cvar_t teamplay = {.name = "teamplay", .string = "0", .archive = false, .server = true};
cvar_t samelevel = {.name = "samelevel", .string = "0"};
cvar_t noexit = {.name = "noexit", .string = "0", .archive = false, .server = true};
cvar_t developer = {.name = "developer", .string = "0"};
cvar_t skill = {.name = "skill", .string = "1"};
cvar_t deathmatch = {.name = "deathmatch", .string = "0"};
cvar_t coop = {.name = "coop", .string = "0"};
cvar_t pausable = {.name = "pausable", .string = "1"};
cvar_t temp1 = {.name = "temp1", .string = "0"};

// view.c's, which its V_CalcRoll reads (V_Init registers them).
cvar_t cl_rollspeed = {.name = "cl_rollspeed", .string = "200"};
cvar_t cl_rollangle = {.name = "cl_rollangle", .string = "2.0"};

// net_main.c's, which the server reads.
cvar_t hostname = {.name = "hostname", .string = "UNNAMED"};
double net_time;
sizebuf_t net_message;
int net_activeconnections;
qboolean tcpipAvailable;
qboolean ipxAvailable;
char my_tcpip_address[NET_NAMELEN];
char my_ipx_address[NET_NAMELEN];

// The client's, which the server reads: it runs as id's dedicated server, so the client is
// never connected (cls.state == ca_dedicated).
client_static_t cls;
client_state_t cl;
cvar_t cl_name = {.name = "_cl_name", .string = "player", .archive = true};
cvar_t cl_color = {.name = "_cl_color", .string = "0", .archive = true};
keydest_t key_dest = key_game;
float scr_centertime_off;

// The software renderer's, which model.c reads while it loads a brush model's textures.
int r_pixbytes = 1;
unsigned short d_8to16table[256];
texture_t *r_notexture_mip;

static qboolean console_on;
static qboolean trace_open;

// ---- the console and the errors ----

void Con_Printf(char *fmt, ...) {
    if (!console_on)
        return;
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
}

void Con_DPrintf(char *fmt, ...) {
    if (!console_on || !developer.value)
        return;
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
}

void Sys_Printf(char *fmt, ...) {
    if (!console_on)
        return;
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
}

// Sys_Error: id's engine stops. The records so far are kept.
void Sys_Error(char *error, ...) {
    va_list args;
    va_start(args, error);
    fprintf(stderr, "engine-oracle: Sys_Error: ");
    vfprintf(stderr, error, args);
    fprintf(stderr, "\n");
    va_end(args);
    if (trace_open)
        qs_trace_close();
    exit(QS_EXIT_ENGINE);
}

// Host_Error: id's host drops the server and goes on; here the run ends, with an error record
// as the trace's last (TRACE-4.6, TRACE-4.11). Of id's files compiled here, only a QuakeC run
// error says "Program error" (PR_RunError, PF_error, PF_objerror), and only then is the function
// and statement the VM's; the others (PR_ExecuteProgram's NULL function, the entity parser's
// errors) are blamed on no function. One before the trace opens stops the engine (exit 4).
void Host_Error(char *error, ...) {
    char text[1024];
    va_list args;
    va_start(args, error);
    vsnprintf(text, sizeof(text), error, args);
    va_end(args);
    size_t length = strlen(text);
    while (length > 0 && text[length - 1] == '\n')
        text[--length] = 0;
    fprintf(stderr, "engine-oracle: Host_Error: %s\n", text);
    if (trace_open) {
        qboolean in_qc = strcmp(text, "Program error") == 0 && pr_xfunction;
        int function = in_qc ? (int)(pr_xfunction - pr_functions) : -1;
        qs_trace_error(function, in_qc ? pr_xstatement : -1, text);
        trace_open = false;
        exit(qs_trace_close() ? QS_EXIT_TRACED : QS_EXIT_IO);
    }
    exit(QS_EXIT_ENGINE);
}

void Sys_Quit(void) { exit(QS_EXIT_TRACED); }

// rand, as the VM oracle's (DIFF-6.2): a 32-bit state from 1, so a run's random numbers are
// the same on every OS. id's files call it through oracles/engine/rand.h.
static unsigned random_state = 1;

int qs_rand(void) {
    random_state = random_state * 1103515245u + 12345u;
    return (int)((random_state >> 16) & 0x7fff);
}

// The clock: the host's own time, advanced by each frame, never the wall clock.
double Sys_FloatTime(void) { return realtime; }

// ---- files, as sys_null.c keeps them: a small table of handles ----

#define QS_MAX_HANDLES 32
static FILE *handles[QS_MAX_HANDLES];

static int free_handle(void) {
    for (int i = 1; i < QS_MAX_HANDLES; i++)
        if (!handles[i])
            return i;
    Sys_Error("out of handles");
    return -1;
}

int Sys_FileOpenRead(char *path, int *hndl) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        *hndl = -1;
        return -1;
    }
    int i = free_handle();
    handles[i] = f;
    *hndl = i;
    fseek(f, 0, SEEK_END);
    long length = ftell(f);
    fseek(f, 0, SEEK_SET);
    return (int)length;
}

int Sys_FileOpenWrite(char *path) {
    FILE *f = fopen(path, "wb");
    if (!f)
        Sys_Error("Error opening %s", path);
    int i = free_handle();
    handles[i] = f;
    return i;
}

void Sys_FileClose(int handle) {
    if (handle > 0 && handle < QS_MAX_HANDLES && handles[handle]) {
        fclose(handles[handle]);
        handles[handle] = NULL;
    }
}

void Sys_FileSeek(int handle, int position) { fseek(handles[handle], position, SEEK_SET); }

int Sys_FileRead(int handle, void *dest, int count) {
    return (int)fread(dest, 1, (size_t)count, handles[handle]);
}

int Sys_FileWrite(int handle, void *data, int count) {
    return (int)fwrite(data, 1, (size_t)count, handles[handle]);
}

int Sys_FileTime(char *path) {
    FILE *f = fopen(path, "rb");
    if (!f)
        return -1;
    fclose(f);
    return 1;
}

void Sys_mkdir(char *path) { (void)path; }

// ---- the network: one local client, whose messages the script gives ----

static qsocket_t client_socket;
static qboolean client_offered;
// The messages the client sends in the next frame, each one SV_ReadClientMessage reads.
#define QS_MAX_MESSAGES 64
static sizebuf_t outgoing[QS_MAX_MESSAGES];
static byte outgoing_data[QS_MAX_MESSAGES][MAX_MSGLEN];
static int outgoing_count;
static int outgoing_read;

// NET_CheckNewConnections: the client connects once, before the first frame's messages.
qsocket_t *NET_CheckNewConnections(void) {
    if (client_offered)
        return NULL;
    client_offered = true;
    strcpy(client_socket.address, "local");
    return &client_socket;
}

// NET_GetMessage: each queued message once, then "nothing more this frame".
int NET_GetMessage(qsocket_t *sock) {
    (void)sock;
    if (outgoing_read >= outgoing_count)
        return 0;
    sizebuf_t *message = &outgoing[outgoing_read++];
    SZ_Clear(&net_message);
    SZ_Write(&net_message, message->data, message->cursize);
    MSG_BeginReading();
    return 1;
}

// What the server sends the client is not read: the trace is of the server's state.
int NET_SendMessage(qsocket_t *sock, sizebuf_t *data) {
    (void)sock;
    (void)data;
    return 1;
}

int NET_SendUnreliableMessage(qsocket_t *sock, sizebuf_t *data) {
    (void)sock;
    (void)data;
    return 1;
}

int NET_SendToAll(sizebuf_t *data, int blocktime) {
    (void)data;
    (void)blocktime;
    return 0;
}

qboolean NET_CanSendMessage(qsocket_t *sock) {
    (void)sock;
    return true;
}

void NET_Close(qsocket_t *sock) { (void)sock; }

static sizebuf_t *new_message(void) {
    if (outgoing_count >= QS_MAX_MESSAGES) {
        fprintf(stderr, "engine-oracle: more than %d messages in one frame\n", QS_MAX_MESSAGES);
        exit(QS_EXIT_REFUSED);
    }
    sizebuf_t *message = &outgoing[outgoing_count];
    message->data = outgoing_data[outgoing_count];
    message->maxsize = MAX_MSGLEN;
    message->cursize = 0;
    message->allowoverflow = false;
    message->overflowed = false;
    outgoing_count++;
    return message;
}

// A clc_stringcmd, as the client sends a console command to the server (CL_ForwardToServer).
static void send_command(const char *text) {
    sizebuf_t *message = new_message();
    MSG_WriteByte(message, clc_stringcmd);
    MSG_WriteString(message, (char *)text);
}

// The move the client sends each frame once it is in the game, as CL_SendMove writes it: the
// time, the three view angles, the three moves, the buttons and the impulse.
typedef struct {
    float angles[3];
    int forward, side, up;
    int buttons;
    int impulse;
} move_t;

static void send_move(const move_t *move) {
    sizebuf_t *message = new_message();
    MSG_WriteByte(message, clc_move);
    MSG_WriteFloat(message, (float)realtime);
    for (int i = 0; i < 3; i++)
        MSG_WriteAngle(message, move->angles[i]);
    MSG_WriteShort(message, move->forward);
    MSG_WriteShort(message, move->side);
    MSG_WriteShort(message, move->up);
    MSG_WriteByte(message, move->buttons);
    MSG_WriteByte(message, move->impulse);
}

// ---- the client's side, which the server calls but which never runs on a dedicated server ----

void CL_Disconnect(void) {}
void CL_Disconnect_f(void) {}
void CL_EstablishConnection(char *host) { (void)host; }
void CL_NextDemo(void) {}
void CL_StopPlayback(void) {}
void Draw_BeginDisc(void) {}
void Draw_EndDisc(void) {}
void SCR_BeginLoadingPlaque(void) {}
void M_Menu_Quit_f(void) {}
void R_InitSky(texture_t *mt) { (void)mt; }
void D_FlushCaches(void) {}

// ---- engine strings and their records (TRACE-4.10) ----

typedef struct {
    int value;
    char *text;
} known_string_t;

static known_string_t *known;
static size_t known_count;
static size_t known_size;

static void emit_if_changed(int value) {
    if (value >= 0 && value < progs->numstrings)
        return;
    const char *text = pr_strings + value;
    for (size_t i = 0; i < known_count; i++) {
        if (known[i].value == value) {
            if (strcmp(known[i].text, text) == 0)
                return;
            free(known[i].text);
            known[i].text = strdup(text);
            qs_trace_string(value, text);
            return;
        }
    }
    if (known_count == known_size) {
        known_size = known_size ? known_size * 2 : 64;
        known = realloc(known, known_size * sizeof(*known));
        if (!known) {
            fprintf(stderr, "engine-oracle: out of memory\n");
            exit(QS_EXIT_IO);
        }
    }
    known[known_count].value = value;
    known[known_count].text = strdup(text);
    known_count++;
    qs_trace_string(value, text);
}

// Every string global, and every string field of every edict in use, whose value is outside
// the string table: its text gets a record when it is new or changed.
void qs_emit_string_records(void) {
    for (int i = 0; i < progs->numglobaldefs; i++) {
        ddef_t *def = &pr_globaldefs[i];
        if ((def->type & ~DEF_SAVEGLOBAL) == ev_string)
            emit_if_changed(((int *)pr_globals)[def->ofs]);
    }
    for (int n = 0; n < sv.num_edicts; n++) {
        edict_t *ent = EDICT_NUM(n);
        for (int i = 0; i < progs->numfielddefs; i++) {
            ddef_t *def = &pr_fielddefs[i];
            if ((def->type & ~DEF_SAVEGLOBAL) == ev_string)
                emit_if_changed(((int *)&ent->v)[def->ofs]);
        }
    }
}

// ---- the host ----

// Host_InitLocal, without the parts of Host_FindMaxClients for a listen server: one client
// slot, as single player has, on a dedicated server.
static void init_local(void) {
    Host_InitCommands();
    Cvar_RegisterVariable(&host_framerate);
    Cvar_RegisterVariable(&host_speeds);
    Cvar_RegisterVariable(&sys_ticrate);
    Cvar_RegisterVariable(&serverprofile);
    Cvar_RegisterVariable(&fraglimit);
    Cvar_RegisterVariable(&timelimit);
    Cvar_RegisterVariable(&teamplay);
    Cvar_RegisterVariable(&samelevel);
    Cvar_RegisterVariable(&noexit);
    Cvar_RegisterVariable(&skill);
    Cvar_RegisterVariable(&developer);
    Cvar_RegisterVariable(&deathmatch);
    Cvar_RegisterVariable(&coop);
    Cvar_RegisterVariable(&pausable);
    Cvar_RegisterVariable(&temp1);
    Cvar_RegisterVariable(&cl_rollspeed);
    Cvar_RegisterVariable(&cl_rollangle);
    Cvar_RegisterVariable(&hostname);
    Cvar_RegisterVariable(&cl_name);
    Cvar_RegisterVariable(&cl_color);
    cls.state = ca_dedicated;
    svs.maxclients = 1;
    svs.maxclientslimit = 4;
    svs.clients = Hunk_AllocName(svs.maxclientslimit * sizeof(client_t), "clients");
    // R_InitTextures, which a game with a client runs and a dedicated server does not: the
    // texture a texinfo whose texture is missing is given (Mod_LoadTexinfo), a 16 by 16
    // checkerboard and its mips, its name all zeros. Without it, such a map crashes the server
    // in Mod_LoadFaces, which reads the texture's name.
    r_notexture_mip = Hunk_AllocName(sizeof(texture_t) + 16 * 16 + 8 * 8 + 4 * 4 + 2 * 2, "notexture");
    r_notexture_mip->width = r_notexture_mip->height = 16;
    r_notexture_mip->offsets[0] = sizeof(texture_t);
    r_notexture_mip->offsets[1] = r_notexture_mip->offsets[0] + 16 * 16;
    r_notexture_mip->offsets[2] = r_notexture_mip->offsets[1] + 8 * 8;
    r_notexture_mip->offsets[3] = r_notexture_mip->offsets[2] + 4 * 4;
    for (int m = 0; m < 4; m++) {
        byte *dest = (byte *)r_notexture_mip + r_notexture_mip->offsets[m];
        for (int y = 0; y < (16 >> m); y++)
            for (int x = 0; x < (16 >> m); x++)
                *dest++ = ((y < (8 >> m)) ^ (x < (8 >> m))) ? 0 : 0xff;
    }
    Cvar_SetValue("deathmatch", 0.0);
    host_time = 1.0;
}

// CL_SignonReply's answers, each in the frame after the server's signon message that asks for
// it, as a local client sends them: the client connects in frame 1 (SV_CheckForNewClients),
// sends prespawn in frame 2, its name and colour (cl_name and cl_color, "player" and 0 by
// default) and spawn in frame 3, and begin in frame 4. Sent all in one frame, the signon and
// the spawn messages of a large map overflow the client's message, and the server drops it.
static void signon_reply(unsigned frame) {
    switch (frame) {
    case 2:
        send_command("prespawn");
        break;
    case 3:
        send_command("name \"player\"\n");
        send_command("color 0 0\n");
        send_command("spawn ");
        break;
    case 4:
        send_command("begin");
        break;
    default:
        break;
    }
}

// Host_ServerFrame of WinQuake's host.c (the build without FPS_20).
static void server_frame(void) {
    pr_global_struct->frametime = (float)host_frametime;
    SV_ClearDatagram();
    SV_CheckForNewClients();
    SV_RunClients();
    if (!sv.paused && (svs.maxclients > 1 || key_dest == key_game))
        SV_Physics();
    SV_SendClientMessages();
}

// ---- the script ----

#define QS_MAX_LINE 512

static void refuse(int line, const char *why) {
    fprintf(stderr, "engine-oracle: script line %d: %s\n", line, why);
    exit(QS_EXIT_REFUSED);
}

static qboolean read_number(const char *word, double *out) {
    char *end;
    *out = strtod(word, &end);
    return end != word && *end == 0;
}

static qboolean read_integer(const char *word, long low, long high, int *out) {
    char *end;
    long value = strtol(word, &end, 10);
    if (end == word || *end != 0 || value < low || value > high)
        return false;
    *out = (int)value;
    return true;
}

// Starts the engine as Host_Init does for a dedicated server with no video, sound or input,
// with `argv` as its command line, loads `map` (Host_Map_f), and opens the trace.
static void start(int argc, char **argv, const char *map, const char *trace_path, int line) {
    // id's filesystem builds `<basedir>/<gamedir>/<name>` in buffers of MAX_OSPATH bytes, a name
    // up to MAX_QPATH, with no check (COM_AddGameDirectory, COM_FindFile): a longer base
    // directory overflows them, which is undefined, so it is refused here.
    size_t basedir = 1, gamedir = 3; // ".", and "id1"
    for (int i = 1; i + 1 < argc; i++) {
        if (strcmp(argv[i], "-basedir") == 0)
            basedir = strlen(argv[i + 1]);
        else if (strcmp(argv[i], "-game") == 0)
            gamedir = strlen(argv[i + 1]) > gamedir ? strlen(argv[i + 1]) : gamedir;
    }
    // The two separators and the terminating NUL.
    if (basedir + gamedir + MAX_QPATH + 3 > MAX_OSPATH)
        refuse(line, "-basedir and -game are too long for id's MAX_OSPATH buffers");
    host_parms.basedir = ".";
    host_parms.memsize = QS_MEMORY;
    host_parms.membase = qs_memory;
    host_parms.argc = argc;
    host_parms.argv = argv;
    COM_InitArgv(argc, argv);
    Memory_Init(host_parms.membase, host_parms.memsize);
    Cbuf_Init();
    Cmd_Init();
    COM_Init(host_parms.basedir);
    init_local();
    PR_Init();
    Mod_Init();
    SZ_Alloc(&net_message, NET_MAXMESSAGE);
    SV_Init();
    Hunk_AllocName(0, "-HOST_HUNKLEVEL-");
    host_hunklevel = Hunk_LowMark();
    host_initialized = true;

    char command[MAX_QPATH + 8];
    snprintf(command, sizeof(command), "map %s\n", map);
    Cbuf_AddText(command);
    Cbuf_Execute();
    if (!sv.active)
        refuse(line, "the map did not load");
    if (!qs_trace_open(trace_path)) {
        fprintf(stderr, "engine-oracle: %s: cannot write\n", trace_path);
        exit(QS_EXIT_IO);
    }
    trace_open = true;
    qs_trace_header((unsigned)pr_crc, progs->numstrings);
    // TRACE-4.12: frame 0, the state the map loaded to.
    qs_trace_state(0);

}

// `move <pitch> <yaw> <roll> <forward> <side> <up> <buttons> <impulse>`.
static void read_move(char *words, move_t *move, int line) {
    char *word[8];
    int count = 0;
    for (char *w = strtok(words, " "); w; w = strtok(NULL, " ")) {
        if (count == 8)
            refuse(line, "move has eight numbers");
        word[count++] = w;
    }
    if (count != 8)
        refuse(line, "move has eight numbers: pitch yaw roll forward side up buttons impulse");
    for (int i = 0; i < 3; i++) {
        double value;
        if (!read_number(word[i], &value))
            refuse(line, "an angle is a number");
        move->angles[i] = (float)value;
    }
    if (!read_integer(word[3], -32768, 32767, &move->forward) ||
        !read_integer(word[4], -32768, 32767, &move->side) ||
        !read_integer(word[5], -32768, 32767, &move->up) ||
        !read_integer(word[6], 0, 255, &move->buttons) ||
        !read_integer(word[7], 0, 255, &move->impulse))
        refuse(line, "a move is an integer of 16 bits, a button or impulse one of 8");
}

int main(int argc, char **argv) {
    int arg = 1;
    if (arg < argc && strcmp(argv[arg], "--console") == 0) {
        console_on = true;
        arg++;
    }
    if (argc - arg < 2) {
        fprintf(stderr, "usage: engine-oracle [--console] <script> <trace> [<engine word>...]\n");
        return QS_EXIT_REFUSED;
    }
    const char *script_path = argv[arg];
    const char *trace_path = argv[arg + 1];
    FILE *script = fopen(script_path, "rb");
    if (!script) {
        fprintf(stderr, "engine-oracle: %s: cannot open\n", script_path);
        return QS_EXIT_REFUSED;
    }

    char line[QS_MAX_LINE];
    int number = 0;
    // The engine's command line: the words after the trace, then the script's `engine` lines.
    char *engine_argv[64] = {"engine-oracle"};
    int engine_argc = 1;
    for (int i = arg + 2; i < argc; i++) {
        if (engine_argc >= 63) {
            fprintf(stderr, "engine-oracle: too many engine words\n");
            return QS_EXIT_REFUSED;
        }
        engine_argv[engine_argc++] = argv[i];
    }
    char map[MAX_QPATH] = "";
    double frametime = 1.0 / 72.0;
    qboolean started = false;
    move_t move;
    memset(&move, 0, sizeof(move));
    unsigned frame = 0;
    while (fgets(line, sizeof(line), script)) {
        number++;
        if (!strchr(line, '\n') && !feof(script))
            refuse(number, "a line of 512 bytes or more");
        line[strcspn(line, "\r\n")] = 0;
        if (line[0] == '#' || line[0] == 0)
            continue;
        // The engine's command line, the map and the frame time come before the frames.
        if (strncmp(line, "engine ", 7) == 0 || strncmp(line, "map ", 4) == 0 ||
            strncmp(line, "frametime ", 10) == 0) {
            if (started)
                refuse(number, "engine, map and frametime come before the frames");
            if (line[0] == 'e') {
                if (engine_argc >= 63)
                    refuse(number, "too many engine words");
                engine_argv[engine_argc++] = strdup(line + 7);
            } else if (line[0] == 'm') {
                if (strlen(line + 4) >= sizeof(map))
                    refuse(number, "a map name too long");
                strcpy(map, line + 4);
            } else if (!read_number(line + 10, &frametime) || frametime <= 0 || frametime > 1) {
                refuse(number, "frametime is a number of seconds above 0, at most 1");
            }
            continue;
        }
        if (!started) {
            if (!map[0])
                refuse(number, "no map line before the frames");
            start(engine_argc, engine_argv, map, trace_path, number);
            started = true;
        }
        if (strncmp(line, "cmd ", 4) == 0) {
            send_command(line + 4);
        } else if (strncmp(line, "move ", 5) == 0) {
            read_move(line + 5, &move, number);
        } else if (strncmp(line, "frames ", 7) == 0) {
            int count;
            if (!read_integer(line + 7, 1, 1000000, &count))
                refuse(number, "frames is a count from 1");
            // _Host_Frame's steps for a local server with no client of its own to draw: rand,
            // the time (Host_FilterTime), the console, the client's move (CL_SendCmd), the
            // server frame and host_time.
            for (int i = 0; i < count; i++) {
                // id's `rand ();`, which rand.h sends to qs_rand in id's files, not here.
                qs_rand();
                host_frametime = frametime;
                realtime += frametime;
                Cbuf_Execute();
                signon_reply(frame + 1);
                if (svs.clients[0].spawned)
                    send_move(&move);
                outgoing_read = 0;
                server_frame();
                outgoing_count = 0;
                host_time += host_frametime;
                // An impulse goes once, as id's client clears in_impulse after sending it.
                move.impulse = 0;
                qs_trace_frame(++frame);
            }
        } else {
            refuse(number, "unknown line");
        }
    }
    fclose(script);
    if (!started)
        refuse(number, "no frames");
    trace_open = false;
    if (!qs_trace_close()) {
        fprintf(stderr, "engine-oracle: %s: write failed\n", trace_path);
        return QS_EXIT_IO;
    }
    return QS_EXIT_TRACED;
}

# Usability: every control is self-evident

UI-24, L-80. The rules every panel, tab, toolbar and window of the app follows, so that no
control makes the modder stop and think. The model is Steve Krug's *Don't Make Me Think*
(2000; third edition 2014): a screen should be self-evident, the obvious choice should be the
right one, conventions are kept, and needless words go. For editor prior art: TrenchBroom's
smart property editors (flags as checkboxes, colours as a picker, choices as a list), Hammer's
SmartEdit (each key shown by its description and type instead of a raw key and value), and
Roblox Studio's Properties window.

[`docs/audits/usability.md`](../audits/usability.md) checks every panel and window against
these rules. A new control follows them from the start; a review that finds one that does not
fixes it in the same PR.

## The rules

### 1. Pick from a known set; do not make the modder type it

When the values a control can take are known, they are offered, and the modder picks one.
Typing is still allowed where a value outside the set is legitimate, but the set comes first.

- A targeting key (`target`, `killtarget`, and any key the QuakeC compares with `targetname`)
  offers the map's targetnames as it is typed.
- `classname` offers the project's classes.
- A new key offers the keys the entity's class reads and does not set yet. A compiler's key
  such as `_minlight`, which no QuakeC reads, is still typed and added.
- A skill or deathmatch exclusion is a checkbox (Not in easy, Not in normal, Not in hard or
  nightmare, Not in deathmatch), not the number 256, 512, 1024 or 2048 typed into
  `spawnflags`.
- The grid size, the pane layout and the entity filter are pickers.
- A key the class's `/*QUAKED*/` comment numbers (`"sounds"`, then `1) stone`) offers those
  values, each with its name, and shows the name of the one set beside the field.
- A key the code passes to `changelevel`, `precache_model` or `precache_sound` (MAP-8.11)
  offers the project's and the game's levels, models or sounds.
- The console offers the engine's commands and settings, and after `map` or `changelevel` the
  game's levels.
- A watch offers what may be read where the game stopped: the locals, parameters and globals
  visible on that line, and the fields of `self`. Any other expression is still typed.
- While the game is paused, an entity field offers the edicts in use and a function field the
  project's functions.
- The settings file completes its tables, keys and values from the settings' schema, as Zed
  completes `settings.json`.

Where the set is open (a new name, a commit message, a model id for a local Ollama), the
control is a plain text field with a hint saying what to type.

### 2. Show a name with what it names

A bare name makes the modder remember what it refers to. Show the thing beside the name.

- An offered targetname shows the classnames of the entities that have it: `gate   func_door`.
- An offered key shows its type, the class's default and what the class's `/*QUAKED*/`
  comment says of it: `speed   float, 120 by default: how fast it walks`.
- An offered class says whether it is a point or a brush entity.
- An offered console name says whether it is a command or a setting, and a setting its
  default: `sv_gravity   setting, 800 by default`. A watch shows its type, a function its file,
  a file whether it is in the project or the game, and a setting what it does.
- The camera position fields say which axis each is: `X -320`, `Y 0`, `Z 88`.
- A screen reader hears the same thing: an offer is named `gate, func_door`.

### 3. Say what an action will do before it is taken

- Every button drawn as an icon names its action on hover and to a screen reader
  (`Icon::tool`, `Icon::toggle`). An icon that only shows a state, such as the lock on the
  running game's Properties or the mark on a target no entity has, says what it means the same
  way (`Icon::sign`).
- Fire says, on hover, that it changes nothing in the map or a running game.
- A value that will not work is marked before it is used: a target no entity has, a file no
  folder has, a texture no WAD holds, a key the class does not read.

### 4. Label in the modder's words, not the code's

- Labels name what the modder is doing: "Compile with", not the profile type; "events shown",
  not "step"; "Colours", for how an imported picture's colours are matched.
- The engine's own flags are named for what they do ("Not in easy"), not by their constant
  (`SPAWNFLAG_NOT_EASY`).
- Where the only name is the code's (a class's own spawnflag such as `DOOR_START_OPEN`, a key
  such as `wait`), the code's name is kept: it is what the modder reads in the QuakeC and in
  other editors.

### 5. One obvious way to do each thing

A second control for the same job splits attention. Where there are two ways, they are
different modes (typing, picking in the view), not two lists of the same values.

- A new key is typed in one field that offers the class's keys, not chosen from a row of
  buttons above the field as well.
- A new targeting key's value is picked in its own field, not in a separate Targetnames list
  beside it.
- The eyedropper beside a target key stays: it picks an entity in the map view, which the list
  cannot do.

### 6. An empty state says what to do next

A panel with nothing to show says why and what to do: "Select an entity in the map to edit its
keys.", "Entities appear here while the game runs.", "Select an entity and choose Fire to see
what its targets do". The welcome page numbers the three steps to a running mod.

### 7. Nothing only the manual explains

Every panel, tab row and window has a help button (P7-11) that explains it in place and links to
the guide, but no control depends on it: a control whose meaning needs the guide is relabelled
or redrawn until it does not. A button looks like a button (framed, or an icon with a hover
name); text that can be clicked is never drawn as a plain label.

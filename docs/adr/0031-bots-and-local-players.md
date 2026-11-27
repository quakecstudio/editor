# ADR-0031: Play coop and deathmatch with local players and bots the preview host drives

- **Status:** Accepted
- **Date:** 2026-11-27
- **Decided by:** Justin Plock
- **Issue:** [#737](https://github.com/quakecstudio/editor/issues/737) (L-15); the work is L-84 to L-88
- **Requirements:** ENG-18, MAP-39, ENG-15

## Question

How should a modder play a coop or deathmatch map on one machine, and where should a bot's
thinking live?

## Context

Justin asked on 2026-11-27 to "build deathmatch and coop maps and play against local bot
clients". He chose several local players and bots first, and left real network play for later.

The preview host's server, `qs-sim`, takes one client. `MAXCLIENTS` is 1
(`crates/qs-sim/src/world.rs`), `ServerMessages` holds one `ClientSlot`
(`crates/qs-sim/src/messages.rs`), and NET-1.3 says the preview host keeps its one client.
Some parts already handle coop and deathmatch:

- The map editor's entity filter and `map.balance` spawn a map as coop or deathmatch
  (`crates/qs-workbench/src/map_filter.rs`).
- `qs-client` has id's scoreboards and the deathmatch overlay (`crates/qs-client/src/hud.rs`).
- `qc-builtins` has `clienttype` (BUILTIN-18.20).
- `qs-map-gen`'s playability check models how a player gets around a compiled map
  (`crates/qs-map-gen/src/playable.rs`).

id1 and LibreQuake have no bot code. The `spawnclient` extension only makes an empty client
slot, which a mod's own QuakeC has to drive.

Prior art:

- Quake III Arena's bot library thinks in the engine: navigation areas, goals weighted by
  item, and aim and combat kept apart from moving.
- Quake's own bots, the Reaper Bot and Frogbot, are written in QuakeC inside a mod.
- Roblox Studio's local test starts a server with a number of players and a window for each.

## Options

| Option | Consequences |
| --- | --- |
| A. Engine bots plus `spawnclient` | A bot is a client with no connection whose moves come from Rust code in the preview host, played by the mod's own player QuakeC, so bots work with any mod, id1 and LibreQuake included. `spawnclient` also lets a mod's own QuakeC bots run. Needs a new crate, `qs-bots` (L4), and a path query on `qs-map-gen`'s movement model. |
| B. Engine bots only | As A, but a mod's own QuakeC bots do not run. |
| C. `spawnclient` only | Small, but a bot plays only when the mod has its own bot code; id1 and LibreQuake get no bots. |

Not chosen: B, because adding `spawnclient` to the slot model costs little and a mod's own
bots are what that mod's players will see. C, because it gives no bots for the mods most
modders start from.

## Decision

Option A, decided by Justin on 2026-11-27.

- `qs-sim` runs several clients (L-84). Each slot is either a local player, joined through an
  in-process loopback connection as id's `net_loop.c` joins its local client, or a bot with
  no connection.
- Local players after the first each play in a docked window of their own (L-86).
- Bots are Rust code that sends each frame's `UserCmd` from what a client in its place could
  see (L-87).
- `spawnclient` gives a bot slot that the mod's QuakeC drives (L-84).
- The server stays id-exact, because a bot is a client that sends moves, and the
  several-client server is proved against id's server frame by frame (L-85).
- Network play is not part of this decision. The loopback connection is where it would join.

## Consequences

- `plan/`: ENG-18 and MAP-39 added, carried by L-84 to L-88. L-15 (#737) loses its
  several-client deliverable to L-84 and depends on it.
- `qs-sim`'s public API names a client in `connect`, `set_command`, `take_messages` and
  `player_view`. NET-1.3's one-client clause is withdrawn and replaced (L-84).
- A new crate, `qs-bots`, in L4 (L-87). Its behaviour is this project's own design, specified
  in `docs/spec/bots.md`, not a port.
- The engine oracle drives several clients (L-85).
- The map editor checks deathmatch and coop starts and plays a map with bots (L-88).

## Links

- [#737](https://github.com/quakecstudio/editor/issues/737) (L-15), and the plan's L-84 to
  L-88 in `plan/backlog/p5-p7.yaml`.
- Related: [ADR-0004](0004-preview-host-base.md) (the preview host),
  [ADR-0018](0018-quakespasm-lineage-porting-upstreams.md) (vkQuake as a porting upstream for
  `spawnclient`), [ADR-0028](0028-embedded-game-view.md) (the game in the IDE's window).
- Sources: `crates/qs-sim/src/world.rs`, `crates/qs-sim/src/messages.rs`,
  `crates/qs-map-gen/src/playable.rs`, `oracles/README.md`, `docs/spec/engine/net.md` (NET-1.3).

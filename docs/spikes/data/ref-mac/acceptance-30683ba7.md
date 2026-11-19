# MVP acceptance on macOS, 2026-11-18

The checks of P4-24 ([#135](https://github.com/quakecstudio/editor/issues/135)) run on the
reference Mac from release builds of `main` at `30683ba7` (`cargo build --release` of
`quakec-studio`, `quakec-ide`, `quakec-engine` and `quakec-lsp`), with GOG's Quake Enhanced
extracted by innoextract at `~/Games/quake-enhanced-gog`. The benchmark rows are in
[`bench-8ca2572e.md`](bench-8ca2572e.md); the original game through the run roots was recorded
on the issue on 2026-11-17.

| Check | Result |
| --- | --- |
| The rerelease through the run roots | Passed. `quakec-ide new rerelease remod`, then `preview.play` of `e1m1` through `quakec-ide mcp --game ~/Games/quake-enhanced-gog`: the run root is `build/run/rerelease`, holding links `id1` and `QuakeEX.kpf` to the install's and `remod` to the project's build; the console's `status` answers `map: e1m1` and the frame shows the level. |
| The install unchanged | Passed. `find ~/Games -newer <marker>` printed nothing after every run below. |
| An agent adds a weapon, its diff is reviewed, it is played and fired | Passed, with the agent an MCP client (Claude Code through `quakec-ide mcp --connect`, in the running app's session), not the built-in agent panel with a model provider. `file.read` of `weapons.qc`; `edit.apply` staged four hunks (a new `W_FireSpreader`, its branch in `W_SetCurrentAmmo` and in `W_Attack`, and impulse 20 in `ImpulseCommands`); `edit.staged` listed them, `edit.accept` accepted them, `file.save` wrote the file, `compile` was clean with no errors. `preview.play` of `e1m1`; `impulse 20` in the game's console set the player's `weapon` to 128 with `weaponmodel` `progs/v_nail2.mdl` and 50 nails; `+attack` put spikes in flight and left 30 nails. |
| The first-run guide's folder | Passed at the command the guide runs. With no game at the folder the app was started on, `preview.play` answered "no Quake game here" and `preview.game` `state: missing` with `id1/pak0.pak` and `id1/pak1.pak`; `preview.choose_game` of the innoextract folder answered `state: found`, and the next `preview.play` played, with the app not restarted. The guide's window was not looked at. |
| The game docked beside the editor | Passed. The game's window sat 8 points right of the IDE's, their tops level; after the IDE was moved to (120, 80) it was at (1568, 80), and after the IDE was resized to 1200 by 800 at (1328, 80). With the IDE focused the game's window was in front of it, at window layer 3 over the IDE's 0. |
| A click on an entity in the paused game | Not shown. `preview.pick`, which the click runs, answered no entity at any point of a grid over the game's window, from the start of `e1m1`, after walking forward, and aimed at each of the eighteen nearest monsters in turn; none was shown to be in sight. It needs a view with a solid entity in it. |

Every call and its answer is in the session's log; the frames are the engine's own
`engine.screenshot`.

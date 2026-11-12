// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent's engine tools (P4-03, AGT-2, PRIN-2): the running game read and driven through
//! the bridge, each answering what the agent can reason over. `engine.inspect` reads edicts,
//! globals or the call stack (BRIDGE-4.9); `engine.console` runs a console command and answers
//! the lines it printed; `engine.screenshot` writes the frame the game draws as a PNG. Each is
//! a function over the bridge's [`Client`], so `cargo xtask bridge-client` runs the same code
//! against the real preview host headless, and a command over the [`Preview`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use qs_commands::{Command, Outcome, RegisterError, Registry};
use qs_engine::{BridgeEvent, Client, ClientError, InspectWhat, base64_decode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{HasPreview, Preview, PreviewError};
use crate::ids::Edict;

/// The id of the command that reads the running game's edicts, globals or call stack.
pub const ENGINE_INSPECT: &str = "engine.inspect";

/// The id of the command that runs a console command and answers what it printed.
pub const ENGINE_CONSOLE: &str = "engine.console";

/// The id of the command that writes the running game's frame as a PNG.
pub const ENGINE_SCREENSHOT: &str = "engine.screenshot";

/// How long `engine.console` waits for the first line a command prints.
pub const CONSOLE_WAIT: Duration = Duration::from_secs(2);

/// How long the console stays quiet before `engine.console` takes a command's output as ended.
pub const CONSOLE_QUIET: Duration = Duration::from_millis(250);

/// What `engine.inspect` reads (BRIDGE-4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Inspected {
    /// The edicts in use, each with its number and classname.
    Entities,
    /// One edict's classname and fields; `edict` names it.
    Edict,
    /// Every named global.
    Globals,
    /// The call stack, while the game is stopped in a frame.
    Stack,
}

/// The parameters of `engine.inspect`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineInspectParams {
    /// What to read.
    pub what: Inspected,
    /// The edict, for `edict`.
    #[serde(default)]
    pub edict: Option<Edict>,
}

/// The parameters of `engine.console`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineConsoleParams {
    /// The console command, such as `give all`.
    pub command: String,
}

/// A size to draw a screenshot at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShotSize {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
}

/// The parameters of `engine.screenshot`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineScreenshotParams {
    /// Where to write the PNG; `build/screenshots/screenshot-<n>.png` in the project, the
    /// first `n` from 1 not taken, when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The size to draw it at; the game's own when absent.
    #[serde(default)]
    pub size: Option<ShotSize>,
}

/// A screenshot written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Screenshot {
    /// The PNG.
    pub path: PathBuf,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
}

/// What a console command printed, and every event that came with it.
#[derive(Debug)]
pub struct ConsoleOutput {
    /// The text of each `console/line` after the command, in order.
    pub lines: Vec<String>,
    /// Every event received, those from before the command first, for the preview to take.
    pub events: Vec<BridgeEvent>,
}

/// Reads what `params` asks of the game behind `client` (BRIDGE-4.9), answered as the host
/// answers it.
///
/// # Errors
///
/// [`PreviewError::BadValue`] for `edict` with no edict, or the bridge's error.
pub fn inspect(client: &mut Client, params: &EngineInspectParams) -> Result<Value, PreviewError> {
    let what = match (params.what, params.edict) {
        (Inspected::Entities, _) => InspectWhat::Entities,
        (Inspected::Globals, _) => InspectWhat::Globals,
        (Inspected::Stack, _) => InspectWhat::Stack,
        (Inspected::Edict, Some(edict)) => InspectWhat::Edict(edict.get()),
        (Inspected::Edict, None) => {
            return Err(PreviewError::BadValue {
                field: "edict".to_owned(),
                kind: "edict number".to_owned(),
            });
        }
    };
    Ok(client.inspect(what)?)
}

/// Runs `command` in the console of the game behind `client` and gathers the lines it printed:
/// the first `console/line` that comes within `within`, then each that follows until none has
/// come for `quiet` (BRIDGE-4.9). A command that prints nothing in time answers no lines.
///
/// # Errors
///
/// The bridge's error, or [`ClientError::Closed`] when the host went away.
pub fn console_output(
    client: &mut Client,
    command: &str,
    within: Duration,
    quiet: Duration,
) -> Result<ConsoleOutput, ClientError> {
    let mut events = client.take_events();
    let before = events.len();
    client.console(command)?;
    let mut lines_seen = Vec::new();
    let mut wait = within;
    loop {
        match client.wait_event("console/line", wait) {
            Ok(line) => lines_seen.push(line),
            Err(ClientError::Timeout) => break,
            Err(error) => return Err(error),
        }
        wait = quiet;
    }
    // The other events that came meanwhile, then the lines, which the console panel shows in
    // the order they came after them.
    events.extend(client.take_events());
    events.extend(lines_seen);
    let lines = events
        .iter()
        .skip(before)
        .filter(|event| event.method == "console/line")
        .map(|event| {
            event
                .params
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    Ok(ConsoleOutput { lines, events })
}

/// Writes the frame the game behind `client` draws, at `size` or its own, to `path` as a PNG
/// (BRIDGE-4.9), making its folder.
///
/// # Errors
///
/// The bridge's error, [`ClientError::Unsupported`] when the answer holds no PNG, or
/// [`PreviewError::Write`] when the file cannot be written.
pub fn screenshot_to(
    client: &mut Client,
    path: &Path,
    size: Option<ShotSize>,
) -> Result<Screenshot, PreviewError> {
    let answer = client.screenshot(size.map(|s| (s.width, s.height)))?;
    let png = answer
        .get("png")
        .and_then(Value::as_str)
        .and_then(base64_decode)
        .ok_or_else(|| ClientError::Unsupported("screenshot".to_owned()))?;
    let dimension = |key: &str| {
        answer
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or_default()
    };
    let write = |error| PreviewError::Write(path.to_path_buf(), error);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(write)?;
    }
    std::fs::write(path, png).map_err(write)?;
    Ok(Screenshot {
        path: path.to_path_buf(),
        width: dimension("width"),
        height: dimension("height"),
    })
}

/// The first `build/screenshots/screenshot-<n>.png` under `project`, from 1, that is not there.
#[must_use]
pub fn next_screenshot_path(project: &Path) -> PathBuf {
    let folder = project.join("build").join("screenshots");
    (1_u32..)
        .map(|n| folder.join(format!("screenshot-{n}.png")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| folder.join("screenshot.png"))
}

impl Preview {
    /// Runs `command` in the game's console and answers the lines it printed, the console
    /// panel taking them too.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn console_lines(&mut self, command: &str) -> Result<Vec<String>, PreviewError> {
        self.push_line(&format!("] {command}"), "print");
        let output = console_output(self.live_client()?, command, CONSOLE_WAIT, CONSOLE_QUIET)?;
        for event in &output.events {
            self.take_event(&event.method, &event.params);
        }
        Ok(output.lines)
    }
}

/// `engine.inspect`: reads the running game's edicts, globals or call stack.
struct EngineInspect;

impl<C: HasPreview + 'static> Command<C> for EngineInspect {
    const ID: &'static str = ENGINE_INSPECT;
    const SUMMARY: &'static str = "Reads the running game: the edicts in use, one edict's \
        fields, every named global, or the call stack while stopped in a frame.";
    type Params = EngineInspectParams;
    type Output = Value;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: EngineInspectParams,
    ) -> Result<Outcome<Value, C>, PreviewError> {
        let (_, preview) = context.workbench_and_preview();
        let answer = inspect(preview.live_client()?, &params)?;
        Ok(Outcome::outside_history(answer))
    }
}

/// `engine.console`: runs a console command and answers what it printed.
struct EngineConsole;

impl<C: HasPreview + 'static> Command<C> for EngineConsole {
    const ID: &'static str = ENGINE_CONSOLE;
    const SUMMARY: &'static str = "Runs a command in the running game's console and answers \
        the lines it printed.";
    type Params = EngineConsoleParams;
    type Output = Vec<String>;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: EngineConsoleParams,
    ) -> Result<Outcome<Vec<String>, C>, PreviewError> {
        let (_, preview) = context.workbench_and_preview();
        let lines = preview.console_lines(&params.command)?;
        Ok(Outcome::outside_history(lines))
    }
}

/// `engine.screenshot`: writes the running game's frame as a PNG.
struct EngineScreenshot;

impl<C: HasPreview + 'static> Command<C> for EngineScreenshot {
    const ID: &'static str = ENGINE_SCREENSHOT;
    const SUMMARY: &'static str = "Writes the frame the running game draws as a PNG, in the \
        project's build/screenshots unless a path is given, and answers where and its size.";
    type Params = EngineScreenshotParams;
    type Output = Screenshot;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: EngineScreenshotParams,
    ) -> Result<Outcome<Screenshot, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        let path = match params.path {
            Some(path) => path,
            None => next_screenshot_path(
                workbench
                    .root()
                    .ok_or(PreviewError::Build(crate::WorkbenchError::NoProject))?,
            ),
        };
        let shot = screenshot_to(preview.live_client()?, &path, params.size)?;
        Ok(Outcome::outside_history(shot))
    }
}

/// Registers `engine.inspect`, `engine.console` and `engine.screenshot`.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register_engine<C: HasPreview + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(EngineInspect)?;
    registry.register(EngineConsole)?;
    registry.register(EngineScreenshot)
}

#[cfg(test)]
mod tests;

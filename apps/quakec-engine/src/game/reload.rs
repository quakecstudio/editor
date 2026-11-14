// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge's `reload` (BRIDGE-4.9, P3-03): the progs read again, or another one, and loaded
//! over the running map by [`World::reload`], which migrates its globals and fields (WORLD-10).

use qs_sim::debug::BreakpointEdit;
use std::path::Path;

use qc_lno::LnoFile;
use qc_progs::Progs;
use qs_bridge::{ErrorCode, RpcError};
use qs_sim::debug::resolve;
use qs_sim::rerelease::{BuiltinTable, patch_builtins};
use qs_sim::{Change, Migration};
use serde_json::{Value, json};

use super::{Game, Reason, State};

impl Game {
    /// `reload`: the progs `params` names, a host path or a name on the game's search path,
    /// else the one running, loaded over the map; its `.lno` read beside it, and each
    /// breakpoint resolved again at its file and line, one that no longer resolves cleared.
    /// It sends `progs/loaded`, and while paused `state/changed` with reason `reload`.
    pub(super) fn reload(&mut self, params: &Value) -> Result<Value, RpcError> {
        self.require_between_frames()?;
        let path = params
            .get("progs")
            .and_then(Value::as_str)
            .unwrap_or(&self.progs_path)
            .to_owned();
        let mut bytes = self.read_file(&path)?;
        // RRE-4.1: the rerelease's named builtins, as the progs was patched when loaded.
        let table = patch_builtins(&mut bytes);
        let new = Progs::read(&bytes)
            .map_err(|e| RpcError::new(ErrorCode::InvalidParams, format!("{path}: {e}")))?;
        // BRIDGE-4.11: a dry run migrates a copy of the world and changes nothing.
        if params.get("dry_run").and_then(Value::as_bool) == Some(true) {
            return self.predict(&path, &new, table);
        }
        let old_bytes = core::mem::take(&mut self.progs);
        let migration = match Progs::read(&old_bytes) {
            Ok(old) => {
                // The changes the old progs made are reported before its values move.
                self.report_changes(&old);
                self.world.reload(&old, &new, table)
            }
            Err(error) => Err(qs_sim::WorldError::Progs(error.to_string())),
        };
        let migration = match migration {
            Ok(migration) => migration,
            Err(error) => {
                self.progs = old_bytes;
                return Err(RpcError::new(ErrorCode::InternalError, error.to_string()));
            }
        };
        let lno = lno_path(&path);
        let lines = self
            .read_file(&lno)
            .ok()
            .and_then(|lno| LnoFile::read(&lno).ok())
            .filter(|lno| lno.matches_header(new.header()).is_ok())
            .map(|lno| lno.lines)
            .unwrap_or_default();
        self.lines = lines;
        let cleared = self.rebind_breakpoints(&new);
        self.snapshot = None;
        self.progs = bytes;
        self.progs_path = path.clone();
        self.progs_loaded(&path);
        if self.state == State::Paused {
            self.state_changed(Some(Reason::Reload), None);
        }
        Ok(result_json(&migration, &cleared, self.world.time()))
    }

    /// `reload` with `dry_run` (BRIDGE-4.11): the report a reload of `new` would give, from
    /// the same migration run over a copy of the world, and the breakpoints it would clear;
    /// the running world, its progs, breakpoints and events are left as they were.
    fn predict(&self, path: &str, new: &Progs<'_>, table: BuiltinTable) -> Result<Value, RpcError> {
        let old = Progs::read(&self.progs)
            .map_err(|e| RpcError::new(ErrorCode::InternalError, e.to_string()))?;
        let mut copy = self.world.clone();
        let migration = copy
            .reload(&old, new, table)
            .map_err(|e| RpcError::new(ErrorCode::InternalError, e.to_string()))?;
        let lines = self
            .read_file(&lno_path(path))
            .ok()
            .and_then(|lno| LnoFile::read(&lno).ok())
            .filter(|lno| lno.matches_header(new.header()).is_ok())
            .map(|lno| lno.lines)
            .unwrap_or_default();
        let cleared: Vec<u64> = self
            .breakpoints
            .iter()
            .filter(|b| resolve(new, &lines, &b.file, b.line).is_none())
            .map(|b| b.id)
            .collect();
        Ok(result_json(&migration, &cleared, self.world.time()))
    }

    /// The bytes of `path`: a host path when it is absolute, else a name on the game's search
    /// path.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, RpcError> {
        let missing = || RpcError::new(ErrorCode::InvalidParams, format!("{path} is not found"));
        if Path::new(path).is_absolute() {
            return std::fs::read(path).map_err(|_| missing());
        }
        self.files
            .as_ref()
            .and_then(|files| files(path))
            .ok_or_else(missing)
    }

    /// Each breakpoint resolved again at its file and line in `progs`; one that no longer
    /// resolves is cleared, and its id returned. The world's new debugger, which
    /// `World::reload` dropped, holds those kept.
    fn rebind_breakpoints(&mut self, progs: &Progs<'_>) -> Vec<u64> {
        let lines = self.lines.clone();
        let mut kept = Vec::new();
        let mut cleared = Vec::new();
        for mut breakpoint in core::mem::take(&mut self.breakpoints) {
            match resolve(progs, &lines, &breakpoint.file, breakpoint.line) {
                Some(statement) => {
                    breakpoint.statement = statement;
                    kept.push(breakpoint);
                }
                None => cleared.push(breakpoint.id),
            }
        }
        if !kept.is_empty() {
            let debugger = self.world.debugger_or_new(&lines);
            for breakpoint in &kept {
                debugger.set_breakpoint(breakpoint.statement, BreakpointEdit::Set);
            }
        }
        self.breakpoints = kept;
        cleared
    }

    /// `progs/loaded` for the progs now running (BRIDGE-3.1).
    pub(super) fn progs_loaded(&mut self, path: &str) {
        let time = self.world.time();
        let Ok(header) = Progs::read(&self.progs).map(|progs| *progs.header()) else {
            return;
        };
        let progs_loaded = json!({
            "path": path,
            "crc": header.crc,
            "version": header.version,
            "functions": header.numfunctions,
            "globals": header.numglobals,
            "fields": header.numfielddefs,
            "lno": !self.lines.is_empty(),
            "time": time,
        });
        self.events.push(("progs/loaded", progs_loaded));
    }
}

/// The `.lno` beside the progs at `path`: its name with the extension `.lno`.
fn lno_path(path: &str) -> String {
    Path::new(path)
        .with_extension("lno")
        .to_string_lossy()
        .into_owned()
}

/// `reload`'s result (BRIDGE-4.8): how many values migrated, what was lost and why, what only
/// the new progs has, and the breakpoints `cleared`.
fn result_json(migration: &Migration, cleared: &[u64], time: f64) -> Value {
    let list = |changes: &[Change]| -> Vec<Value> {
        changes
            .iter()
            .map(|c| json!({ "name": c.name, "kind": c.kind.name(), "reason": c.reason.to_string() }))
            .collect()
    };
    json!({
        "migrated": migration.migrated,
        "lost": list(&migration.lost),
        "added": list(&migration.added),
        "breakpoints_cleared": cleared,
        "time": time,
    })
}

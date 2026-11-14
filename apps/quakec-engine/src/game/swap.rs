// SPDX-License-Identifier: GPL-3.0-or-later

//! `map/swap` (BRIDGE-4.14 to BRIDGE-4.17): a new build of the running map's BSP put in place of
//! the old, every edict and global kept, each of the map's brush entities given its new model.

use qc_progs::Progs;
use qs_bridge::{ErrorCode, RpcError};
use qs_bsp::{Bsp, BspLoader};
use qs_sim::{BrushModel, MapPlace, SwapError};
use serde_json::{Value, json};

use super::Game;

impl Game {
    /// `map/swap`: reads `bsp` as `reload` reads its progs (BRIDGE-4.14), swaps it in as
    /// [`qs_sim::World::swap_bsp`] does (BRIDGE-4.15, BRIDGE-4.16), and keeps it for the window
    /// to draw (BRIDGE-4.17); a refused swap changes nothing.
    pub(super) fn map_swap(
        &mut self,
        progs: &Progs<'_>,
        params: &Value,
    ) -> Result<Value, RpcError> {
        self.require_between_frames()?;
        let invalid = |why: String| RpcError::new(ErrorCode::InvalidParams, why);
        let path = params
            .get("bsp")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("map/swap takes bsp".to_owned()))?;
        let models = models_param(params)?;
        let bytes = self.read_file(path)?;
        let bsp = Bsp::read(&bytes, BspLoader::default()).map_err(|e| invalid(e.message(path)))?;
        let swapped = self
            .world
            .swap_bsp(progs, &bsp, &models)
            .map_err(|error| match error {
                SwapError::NoSuchModel(_) | SwapError::NotPrecached(_) => {
                    invalid(error.to_string())
                }
                SwapError::Link(_) => RpcError::new(ErrorCode::InternalError, error.to_string()),
            })?;
        self.report_changes(progs);
        self.swapped = Some(bytes);
        Ok(json!({ "swapped": swapped }))
    }

    /// The BSP the last `map/swap` put in place, once, for the window to draw (BRIDGE-4.17).
    pub(crate) fn take_swapped(&mut self) -> Option<Vec<u8>> {
        self.swapped.take()
    }
}

/// `models`: each map entity and its brush model in the new BSP, which the schema has checked
/// are a place and a `*n` (BRIDGE-4.2).
fn models_param(params: &Value) -> Result<Vec<(MapPlace, BrushModel)>, RpcError> {
    let invalid = || {
        RpcError::new(
            ErrorCode::InvalidParams,
            "models lists map_entity and model",
        )
    };
    let list = params
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    list.iter()
        .map(|item| {
            let place = item
                .get("map_entity")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let model = item
                .get("model")
                .and_then(Value::as_str)
                .and_then(BrushModel::parse)
                .ok_or_else(invalid)?;
            Ok((MapPlace::new(place), model))
        })
        .collect()
}

// SPDX-License-Identifier: GPL-3.0-or-later

//! `map/swap` (BRIDGE-4.14 to BRIDGE-4.17): a new build of the running map's BSP put in place of
//! the old, every edict and global kept, each of the map's brush entities given its new model.
//! A host that draws answers once it has drawn the new build, so a swap it cannot draw changes
//! nothing; one that draws nothing swaps at once.

use qc_progs::Progs;
use qs_bridge::{Answer, ErrorCode, Id, RpcError};
use qs_bsp::Submodel;
use qs_sim::{MapPlace, SwapError};
use serde_json::{Value, json};

use super::Game;
use crate::map_bsp::MapBsp;

/// A `map/swap` checked and waiting for the host to draw its BSP (BRIDGE-4.17).
pub(crate) struct PendingSwap {
    /// The request, answered when the swap is done or refused.
    id: Id,
    /// The new build.
    bsp: MapBsp,
    /// Each map entity and its brush model in it.
    models: Vec<(MapPlace, Submodel)>,
}

impl PendingSwap {
    /// The new build, for the host to draw.
    pub(crate) const fn bsp(&self) -> &MapBsp {
        &self.bsp
    }
}

impl Game {
    /// `map/swap` on a host that draws: checked now, answered once the host has drawn the new
    /// build ([`Game::finish_swap`]); a refusal is answered at once and changes nothing.
    pub(super) fn ask_swap(&mut self, id: &Id, params: &Value) -> Answer {
        match self.check_swap(params) {
            Ok((bsp, models)) => {
                self.swap = Some(PendingSwap {
                    id: id.clone(),
                    bsp,
                    models,
                });
                Answer::Later
            }
            Err(error) => Answer::Now(Err(error)),
        }
    }

    /// `map/swap` on a host that draws nothing: checked and swapped at once.
    pub(super) fn map_swap(
        &mut self,
        progs: &Progs<'_>,
        params: &Value,
    ) -> Result<Value, RpcError> {
        let (bsp, models) = self.check_swap(params)?;
        self.apply_swap(progs, &bsp, &models)
    }

    /// The swap waiting to be drawn, once.
    pub(crate) const fn take_swap(&mut self) -> Option<PendingSwap> {
        self.swap.take()
    }

    /// Puts `swap` in place once the host has drawn its BSP, or, with the reason, refuses it
    /// for a BSP the host could not draw, changing nothing; answers its request. The new build
    /// when it is in place, for the host to keep.
    pub(crate) fn finish_swap(
        &mut self,
        swap: PendingSwap,
        drawn: Result<(), String>,
    ) -> Option<MapBsp> {
        let PendingSwap { id, bsp, models } = swap;
        let outcome = drawn
            .map_err(|why| RpcError::new(ErrorCode::InternalError, why))
            .and_then(|()| {
                self.with_progs(|game, progs| game.apply_swap(progs, &bsp, &models))
                    .and_then(|swapped| swapped)
            });
        let done = outcome.is_ok();
        self.responses.push((id, outcome));
        done.then_some(bsp)
    }

    /// The new build `params` names and its models, read as `reload` reads its progs and
    /// checked as the swap checks them (BRIDGE-4.14).
    fn check_swap(&self, params: &Value) -> Result<(MapBsp, Vec<(MapPlace, Submodel)>), RpcError> {
        self.require_between_frames()?;
        let invalid = |why: String| RpcError::new(ErrorCode::InvalidParams, why);
        let path = params
            .get("bsp")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("map/swap takes bsp".to_owned()))?;
        let models = models_param(params)?;
        let bsp = MapBsp::read(self.read_file(path)?).map_err(|e| invalid(e.message(path)))?;
        self.world.check_swap(bsp.bsp(), &models).map_err(refusal)?;
        Ok((bsp, models))
    }

    /// The checked swap put in place (BRIDGE-4.15, BRIDGE-4.18) and the changes reported.
    fn apply_swap(
        &mut self,
        progs: &Progs<'_>,
        bsp: &MapBsp,
        models: &[(MapPlace, Submodel)],
    ) -> Result<Value, RpcError> {
        let swapped = self
            .world
            .swap_bsp(progs, bsp.bsp(), models)
            .map_err(refusal)?;
        self.report_changes(progs);
        Ok(json!({ "swapped": swapped }))
    }
}

/// The answer to a swap the world refused: its params for a model or an edict it cannot place
/// (BRIDGE-4.14), the host's for a link that failed.
fn refusal(error: SwapError) -> RpcError {
    let code = match error {
        SwapError::NoSuchModel(_) | SwapError::NotPrecached(_) | SwapError::Unplaced { .. } => {
            ErrorCode::InvalidParams
        }
        SwapError::Link(_) => ErrorCode::InternalError,
    };
    RpcError::new(code, error.to_string())
}

/// `models`: each map entity and its brush model in the new BSP, which the schema has checked
/// are a place and a `*n` (BRIDGE-4.2).
fn models_param(params: &Value) -> Result<Vec<(MapPlace, Submodel)>, RpcError> {
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
                .and_then(Submodel::parse)
                .ok_or_else(invalid)?;
            Ok((MapPlace::new(place), model))
        })
        .collect()
}

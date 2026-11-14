// SPDX-License-Identifier: GPL-3.0-or-later

//! `vm.run` (P4-02): builds the open project, calls one QuakeC function in `qc-vm` over the
//! mock engine (`qc_builtins::MockBuiltins`), with the globals and the fields of a `self`
//! edict set from the parameters, and answers what the call changed. A unit test of a weapon
//! function, for the agent and the QuakeC test command (P4-26), with no map and no game.

use std::collections::BTreeMap;
use std::path::PathBuf;

use qc_builtins::{MockBuiltins, MockEngine};
use qc_progs::Progs;
use qc_types::{DefType, EntityRef, Etype, Slot};
use qc_vm::{Machine, NoObserver, ServerState};
use qs_sim::{FieldOffset, GlobalOffset, NamedValue, QcValue, RawState, State, read_state};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use qs_commands::{Command, Outcome, RegisterError, Registry};

use crate::{HasWorkbench, WorkbenchError};

/// The id of the command that calls a QuakeC function in the VM over the mock engine.
pub const VM_RUN: &str = "vm.run";

use qc_edict::EdictNumber;

/// The parameters of `vm.run`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VmRunParams {
    /// The function to call, by name.
    pub function: String,
    /// Named globals set before the call: a number for a float, three for a vector.
    #[serde(default)]
    pub globals: BTreeMap<String, Value>,
    /// Fields of a fresh edict that `self` names during the call, set the same way; with
    /// none, `self` is the world.
    #[serde(default)]
    pub self_fields: BTreeMap<String, Value>,
}

/// One value the call changed.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct VmChange {
    /// Where: `global <name>`, or `edict <n> .<name>`.
    pub place: String,
    /// The value before the call, as the bridge writes values (BRIDGE-5.1).
    pub before: Value,
    /// The value after it.
    pub after: Value,
}

/// What `vm.run` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct VmRun {
    /// The program the project built, which the call ran in.
    pub progs: PathBuf,
    /// The run error the call stopped with, if it did not return.
    pub error: Option<String>,
    /// Every global and edict field whose value the call changed, globals first.
    pub changes: Vec<VmChange>,
}

/// Why `vm.run` could not run the function.
#[derive(Debug)]
pub enum VmRunError {
    /// The project did not build.
    Build(WorkbenchError),
    /// No module built a program.
    NothingBuilt,
    /// The progs the build wrote could not be read.
    Progs(String),
    /// No function has the name.
    NoFunction(String),
    /// A global or field named in the parameters is not one, or its value is not a float or
    /// a vector of that type.
    Bad(String),
    /// The VM could not be set up.
    Machine(String),
}

impl std::fmt::Display for VmRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Build(error) => write!(f, "the project did not build: {error}"),
            Self::NothingBuilt => f.write_str("no module of the project built a program"),
            Self::Progs(why) => write!(f, "the built program does not read: {why}"),
            Self::NoFunction(name) => write!(f, "no function is named {name}"),
            Self::Bad(why) | Self::Machine(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for VmRunError {}

/// The seed of the mock engine's random source, the same on every run (DIFF-6.2).
const SEED: u32 = 1;
/// The components of a vector: its slots (`ev_vector` takes three).
const VECTOR_SLOTS: usize = 3;

/// Runs `params.function` in the program `bytes`, read from `progs`.
///
/// # Errors
///
/// [`VmRunError`] when the program does not read, the function or a global or field is not
/// there, or a value is not of its type. A run error in the call is not an error: it is
/// answered in [`VmRun::error`] with what changed before it.
pub fn run(progs: PathBuf, bytes: &[u8], params: &VmRunParams) -> Result<VmRun, VmRunError> {
    let program = Progs::read(bytes).map_err(|e| VmRunError::Progs(e.to_string()))?;
    let function = (0..program.functions().len())
        .find(|&n| {
            program
                .functions()
                .get(n)
                .and_then(|f| program.string(f.s_name))
                == Some(params.function.as_bytes())
        })
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| VmRunError::NoFunction(params.function.clone()))?;
    let mut machine = Machine::new(&program, 1).map_err(|e| VmRunError::Machine(e.to_string()))?;
    let mut builtins = MockBuiltins::with_engine(MockEngine::new("vm_run", 0, SEED));
    for (name, value) in &params.globals {
        let (slot, etype) = named_def(&program, program.global_defs().iter(), name)
            .ok_or_else(|| VmRunError::Bad(format!("no global is named {name}")))?;
        for (i, part) in slots(etype, value, name)?.into_iter().enumerate() {
            let at = i32::try_from(slot.saturating_add(i)).map_err(|_| bad_slot(name))?;
            machine.set_global(at, part).map_err(|_| bad_slot(name))?;
        }
    }
    if !params.self_fields.is_empty() {
        let edict = machine
            .edicts_mut()
            .alloc(0.0)
            .map_err(|e| VmRunError::Machine(e.to_string()))?;
        for (name, value) in &params.self_fields {
            let (slot, etype) = named_def(&program, program.field_defs().iter(), name)
                .ok_or_else(|| VmRunError::Bad(format!("no field is named {name}")))?;
            for (i, part) in slots(etype, value, name)?.into_iter().enumerate() {
                machine
                    .edicts_mut()
                    .set_field(edict, slot.saturating_add(i), part)
                    .map_err(|_| bad_slot(name))?;
            }
        }
        let entity = machine
            .edicts()
            .edict_num(i32::try_from(edict.index()).unwrap_or(0))
            .map_err(|e| VmRunError::Machine(e.to_string()))?;
        let (self_slot, _) = named_def(&program, program.global_defs().iter(), "self")
            .ok_or_else(|| VmRunError::Bad("the program has no self".to_owned()))?;
        let at = i32::try_from(self_slot).map_err(|_| bad_slot("self"))?;
        machine
            .set_global(at, Slot::from(entity))
            .map_err(|_| bad_slot("self"))?;
    }
    let before = read_state(&program, &Raw::new(&machine, &builtins));
    let error = machine
        .execute_program(
            &program,
            ServerState::Active,
            &mut builtins,
            &mut NoObserver,
            function,
        )
        .err()
        .map(|e| e.to_string());
    let after = read_state(&program, &Raw::new(&machine, &builtins));
    Ok(VmRun {
        progs,
        error,
        changes: VmChange::between(&before, &after),
    })
}

/// `vm.run`: builds the open project and calls one function of its first program.
struct VmRunCommand;

impl<C: HasWorkbench + 'static> Command<C> for VmRunCommand {
    const ID: &'static str = VM_RUN;
    const SUMMARY: &'static str = "Builds the open project and calls one QuakeC function in the \
        VM over a mock engine, with globals and the fields of a self edict set first, answering \
        each global and field the call changed and any run error.";
    type Params = VmRunParams;
    type Output = VmRun;
    type Error = VmRunError;

    fn run(&self, context: &mut C, params: VmRunParams) -> Result<Outcome<VmRun, C>, VmRunError> {
        let modules = context.workbench().build().map_err(VmRunError::Build)?;
        let built = modules
            .into_iter()
            .find(|module| module.built)
            .ok_or(VmRunError::NothingBuilt)?;
        let bytes = std::fs::read(&built.output)
            .map_err(|e| VmRunError::Progs(format!("{}: {e}", built.output.display())))?;
        Ok(Outcome::outside_history(run(
            built.output,
            &bytes,
            &params,
        )?))
    }
}

/// Registers `vm.run`.
///
/// # Errors
///
/// [`RegisterError`] when its id is taken.
pub fn register_vm<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(VmRunCommand)
}

fn bad_slot(name: &str) -> VmRunError {
    VmRunError::Bad(format!("{name} could not be set"))
}

/// The first slot and type of the definition named `name` among `defs`.
fn named_def(
    program: &Progs<'_>,
    mut defs: impl Iterator<Item = qc_progs::Def>,
    name: &str,
) -> Option<(usize, Etype)> {
    let def = defs.find(|def| program.string(def.s_name) == Some(name.as_bytes()))?;
    let DefType::Known(etype) = def.def_type() else {
        return None;
    };
    Some((usize::try_from(def.ofs).ok()?, etype))
}

/// The slots `value` sets for a definition of `etype`: a float's one, a vector's three.
fn slots(etype: Etype, value: &Value, name: &str) -> Result<Vec<Slot>, VmRunError> {
    let float = |v: &Value| v.as_f64().map(f64_to_f32);
    let wrong = || VmRunError::Bad(format!("{name} takes a value of its type"));
    match etype {
        Etype::Float => Ok(vec![Slot::from_f32(float(value).ok_or_else(wrong)?)]),
        Etype::Vector => {
            let parts: Option<Vec<Slot>> = value
                .as_array()
                .filter(|parts| parts.len() == VECTOR_SLOTS)
                .and_then(|parts| parts.iter().map(|p| float(p).map(Slot::from_f32)).collect());
            parts.ok_or_else(wrong)
        }
        _ => Err(wrong()),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a test's value, as QuakeC's floats are f32"
)]
const fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

impl VmChange {
    /// Each global and edict field whose value differs between `before` and `after`.
    fn between(before: &State, after: &State) -> Vec<Self> {
        let mut out = Vec::new();
        let named = |values: &[NamedValue]| -> BTreeMap<String, QcValue> {
            values
                .iter()
                .map(|v| (v.name.clone(), v.value.clone()))
                .collect()
        };
        let (old, new) = (named(&before.globals), named(&after.globals));
        for (name, value) in &new {
            if old.get(name) != Some(value) {
                out.push(VmChange {
                    place: format!("global {name}"),
                    before: old.get(name).map_or(Value::Null, bridge_value),
                    after: bridge_value(value),
                });
            }
        }
        for (n, fields) in after.edicts.iter().enumerate() {
            let old = before.edicts.get(n).map(|f| named(f)).unwrap_or_default();
            for (name, value) in named(fields) {
                if old.get(&name) != Some(&value) {
                    out.push(VmChange {
                        place: format!("edict {n} .{name}"),
                        before: old.get(&name).map_or(Value::Null, bridge_value),
                        after: bridge_value(&value),
                    });
                }
            }
        }
        out
    }
}

/// A value as the bridge writes it (BRIDGE-5.1): its type and its value.
fn bridge_value(value: &QcValue) -> Value {
    match value {
        QcValue::Float(f) => json!({ "type": "float", "value": f }),
        QcValue::Vector(v) => json!({ "type": "vector", "value": v }),
        QcValue::String(s) => json!({ "type": "string", "value": String::from_utf8_lossy(s) }),
        QcValue::Entity(e) => json!({ "type": "entity", "value": e }),
        QcValue::Field(f) => json!({ "type": "field", "value": f }),
        QcValue::Function(f) => json!({ "type": "function", "value": f }),
        QcValue::Void => json!({ "type": "void" }),
    }
}

/// The machine and the mock engine's strings, read as [`RawState`].
struct Raw<'a> {
    machine: &'a Machine,
    builtins: &'a MockBuiltins,
}

impl<'a> Raw<'a> {
    const fn new(machine: &'a Machine, builtins: &'a MockBuiltins) -> Self {
        Self { machine, builtins }
    }
}

impl RawState for Raw<'_> {
    fn global(&self, slot: GlobalOffset) -> Slot {
        i32::try_from(slot.index())
            .ok()
            .and_then(|slot| self.machine.global(slot).ok())
            .unwrap_or(Slot::ZERO)
    }

    fn field(&self, edict: EdictNumber, slot: FieldOffset) -> Slot {
        self.machine
            .edicts()
            .field(edict, slot.index())
            .unwrap_or(Slot::ZERO)
    }

    fn num_edicts(&self) -> usize {
        self.machine.edicts().num_edicts()
    }

    fn entity(&self, value: Slot) -> usize {
        self.machine
            .edicts()
            .num_for_edict(EntityRef::from(value))
            .map_or(0, EdictNumber::index)
    }

    fn string(&self, progs: &Progs<'_>, value: Slot) -> Vec<u8> {
        let offset = value.as_i32();
        if offset >= 0 {
            return progs.string(offset).map(<[u8]>::to_vec).unwrap_or_default();
        }
        self.builtins
            .engine()
            .strings
            .text(offset)
            .map(<[u8]>::to_vec)
            .unwrap_or_default()
    }
}

#[cfg(test)]
pub(crate) mod tests;

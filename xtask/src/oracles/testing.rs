// SPDX-License-Identifier: GPL-3.0-or-later

//! What the oracle module's tests share: oracles that a fake runs, and fresh directories.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_syntax::Dialect;

use super::Oracles;
use super::builtins::{Case, Outcome, Suite, graph_cases, read_fixture};
use crate::exec::Exec;
use crate::test_dirs::fresh;

/// The repository.
pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// A new, empty `target/xtask-tests/oracles/<name>`, the running test's own.
pub(crate) fn fresh_dir(name: &str) -> PathBuf {
    fresh(&format!("oracles/{name}"))
}

/// Oracles whose programs `exec` runs, named `qcc`, `vm-oracle` and `fteqcc`, working in
/// `work`.
pub(crate) fn fake_oracles<'a>(work: &Path, exec: &'a dyn Exec) -> Oracles<'a> {
    Oracles {
        qcc: PathBuf::from("qcc"),
        vm: PathBuf::from("vm-oracle"),
        fteqcc: PathBuf::from("fteqcc"),
        work: work.to_path_buf(),
        rerelease: work.join("artifacts"),
        exec,
    }
}

/// The arguments of `command`, as paths.
pub(crate) fn args(command: &Command) -> Vec<PathBuf> {
    command.get_args().map(PathBuf::from).collect()
}

/// The last argument of `command`, where the oracle writes its trace.
pub(crate) fn last_arg(command: &Command) -> PathBuf {
    args(command).pop().unwrap_or_default()
}

/// A dozen cases of the builtin differential, both dialects, a wide `#pragma line` (FTEF-14.50),
/// an overrun `#pragma includedir` (FTEF-14.66) and two project graph cases, with the outcomes
/// the fixture records for them: what a unit test runs in place of the whole cross product.
pub(crate) fn sample_suite() -> (Suite, BTreeMap<String, Outcome>) {
    let fte = |source: &str| Case::new(Dialect::Fteqcc, source.to_owned());
    let vanilla = |source: &str| Case::new(Dialect::Vanilla, source.to_owned());
    let cases = vec![
        vanilla("void() f = #5;\n"),
        vanilla("void() f = #0;\n"),
        vanilla("void() f = #16;\n"),
        vanilla("void() f = #16d;\n"),
        fte("void() f = #5;\n"),
        fte("void() f = #0;\n"),
        fte("void() f = #16;\n"),
        fte("void() f = #16d;\n"),
        fte("void() f = #010;\n"),
        fte("void() f = :5;\n"),
        fte("#pragma line 4294967297\nvoid() f = :__LINESTR__;\n"),
        fte(&format!(
            "#pragma includedir {}\nvoid() f = #5;\n",
            "d".repeat(1024)
        )),
    ];
    let graph: Vec<Case> = graph_cases().into_iter().take(2).collect();
    let fixture = read_fixture(&root()).unwrap_or_default();
    let recorded: BTreeMap<String, Outcome> = cases
        .iter()
        .chain(&graph)
        .filter_map(|case| fixture.get_key_value(&case.key()))
        .map(|(key, outcome)| (key.clone(), outcome.clone()))
        .collect();
    assert_eq!(recorded.len(), cases.len().saturating_add(graph.len()));
    (Suite { cases, graph }, recorded)
}

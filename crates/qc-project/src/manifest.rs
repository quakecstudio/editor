// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec.toml` (PRJ-2).

use std::path::{Component, Path};

use serde::Deserialize;

use crate::{Define, Dialect, ModuleKind, Target};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectTable {
    pub(crate) name: Option<String>,
    #[serde(rename = "mod")]
    pub(crate) mod_name: Option<String>,
    pub(crate) dialect: Option<Dialect>,
    pub(crate) target: Option<Target>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModuleTable {
    pub(crate) kind: ModuleKind,
    pub(crate) src: String,
    pub(crate) name: Option<String>,
    pub(crate) dialect: Option<Dialect>,
    pub(crate) target: Option<Target>,
    pub(crate) output: Option<String>,
    #[serde(default)]
    pub(crate) defines: Vec<String>,
    #[serde(default)]
    pub(crate) include_dirs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    #[serde(default)]
    pub(crate) project: ProjectTable,
    #[serde(default, rename = "module")]
    pub(crate) modules: Vec<ModuleTable>,
}

/// One module of the manifest, checked.
pub(crate) struct CheckedModule {
    pub(crate) name: String,
    pub(crate) kind: ModuleKind,
    pub(crate) src: String,
    pub(crate) dialect: Option<Dialect>,
    pub(crate) target: Option<Target>,
    pub(crate) output: Option<String>,
    pub(crate) defines: Vec<Define>,
    pub(crate) include_dirs: Vec<String>,
}

/// Parses and checks `quakec.toml` (PRJ-2.1 to PRJ-2.6). The error is a message without the
/// file name.
pub(crate) fn parse(text: &str) -> Result<(ProjectTable, Vec<CheckedModule>), String> {
    let manifest: Manifest = toml::from_str(text).map_err(|e| {
        let line = e.span().map_or(1, |span| {
            let before = text.get(..span.start).unwrap_or_default();
            before
                .bytes()
                .filter(|b| *b == b'\n')
                .count()
                .saturating_add(1)
        });
        format!("line {line}: {}", e.message())
    })?;
    if manifest.modules.is_empty() {
        return Err("no [[module]] is declared".to_owned());
    }
    if let Some(name) = &manifest.project.mod_name
        && !valid_mod_name(name)
    {
        return Err(format!("mod name {name:?} is not a single directory name"));
    }
    let mut modules: Vec<CheckedModule> = Vec::new();
    for module in manifest.modules {
        let name = module
            .name
            .unwrap_or_else(|| module.kind.as_str().to_owned());
        if modules.iter().any(|m| m.name == name) {
            return Err(format!("module name {name:?} is used twice"));
        }
        if rooted(Path::new(&module.src)) {
            return Err(format!("module {name}: src must be a relative path"));
        }
        if let Some(output) = &module.output {
            let path = Path::new(output);
            let escapes = path.components().any(|c| c == Component::ParentDir);
            if rooted(path) || escapes {
                return Err(format!(
                    "module {name}: output must be a relative path inside the project"
                ));
            }
        }
        if let Some(dir) = module.include_dirs.iter().find(|d| rooted(Path::new(d))) {
            return Err(format!(
                "module {name}: include directory {dir:?} must be a relative path"
            ));
        }
        let mut defines = Vec::new();
        for entry in &module.defines {
            let Some(define) = Define::parse(entry) else {
                return Err(format!(
                    "module {name}: {entry:?} is not NAME or NAME=VALUE"
                ));
            };
            defines.push(define);
        }
        modules.push(CheckedModule {
            name,
            kind: module.kind,
            src: module.src,
            dialect: module.dialect,
            target: module.target,
            output: module.output,
            defines,
            include_dirs: module.include_dirs,
        });
    }
    Ok((manifest.project, modules))
}

/// Whether `path` has a root (`/x`) or, on Windows, a drive (`C:`) (PRJ-2.5).
fn rooted(path: &Path) -> bool {
    path.has_root()
        || matches!(path.components().next(), Some(Component::Prefix(_)))
}

/// Whether `name` is a single directory name (PRJ-2.3).
pub(crate) fn valid_mod_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

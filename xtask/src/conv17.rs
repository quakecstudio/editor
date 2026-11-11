// SPDX-License-Identifier: GPL-3.0-or-later

//! The two CONV-17 rules a parser can check (`docs/conventions.md`, "Type design (CONV-17)",
//! P5-18), part of `cargo xtask conventions`:
//!
//! - **bool argument:** a function or method in a shipped crate or app that takes a `bool`
//!   parameter, where a closed set of choices belongs in an enum;
//! - **String error:** a function in a library crate (layers L0 to L7) that returns
//!   `Result<_, String>`, where an error enum belongs;
//! - **free function on a type:** a free function (not in an `impl` or trait) whose first
//!   parameter is a type the crate defines, by value or by reference, where a method belongs
//!   ("Behaviour that reads one type is a method on it");
//! - **bare ID:** a visible struct field, or a parameter of a visible function, named as an ID
//!   or a counted place (`id`, `index`, `entity`, `edict`, `slot`, `revision`, or ending
//!   `_id`, `_index`, `_slot`, `_edict`) and typed as a raw integer, where a newtype belongs
//!   ("Newtypes for IDs and units");
//! - **magic number:** a numeric literal in a function's body, other than 0, 1 and 2, where a
//!   named constant belongs; constants, statics, types (`[u8; 16]`), patterns and macro
//!   arguments are not read. Listed once per function and value, so a new value in a listed
//!   function still fails.
//!
//! Test code is left out: `tests.rs`, `*_tests.rs` and `fixtures.rs` files, files under a
//! `tests` directory, and items under `#[cfg(test)]` or `#[test]`. Each violation that existed
//! when the check came is listed in [`BASELINE`], by file and qualified name rather than line,
//! so editing a file does not move it; the check fails on a violation not listed and on a listed
//! one that no longer exists, so the list only shrinks as P5-19's refactors land. A new entry
//! is a reviewed exception and carries its reason.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::{
    Attribute, Fields, FnArg, ImplItem, Item, Pat, ReturnType, Signature, TraitItem, Type,
    Visibility,
};

use crate::Report;
use crate::layers::assignments;

/// The baseline of existing violations, under the repository root.
pub(crate) const BASELINE: &str = "docs/audits/conv-17-baseline.tsv";

/// Which rule a violation breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rule {
    /// A `bool` parameter.
    BoolArgument,
    /// A `Result<_, String>` return type in a library crate.
    StringError,
    /// A free function whose first parameter is a type of its own crate.
    FreeFnOnType,
    /// A visible field or parameter that names an ID and is a raw integer.
    BareId,
    /// A numeric literal in a function body that is not a named constant.
    MagicNumber,
}

impl Rule {
    const fn name(self) -> &'static str {
        match self {
            Self::BoolArgument => "bool-argument",
            Self::StringError => "string-error",
            Self::FreeFnOnType => "free-fn-on-type",
            Self::BareId => "bare-id",
            Self::MagicNumber => "magic-number",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "bool-argument" => Some(Self::BoolArgument),
            "string-error" => Some(Self::StringError),
            "free-fn-on-type" => Some(Self::FreeFnOnType),
            "bare-id" => Some(Self::BareId),
            "magic-number" => Some(Self::MagicNumber),
            _ => None,
        }
    }
}

/// One violation: the rule, the file under the root and the item's qualified name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Violation {
    pub(crate) rule: Rule,
    pub(crate) file: String,
    pub(crate) item: String,
}

impl Violation {
    fn line(&self) -> String {
        format!("{}\t{}\t{}", self.rule.name(), self.file, self.item)
    }
}

/// Whether a crate's code is held to which rules: by its layer in `layers.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A library crate, layers L0 to L7: both rules.
    Library,
    /// An app: the bool rule only.
    App,
}

/// Checks the source against the baseline, recording each problem in `report`.
pub(crate) fn check(root: &Path, report: &mut Report) -> Result<(), String> {
    let found: BTreeSet<Violation> = violations(root)?.into_iter().collect();
    let baseline = read_baseline(root)?;
    let mut ok = true;
    for violation in found.difference(&baseline) {
        ok = false;
        let what = match violation.rule {
            Rule::BoolArgument => "takes a bool; pass an enum (CONV-17)",
            Rule::StringError => "returns Result<_, String>; return an error enum (CONV-17)",
            Rule::FreeFnOnType => {
                "is a free function on a type of its crate; make it a method (CONV-17)"
            }
            Rule::BareId => "is an ID as a raw integer; give it a named type (CONV-17)",
            Rule::MagicNumber => "is a magic number; name it as a constant (CONV-17)",
        };
        report.record(
            false,
            format!("{}: {} {what}", violation.file, violation.item),
        );
    }
    for stale in baseline.difference(&found) {
        ok = false;
        report.record(
            false,
            format!(
                "{BASELINE}: `{}` no longer matches the code; remove the line",
                stale.line()
            ),
        );
    }
    if ok {
        report.record(
            true,
            format!(
                "CONV-17: no bool argument or String error beyond the {} in {BASELINE}",
                baseline.len()
            ),
        );
    }
    Ok(())
}

/// Every violation in the shipped crates and apps, sorted.
pub(crate) fn violations(root: &Path) -> Result<Vec<Violation>, String> {
    let layers = assignments(root)?;
    let mut out = Vec::new();
    for parent in ["crates", "apps"] {
        let Ok(entries) = fs::read_dir(root.join(parent)) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            let Some(name) = package_name(&dir) else {
                continue;
            };
            let kind = match layers.iter().find(|(crate_name, _)| *crate_name == name) {
                Some((_, layer)) if layer.starts_with('L') => Kind::Library,
                Some((_, layer)) if layer == "apps" => Kind::App,
                _ => continue,
            };
            let files = rust_files(&dir.join("src"));
            let mut parsed_files = Vec::new();
            for file in files {
                let relative = file
                    .strip_prefix(root)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .replace('\\', "/");
                let source = fs::read_to_string(&file).map_err(|e| format!("{relative}: {e}"))?;
                let parsed = syn::parse_file(&source).map_err(|e| format!("{relative}: {e}"))?;
                parsed_files.push((relative, parsed));
            }
            // The types the crate defines, for the free-function rule.
            let mut local = BTreeSet::new();
            for (_, parsed) in &parsed_files {
                local_types(&parsed.items, &mut local);
            }
            for (relative, parsed) in &parsed_files {
                let mut scan = Scan {
                    file: relative,
                    kind,
                    local: &local,
                    out: &mut out,
                };
                scan.items(&parsed.items, "");
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The baseline file's entries; an empty set when it does not exist.
fn read_baseline(root: &Path) -> Result<BTreeSet<Violation>, String> {
    let Ok(text) = fs::read_to_string(root.join(BASELINE)) else {
        return Ok(BTreeSet::new());
    };
    let mut out = BTreeSet::new();
    for (number, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let (Some(rule), Some(file), Some(item)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(format!(
                "{BASELINE}:{}: want rule, file and item",
                number.saturating_add(1)
            ));
        };
        let rule = Rule::parse(rule).ok_or_else(|| {
            format!(
                "{BASELINE}:{}: unknown rule {rule}",
                number.saturating_add(1)
            )
        })?;
        out.insert(Violation {
            rule,
            file: file.to_owned(),
            item: item.to_owned(),
        });
    }
    Ok(out)
}

/// Writes the baseline from the code as it is.
pub(crate) fn write_baseline(root: &Path) -> Result<usize, String> {
    let found = violations(root)?;
    let mut text = String::from(
        "# CONV-17 violations that existed when cargo xtask conventions began checking them\n\
         # (P5-18): rule, file, item. Only shrinks: remove a line when its item is fixed\n\
         # (P5-19). A line added later is a reviewed exception; say why in a comment above it.\n",
    );
    for violation in &found {
        text.push_str(&violation.line());
        text.push('\n');
    }
    if let Some(parent) = root.join(BASELINE).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{BASELINE}: {e}"))?;
    }
    fs::write(root.join(BASELINE), text).map_err(|e| format!("{BASELINE}: {e}"))?;
    Ok(found.len())
}

/// The package name in `dir/Cargo.toml`.
fn package_name(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table
        .get("package")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

/// The `.rs` files under `dir` that are not test code, sorted.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "tests" {
                    pending.push(path);
                }
            } else if name.ends_with(".rs")
                && name != "tests.rs"
                && name != "fixtures.rs"
                && !name.ends_with("_tests.rs")
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// A walk over one file's items.
struct Scan<'a> {
    file: &'a str,
    kind: Kind,
    /// The names of the types the crate defines.
    local: &'a BTreeSet<String>,
    out: &'a mut Vec<Violation>,
}

impl Scan<'_> {
    fn items(&mut self, items: &[Item], prefix: &str) {
        for item in items {
            match item {
                Item::Fn(f) if !is_test(&f.attrs) => {
                    self.signature(&f.sig, prefix, visible(&f.vis));
                    self.free_function(&f.sig, prefix);
                    self.body(&f.block, &format!("{prefix}{}", f.sig.ident));
                }
                Item::Struct(st) if !is_test(&st.attrs) => {
                    if let Fields::Named(fields) = &st.fields {
                        for field in &fields.named {
                            if let Some(name) = &field.ident
                                && visible(&field.vis)
                                && names_an_id(&name.to_string())
                                && is_raw_integer(&field.ty)
                            {
                                self.push(Rule::BareId, &format!("{prefix}{}.{name}", st.ident));
                            }
                        }
                    }
                }
                Item::Mod(m) if !is_test(&m.attrs) => {
                    if let Some((_, inner)) = &m.content {
                        self.items(inner, &format!("{prefix}{}::", m.ident));
                    }
                }
                Item::Impl(i) if !is_test(&i.attrs) => {
                    let owner = format!("{prefix}{}::", type_name(&i.self_ty));
                    for inner in &i.items {
                        // A trait impl's methods are as visible as the trait.
                        if let ImplItem::Fn(f) = inner
                            && !is_test(&f.attrs)
                        {
                            let shown = i.trait_.is_some() || visible(&f.vis);
                            self.signature(&f.sig, &owner, shown);
                            self.body(&f.block, &format!("{owner}{}", f.sig.ident));
                        }
                    }
                }
                Item::Trait(t) if !is_test(&t.attrs) => {
                    let owner = format!("{prefix}{}::", t.ident);
                    for inner in &t.items {
                        if let TraitItem::Fn(f) = inner {
                            self.signature(&f.sig, &owner, visible(&t.vis));
                            if let Some(block) = &f.default {
                                self.body(block, &format!("{owner}{}", f.sig.ident));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn signature(&mut self, sig: &Signature, prefix: &str, shown: bool) {
        let item = format!("{prefix}{}", sig.ident);
        if shown {
            for input in &sig.inputs {
                if let FnArg::Typed(typed) = input
                    && let Pat::Ident(name) = &*typed.pat
                    && names_an_id(&name.ident.to_string())
                    && is_raw_integer(&typed.ty)
                {
                    self.push(Rule::BareId, &format!("{item}({})", name.ident));
                }
            }
        }
        let takes_bool = sig.inputs.iter().any(|input| match input {
            FnArg::Typed(typed) => is_named(&typed.ty, "bool"),
            FnArg::Receiver(_) => false,
        });
        if takes_bool {
            self.push(Rule::BoolArgument, &item);
        }
        if self.kind == Kind::Library
            && let ReturnType::Type(_, ty) = &sig.output
            && string_error(ty)
        {
            self.push(Rule::StringError, &item);
        }
    }

    /// Each magic number in the body of `item`.
    fn body(&mut self, block: &syn::Block, item: &str) {
        let mut literals = Literals::default();
        syn::visit::Visit::visit_block(&mut literals, block);
        for value in literals.found {
            self.push(Rule::MagicNumber, &format!("{item}:{value}"));
        }
    }

    /// A free function whose first parameter is one of the crate's own types.
    fn free_function(&mut self, sig: &Signature, prefix: &str) {
        let Some(FnArg::Typed(first)) = sig.inputs.first() else {
            return;
        };
        let mut ty = &*first.ty;
        while let Type::Reference(reference) = ty {
            ty = &reference.elem;
        }
        let Type::Path(path) = ty else {
            return;
        };
        if path.qself.is_none()
            && let Some(last) = path.path.segments.last()
            && self.local.contains(&last.ident.to_string())
        {
            self.push(Rule::FreeFnOnType, &format!("{prefix}{}", sig.ident));
        }
    }

    fn push(&mut self, rule: Rule, item: &str) {
        self.out.push(Violation {
            rule,
            file: self.file.to_owned(),
            item: item.to_owned(),
        });
    }
}

/// The numeric literals of a body that are magic numbers, by their value as written in base
/// 10 with no suffix; types, items and patterns inside the body are not read.
#[derive(Default)]
struct Literals {
    found: BTreeSet<String>,
}

impl<'ast> syn::visit::Visit<'ast> for Literals {
    fn visit_expr_lit(&mut self, expr: &'ast syn::ExprLit) {
        let value = match &expr.lit {
            syn::Lit::Int(int) => int.base10_digits().to_owned(),
            syn::Lit::Float(float) => float.base10_digits().to_owned(),
            _ => return,
        };
        let trivial = matches!(value.as_str(), "0" | "1" | "2" | "0.0" | "1.0" | "2.0");
        if !trivial {
            self.found.insert(value);
        }
    }

    fn visit_type(&mut self, _: &'ast Type) {}

    fn visit_item(&mut self, _: &'ast Item) {}

    fn visit_pat(&mut self, _: &'ast Pat) {}
}

/// The structs, enums, unions and type aliases `items` define, in any module but a test one.
fn local_types(items: &[Item], out: &mut BTreeSet<String>) {
    for item in items {
        match item {
            Item::Struct(s) if !is_test(&s.attrs) => {
                out.insert(s.ident.to_string());
            }
            Item::Enum(e) if !is_test(&e.attrs) => {
                out.insert(e.ident.to_string());
            }
            Item::Union(u) if !is_test(&u.attrs) => {
                out.insert(u.ident.to_string());
            }
            Item::Type(t) if !is_test(&t.attrs) => {
                out.insert(t.ident.to_string());
            }
            Item::Mod(m) if !is_test(&m.attrs) => {
                if let Some((_, inner)) = &m.content {
                    local_types(inner, out);
                }
            }
            _ => {}
        }
    }
}

/// Whether an item is seen outside its module: any `pub`, `pub(crate)` or `pub(super)`.
const fn visible(vis: &Visibility) -> bool {
    !matches!(vis, Visibility::Inherited)
}

/// Whether `name` names an ID or a counted place (the bare-id rule).
fn names_an_id(name: &str) -> bool {
    matches!(
        name,
        "id" | "index" | "entity" | "edict" | "slot" | "revision"
    ) || ["_id", "_index", "_slot", "_edict"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// Whether `ty` is a raw integer, or an `Option` of one.
fn is_raw_integer(ty: &Type) -> bool {
    const INTEGERS: [&str; 12] = [
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
    ];
    let Type::Path(path) = ty else {
        return false;
    };
    let Some(last) = path.path.segments.last() else {
        return false;
    };
    if path.qself.is_none()
        && path.path.segments.len() == 1
        && INTEGERS.contains(&&*last.ident.to_string())
    {
        return true;
    }
    if last.ident == "Option"
        && let syn::PathArguments::AngleBracketed(args) = &last.arguments
        && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
    {
        return is_raw_integer(inner);
    }
    false
}

/// Whether the attributes mark test code: `#[test]` or `#[cfg(test)]`.
fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Ident>()
                    .is_ok_and(|ident| ident == "test"))
    })
}

/// Whether `ty` is the plain type `name`.
fn is_named(ty: &Type, name: &str) -> bool {
    match ty {
        Type::Path(path) => path.qself.is_none() && path.path.is_ident(name),
        _ => false,
    }
}

/// Whether `ty` is `Result<_, String>`.
fn string_error(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let Some(last) = path.path.segments.last() else {
        return false;
    };
    if last.ident != "Result" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return false;
    };
    let types: Vec<&Type> = args
        .args
        .iter()
        .filter_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .collect();
    types.get(1).is_some_and(|error| {
        matches!(error, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "String"))
    })
}

/// The name an `impl`'s self type is listed by: its last path segment.
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map_or_else(String::new, |s| s.ident.to_string()),
        Type::Reference(reference) => type_name(&reference.elem),
        _ => String::from("_"),
    }
}

#[cfg(test)]
mod tests;

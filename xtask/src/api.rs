// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask api [--check]`: writes a sorted list of each library crate's public items to
//! `docs/api/<crate>.txt` (P2-154, ARCH-1, CONV-17); `--check` writes nothing and fails when a
//! checked-in list differs from the crate's source. `cargo xtask ci` runs the same check
//! in-process ([`check`]), so a change to a crate's public API shows in the diff that makes it.
//!
//! The library crates are those in layers `L0` to `L5` of `layers.toml`. A crate's public items
//! are read from its source with `syn`: every item with `pub` visibility (not `pub(crate)` or
//! narrower) in every module the crate root reaches, test modules (`#[cfg(test)]`) skipped;
//! `unreachable_pub` is a workspace lint, so every `pub` item is reachable from the root. Each
//! line is a kind and a path: `struct qc_progs::Statement`, `field qc_progs::Statement.op`,
//! `fn qc_progs::Progs::read` for a method, `variant`, `trait`, `type`, `const`, `static`,
//! `mod`, `macro`, `impl qc_progs::Progs: Clone` for a trait implemented, and
//! `use qc_progs::Opcode = crate::opcode::Opcode` for a re-export.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::{Attribute, Item, Visibility};

use crate::{Report, layers, print_outcome};

/// The directory the lists are written to, relative to the workspace root.
pub(crate) const DIR: &str = "docs/api";

/// The layers whose crates are library crates with a public API boundary.
const LIBRARY_LAYERS: [&str; 6] = ["L0", "L1", "L2", "L3", "L4", "L5"];

/// `cargo xtask api [--check]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Check the lists instead of writing them.
    #[arg(long)]
    check: bool,
}

/// Runs `cargo xtask api` with the arguments `args`.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if args.check {
        return Ok(print_outcome("api", check(root)));
    }
    let dir = root.join(DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("{DIR}: {e}"))?;
    let crates = library_crates(root)?;
    for stale in stale_lists(root, &crates)? {
        let path = dir.join(&stale);
        fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("api: removed {DIR}/{stale}");
    }
    for krate in crates {
        let path = dir.join(format!("{}.txt", krate.name));
        fs::write(&path, krate.listing()?).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("api: wrote {DIR}/{}.txt", krate.name);
    }
    Ok(true)
}

/// Checks that each library crate's list in `docs/api/` is what its source gives, and that
/// no list is left for a crate that is no library crate any more.
pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let mut report = Report::default();
    let crates = library_crates(root)?;
    for stale in stale_lists(root, &crates)? {
        report.record(
            false,
            format!("{DIR}/{stale} lists no library crate; run cargo xtask api"),
        );
    }
    for krate in crates {
        let file = format!("{DIR}/{}.txt", krate.name);
        let current = fs::read_to_string(root.join(&file)).unwrap_or_default();
        let listing = krate.listing()?;
        let matches = current == listing;
        report.record(
            matches,
            if matches {
                format!("{file} matches the crate's public items")
            } else {
                format!("{file} differs from the crate's public items; run cargo xtask api")
            },
        );
    }
    Ok(report)
}

/// The files in `docs/api/` that list no crate in `crates`.
fn stale_lists(root: &Path, crates: &[Crate]) -> Result<Vec<String>, String> {
    let dir = root.join(DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut stale = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{DIR}: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let listed = name
            .strip_suffix(".txt")
            .is_some_and(|stem| crates.iter().any(|c| c.name == stem));
        if !listed {
            stale.push(name);
        }
    }
    stale.sort();
    Ok(stale)
}

/// A library crate: its name and the directory its `Cargo.toml` is in.
struct Crate {
    name: String,
    dir: PathBuf,
}

/// The library crates of the workspace: those `layers.toml` puts in [`LIBRARY_LAYERS`], in
/// name order.
fn library_crates(root: &Path) -> Result<Vec<Crate>, String> {
    let mut crates = Vec::new();
    for (name, layer) in layers::assignments(root)? {
        if !LIBRARY_LAYERS.contains(&layer.as_str()) {
            continue;
        }
        let dir = root.join("crates").join(&name);
        if !dir.join("Cargo.toml").is_file() {
            return Err(format!("{name}: no crates/{name}/Cargo.toml"));
        }
        crates.push(Crate { name, dir });
    }
    crates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(crates)
}

impl Crate {
    /// The crate's public items, one per line, sorted.
    fn listing(&self) -> Result<String, String> {
        let root_name = self.name.replace('-', "_");
        let lib = self.dir.join("src").join("lib.rs");
        let mut items = BTreeSet::new();
        read_module(&lib, &root_name, &mut items)?;
        // An `impl` counts only on a type the crate itself makes public.
        let types: BTreeSet<String> = items
            .iter()
            .filter_map(|item| {
                item.split_once(' ')
                    .filter(|(kind, _)| matches!(*kind, "struct" | "enum" | "type" | "union"))
                    .map(|(_, path)| path.to_owned())
            })
            .collect();
        items.retain(|item| {
            item.strip_prefix("impl ")
                .and_then(|rest| rest.split_once(": "))
                .is_none_or(|(self_type, _)| types.contains(self_type))
        });
        let mut out = String::new();
        for item in items {
            out.push_str(&item);
            out.push('\n');
        }
        Ok(out)
    }
}

/// Reads the module in `file`, whose path from the crate root is `path`, adding its public
/// items to `items` and following its `mod` declarations to their files.
fn read_module(file: &Path, path: &str, items: &mut BTreeSet<String>) -> Result<(), String> {
    let source = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let parsed = syn::parse_file(&source).map_err(|e| format!("{}: {e}", file.display()))?;
    let dir = file
        .parent()
        .ok_or_else(|| format!("{}: no directory", file.display()))?;
    let own_dir = if file
        .file_name()
        .is_some_and(|n| n == "lib.rs" || n == "mod.rs")
    {
        dir.to_path_buf()
    } else {
        dir.join(file.file_stem().map_or_else(|| Path::new(""), Path::new))
    };
    let mut visitor = Items {
        path: path.to_owned(),
        items,
        pending: Vec::new(),
        dir: own_dir,
    };
    visitor.items_of(&parsed.items);
    for (dir, name, module_path) in visitor.pending {
        let candidates = [
            dir.join(format!("{name}.rs")),
            dir.join(&name).join("mod.rs"),
        ];
        let Some(child) = candidates.iter().find(|c| c.is_file()) else {
            return Err(format!(
                "{}: module {name} has no file under {}",
                file.display(),
                dir.display()
            ));
        };
        read_module(child, &module_path, items)?;
    }
    Ok(())
}

/// Collects the public items of one module.
struct Items<'a> {
    /// The module's path from the crate root.
    path: String,
    items: &'a mut BTreeSet<String>,
    /// Out-of-line `mod` declarations to read: the directory the file is looked for in, the
    /// module's name and its path.
    pending: Vec<(PathBuf, String, String)>,
    /// The directory an out-of-line module declared here is looked for in.
    dir: PathBuf,
}

/// Whether an attribute list carries a `#[cfg(...)]` whose predicate names `test`:
/// `#[cfg(test)]`, `#[cfg(all(test))]` or `#[cfg(any(test, feature = "x"))]`.
fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr.meta.require_list().is_ok_and(|list| {
                list.tokens
                    .to_string()
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|word| word == "test")
            })
    })
}

/// Whether a visibility is `pub`, with no restriction.
fn is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

impl Items<'_> {
    fn add(&mut self, kind: &str, name: &str) {
        self.items.insert(format!("{kind} {}::{name}", self.path));
    }

    fn items_of(&mut self, list: &[Item]) {
        for item in list {
            self.item(item);
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Mod(m) => {
                if is_test(&m.attrs) {
                    return;
                }
                let name = m.ident.to_string();
                let module_path = format!("{}::{name}", self.path);
                if is_pub(&m.vis) {
                    self.add("mod", &name);
                }
                match &m.content {
                    Some((_, content)) => {
                        let mut inner = Items {
                            path: module_path,
                            items: self.items,
                            pending: Vec::new(),
                            dir: self.dir.join(&name),
                        };
                        inner.items_of(content);
                        let pending = inner.pending;
                        self.pending.extend(pending);
                    }
                    None => self.pending.push((self.dir.clone(), name, module_path)),
                }
            }
            Item::Struct(s) if is_pub(&s.vis) && !is_test(&s.attrs) => {
                let name = s.ident.to_string();
                self.add("struct", &name);
                for (i, field) in s.fields.iter().enumerate() {
                    if is_pub(&field.vis) {
                        let field_name = field
                            .ident
                            .as_ref()
                            .map_or_else(|| i.to_string(), ToString::to_string);
                        self.add("field", &format!("{name}.{field_name}"));
                    }
                }
            }
            Item::Enum(e) if is_pub(&e.vis) && !is_test(&e.attrs) => {
                let name = e.ident.to_string();
                self.add("enum", &name);
                for variant in &e.variants {
                    self.add("variant", &format!("{name}::{}", variant.ident));
                }
            }
            Item::Union(u) if is_pub(&u.vis) => self.add("union", &u.ident.to_string()),
            Item::Fn(f) if is_pub(&f.vis) && !is_test(&f.attrs) => {
                self.add("fn", &f.sig.ident.to_string());
            }
            Item::Trait(t) if is_pub(&t.vis) && !is_test(&t.attrs) => {
                let name = t.ident.to_string();
                self.add("trait", &name);
                for item in &t.items {
                    match item {
                        syn::TraitItem::Fn(f) => {
                            self.add("fn", &format!("{name}::{}", f.sig.ident));
                        }
                        syn::TraitItem::Type(t) => {
                            self.add("type", &format!("{name}::{}", t.ident));
                        }
                        syn::TraitItem::Const(c) => {
                            self.add("const", &format!("{name}::{}", c.ident));
                        }
                        _ => {}
                    }
                }
            }
            Item::Type(t) if is_pub(&t.vis) && !is_test(&t.attrs) => {
                self.add("type", &t.ident.to_string());
            }
            Item::Const(c) if is_pub(&c.vis) && !is_test(&c.attrs) => {
                self.add("const", &c.ident.to_string());
            }
            Item::Static(s) if is_pub(&s.vis) && !is_test(&s.attrs) => {
                self.add("static", &s.ident.to_string());
            }
            Item::Macro(m) if m.attrs.iter().any(|a| a.path().is_ident("macro_export")) => {
                if let Some(ident) = &m.ident {
                    self.add("macro", &ident.to_string());
                }
            }
            Item::Use(u) if is_pub(&u.vis) && !is_test(&u.attrs) => {
                let mut names = Vec::new();
                ReExports {
                    prefix: Vec::new(),
                    names: &mut names,
                }
                .visit_use_tree(&u.tree);
                for (name, source) in names {
                    self.add("use", &format!("{name} = {source}"));
                }
            }
            Item::Impl(i) if !is_test(&i.attrs) => {
                let self_type = type_name(&i.self_ty);
                match &i.trait_ {
                    Some((trait_path, _)) => {
                        let trait_name = path_name(trait_path);
                        self.add("impl", &format!("{self_type}: {trait_name}"));
                    }
                    None => {
                        for item in &i.items {
                            match item {
                                syn::ImplItem::Fn(f) if is_pub(&f.vis) => {
                                    self.add("fn", &format!("{self_type}::{}", f.sig.ident));
                                }
                                syn::ImplItem::Const(c) if is_pub(&c.vis) => {
                                    self.add("const", &format!("{self_type}::{}", c.ident));
                                }
                                syn::ImplItem::Type(t) if is_pub(&t.vis) => {
                                    self.add("type", &format!("{self_type}::{}", t.ident));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The names a `pub use` tree brings in, each with the path it names.
struct ReExports<'a> {
    prefix: Vec<String>,
    names: &'a mut Vec<(String, String)>,
}

impl ReExports<'_> {
    fn visit_use_tree(&mut self, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(p) => {
                self.prefix.push(p.ident.to_string());
                self.visit_use_tree(&p.tree);
                self.prefix.pop();
            }
            syn::UseTree::Name(n) => {
                let name = n.ident.to_string();
                let source = self.source(&name);
                self.names.push((name, source));
            }
            syn::UseTree::Rename(r) => {
                let source = self.source(&r.ident.to_string());
                self.names.push((r.rename.to_string(), source));
            }
            syn::UseTree::Glob(_) => {
                let source = self.source("*");
                self.names.push(("*".to_owned(), source));
            }
            syn::UseTree::Group(g) => {
                for item in &g.items {
                    self.visit_use_tree(item);
                }
            }
        }
    }

    fn source(&self, last: &str) -> String {
        let mut parts = self.prefix.clone();
        parts.push(last.to_owned());
        parts.join("::")
    }
}

/// A type's path with its generics dropped, `Vec<T>` is `Vec` and `Bsp<'a>` is `Bsp`, so a
/// method or an implementation keys on the type itself.
fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        syn::Type::Reference(r) => type_name(&r.elem),
        syn::Type::Tuple(t) => format!(
            "({})",
            t.elems.iter().map(type_name).collect::<Vec<_>>().join(", ")
        ),
        syn::Type::Slice(s) => format!("[{}]", type_name(&s.elem)),
        syn::Type::Array(a) => format!("[{}; _]", type_name(&a.elem)),
        _ => "_".to_owned(),
    }
}

/// A path's segments, each with its type arguments: `std::fmt::Display` stays, `From<Foo>`
/// keeps its `Foo` so two implementations of one generic trait stay two lines.
fn path_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| match &s.arguments {
            syn::PathArguments::None | syn::PathArguments::Parenthesized(_) => s.ident.to_string(),
            syn::PathArguments::AngleBracketed(args) => {
                let inner: Vec<String> = args
                    .args
                    .iter()
                    .map(|arg| match arg {
                        syn::GenericArgument::Type(ty) => type_name(ty),
                        syn::GenericArgument::Lifetime(l) => format!("'{}", l.ident),
                        _ => "_".to_owned(),
                    })
                    .collect();
                format!("{}<{}>", s.ident, inner.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::fs;
    use std::path::Path;

    use super::{Args, DIR, check, main};
    use crate::test_dirs::fresh as fresh_dir;

    /// A workspace of one library crate `lib` in L0 whose public items are `source`.
    fn workspace(dir: &Path, source: &str) {
        fs::write(
            dir.join("layers.toml"),
            "[crates]\nlib = \"L0\"\nhelper = \"tests\"\n",
        )
        .unwrap();
        let src = dir.join("crates").join("lib").join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(dir.join("crates").join("lib").join("Cargo.toml"), "").unwrap();
        fs::write(src.join("lib.rs"), source).unwrap();
    }

    const SOURCE: &str = "pub mod inner;\nmod private;\npub use private::Hidden;\n\
                          pub struct S { pub a: u32, b: u32 }\npub enum E { A, B(u8) }\n\
                          impl S { pub fn new() -> Self { S { a: 0, b: 0 } } fn secret() {} }\n\
                          impl Clone for S { fn clone(&self) -> Self { S { a: 0, b: 0 } } }\n\
                          impl From<u8> for S { fn from(_: u8) -> Self { S::new() } }\n\
                          impl From<u16> for S { fn from(_: u16) -> Self { S::new() } }\n\
                          struct P;\nimpl Default for P { fn default() -> Self { P } }\n\
                          pub trait T { fn t(&self); type Out; }\npub type A = u32;\n\
                          pub(crate) fn crate_only() {}\npub fn free() {}\n\
                          #[cfg(any(test, feature = \"x\"))]\npub const N: u32 = 1;\n\
                          pub mod outer { pub mod deep; }\n\
                          #[cfg(test)]\nmod tests { pub fn in_test() {} }\n";

    // P2-154: the list has every public item of every module the root reaches, by kind, and
    // nothing private, crate-visible or in a test module
    #[test]
    fn the_listing_has_every_public_item_and_nothing_else() {
        let dir = fresh_dir("api-listing");
        workspace(&dir, SOURCE);
        let src = dir.join("crates").join("lib").join("src");
        fs::write(
            src.join("inner.rs"),
            "pub fn deep() {}\npub(super) fn up() {}\n",
        )
        .unwrap();
        fs::write(src.join("private.rs"), "pub struct Hidden(pub u8);\n").unwrap();
        fs::create_dir_all(src.join("outer")).unwrap();
        fs::write(src.join("outer").join("deep.rs"), "pub fn bottom() {}\n").unwrap();
        assert!(main(&dir, &Args { check: false }).unwrap());
        let listing = fs::read_to_string(dir.join(DIR).join("lib.txt")).unwrap();
        let expected = "\
            enum lib::E\n\
            field lib::S.a\n\
            field lib::private::Hidden.0\n\
            fn lib::S::new\n\
            fn lib::T::t\n\
            fn lib::free\n\
            fn lib::inner::deep\n\
            fn lib::outer::deep::bottom\n\
            impl lib::S: Clone\n\
            impl lib::S: From<u16>\n\
            impl lib::S: From<u8>\n\
            mod lib::inner\n\
            mod lib::outer\n\
            mod lib::outer::deep\n\
            struct lib::S\n\
            struct lib::private::Hidden\n\
            trait lib::T\n\
            type lib::A\n\
            type lib::T::Out\n\
            use lib::Hidden = private::Hidden\n\
            variant lib::E::A\n\
            variant lib::E::B\n";
        assert_eq!(listing, expected);
    }

    // P2-154: `--check` passes on a list the source gives and fails on a public item added
    // without regenerating it, or with no list at all
    #[test]
    fn check_fails_when_a_public_item_is_not_in_the_list() {
        let dir = fresh_dir("api-check");
        workspace(&dir, "pub fn one() {}\n");
        let src = dir.join("crates").join("lib").join("src");
        assert!(!check(&dir).unwrap().problems.is_empty(), "no list yet");
        assert!(main(&dir, &Args { check: false }).unwrap());
        assert!(check(&dir).unwrap().problems.is_empty());
        fs::write(src.join("lib.rs"), "pub fn one() {}\npub fn two() {}\n").unwrap();
        let report = check(&dir).unwrap();
        assert_eq!(report.problems.len(), 1);
        assert!(report.problems[0].contains("docs/api/lib.txt differs"));
        assert!(main(&dir, &Args { check: true }).is_ok());
        assert!(crate::parse_cli::<Args, _>(&["--other"]).is_err());
    }

    // P2-154: a list left for a crate that is no library crate fails the check, and the
    // write removes it
    #[test]
    fn a_stale_list_fails_the_check_and_is_removed() {
        let dir = fresh_dir("api-stale");
        workspace(&dir, "pub fn one() {}\n");
        assert!(main(&dir, &Args { check: false }).unwrap());
        fs::write(dir.join(DIR).join("gone.txt"), "fn gone::f\n").unwrap();
        let report = check(&dir).unwrap();
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("docs/api/gone.txt lists no library crate"))
        );
        assert!(main(&dir, &Args { check: false }).unwrap());
        assert!(!dir.join(DIR).join("gone.txt").exists());
        assert!(check(&dir).unwrap().problems.is_empty());
    }

    // P2-154: a module declaration with no file is an error, not a silent gap
    #[test]
    fn a_missing_module_file_is_an_error() {
        let dir = fresh_dir("api-missing");
        workspace(&dir, "pub mod gone;\n");
        assert!(check(&dir).unwrap_err().contains("module gone has no file"));
    }
}

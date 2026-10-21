// SPDX-License-Identifier: GPL-3.0-or-later

//! Properties of the graph on generated input.

use proptest::prelude::*;

use super::{GraphSpec, build, diags, names};
use crate::{PRJ_MAX_FILES, Via};

const NAMES: [&str; 4] = ["a.qc", "b.qc", "sub/c.qc", "d.qh"];

fn lines() -> impl Strategy<Value = String> {
    let line = prop_oneof![
        Just("#include \"a.qc\"".to_owned()),
        Just("#include <sub/c.qc>".to_owned()),
        Just("#includelist".to_owned()),
        Just("#endlist".to_owned()),
        Just("#ifdef X".to_owned()),
        Just("#if X > 1 || !defined(Y)".to_owned()),
        Just("#elif 1".to_owned()),
        Just("#else".to_owned()),
        Just("#endif".to_owned()),
        Just("#define X 2".to_owned()),
        Just("#pragma once".to_owned()),
        Just("/* open".to_owned()),
        Just("close */".to_owned()),
        Just("d.qh b.qc".to_owned()),
        Just("#pragma DONT_COMPILE_THIS_FILE".to_owned()),
        Just("#eof".to_owned()),
        Just("#undef X // c \\".to_owned()),
        Just("string s = R\"x(".to_owned()),
        Just(")x\";".to_owned()),
        "[ -~]{0,20}",
    ];
    prop::collection::vec(line, 0..20).prop_map(|v| v.join("\n"))
}

proptest! {
    // spec PRJ-6.1: on any input the graph ends, every parent comes before its child, and
    // spec PRJ-6.4: the graph never holds more than PRJ_MAX_FILES entries, and
    // spec PRJ-5.13: so under FTEQCC, whose files the directive reader reads
    #[test]
    fn any_input_builds_a_graph(
        root in lines(),
        a in lines(),
        c in lines(),
        bytes in prop::collection::vec(any::<u8>(), 0..200),
    ) {
        let bytes = String::from_utf8_lossy(&bytes).into_owned();
        let dialects = [
            qc_lexer::Dialect::Vanilla,
            qc_lexer::Dialect::Fteqcc,
            qc_lexer::Dialect::Gmqcc,
        ];
        let inputs = [&root, &bytes].into_iter().flat_map(|t| dialects.map(|d| (t, d)));
        for (src_text, dialect) in inputs {
            let files = [
                ("main.src", src_text.as_str()),
                ("a.qc", a.as_str()),
                ("b.qc", ""),
                ("sub/c.qc", c.as_str()),
                ("d.qh", "#pragma once\n"),
            ];
            let graph = build(&GraphSpec {
                files: &files,
                src: "main.src",
                dialect,
                ..GraphSpec::default()
            });
            prop_assert!(graph.files.len() <= PRJ_MAX_FILES);
            for (i, file) in graph.files.iter().enumerate() {
                if let Some(parent) = file.parent {
                    prop_assert!(parent < i);
                }
                prop_assert_eq!(file.via == Via::Root, i == 0 && file.parent.is_none()
                    && graph.form == Some(crate::Form::Source));
            }
        }
    }

    // spec PRJ-4.11: under the vanilla dialect a list with a NUL outside quotes reads as the text
    // before that NUL, whatever follows it
    #[test]
    fn a_vanilla_list_reads_to_its_nul(
        before in prop::collection::vec(
            prop_oneof![
                Just(" "), Just("\n"), Just("\t"), Just("a.qc"), Just("b.qc"), Just("x"),
                Just("// c"), Just("("), Just(":"),
            ],
            0..12,
        ),
        after in "[ -~\n]{0,30}",
    ) {
        let before = format!("out.dat\n{}", before.concat());
        let cut = format!("{before}\0{after}");
        let read = |text: &str| {
            let files = [("progs.src", text), ("a.qc", ""), ("b.qc", "")];
            let graph = build(&GraphSpec { files: &files, ..GraphSpec::default() });
            (graph.form, graph.declared_output.clone(), names(&graph), diags(&graph))
        };
        prop_assert_eq!(read(&cut), read(&before));
    }

    // spec QC-23.3: a list of files with no directives is compiled exactly in the order listed
    #[test]
    fn list_order_is_kept(picks in prop::collection::vec(0..NAMES.len(), 0..30)) {
        let listed: Vec<&str> = picks.iter().map(|&i| NAMES[i]).collect();
        let text = format!("out.dat\n{}\n", listed.join("\n"));
        let files = [
            ("progs.src", text.as_str()),
            ("a.qc", ""),
            ("b.qc", ""),
            ("sub/c.qc", ""),
            ("d.qh", ""),
        ];
        let graph = build(&GraphSpec { files: &files, ..GraphSpec::default() });
        prop_assert_eq!(names(&graph), listed);
        prop_assert!(graph.diagnostics.is_empty());
    }
}

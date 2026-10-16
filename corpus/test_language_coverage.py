# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for corpus/language_coverage.py. Run: cd corpus && python3.11 -m unittest test_language_coverage.py

Everything runs offline on snippets written here; nothing is downloaded.
"""

import contextlib
import io
import tempfile
import unittest
from pathlib import Path

import language_coverage as lc

SPEC = """# Spec

## QC-1 Things

| Construct | Sections |
| --- | --- |
| `float` | QC-1 |
| `;` | QC-1 |
| `\\|\\|` | QC-1, QC-2 |

## QC-2 More
"""


def keys(text: str) -> list[str]:
    return [k for k, _ in lc.constructs(text)]


class Constructs(unittest.TestCase):
    def test_keywords_punctuation_and_names(self):
        self.assertEqual(
            keys("float a, b;\nvoid() f = { if (a || b) return; };"),
            ["float", ",", ";", "void", "(", ")", "=", "{", "if", "(", "||", ")", "return", ";",
             "}", ";"],
        )  # fmt: skip

    def test_longest_punctuation_wins(self):
        self.assertEqual(keys("a <= b == c != d && e ... f"), ["<=", "==", "!=", "&&", "..."])

    def test_minus_before_a_digit_is_a_negative_literal(self):
        self.assertEqual(keys("a-5"), [lc.NEGATIVE])
        self.assertEqual(keys("a - 5"), ["-", lc.FLOAT])
        self.assertEqual(keys("a - b"), ["-"])

    def test_float_literal_takes_digits_and_dots(self):
        self.assertEqual(keys("1.2.3;"), [lc.FLOAT, ";"])

    def test_comments_are_listed_and_skipped(self):
        self.assertEqual(keys("// if\n/* while\n */ do"), [lc.LINE_COMMENT, lc.BLOCK_COMMENT, "do"])

    def test_unterminated_block_comment_ends_the_file(self):
        self.assertEqual(keys("/* if"), [lc.BLOCK_COMMENT])

    def test_line_numbers_count_lines_inside_comments_and_vectors(self):
        self.assertEqual(
            list(lc.constructs("/*\n\n*/ '1\n2 3' ;")),
            [(lc.BLOCK_COMMENT, 1), (lc.VECTOR, 3), (";", 4)],
        )

    def test_string_escapes(self):
        self.assertEqual(keys('"a\\n\\"b"'), [lc.STRING, "escape \\n", 'escape \\"'])

    def test_keywords_inside_strings_and_comments_are_not_counted(self):
        self.assertEqual(keys('"if while" // do'), [lc.STRING, lc.LINE_COMMENT])

    def test_vector_literal(self):
        self.assertEqual(keys("'0 -1 2.5'"), [lc.VECTOR])

    def test_frame_macros(self):
        self.assertEqual(
            keys("$frame walk1 walk2\n$cd /a/b\nx = $walk2;"),
            ["$frame", "$cd", "=", lc.FRAME_REF, ";"],
        )

    def test_frame_macros_are_per_file_and_unknown_dollar_words_are_listed(self):
        self.assertEqual(keys("$modelname foo\nx = $walk1;"), ["$modelname", "=", "$walk1"])

    def test_frame_reference_stops_only_at_blank_comma_semicolon(self):
        self.assertEqual(keys("$frame b\nx = ($b);"), ["$frame", "=", "(", "$b)"])

    def test_vector_component_names(self):
        self.assertEqual(keys("v_x = v_y + vz;"), [lc.COMPONENT, "=", lc.COMPONENT, "+", ";"])

    def test_errors(self):
        for text, want in [
            ("a % b", "unknown punctuation"),
            ('"a\\tb"', "unknown escape"),
            ('"a\nb"', "newline inside quote"),
            ('"ab', "EOF inside quote"),
            ("'1 2 3", "bad vector"),
            ("$ \nfloat", r"hanging \$"),
        ]:
            with self.subTest(text=text), self.assertRaisesRegex(lc.LexError, want):
                keys(text)


class SpecTable(unittest.TestCase):
    def test_rows_and_headings(self):
        rows, headings = lc.spec_table(SPEC)
        self.assertEqual(rows["||"], ["QC-1", "QC-2"])
        self.assertEqual(rows["float"], ["QC-1"])
        self.assertEqual(headings, {"QC-1", "QC-2"})


class Check(unittest.TestCase):
    def test_covered_source_passes(self):
        counts, errs = lc.check({"a.qc": "float a; float b;"}, SPEC)
        self.assertEqual(errs, [])
        self.assertEqual(counts["float"], 2)

    def test_construct_without_a_row_fails(self):
        _, errs = lc.check({"a.qc": "float a;\nvoid b;"}, SPEC)
        self.assertEqual(errs, ["`void` (first at a.qc:2) has no row in the coverage table"])

    def test_row_naming_a_missing_section_fails(self):
        spec = SPEC.replace("| `float` | QC-1 |", "| `float` | QC-7 |")
        _, errs = lc.check({"a.qc": "float a;"}, spec)
        self.assertEqual(errs, ["`float` maps to QC-7, which is not a section of the spec"])

    def test_lex_error_fails(self):
        _, errs = lc.check({"a.qc": "float a % b;"}, SPEC)
        self.assertEqual(errs, ["a.qc: line 1: unknown punctuation '%'"])


class Main(unittest.TestCase):
    def run_main(self, argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = lc.main(argv)
        return code, out.getvalue(), err.getvalue()

    def test_local_directory(self):
        with tempfile.TemporaryDirectory() as d:
            spec = Path(d) / "spec.md"
            spec.write_text(SPEC)
            src = Path(d) / "src"
            src.mkdir()
            (src / "defs.qc").write_text("float a;")
            (src / "notes.txt").write_text("void")
            code, out, _ = self.run_main(["--spec", str(spec), "--source-dir", str(src)])
            self.assertEqual(code, 0)
            self.assertIn("1 .qc files, 2 distinct constructs", out)
            (src / "more.qc").write_text("void b;")
            code, out, err = self.run_main(["--spec", str(spec), "--source-dir", str(src)])
            self.assertEqual(code, 1)
            self.assertIn("MISSING", out)
            self.assertIn("`void` (first at more.qc:1) has no row", err)

    def test_empty_directory(self):
        with tempfile.TemporaryDirectory() as d:
            code, _, err = self.run_main(["--source-dir", d])
            self.assertEqual(code, 1)
            self.assertIn("no .qc files", err)

    def test_missing_directory(self):
        code, _, err = self.run_main(["--source-dir", "/nonexistent/quakec"])
        self.assertEqual(code, 1)
        self.assertIn("error:", err)

    def test_source_dir_is_required(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            lc.main([])

if __name__ == "__main__":
    unittest.main()

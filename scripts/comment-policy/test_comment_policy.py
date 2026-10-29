import contextlib
import io
import json
import tempfile
import subprocess
import sqlite3
import unittest
from pathlib import Path

import comment_policy as policy


class TransformTests(unittest.TestCase):
    def cleaned(self, language, source, **kwargs):
        output, found, notices = policy.transform(source.encode(), language, **kwargs)
        self.assertTrue(found)
        self.assertFalse([comment for comment in policy.comments(output, language) if not policy.syntax_directive(comment, output)])
        self.assertEqual(policy.transform(output, language)[0], output)
        return output.decode()

    def test_rust_nested_comments_raw_strings_lifetimes_and_docs(self):
        source = '//! inner docs\n/// outer docs\nfn f<\'a>(v: &\'a str) -> &\'a str { /* outer /* nested */ tail */ let s = r###"// /* #"###; v } // end\n'
        output = self.cleaned("rust", source)
        self.assertIn('r###"// /* #"###', output)
        self.assertIn("<'a>", output)
        self.assertEqual(source.count("\n"), output.count("\n"))

    def test_python_strings_unicode_continuations_and_docstrings(self):
        source = '#!/usr/bin/env python3\n# coding: utf-8\n"""This is observable __doc__. # kept"""\nvalue = "é#//" # removed\nvalue = (\n  value # explanation\n)\n'
        output = self.cleaned("python", source)
        self.assertTrue(output.startswith("#!/usr/bin/env python3\n# coding: utf-8"))
        self.assertIn('"é#//"', output)
        self.assertIn('observable __doc__', output)

    def test_javascript_regex_templates_and_asi(self):
        source = 'const re = /https?:\\/\\/x/; /* block */\nconst s = `/* text */ ${1 /* nested */ + 2}`;\nfunction f() { return /*\n newline matters */ 1; } // end\n'
        output = self.cleaned("javascript", source)
        self.assertIn('/https?:\\/\\/x/', output)
        self.assertIn('`/* text */', output)
        self.assertEqual(source.count("\n"), output.count("\n"))

    def test_typescript_and_tsx(self):
        for language, source in [("typescript", 'const a: string = "//"; // gone\n'), ("tsx", 'const a = <div>{/* gone */}<span>text</span></div>; // gone\n')]:
            with self.subTest(language=language):
                self.cleaned(language, source)

    def test_toml_multiline_strings(self):
        self.cleaned("toml", 'a = "# string" # gone\nb = """\n# still string\n"""\n# gone\n')

    def test_yaml_scalars(self):
        output = self.cleaned("yaml", 'a: "# string" # gone\nb: |\n  # scalar text\n# gone\n')
        self.assertIn('# scalar text', output)

    def test_bash_heredoc_parameter_substitution_and_shebang(self):
        source = '#!/bin/bash\nx="# string"\ny=${x#prefix}\ncat <<\'EOF\'\n# body\nEOF\n# gone\n'
        output = self.cleaned("bash", source)
        self.assertIn('${x#prefix}', output)
        self.assertIn('# body', output)
        self.assertTrue(output.startswith('#!/bin/bash'))

    def test_css_strings_and_comments(self):
        self.cleaned("css", '/* gone */\na::before { content: "/* text */"; color: red; }')

    def test_html_embedded_languages(self):
        self.cleaned("html", '<!-- gone --><script>const s = "//"; // gone\n</script><style>/* gone */a {color: red}</style>')

    def test_html_comment_removal_does_not_add_visible_whitespace(self):
        output = self.cleaned("html", '<span>a</span><!-- gone --><span>b</span>')
        self.assertEqual(output, '<span>a</span><span>b</span>')

    def test_html_json_script(self):
        self.cleaned("html", '<!-- gone --><script type="application/json">{"a":"//"}</script>')

    def test_sql_literals(self):
        self.cleaned("sql", "SELECT 1; -- gone\n")
        self.cleaned("sql", "SELECT '-- text', '/* text */'; -- gone\n/* gone */")

    def test_json_has_no_comments(self):
        source = b'{"a":"//"}'
        self.assertEqual(policy.transform(source, "json")[0], source)

    def test_tool_directives_removed_and_reportable(self):
        for language, source in [("python", 'x=1 # type: ignore\n'), ("javascript", '// @ts-ignore\nx();'), ("bash", '# shellcheck disable=SC2086\necho $x'), ("javascript", 'const x = /*#__PURE__*/ f();')]:
            with self.subTest(language=language, source=source):
                output, found, notices = policy.transform(source.encode(), language)
                self.assertTrue(any(policy.tooling_directive(comment) for comment in found))
                self.assertFalse(policy.comments(output, language))

    def test_legal_requires_export(self):
        source = b'// SPDX-License-Identifier: MIT\nfn f() {}'
        with self.assertRaisesRegex(policy.Refusal, "legal notice"):
            policy.transform(source, "rust")
        output, found, notices = policy.transform(source, "rust", export_legal=True)
        self.assertEqual(notices, [(1, '// SPDX-License-Identifier: MIT')])
        self.assertNotIn(b'SPDX', output)

    def test_invalid_source_refused(self):
        for language, source in [("rust", b'fn { // incomplete'), ("python", b'x = ( # incomplete'), ("json", b'{ /* gone */ }')]:
            with self.subTest(language=language):
                with self.assertRaises((policy.Refusal, SyntaxError, IndentationError, policy.tokenize.TokenError)):
                    policy.transform(source, language)

    def test_unsupported_source_refused(self):
        with self.assertRaisesRegex(policy.Refusal, "unsupported"):
            policy.language_for(Path('unknown.vue'), b'<template/>')

    def test_rust_directive_mentions_are_prose(self):
        self.cleaned("rust", "/// Uses std::fmt::Display and type: ignore text\nfn f() {}")

    def test_jsonc(self):
        self.cleaned("jsonc", '{"a": "//" /* gone */} // gone\n')

    def test_notebook_cells_preserve_data_and_docstrings(self):
        notebook = {"cells": [{"cell_type": "markdown", "source": ["# prose"]}, {"cell_type": "code", "source": ["\"\"\"doc\"\"\"\n", "x = \"# literal\" # gone\n"], "outputs": [{"text": "# output"}]}], "metadata": {"kernelspec": {"language": "python"}}, "nbformat": 4}
        output, found, notices = policy.transform(json.dumps(notebook).encode(), "notebook")
        result = json.loads(output)
        self.assertEqual(result["cells"][0], notebook["cells"][0])
        self.assertEqual(result["cells"][1]["outputs"], notebook["cells"][1]["outputs"])
        self.assertIn('"""doc"""', ''.join(result['cells'][1]['source']))
        self.assertTrue(found)
        self.assertEqual(policy.transform(output, 'notebook')[0], output)

    def test_notebook_magic_refused(self):
        notebook = {"cells": [{"cell_type": "code", "source": ["%time x()\n"]}]}
        with self.assertRaisesRegex(policy.Refusal, "notebook cell"):
            policy.transform(json.dumps(notebook).encode(), "notebook")

    def test_unicode_javascript_line_terminators(self):
        for ending in ("\u2028", "\u2029"):
            source = f"function f() {{ return /* {ending} */ 7; }}"
            output = self.cleaned("javascript", source)
            self.assertIn(ending, output)
            self.assertEqual(policy.fingerprint(source.encode(), 'javascript'), policy.fingerprint(output.encode(), 'javascript'))

    def test_contiguous_legal_header_is_exported_in_full(self):
        source = b'// Derived algorithm\n// Copyright Author\n// THE SOFTWARE IS PROVIDED AS IS\n// NO WARRANTY\nfn f() {}\n'
        output, found, notices = policy.transform(source, 'rust', export_legal=True)
        self.assertEqual(notices, [(1, source[:source.index(b'\nfn f')].decode())])
        self.assertNotIn(b'WARRANTY', output)

    def test_removed_comments_do_not_leave_trailing_whitespace(self):
        output = self.cleaned('rust', 'fn f() { // gone\n    // gone\n    let s = " "; /* gone */\n}\n')
        self.assertFalse(any(line.endswith((' ', '\t')) for line in output.splitlines()))
        self.assertIn('" "', output)

    def test_cargo_lock_and_original_manifests_are_toml(self):
        for name in ('Cargo.lock', 'uv.lock', 'Cargo.toml.orig'):
            with self.subTest(name=name):
                self.assertEqual(policy.language_for(Path(name), b'# generated\nversion = 4'), 'toml')
                self.cleaned('toml', '# generated\nversion = 4')

    def test_make_and_docker(self):
        for name, source in [('Makefile', '# gone\nall:\n\techo "# literal" # gone\n'), ('Dockerfile', '# gone\nFROM alpine\nRUN echo "# literal"\n')]:
            with self.subTest(name=name):
                language = policy.language_for(Path(name), source.encode())
                self.cleaned(language, source)

    def test_comment_free_sql_dialect_needs_no_transformation(self):
        source = b'SELECT a//2 FROM t;'
        self.assertTrue(policy.sql_comment_free(source, 'sql'))
        self.assertEqual(policy.transform(source, 'sql')[0], source)

    def test_crlf(self):
        source = b'fn f() {} // gone\r\n'
        output, found, notices = policy.transform(source, 'rust')
        self.assertEqual(source.count(b'\r\n'), output.count(b'\r\n'))


class CliTests(unittest.TestCase):
    def call(self, args):
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            status = policy.main(args)
        return status, json.loads(stream.getvalue())

    def test_check_write_then_clean_and_legal_export(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'example.rs'
            notice = root / 'NOTICES.md'
            original = b'// Copyright Example\nfn main() {} // gone\n'
            source.write_bytes(original)
            status, report = self.call(['--notices', str(notice), str(source)])
            self.assertEqual(status, 1)
            self.assertEqual(source.read_bytes(), original)
            self.assertFalse(notice.exists())
            status, report = self.call(['--write', '--notices', str(notice), str(source)])
            self.assertEqual(status, 0)
            self.assertIn('Copyright Example', notice.read_text())
            first = source.read_bytes()
            status, report = self.call([str(source)])
            self.assertEqual(status, 0)
            self.assertEqual(source.read_bytes(), first)

    def test_refusal_prevents_all_writes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            good = root / 'a.rs'
            bad = root / 'b.rs'
            good.write_bytes(b'fn f() {} // gone\n')
            bad.write_bytes(b'fn { // invalid')
            before = good.read_bytes()
            status, report = self.call(['--write', str(good), str(bad)])
            self.assertEqual(status, 2)
            self.assertEqual(good.read_bytes(), before)


if __name__ == '__main__':
    unittest.main()


class ConfigurationTests(unittest.TestCase):
    def test_config_classification(self):
        for name, language in [("Makefile.extra", "make"), ("Dockerfile.dockerignore", "ignore"), (".gitignore", "ignore"), (".gitattributes", "attributes"), ("x.service", "systemd"), ("x.timer", "systemd"), (".env.example", "bash"), ("x.env.example", "bash"), ("x.toml.in", "toml"), ("x.js.tpl", "javascript")]:
            self.assertEqual(policy.language_for(Path(name), b""), language)

    def test_gitignore_matches_preserved(self):
        source = b"# gone\n\\#literal\n leading\n*.log\n!keep.log\n"
        output, found, notices = policy.transform(source, "ignore")
        self.assertEqual(len(found), 1)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", directory], check=True)
            def matches(data):
                (root / ".gitignore").write_bytes(data)
                result = subprocess.run(["git", "check-ignore", "--no-index", "--stdin"], cwd=root, input=b"#literal\n leading\nleading\na.log\nkeep.log\n", capture_output=True)
                self.assertEqual(result.returncode, 0)
                return result.stdout
            self.assertEqual(matches(source), matches(output))
            self.assertEqual(matches(output), b"#literal\n leading\na.log\n")

    def test_gitattributes_preserves_literal_hash_pattern(self):
        source = b"  # gone\n\\#literal text\n*.rs -text\n"
        output, found, notices = policy.transform(source, "attributes")
        self.assertEqual(len(found), 1)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", directory], check=True)
            def attrs(data):
                (root / ".gitattributes").write_bytes(data)
                return subprocess.check_output(["git", "check-attr", "text", "--", "#literal", "x.rs"], cwd=root)
            self.assertEqual(attrs(source), attrs(output))

    def test_systemd_literals_and_full_line_comments(self):
        source = b"# gone\n[Service]\n ; gone\nEnvironment=VALUE=#literal;kept\nExecStart=/bin/echo '#literal'\n"
        output, found, notices = policy.transform(source, "systemd")
        self.assertEqual(len(found), 2)
        self.assertIn(b"Environment=VALUE=#literal;kept", output)
        self.assertEqual(policy.fingerprint(source, "systemd"), policy.fingerprint(output, "systemd"))

    def test_encoding_exemption_only_real_pep263_position(self):
        for source, kept in [(b"# coding: utf-8\nx=1\n", True), (b"#!/usr/bin/python\n# coding: utf-8\nx=1\n", True), (b"x=1\n# coding: utf-8\n", False), (b"\n\n# coding: utf-8\nx=1\n", False), (b"# prose\n# prose\n# coding: utf-8\n", False)]:
            output, found, notices = policy.transform(source, "python")
            self.assertEqual(b"coding:" in output, kept)


class SqlDialectTests(unittest.TestCase):
    def test_duckdb_literals_nested_comments_and_casts(self):
        source = b"-- gone\nSELECT a::VARCHAR, $$-- literal$$, $tag$/* literal */$tag$, '--''kept', \"#id\", E'escaped\\\\--x'; /* outer /* nested */ gone */"
        output, found, notices = policy.transform(source, "sql")
        self.assertEqual(len(found), 2)
        self.assertEqual(policy.fingerprint(source, "sql"), policy.fingerprint(output, "sql"))
        self.assertIn(b"$tag$/* literal */$tag$", output)

    def test_sql_unclosed_delimiters_refused(self):
        for source in [b"SELECT 'unterminated", b'SELECT "unterminated', b"SELECT $tag$unterminated", b"/* outer /* nested */"]:
            with self.assertRaises(policy.Refusal):
                policy.transform(source, "sql")

    def test_sql_comment_separates_identifiers(self):
        output, found, notices = policy.transform(b"SELECT a/* gone */b;", "sql")
        self.assertEqual(policy.fingerprint(output, "sql"), (b"SELECT", b"a", b"b", b";"))


class MakeDialectTests(unittest.TestCase):
    def test_real_make_preserves_expansion_quotes_escaped_hash_and_recipe(self):
        source = b"# gone\nLITERAL := \\#kept\nVALUE := $(shell printf '%s' '#literal') # gone\nall:\n\t@printf '%s|%s|%s' '$(LITERAL)' '$(VALUE)' '#recipe literal' # gone\n"
        output, found, notices = policy.transform(source, "make")
        self.assertEqual(len(found), 3)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def execute(data):
                (root / "Makefile").write_bytes(data)
                return subprocess.check_output(["make", "--no-print-directory", "-s", "all"], cwd=root)
            self.assertEqual(execute(source), execute(output))
            self.assertEqual(execute(output), b"#kept|#literal |#recipe literal")

    def test_real_make_oneshell_heredoc_data_preserved(self):
        source = b".ONESHELL:\nall:\n\t@cat <<'END' # gone\n\t# literal body\n\tEND\n\t# gone\n"
        output, found, notices = policy.transform(source, "make")
        self.assertEqual(len(found), 2)
        self.assertIn(b"# literal body", output)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def execute(data):
                (root / "Makefile").write_bytes(data)
                return subprocess.check_output(["make", "--no-print-directory", "-s", "all"], cwd=root)
            self.assertEqual(execute(source), execute(output))
            self.assertEqual(execute(output), b"# literal body\n")

    def test_make_opaque_awk_without_comment_marker(self):
        source = b"VALUE = $(strip $(shell awk 'BEGIN { print (1 + 2) }'))\n# gone\nall:\n\t@printf '%s' '$(VALUE)'\n"
        output, found, notices = policy.transform(source, "make")
        self.assertEqual(len(found), 1)
        self.assertIn(b"awk 'BEGIN { print (1 + 2) }'", output)

    def test_make_unsupported_define_refuses(self):
        with self.assertRaises(policy.Refusal):
            policy.transform(b"define X\n# literal\nendef\n", "make")


class AdditionalFormatTests(unittest.TestCase):
    def test_svg_preserves_xml_declaration_text_and_paths(self):
        source = b'<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"><!-- gone --><text>a<!-- gone -->b</text><path d="M 0 0"/></svg>'
        output, found, notices = policy.transform(source, "xml")
        self.assertEqual(len(found), 2)
        self.assertTrue(output.startswith(b'<?xml version="1.0"?>'))
        self.assertIn(b'<text>ab</text>', output)
        self.assertEqual(policy.fingerprint(source, "xml"), policy.fingerprint(output, "xml"))

    def test_doxygen_quoted_hash_and_syncthing_include(self):
        output, found, notices = policy.transform(b'# gone\nINPUT = "#literal" src # gone\n', "doxygen")
        self.assertEqual(len(found), 2)
        self.assertIn(b'"#literal"', output)
        output, found, notices = policy.transform(b' // gone\n#include defaults\n(?d)cache\n', "syncthing")
        self.assertEqual(len(found), 1)
        self.assertIn(b'#include defaults', output)

    def test_sql_cr_terminates_comment(self):
        output, found, notices = policy.transform(b"-- gone\rSELECT 1;", "sql")
        self.assertIn(b"SELECT 1;", output)
        self.assertEqual(len(found), 1)

    def test_make_preserves_blank_heredoc_line(self):
        source = b".ONESHELL:\nall:\n\t@cat <<'END' # gone\n\t  \n\tEND\n"
        output, found, notices = policy.transform(source, "make")
        self.assertIn(b"\t  \n", output)


class SafetyTests(unittest.TestCase):
    def test_sqlite_backtick_names_execute_identically(self):
        source = b"CREATE TABLE t (`a--b` INTEGER, `c/*d*/` TEXT, `e``f` INTEGER); INSERT INTO t VALUES (7, 'ok', 9); SELECT `a--b`, `c/*d*/`, `e``f` FROM t; -- gone\n"
        output, found, notices = policy.transform(source, "sql")
        self.assertEqual(len(found), 1)
        def execute(data):
            database = sqlite3.connect(":memory:")
            statements = data.decode().split(';')
            for statement in statements[:2]:
                database.execute(statement)
            return database.execute(statements[2]).fetchall()
        self.assertEqual(execute(source), execute(output))
        self.assertEqual(execute(output), [(7, 'ok', 9)])

    def test_ambiguous_sqlite_bracket_name_refuses_without_write(self):
        source = b"SELECT 7 AS [a--b];"
        self.assertEqual(sqlite3.connect(":memory:").execute(source.decode()).fetchone(), (7,))
        with self.assertRaisesRegex(policy.Refusal, "ambiguous SQL bracket"):
            policy.transform(source, "sql")

    def test_unterminated_html_comment_refused(self):
        with self.assertRaisesRegex(policy.Refusal, "unterminated HTML comment"):
            policy.transform(b"<div>before</div><!-- retained prose", "html")

    def test_tracked_common_source_cannot_bypass_selector(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", directory], check=True)
            for name, source in [("future.c", b"int main(void) {return 0;} // gone\n"), ("future.vue", b"<template><!-- gone --></template>"), ("script.custom", b"#!/usr/bin/ruby\n# gone\n")]:
                (root / name).write_bytes(source)
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            self.assertEqual({path.name for path in policy.tracked_sources(root)}, {"future.c", "future.vue", "script.custom"})
            self.assertTrue(policy.transform((root / "future.c").read_bytes(), "c")[1])
            with self.assertRaises(policy.Refusal):
                policy.language_for(root / "future.vue", (root / "future.vue").read_bytes())


class HtmlAndSqlSafetyTests(unittest.TestCase):
    def test_rcdata_and_rawtext_comment_looking_data_preserved(self):
        for tag in ("textarea", "title", "xmp", "iframe", "noembed", "noframes"):
            source = f"<{tag}><!-- literal --></{tag}>".encode()
            self.assertEqual(policy.transform(source, "html")[0], source)

    def test_sql_hash_dialect_refused(self):
        with self.assertRaises(policy.Refusal):
            policy.transform(b"SELECT 1; # mysql prose", "sql")


class WorkflowTests(unittest.TestCase):
    def test_literal_run_shell_comments_and_expression_strings(self):
        source = b"jobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          echo '#literal'\n          echo ${{ contains(github.ref, '#}}literal') }} # gone\n          # gone\n        shell: bash\n"
        output, found, notices = policy.transform(source, "workflow")
        self.assertEqual(len(found), 2)
        self.assertIn(b"'#literal'", output)
        self.assertIn(b"'#}}literal'", output)
        self.assertEqual(policy.fingerprint(source, "workflow"), policy.fingerprint(output, "workflow"))
        self.assertEqual(policy.transform(output, "workflow")[0], output)

    def test_workflow_quoted_or_unsupported_shell_refuses(self):
        for source in [b"run: \"echo x # gone\"\n", b"run: >\n  echo x # gone\n", b"run: |\n  echo x # gone\nshell: pwsh\n"]:
            with self.assertRaises(policy.Refusal):
                policy.transform(source, "workflow")

    def test_workflow_noncode_scalar_remains_data(self):
        source = b"env:\n  DESCRIPTION: |\n    # prose data\nsteps:\n  - run: echo ok\n"
        self.assertEqual(policy.transform(source, "workflow")[0], source)


class InterpreterDirectiveTests(unittest.TestCase):
    def test_docker_frontend_and_escape_are_initial_parser_syntax(self):
        source = b"# syntax=docker/dockerfile:1.7\n# escape=\\\n# prose gone\nFROM alpine\nRUN --mount=type=cache,target=/tmp echo ok\n"
        output, found, notices = policy.transform(source, "dockerfile")
        self.assertEqual(len(found), 1)
        self.assertTrue(output.startswith(b"# syntax=docker/dockerfile:1.7\n# escape=\\\n"))
        self.assertIn(b"RUN --mount=type=cache", output)

    def test_docker_disabled_or_lint_directives_are_removed(self):
        for source in [b"\n# syntax=not-effective\nFROM alpine\n", b"# prose\n# syntax=not-effective\nFROM alpine\n", b"# check=skip=all\nFROM alpine\n"]:
            output, found, notices = policy.transform(source, "dockerfile")
            self.assertNotIn(b"#", output)

    def test_symlink_ancestor_refuses_before_any_write(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outside = root / "outside"
            outside.mkdir()
            source = outside / "a.py"
            source.write_bytes(b"x=1 # keep until refusal\n")
            (root / "linked").symlink_to(outside, target_is_directory=True)
            with contextlib.redirect_stdout(io.StringIO()):
                status = policy.main([str(root / "linked" / "a.py"), "--write"])
            self.assertEqual(status, 2)
            self.assertIn(b"# keep until refusal", source.read_bytes())

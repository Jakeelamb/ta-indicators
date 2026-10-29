# Code comment policy tool

`comment_policy.py` checks source by default. Exit 0 means no comments; 1 means comments remain; 2 means a source could not be safely processed. `--write` removes comments only after every selected file passes preflight. `--notices` exports complete contiguous legal comment groups before source changes. Raw hashes, comment counts, removed Rust documentation counts and removed tooling-directive counts are printed as JSON.

Portable check, independent of repository Python configuration:

```sh
uv run --python 3.12 --no-project --with tree-sitter==0.25.2 --with tree-sitter-language-pack==0.13.0 python /path/to/comment_policy.py --repo /path/to/repository --notices /path/to/repository/SOURCE_NOTICES.md
```

Append `--write` to perform the authorized removal. Explicit source paths may replace `--repo`; unsupported explicit source languages are refused. `--repo` selects tracked recognized source extensions, extensionless shebang scripts, Makefiles, Dockerfiles, Cargo.lock, uv.lock and original TOML manifests. Removed tracked files are skipped; untracked files require explicit paths or addition to Git. Documentation, patches and other non-source files are outside that selector.

Verification:

```sh
uv run --python 3.12 --project /path/to/tool python -m unittest discover -s /path/to/tool -v
```

The parser policy preserves executable shebangs, eligible first/second-line Python encoding declarations and effective initial Docker syntax/escape parser directives as language syntax. Docker check/lint suppressions remain removable comments. Python docstrings and comment-looking strings remain data. Suppression comments and Rust documentation comments are removed; runtime Python AST equality does not qualify static typing or documentation/doctest preservation. The report identifies both intentional changes.

Rust, C/C++, Go, Java, Kotlin, Swift, Lua, Protocol Buffers, JavaScript, TypeScript, TSX, CSS, shell, TOML, YAML and Dockerfile use pinned tree-sitter grammars and compare the complete syntax structure and leaf bytes excluding comments. Python uses tokenize and runtime AST equality. HTML uses HTMLParser events and embedded JavaScript/CSS parsers. TOML also compares decoded values. JSON is checked with the standard JSON decoder; JSONC permits comment tokens. Notebook code cells are processed in their declared language; markdown, outputs and metadata remain data. Unsupported magics are refused.

SQL uses a bounded dialect lexer that preserves complete quoted and dollar literals, nested block comments and outside-comment tokens. Backtick identifiers retain doubled escapes. Bracket syntax containing possible comment delimiters is ambiguous across supported dialects and refuses; hash comments/operators also refuse. This is lexical equivalence, not SQL compilation acceptance. SQLite producer/control fixtures execute actual statements.

Make uses a bounded lexer plus Bash parsing for recipes and shell expansions containing possible comment markers. Opaque comment-free expressions remain exact bytes. Existing GNU Make fixtures verify escaped hashes, shell expansions, quoted recipe text and heredoc data. Define bodies and custom recipe prefixes refuse. Significant spaces before inline Make comments remain significant and are preserved.

Ignore files, Git attributes and Syncthing ignore configuration use their full-line comment rules and preserve all pattern bytes; real Git fixtures check matching. Systemd units preserve complete non-comment lines and parse configuration before removal. Environment templates use Bash syntax. Doxyfile uses bounded quoted-value-aware hash comments. SVG/XML uses Expat comment spans and an independent ElementTree structure comparison. Python regression seeds, license files and other data/documents remain data.

GitHub workflow YAML additionally inspects literal Bash/sh `run` blocks, preserving GitHub expression data. Quoted/folded run scalars with possible comments and unsupported or ambiguous shells refuse. Arbitrary YAML data scalars remain data. Common source extensions select supported parsers or fail closed with an unsupported-source error; tracked shebang programs are selected regardless of suffix. Source symlinks and symlink ancestors refuse. This finite selector does not infer arbitrary unknown text formats as executable source.

This tool does not certify original source compilation, runtime behavior of removed tooling directives, or financial behavior. Its proof concerns source structure outside comments and the explicit documentation/directive removal policy. Run the owning project's checks after migration.

HTML/XML comments are deleted without adding rendered whitespace. HTML RCDATA/rawtext preserves comment-looking literal text, and unsupported or unterminated comment syntax refuses. Other comments are masked with spaces while preserving CR/LF and JavaScript Unicode line terminators; trailing horizontal whitespace on touched lines is removed. A source mismatch, parser failure or changed non-comment structure refuses the whole batch before writes. File-system write failures can leave a partially completed batch; Git and the JSON hashes are the recovery record. One coordinator must own mutation; this is not a concurrent-write protocol.

Integration consists of the Python source, its fixture tests and this documentation. The included pyproject.toml/uv.lock supply the isolated development test environment; the portable production command above pins its two parser dependencies directly. Build outputs, virtual environments and evidence stay outside the repository.

# Agent instructions

Read `/home/jake/Projects/agent-scripts/AGENTS.MD` first when it exists.

## Source comment policy

Do not add code comments, prose comments, documentation comments, or comment-based lint/type suppressions. This applies to tracked source, scripts, manifests and configuration, including vendored source. Keep explanations in standalone Markdown documentation. Required interpreter/parser and encoding directives are syntax: preserve executable shebangs, eligible Python encoding declarations and effective initial Docker syntax/escape directives. Python docstrings remain executable strings used by `__doc__` and CLI help. Preserve full license files and move legal source notices to `SOURCE_NOTICES.md`. Do not replace removed comments with Rust doc attributes. Removal intentionally changes Rust documentation/doctests and comment-based tooling directives.

Run the portable check in `scripts/comment-policy/README.md` before committing. CI enforces the same parser-backed policy. Work only in the canonical checkout with bounded ownership; serialize writers through one coordinator. Do not create or retain linked Git worktrees.

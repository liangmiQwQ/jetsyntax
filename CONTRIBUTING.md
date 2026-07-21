# Contributing

JetSyntax is an agent-driven experiment and is not maintained. Issues, support requests, and pull requests may be closed without response.

For reproducibility work, use Rust 1.96.1, Node.js 22.18 or newer, and pnpm 10.34.4. Run `just check` and `just test` before committing. Commit and pull-request titles must follow Conventional Commits.

Conformance changes must report discovered, executed, passed, failed, and skipped counts. A change may not hide a failure by filtering, deleting, or silently skipping a fixture.

Benchmark changes must retain the exact fixture pins and checksums, materialize returned ASTs, and publish raw JSON. Do not update README performance claims from smoke runs or shared CI machines.

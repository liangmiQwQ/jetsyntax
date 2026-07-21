# JetSyntax

JetSyntax is an experimental, independently implemented JavaScript, TypeScript, JSX, and TSX parser written in Rust. It exposes the same parser as a native Rust crate and as a NAPI package that returns an ESTree-compatible AST.

> [!WARNING]
> JetSyntax is an agent-driven research project. It is incomplete, is not production-ready, will not be maintained, and comes with no support commitment.

## Status

The repository has the required language modes, native API, NAPI transfer layer, ESTree decoder, conformance harnesses, and native/NAPI benchmark harnesses. Full grammar conformance and the performance target are still in progress.

The latest complete, correctly wired official-suite baseline was captured at [`57e0ffb`](https://github.com/liangmiQwQ/jetsyntax/commit/57e0ffb) in [GitHub Actions run 29872963198](https://github.com/liangmiQwQ/jetsyntax/actions/runs/29872963198):

| Suite                 | Passed | Failed | Unsupported | Executed | Skipped |
| --------------------- | -----: | -----: | ----------: | -------: | ------: |
| Test262               | 98,994 |  3,603 |           0 |  102,597 |       0 |
| TypeScript            | 11,266 |  9,476 |           0 |   20,742 |       0 |
| Babel parser fixtures |  3,515 |  1,377 |       1,044 |    5,936 |       0 |

These are development numbers, not a conformance claim. The table remains pinned to a reproducible full run until it is replaced by a newer complete run. CI enumerates every pinned case and rejects missing or skipped fixtures.

## Architecture

JetSyntax is built around a compact parser-owned wire format:

1. An on-demand lexer scans only as the recursive-descent and Pratt parsers request tokens. Specialized rescans handle regular expressions, templates, and JSX text.
2. Grammar state, scopes, labels, private names, and speculative checkpoints are kept in compact parser contexts. Recovery rolls the parser and output tape back together.
3. The parser emits an append-only postfix tape of 32-bit words. Child records always precede their parent, and a reference marker plus scalar edge count proves at construction time that every non-root record has exactly one parent.
4. Native Rust callers can consume the construction-proven tape directly. NAPI moves its word vector into a `Uint32Array`; a handwritten JavaScript decoder materializes ESTree without serializing a Rust AST through JSON or Serde.

The postfix layout keeps native output independent from Rust struct layout and gives language bindings a stable boundary. Parser finalization is O(1), and the native random-access record index is initialized lazily only if requested. The public untrusted tape constructor still performs full structural, reference, reachability, marker, and UTF-8 validation.

JetSyntax's product parser does not depend on Yuku, OXC, or SWC. Those projects are development-only benchmark competitors.

## JavaScript API

Requirements: Node.js 20.19 or newer and a platform supported by the generated NAPI binary.

```js
import { parse } from "jetsyntax";

const result = parse("const answer: number = 42", {
  lang: "ts",
  sourceType: "module",
  range: true,
});

console.log(result.program); // ESTree Program with Babel-style TypeScript nodes
console.log(result.diagnostics);
```

`lang` accepts `js`, `jsx`, `ts`, `tsx`, or `dts`. `sourceType` accepts `script`, `module`, `unambiguous`, or `commonjs`.

## Native Rust API

```rust
use jetsyntax::{Language, ParseOptions, SourceKind, parse};

let result = parse(
    "export const answer = 42;",
    ParseOptions {
        language: Language::JavaScript,
        source_kind: SourceKind::Module,
        ..ParseOptions::default()
    },
)?;

assert!(result.diagnostics.is_empty());
let words: &[u32] = result.tape.words();
# Ok::<(), jetsyntax::ParseError>(())
```

The Rust crate returns the native postfix tape. ESTree materialization currently lives in the JavaScript package.

## Development

The workspace pins Rust 1.96.1, pnpm 10.34.4, and the CI Node.js runtime. With `just` installed:

```sh
pnpm install --frozen-lockfile
just check
just test
```

Useful direct commands:

```sh
pnpm build
cargo test -p jetsyntax
pnpm --filter jetsyntax test
```

## Conformance

Official suites are pinned by immutable revisions in [`tasks/conformance/official-refs.json`](tasks/conformance/official-refs.json):

- Test262: 102,597 strict/non-strict/module executions across 53,414 files.
- TypeScript: 20,742 executions derived from the official parser cases and their configurations.
- Babel parser: 5,936 enabled parser fixtures, including success, recovery, and fatal cases.

The GitHub Actions matrix runs all three suites with zero silent skips. Local runners accept an already checked-out suite root; for example:

```sh
pnpm build
pnpm --filter @jetsyntax/conformance test:official -- \
  --suite test262 \
  --root /absolute/path/to/test262/test \
  --ref 9e61c12835c5e4a3bdba93850427e6742c4f64c4
```

## Benchmarks

The benchmark gate requires JetSyntax to be at least 10% faster than pinned Yuku on every required fixture, independently on the native and NAPI paths. OXC and SWC are included as additional comparisons.

| Fixture                                                |     Bytes | SHA-256                                                            |
| ------------------------------------------------------ | --------: | ------------------------------------------------------------------ |
| `npm:typescript@5.1.6/lib/typescript.js`               | 8,207,497 | `804f9c1b6c64568c39dd48eee88b77ba92d0b5d0f44f425bc96bcfe052824644` |
| `microsoft/TypeScript@c9e7428/src/compiler/checker.ts` | 3,151,774 | `ffe288edd0eae68f65e4b81b5bbfd4fe5fbed62b55246dc140813078555050fb` |
| `npm:react@17.0.2/cjs/react.development.js`            |    72,141 | `ec670cc82d2aac81844bae49353d11bef1a8a21e727290a3bcc24a2928839496` |

### Current result

| Path   | Result                                                                                          |
| ------ | ----------------------------------------------------------------------------------------------- |
| Native | Not publishable: the correctness preflight still finds parser diagnostics on required fixtures. |
| NAPI   | Not publishable: the correctness preflight still finds parser diagnostics on required fixtures. |

No throughput or speedup number is published while that gate is blocked. This deliberately prevents a fast recovery parse from being presented as a valid benchmark result.

Both harnesses use 50 warmups and 300 measured samples by default, report median/minimum/p99 latency, verify fixture checksums and competitor versions, materialize parser output, write raw JSON under `reports/`, and enforce the per-fixture Yuku threshold when `BENCH_ENFORCE=1`. NAPI samples are interleaved in rotating parser order. Native Rust parsers share one process; pinned native Yuku runs in a separate Zig process with startup excluded.

```sh
pnpm build
pnpm bench:napi

YUKU_DIR=/absolute/path/to/yuku pnpm bench:native
```

The benchmark workflow uses Yuku revision `217133c5db1cb65bc8f0a44281c505cd46fa7a96`, OXC `0.140.0`, and SWC `1.15.46`. Only correctness-gated, reproducible results may replace the table above.

## License

[MIT](LICENSE). Third-party benchmark and tooling notices are recorded in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

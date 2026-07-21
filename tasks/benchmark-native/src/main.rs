use std::{
    env,
    fmt::Write as _,
    fs,
    hint::black_box,
    path::Path,
    time::{Duration, Instant},
};

use jetsyntax::{Language, ParseOptions, SourceKind};
use mimalloc_safe::MiMalloc;
use oxc::{allocator::Allocator as OxcAllocator, parser::Parser as OxcParser, span::SourceType};
use swc_common::BytePos;
use swc_ecma_parser::{EsSyntax, Parser as SwcParser, StringInput, Syntax, TsSyntax};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const DEFAULT_WARMUPS: usize = 50;
const DEFAULT_SAMPLES: usize = 300;

struct Stats {
    median: Duration,
    minimum: Duration,
    p99: Duration,
}

fn main() {
    let warmups = environment_usize("BENCH_WARMUPS", DEFAULT_WARMUPS);
    let samples = environment_usize("BENCH_SAMPLES", DEFAULT_SAMPLES);
    let mut output =
        format!("{{\"methodology\":{{\"warmups\":{warmups},\"samples\":{samples}}},\"results\":[");
    let mut first = true;

    for path in env::args().skip(1) {
        let source =
            fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
        let language = if Path::new(&path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ts"))
        {
            Language::TypeScript
        } else {
            Language::JavaScript
        };
        let source_type = SourceType::from_path(&path)
            .unwrap_or_default()
            .with_module(true);
        let cases = [
            (
                "JetSyntax",
                benchmark(warmups, samples, || parse_jetsyntax(&source, language)),
            ),
            (
                "OXC",
                benchmark(warmups, samples, || parse_oxc(&source, source_type)),
            ),
            (
                "SWC",
                benchmark(warmups, samples, || parse_swc(&source, language)),
            ),
        ];

        for (parser, stats) in cases {
            if !first {
                output.push(',');
            }
            first = false;
            // SAFETY: formatting into a String cannot fail.
            write!(
                output,
                "{{\"fixture\":\"{}\",\"bytes\":{},\"parser\":\"{parser}\",\"medianMs\":{},\"minimumMs\":{},\"p99Ms\":{}}}",
                Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown"),
                source.len(),
                milliseconds(stats.median),
                milliseconds(stats.minimum),
                milliseconds(stats.p99),
            )
            .expect("String formatting must succeed");
            eprintln!(
                "{} {parser}: {:.3} ms",
                Path::new(&path).display(),
                milliseconds(stats.median)
            );
        }
    }

    output.push_str("]}");
    println!("{output}");
}

fn benchmark(warmups: usize, samples: usize, mut parse: impl FnMut() -> Duration) -> Stats {
    assert!(samples > 0, "BENCH_SAMPLES must be positive");
    for _ in 0..warmups {
        black_box(parse());
    }

    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        timings.push(parse());
    }
    timings.sort_unstable();

    Stats {
        median: timings[samples / 2],
        minimum: timings[0],
        p99: timings[(samples * 99 / 100).min(samples - 1)],
    }
}

fn parse_jetsyntax(source: &str, language: Language) -> Duration {
    let start = Instant::now();
    let result = jetsyntax::parse(
        black_box(source),
        ParseOptions {
            language,
            source_kind: SourceKind::Module,
            ..ParseOptions::default()
        },
    )
    .expect("JetSyntax parse should fit the wire format");
    let elapsed = start.elapsed();
    assert!(
        result.diagnostics.is_empty(),
        "JetSyntax must parse benchmark fixtures without diagnostics"
    );
    black_box(result.tape.words().len());
    elapsed
}

fn parse_oxc(source: &str, source_type: SourceType) -> Duration {
    let allocator = OxcAllocator::default();
    let start = Instant::now();
    let result = OxcParser::new(&allocator, black_box(source), source_type).parse();
    let elapsed = start.elapsed();
    assert!(result.diagnostics.is_empty());
    black_box(result.program.body.len());
    elapsed
}

fn parse_swc(source: &str, language: Language) -> Duration {
    let syntax = if language == Language::TypeScript {
        Syntax::Typescript(TsSyntax::default())
    } else {
        Syntax::Es(EsSyntax::default())
    };
    // SAFETY: the pinned benchmark fixtures are all smaller than four GiB.
    let source_length = u32::try_from(source.len()).expect("fixture must fit SWC's byte position");
    let input = StringInput::new(source, BytePos(0), BytePos(source_length));
    let start = Instant::now();
    let result = SwcParser::new(syntax, black_box(input), None).parse_module();
    let elapsed = start.elapsed();
    assert!(result.is_ok());
    let _ = black_box(result);
    elapsed
}

fn environment_usize(name: &str, default: usize) -> usize {
    env::var(name).map_or(default, |value| {
        value
            .parse()
            .unwrap_or_else(|error| panic!("invalid {name}: {error}"))
    })
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

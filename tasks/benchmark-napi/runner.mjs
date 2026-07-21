import { performance } from "node:perf_hooks";

export function measureInterleaved(parsers, { initialOffset = 0, now = performance.now, samples, warmups }) {
  if (parsers.length === 0) throw new Error("at least one parser is required");
  const timings = parsers.map(() => new Float64Array(samples));

  for (let round = 0; round < warmups; round++) {
    runRound(parsers, initialOffset + round, (parser) => parser.run());
  }

  for (let sample = 0; sample < samples; sample++) {
    runRound(parsers, initialOffset + sample, (parser, parserIndex) => {
      const start = now();
      parser.run();
      timings[parserIndex][sample] = now() - start;
    });
  }

  return new Map(parsers.map((parser, index) => [parser.name, statistics(timings[index])]));
}

function runRound(parsers, offset, run) {
  for (let index = 0; index < parsers.length; index++) {
    const parserIndex = (offset + index) % parsers.length;
    run(parsers[parserIndex], parserIndex);
  }
}

function statistics(timings) {
  timings.sort();
  return {
    medianMs: timings[Math.floor(timings.length / 2)],
    minimumMs: timings[0],
    p99Ms: timings[Math.min(timings.length - 1, Math.floor(timings.length * 0.99))],
  };
}

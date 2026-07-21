export function compareWithYuku(results, thresholdPercent = 10) {
  const fixtures = [...new Set(results.map((result) => result.fixture))];
  return fixtures.map((fixture) => {
    const jetSyntax = uniqueResult(results, fixture, "JetSyntax");
    const yuku = uniqueResult(results, fixture, "Yuku");
    const fasterPercent = (yuku.medianMs / jetSyntax.medianMs - 1) * 100;
    return {
      fixture,
      jetSyntaxMedianMs: jetSyntax.medianMs,
      yukuMedianMs: yuku.medianMs,
      fasterPercent,
      passes: fasterPercent >= thresholdPercent,
    };
  });
}

export function positiveInteger(value, name) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new Error(`${name} must be a positive integer`);
  }
  return parsed;
}

function uniqueResult(results, fixture, parser) {
  const matches = results.filter((result) => result.fixture === fixture && result.parser === parser);
  if (matches.length !== 1) {
    throw new Error(`expected one ${parser} result for ${fixture}, found ${matches.length}`);
  }
  return matches[0];
}

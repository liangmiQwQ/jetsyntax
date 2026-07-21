import { parseToTape } from "./binding.js";

import { decodeTrustedTape } from "./decoder.js";

export function parse(source, options) {
  const result = parseToTape(source, options);

  return {
    program: decodeTrustedTape(source, result.tape, options),
    diagnostics: result.diagnostics,
    panicked: false,
  };
}

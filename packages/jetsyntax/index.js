import { parseToTape } from "./binding.js";

import { decodeTape } from "./decoder.js";

export function parse(source, options) {
  const result = parseToTape(source, options);

  return {
    program: decodeTape(source, result.tape, options),
    diagnostics: result.diagnostics,
    panicked: false,
  };
}

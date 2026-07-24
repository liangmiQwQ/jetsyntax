export type Language = "js" | "jsx" | "ts" | "tsx" | "dts";
export type SourceType = "script" | "module" | "unambiguous" | "commonjs";
export type DecoratorMode = "auto" | "standard" | "typescript";

export interface ParseOptions {
  lang?: Language;
  sourceType?: SourceType;
  preserveParens?: boolean;
  allowReturnOutsideFunction?: boolean;
  range?: boolean;
  semanticErrors?: boolean;
  typescriptJsCompatibility?: boolean;
  optionalChainingAssign?: boolean;
  decorators?: boolean;
  decoratorMode?: DecoratorMode;
}

export interface Program {
  type: "Program";
  start: number;
  end: number;
  sourceType: "script" | "module" | "commonjs";
  body: Array<Record<string, unknown>>;
  [key: string]: unknown;
}

export interface ParseResult {
  program: Program;
  diagnostics: string[];
  panicked: boolean;
}

export function parse(source: string, options?: ParseOptions): ParseResult;

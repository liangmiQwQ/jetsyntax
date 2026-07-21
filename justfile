set dotenv-load := true

default:
  @just --list

bootstrap:
  pnpm install

build:
  pnpm build

check:
  cargo check --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  pnpm dprint check

test:
  pnpm test

conformance suite:
  pnpm build
  pnpm test:conformance -- --suite {{suite}}

bench-native:
  pnpm bench:native

bench-napi:
  pnpm build
  pnpm bench:napi

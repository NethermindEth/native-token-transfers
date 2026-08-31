/**
 * Guards the error tables in `src/errors.ts` against the contract enums they
 * name.
 *
 * The tables are written by hand because they carry grouping and provenance
 * that a generator cannot produce, so nothing but this stops a renamed or
 * renumbered Rust variant from leaving the SDK reporting the wrong name. The
 * comparison runs in both directions: an added variant and a retired one both
 * fail.
 *
 * `OZ_ERRORS` and `TOKEN_ERRORS` are out of scope. They come from external
 * crates with no source in this repository to read.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { ERROR_TABLES } from "../src/errors.js";

const CONTRACTS = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "contracts"
);

/** Where each table's enum is declared. */
const SOURCES: Record<keyof typeof ERROR_TABLES, string> = {
  NttManagerError: "soroban-ntt-client/src/errors.rs",
  TransceiverError: "soroban-ntt-client/src/errors.rs",
  WrapperError: "ntt-with-executor/src/lib.rs",
  ExecutorError: "ntt-with-executor/src/executor.rs",
};

/**
 * The `Variant = N` pairs of the `#[contracterror]` enum named `name`, read
 * from `file`. Doc comments and attributes sit between the variants, so the
 * body is matched first and the variants are picked out of it.
 */
function rustEnum(file: string, name: string): Record<number, string> {
  const source = readFileSync(join(CONTRACTS, file), "utf8");
  const body = new RegExp(`pub enum ${name} \\{([\\s\\S]*?)\\n\\}`).exec(
    source
  );
  if (body?.[1] === undefined)
    throw new Error(`No enum ${name} in ${file}; the parity test cannot run`);

  // The trailing comma is optional so a last variant written without one is
  // still seen; missing it would let a drifted enum agree with a drifted table.
  const variants: Record<number, string> = {};
  for (const [, variant, code] of [
    ...body[1].matchAll(/^\s*(\w+) = (\d+),?$/gm),
  ])
    if (variant !== undefined && code !== undefined)
      variants[Number(code)] = variant;
  return variants;
}

describe("error tables match the contract enums", () => {
  it.each(Object.keys(SOURCES) as (keyof typeof ERROR_TABLES)[])(
    "%s",
    (name) => {
      const parsed = rustEnum(SOURCES[name], name);
      // A non-empty parse proves the regex still matches the Rust, so an
      // enum this test can no longer read fails rather than passing vacuously.
      expect(Object.keys(parsed).length).toBeGreaterThan(0);
      expect(ERROR_TABLES[name]).toEqual(parsed);
    }
  );
});

import { AnchorError, ProgramError } from "@coral-xyz/anchor";
import { AssertionError } from "chai";

/**
 * The identity of an on-chain failure, as far as it can be recovered from the
 * thrown client error.
 */
export interface TxErrorIdentity {
  /** Anchor error name, e.g. "VenueInactive" or "AccountNotInitialized". */
  name?: string;
  /** Numeric error code (Anchor/custom program error number). */
  code?: number;
  /** Program logs, kept for diagnostics. */
  logs: string[];
}

/** Extract name/code/logs from whatever the Anchor client threw. */
export function identifyTxError(err: unknown): TxErrorIdentity {
  const e = err as any;
  const logs: string[] = e?.logs ?? e?.transactionLogs ?? e?.simulationResponse?.logs ?? [];

  if (e instanceof AnchorError || e?.error?.errorCode) {
    return {
      name: e.error.errorCode.code,
      code: e.error.errorCode.number,
      logs,
    };
  }
  if (e instanceof ProgramError || (typeof e?.code === "number" && typeof e?.msg === "string")) {
    return { code: e.code, logs };
  }
  // Fall back to parsing the logs (e.g. SendTransactionError from preflight).
  const parsed = logs.length > 0 ? AnchorError.parse(logs) : null;
  if (parsed) {
    return {
      name: parsed.error.errorCode.code,
      code: parsed.error.errorCode.number,
      logs,
    };
  }
  // Raw "custom program error: 0x…" without Anchor framing.
  const m = /custom program error: (0x[0-9a-fA-F]+)/.exec(String(e?.message ?? e));
  if (m) {
    return { code: parseInt(m[1], 16), logs };
  }
  return { logs };
}

/**
 * Await `pending` and assert that it fails with one of `expected`
 * (Anchor error names or numeric codes). Fails if the transaction
 * succeeds, if it fails with a different error, or if the error cannot be
 * identified. Never swallows an AssertionError raised inside `pending`.
 *
 * Returns the identity of the observed error for further assertions.
 */
export async function expectTxError(
  pending: Promise<unknown>,
  expected: string | number | Array<string | number>
): Promise<TxErrorIdentity> {
  const wanted = Array.isArray(expected) ? expected : [expected];
  if (wanted.length === 0) {
    throw new Error("expectTxError: at least one expected error is required");
  }

  let result: unknown;
  try {
    result = await pending;
  } catch (err) {
    if (err instanceof AssertionError || (err as any)?.name === "AssertionError") {
      throw err;
    }
    const id = identifyTxError(err);
    const ok = wanted.some((w) =>
      typeof w === "number" ? id.code === w : id.name === w
    );
    if (!ok) {
      throw new AssertionError(
        `expected transaction to fail with ${JSON.stringify(wanted)}, ` +
          `got name=${id.name ?? "?"} code=${id.code ?? "?"}: ${String((err as any)?.message ?? err)}\n` +
          `logs:\n  ${id.logs.join("\n  ")}`
      );
    }
    return id;
  }
  throw new AssertionError(
    `expected transaction to fail with ${JSON.stringify(wanted)}, but it succeeded` +
      (typeof result === "string" ? ` (signature ${result})` : "")
  );
}

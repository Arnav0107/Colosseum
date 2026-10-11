import * as anchor from "@coral-xyz/anchor";

/** A short, human-readable reason for a failed transaction or RPC call. */
export function describeError(e: unknown): string {
  if (e instanceof anchor.AnchorError) {
    return `${e.error.errorCode.code}: ${e.error.errorMessage}`;
  }
  const logs: string[] | undefined = (e as { logs?: string[] })?.logs;
  if (Array.isArray(logs)) {
    const parsed = anchor.AnchorError.parse(logs);
    if (parsed) return `${parsed.error.errorCode.code}: ${parsed.error.errorMessage}`;
  }
  const msg = e instanceof Error ? e.message : String(e);
  return msg.length > 300 ? `${msg.slice(0, 300)}…` : msg;
}

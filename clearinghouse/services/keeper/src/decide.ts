/**
 * Keeper decisions, as a pure function of on-chain state.
 *
 * The keeper is liveness only (F14): refresh and invalidation are
 * permissionless and deterministic, so a keeper can never create credit the
 * program would not create for anyone else. Because every decision is
 * derived from chain state alone, two keeper instances looking at the same
 * slot choose the same actions. A duplicate refresh recomputes the same result
 * (the program releases the previous share first); a duplicate invalidation
 * fails with `InvalidationNotJustified`, which the runner treats as "already done".
 */

export const STATUS_ACTIVE = 1;

export interface PortfolioState {
  /** Incremented by the program on every computation and every input change. */
  version: bigint;
  status: number;
  numRegistered: number;
  riskEpoch: bigint;
  expiresSlot: bigint;
  /** Included in the outstanding-credit counters. */
  counted: boolean;
}

export interface ProtocolState {
  slot: bigint;
  configEpoch: bigint;
  paused: boolean;
}

export type Action =
  | { kind: "none"; why: string }
  | { kind: "refresh"; why: string }
  /** `invalidate_portfolio` with no oracle accounts: releases counters / marks invalid. */
  | { kind: "release"; why: string };

export function decide(p: PortfolioState, s: ProtocolState, refreshMarginSlots: bigint): Action {
  const active = p.status === STATUS_ACTIVE;
  const epochOk = p.riskEpoch === s.configEpoch;
  const expired = s.slot > p.expiresSlot;

  if (p.numRegistered === 0) {
    return p.counted || (active && (expired || !epochOk))
      ? { kind: "release", why: "no legs but credit still counted" }
      : { kind: "none", why: "no legs" };
  }
  if (s.paused) {
    // Refresh is blocked while paused; only removing credit is possible.
    if ((active && (expired || !epochOk)) || (!active && p.counted)) {
      return { kind: "release", why: "paused; credit expired, stale or invalid but still counted" };
    }
    return { kind: "none", why: "paused" };
  }
  if (!active) return { kind: "refresh", why: "not active" };
  if (!epochOk) return { kind: "refresh", why: "risk epoch changed" };
  if (s.slot + refreshMarginSlots >= p.expiresSlot) return { kind: "refresh", why: "expiring" };
  return { kind: "none", why: "fresh" };
}

/** How the runner treats a failed transaction. */
export type FailureClass =
  /** Another keeper (or anyone) got there first: success for our purposes. */
  | "benign-race"
  /** Inputs are not refreshable now (stale snapshot or price, venue inactive…); try releasing. */
  | "inputs-not-ready"
  | "unexpected";

const BENIGN = new Set(["InvalidationNotJustified"]);
const INPUTS = new Set([
  "SnapshotStale",
  "PriceStale",
  "PriceUncertain",
  "InvalidPrice",
  "RiskModelStale",
  "VenueInactive",
  "VenueNotAuthorized",
  "UserConsentMissing",
  "IncompletePortfolio",
  "UnsupportedAsset",
  "ProgramPaused",
  "OracleNotConfigured",
]);

export function classifyFailure(errorCode: string | null): FailureClass {
  if (errorCode && BENIGN.has(errorCode)) return "benign-race";
  if (errorCode && INPUTS.has(errorCode)) return "inputs-not-ready";
  return "unexpected";
}

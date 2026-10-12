/**
 * C3: what the credit buys at a venue. A pure model of the reference venue
 * `programs/mock_perps_engine` (C1): one venue holds the trader's collateral,
 * a new position must satisfy equity ≥ max(IM − usable credit, MM), with the
 * credit computed jointly with the trader's legs on other venues (here: one
 * hedge leg on venue B) by the same engine as ch_core (TS port, bit-identical
 * through vectors.test.ts).
 *
 * Mirrors the engine's arithmetic: size = ⌊notional·1e6/price⌋, reported
 * notional = ⌊size·price/1e6⌋, IM/MM = ⌈notional·bps/10,000⌉ (MM ≥ 1). The
 * probe (`maxOpen`) uses the same $500 steps as the LiteSVM test
 * `crates/ch_svm_tests/tests/engine.rs`, which pins the result on chain
 * (venueRule.test.ts checks both agree).
 *
 * Assumes a calm market (throttle scale 1: price = EMA, small confidence) and
 * empty credit budgets. It is an illustration, not a source of truth: the
 * Portfolio panel's verifier is the check against the chain.
 */
import { computeViewCredit, PortfolioLeg } from "../portfolio";
import { creditParams, effectiveInitialMargin, riskModelFor } from "./verify";
import type { ConfigView, RiskModelView } from "./views";

export const USD = 1_000_000n;
const E6 = 1_000_000n;

export interface VenueRuleScenario {
  /** Collateral on venue A (micro-USD). */
  collateral: bigint;
  /** Venue A's margin rates. */
  imBps: number;
  mmBps: number;
  /** The new position on venue A. */
  venueIndex: number;
  asset: number;
  isLong: boolean;
  price: bigint;
  /** The trader's existing leg on another venue, or null for an unhedged trader. */
  hedge: { venueIndex: number; asset: number; isLong: boolean; notional: bigint; price: bigint } | null;
  /** Probe step (micro-USD). */
  step: bigint;
}

const bpsCeil = (x: bigint, bps: number): bigint => (x * BigInt(bps) + 9_999n) / 10_000n;

/** A position of `requested` micro-USD as the engine stores and reports it. */
export function engineLeg(venueIndex: number, asset: number, isLong: boolean, requested: bigint, price: bigint, imBps: number, mmBps: number): PortfolioLeg {
  const size = (requested * E6) / price;
  const notional = (size * price) / E6;
  const mm = bpsCeil(notional, mmBps);
  return {
    venueIndex,
    asset,
    isLong,
    notional,
    initialMargin: bpsCeil(notional, imBps),
    maintenanceMargin: mm > 0n ? mm : 1n,
  };
}

export interface VenueRuleCheck {
  leg: PortfolioLeg;
  credit: bigint;
  required: bigint;
  ok: boolean;
}

/** The venue's margin check for a new position of `requested` micro-USD. */
export function checkOpen(cfg: ConfigView, rm: RiskModelView, s: VenueRuleScenario, requested: bigint): VenueRuleCheck {
  const leg = engineLeg(s.venueIndex, s.asset, s.isLong, requested, s.price, s.imBps, s.mmBps);
  let credit = 0n;
  if (s.hedge) {
    const h = s.hedge;
    const hedgeLeg = engineLeg(h.venueIndex, h.asset, h.isLong, h.notional, h.price, s.imBps, s.mmBps);
    const v = computeViewCredit([leg, hedgeLeg], riskModelFor(rm, cfg.params), creditParams(cfg.params), (i) => cfg.venueAccepts[i] ?? 0n);
    credit = v.legCredit[0];
  }
  const required = effectiveInitialMargin(leg.initialMargin, leg.maintenanceMargin, credit);
  return { leg, credit, required, ok: s.collateral >= required };
}

/** Largest multiple of `step` the venue accepts (the LiteSVM probe). */
export function maxOpen(cfg: ConfigView, rm: RiskModelView, s: VenueRuleScenario): bigint {
  let n = 0n;
  const limit = s.collateral * 100n;
  while (n < limit && checkOpen(cfg, rm, s, n + s.step).ok) n += s.step;
  return n;
}

/** C2(a): $8,000 on venue A, long SOL at $150; hedge = $50,000 ETH short on venue B at $3,000. */
export function referenceScenario(hedged: boolean): VenueRuleScenario {
  return {
    collateral: 8_000n * USD,
    imBps: 2_000,
    mmBps: 1_000,
    venueIndex: 0,
    asset: 0,
    isLong: true,
    price: 150n * USD,
    hedge: hedged ? { venueIndex: 1, asset: 1, isLong: false, notional: 50_000n * USD, price: 3_000n * USD } : null,
    step: 500n * USD,
  };
}

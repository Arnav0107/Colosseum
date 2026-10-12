import { describe, expect, it } from "vitest";
import { configFixture, riskModelFixture } from "./fixtures";
import { checkOpen, maxOpen, referenceScenario, USD } from "./venueRule";

// The fixtures use the same parameters as the LiteSVM world
// (crates/ch_svm_tests/src/setup.rs: default_params, DEFAULT_ASSET_RISK,
// ρ(SOL, ETH) = 0.8), so these numbers must equal what
// `cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test engine`
// measures against the compiled reference venue.
describe("C3: reference venue rule (mirror of mock_perps_engine)", () => {
  const cfg = configFixture();
  const rm = riskModelFixture();

  it("unhedged: $8,000 at IM 20% opens at most $40,000", () => {
    expect(maxOpen(cfg, rm, referenceScenario(false))).toBe(40_000n * USD);
  });

  it("hedged: the same $8,000 opens $59,000 (LiteSVM C2(a))", () => {
    expect(maxOpen(cfg, rm, referenceScenario(true))).toBe(59_000n * USD);
  });

  it("$50,000 hedged: credit ≈ $3,555.56, requirement IM − credit, unhedged refused", () => {
    const h = checkOpen(cfg, rm, referenceScenario(true), 50_000n * USD);
    expect(h.credit > 3_555_000_000n && h.credit <= 3_555_555_555n).toBe(true);
    expect(h.required).toBe(h.leg.initialMargin - h.credit);
    expect(h.ok).toBe(true);
    const u = checkOpen(cfg, rm, referenceScenario(false), 50_000n * USD);
    expect(u.credit).toBe(0n);
    expect(u.ok).toBe(false);
  });

  it("never below maintenance margin", () => {
    for (const n of [10_000n, 40_000n, 59_000n, 80_000n]) {
      const c = checkOpen(cfg, rm, referenceScenario(true), n * USD);
      expect(c.required >= c.leg.maintenanceMargin).toBe(true);
    }
  });
});

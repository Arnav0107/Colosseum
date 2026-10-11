import { describe, expect, it } from "vitest";
import { capGroups } from "../portfolio";
import { configFixture, IM, portfolioFixture, riskModelFixture } from "./fixtures";
import { formatUsd, parseUsd } from "./format";
import { isLocalRpc } from "./rpc";
import { corrIndex, effectiveInitialMargin, legCap, usableCredit, verifyPortfolio } from "./verify";

describe("corrIndex (same layout as ch_core::state::corr_index)", () => {
  it("maps the upper triangle onto 0..496 exactly once, symmetrically", () => {
    const seen = new Set<number>();
    for (let a = 0; a < 32; a++)
      for (let b = 0; b < 32; b++) {
        const k = corrIndex(a, b);
        if (a === b) {
          expect(k).toBeNull();
          continue;
        }
        expect(k).toBe(corrIndex(b, a));
        if (a < b) seen.add(k!);
      }
    expect(seen.size).toBe(496);
    expect([corrIndex(0, 1), corrIndex(0, 31), corrIndex(1, 2), corrIndex(30, 31)]).toEqual([0, 30, 31, 495]);
    expect(corrIndex(0, 32)).toBeNull();
  });
});

describe("verifyPortfolio", () => {
  it("reproduces the on-chain worked example exactly (MATCH)", () => {
    const v = verifyPortfolio(configFixture(), riskModelFixture(), portfolioFixture());
    expect(v.kind).toBe("match");
    if (v.kind === "match") {
      expect(v.expected).toEqual([3_555_555_555n, 3_555_555_555n]);
      expect(v.engine.portfolioRisk).toBe(5_000_000_000n);
    }
  });

  it("reports every differing quantity (MISMATCH)", () => {
    const pf = portfolioFixture();
    pf.legs[0].credit += 1n;
    pf.totalCredit += 1n;
    const v = verifyPortfolio(configFixture(), riskModelFixture(), pf);
    expect(v.kind).toBe("mismatch");
    if (v.kind === "mismatch") {
      expect(v.differences.some((d) => d.startsWith("leg 1 credit"))).toBe(true);
      expect(v.differences.some((d) => d.startsWith("total credit"))).toBe(true);
    }
  });

  it("uses the on-chain haircut, not a UI default (F12)", () => {
    const cfg = configFixture();
    cfg.params.haircutBps = 0;
    const v = verifyPortfolio(cfg, riskModelFixture(), portfolioFixture());
    expect(v.kind).toBe("mismatch");
  });

  it("refuses to compare across an epoch or model change", () => {
    const cfg = configFixture();
    cfg.riskEpoch += 1n;
    expect(verifyPortfolio(cfg, riskModelFixture(), portfolioFixture()).kind).toBe("stale");
    const rm = riskModelFixture();
    rm.version += 1n;
    expect(verifyPortfolio(configFixture(), rm, portfolioFixture()).kind).toBe("stale");
  });

  it("has nothing to verify for an invalidated portfolio", () => {
    const pf = portfolioFixture();
    pf.status = 2;
    expect(verifyPortfolio(configFixture(), riskModelFixture(), pf).kind).toBe("nothing");
  });

  it("reproduces protocol budgets, reconstructing counters before the portfolio's own share", () => {
    const cfg = configFixture();
    cfg.params.maxCreditPerVenue = 2_000_000_000n;
    const pf = portfolioFixture();
    pf.legs.forEach((l) => (l.credit = 2_000_000_000n));
    pf.totalCredit = 4_000_000_000n;
    cfg.venueOutstanding[0] = 2_000_000_000n;
    cfg.venueOutstanding[1] = 2_000_000_000n;
    cfg.assetOutstanding[0] = 2_000_000_000n;
    cfg.assetOutstanding[1] = 2_000_000_000n;
    cfg.globalOutstandingCredit = 4_000_000_000n;
    expect(verifyPortfolio(cfg, riskModelFixture(), pf).kind).toBe("match");
  });

  it("reports an engine rejection instead of guessing", () => {
    const rm = riskModelFixture();
    rm.assets[1] = { shockBps: 0, gapBps: 0, basisBps: 0 };
    const v = verifyPortfolio(configFixture(), rm, portfolioFixture());
    expect(v.kind).toBe("engine-error");
  });
});

describe("venue-side rules", () => {
  it("usable credit follows status, epoch, kind and expiry", () => {
    const cfg = configFixture();
    const pf = portfolioFixture();
    expect(usableCredit(cfg, pf, 4_000n, 0, 0)).toBe(3_555_555_555n);
    expect(usableCredit(cfg, pf, 4_001n, 0, 0)).toBe(0n);
    expect(usableCredit(cfg, pf, 1_000n, 0, 1)).toBe(0n); // no such leg
    expect(usableCredit({ ...cfg, riskEpoch: 8n }, pf, 1_000n, 0, 0)).toBe(0n);
    expect(usableCredit(cfg, { ...pf, status: 2 }, 1_000n, 0, 0)).toBe(0n);
    expect(usableCredit(cfg, { ...pf, creditKind: 0 }, 1_000n, 0, 0)).toBe(0n);
  });

  it("credit never takes the venue requirement below maintenance margin", () => {
    expect(effectiveInitialMargin(IM, IM / 2n, 3_000_000_000n)).toBe(7_000_000_000n);
    expect(effectiveInitialMargin(IM, IM / 2n, 9_000_000_000n)).toBe(IM / 2n);
    expect(effectiveInitialMargin(IM, IM / 2n, 20_000_000_000n)).toBe(IM / 2n);
    // Same guard as ch_client: MM > IM ignores the credit.
    expect(effectiveInitialMargin(IM, IM + 1n, 3_000_000_000n)).toBe(IM);
    expect(legCap(IM, IM / 2n, 7_500)).toBe(IM / 2n);
    expect(legCap(IM, 9_000_000_000n, 7_500)).toBe(1_000_000_000n);
  });

  it("capGroups matches the Rust rule (pro-rata floor, unknown group gets 0)", () => {
    expect(capGroups([600n, 400n, 5n], (i) => (i < 2 ? 0 : 1), [500n, 100n])).toEqual([300n, 200n, 5n]);
    expect(capGroups([7n, 7n], (i) => i, [7n])).toEqual([7n, 0n]);
  });
});

describe("input handling", () => {
  it("parses and formats USD in micro units without floats", () => {
    expect(parseUsd("1,234.56")).toBe(1_234_560_000n);
    expect(parseUsd("0.000001")).toBe(1n);
    expect(parseUsd("1.0000001")).toBeNull();
    expect(parseUsd("-5")).toBeNull();
    expect(parseUsd("abc")).toBeNull();
    expect(formatUsd(7_111_111_110n)).toBe("$7,111.11");
  });

  it("only allows loopback RPC URLs", () => {
    for (const ok of ["http://127.0.0.1:8899", "http://localhost:8899", "http://[::1]:8899"]) expect(isLocalRpc(ok)).toBe(true);
    for (const bad of [
      "https://api.devnet.solana.com",
      "https://api.mainnet-beta.solana.com",
      "http://127.0.0.1.evil.com:8899",
      "ftp://127.0.0.1",
      "not a url",
    ])
      expect(isLocalRpc(bad)).toBe(false);
  });
});

/**
 * Hand-built on-chain views for tests: the worked example (long SOL / short
 * ETH, $50k notional each, IM $10k, MM $5k, ρ = 0.8, 20% haircut). The same
 * case is pinned in crates/ch_math/tests/vectors/portfolio.json and in the
 * LiteSVM suite (7,111,111,110 total).
 */
import { corrIndex } from "./verify";
import type { BenchState, ConfigView, PortfolioView, RiskModelView } from "./views";

export const IM = 10_000_000_000n;

export function configFixture(): ConfigView {
  return {
    admin: "admin",
    riskAuthority: "admin",
    venueManager: "admin",
    guardian: "admin",
    riskEpoch: 7n,
    params: {
      maxVenues: 16,
      haircutBps: 2_000,
      creditTtlSlots: 3_000n,
      snapshotMaxAgeSlots: 10_000n,
      maxCreditPerUser: 100_000_000_000n,
      maxCreditBpsOfIm: 7_500,
      maxCreditPerVenue: 1_000_000_000_000n,
      maxCreditPerAsset: 1_000_000_000_000n,
      globalCreditCap: 10_000_000_000_000n,
      riskModelMaxAgeSlots: 100_000n,
      maxPriceAgeSecs: 300n,
      maxConfBps: 100,
      maxMoveBps: 500,
      timelockSlots: 0n,
    },
    usedVenuesBitmap: 3n,
    activeVenuesBitmap: 3n,
    globalOutstandingCredit: 7_111_111_110n,
    venueOutstanding: Array.from({ length: 64 }, (_, i) => (i < 2 ? 3_555_555_555n : 0n)),
    assetOutstanding: Array.from({ length: 32 }, (_, i) => (i < 2 ? 3_555_555_555n : 0n)),
    paused: false,
  };
}

export function riskModelFixture(): RiskModelView {
  const corr = new Array(496).fill(0);
  corr[corrIndex(0, 1)!] = 800_000;
  const assets = Array.from({ length: 32 }, () => ({ shockBps: 0, gapBps: 0, basisBps: 0 }));
  assets[0] = { shockBps: 1_000, gapBps: 1_000, basisBps: 100 };
  assets[1] = { shockBps: 800, gapBps: 800, basisBps: 100 };
  assets[2] = { shockBps: 600, gapBps: 600, basisBps: 100 };
  return {
    version: 12n,
    updatedSlot: 900n,
    corr,
    assets,
    corrStressPpm: 100_000,
    basisDecorrPpm: 20_000,
    scenarioKind: new Array(8).fill(0),
    scenarios: Array.from({ length: 8 }, () => new Array(32).fill(0)),
  };
}

export function portfolioFixture(): PortfolioView {
  const leg = (venueIndex: number, assetId: number, isLong: boolean) => ({
    venueIndex,
    assetId,
    isLong,
    snapshotVersion: 1n,
    snapshotSlot: 1_000n,
    notional: 5n * IM,
    initialMargin: IM,
    maintenanceMargin: IM / 2n,
    baselinePrice: assetId === 0 ? 150_000_000n : 3_000_000_000n,
    baselineConf: 15_000n,
    baselinePublishTs: 1_700_000_000n,
    credit: 3_555_555_555n,
  });
  return {
    status: 1,
    creditKind: 1,
    version: 4n,
    riskEpoch: 7n,
    riskModelVersion: 12n,
    computedSlot: 1_000n,
    expiresSlot: 4_000n,
    totalCredit: 7_111_111_110n,
    counted: true,
    countedEpoch: 7n,
    sumStandaloneRisk: 9_000_000_000n,
    portfolioRisk: 5_000_000_000n,
    hedgeBenefitPpm: 444_444,
    registered: [
      { venueIndex: 0, assetId: 0 },
      { venueIndex: 1, assetId: 1 },
    ],
    legs: [leg(0, 0, true), leg(1, 1, false)],
  };
}

export function stateFixture(): BenchState {
  const pf = portfolioFixture();
  return {
    clock: { slot: 1_100n, unixTimestamp: 1_700_000_040n },
    programDeployed: true,
    config: configFixture(),
    riskModel: riskModelFixture(),
    venues: [
      { name: "venue-perp-a", index: 0, isActive: true, authority: "a", activationEtaSlot: 0n },
      { name: "venue-perp-b", index: 1, isActive: true, authority: "b", activationEtaSlot: 0n },
    ],
    consent: { isActive: true, authorizedVenuesBitmap: 3n },
    portfolio: pf,
    snapshots: pf.legs.map((l) => ({
      venueIndex: l.venueIndex,
      assetId: l.assetId,
      version: 1n,
      isLong: l.isLong,
      notional: l.notional,
      initialMargin: l.initialMargin,
      maintenanceMargin: l.maintenanceMargin,
      slot: 1_000n,
      timestamp: 1_700_000_000n,
    })),
    prices: [
      { assetId: 0, priceMicro: 150_000_000n, confMicro: 15_000n, publishTs: 1_700_000_000n },
      { assetId: 1, priceMicro: 3_000_000_000n, confMicro: 300_000n, publishTs: 1_699_999_000n },
    ],
  };
}

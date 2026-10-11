/**
 * Plain views of on-chain accounts (bigint for u64/i64, number for small ints).
 *
 * The chain layer (src/chain/bench.ts) decodes Anchor accounts into these; the
 * model layer and the UI only ever see these types, so they can be tested
 * without a validator.
 */
import type { AssetRisk } from "../portfolio";

export const STATUS = { EMPTY: 0, ACTIVE: 1, INVALIDATED: 2 } as const;
export const STATUS_NAME: Record<number, string> = { 0: "Empty", 1: "Active", 2: "Invalidated" };

/** `ch_core::events::invalidation` reason bits. */
export const INVALIDATION_REASONS: [number, string][] = [
  [1 << 0, "release only"],
  [1 << 1, "epoch changed"],
  [1 << 2, "expired"],
  [1 << 3, "venue inactive"],
  [1 << 4, "consent withdrawn"],
  [1 << 5, "snapshot stale"],
  [1 << 6, "price breach"],
  [1 << 7, "oracle unusable"],
];

export interface ConfigParamsView {
  maxVenues: number;
  haircutBps: number;
  creditTtlSlots: bigint;
  snapshotMaxAgeSlots: bigint;
  maxCreditPerUser: bigint;
  maxCreditBpsOfIm: number;
  maxCreditPerVenue: bigint;
  maxCreditPerAsset: bigint;
  globalCreditCap: bigint;
  riskModelMaxAgeSlots: bigint;
  maxPriceAgeSecs: bigint;
  maxConfBps: number;
  maxMoveBps: number;
  timelockSlots: bigint;
}

export interface ConfigView {
  admin: string;
  riskAuthority: string;
  venueManager: string;
  guardian: string;
  riskEpoch: bigint;
  params: ConfigParamsView;
  usedVenuesBitmap: bigint;
  activeVenuesBitmap: bigint;
  globalOutstandingCredit: bigint;
  venueOutstanding: bigint[];
  assetOutstanding: bigint[];
  paused: boolean;
}

export interface RiskModelView {
  version: bigint;
  updatedSlot: bigint;
  /** Upper-triangle correlations, 1e6 scale (see corrIndex). */
  corr: number[];
  assets: AssetRisk[];
  corrStressPpm: number;
  basisDecorrPpm: number;
  scenarioKind: number[];
  scenarios: number[][];
}

export interface LegKeyView {
  venueIndex: number;
  assetId: number;
}

export interface LegStateView extends LegKeyView {
  isLong: boolean;
  snapshotVersion: bigint;
  snapshotSlot: bigint;
  notional: bigint;
  initialMargin: bigint;
  maintenanceMargin: bigint;
  baselinePrice: bigint;
  baselineConf: bigint;
  baselinePublishTs: bigint;
  credit: bigint;
}

export interface PortfolioView {
  status: number;
  creditKind: number;
  version: bigint;
  riskEpoch: bigint;
  riskModelVersion: bigint;
  computedSlot: bigint;
  expiresSlot: bigint;
  totalCredit: bigint;
  counted: boolean;
  countedEpoch: bigint;
  sumStandaloneRisk: bigint;
  portfolioRisk: bigint;
  hedgeBenefitPpm: number;
  registered: LegKeyView[];
  legs: LegStateView[];
}

export interface SnapshotView extends LegKeyView {
  version: bigint;
  isLong: boolean;
  notional: bigint;
  initialMargin: bigint;
  maintenanceMargin: bigint;
  slot: bigint;
  timestamp: bigint;
}

export interface VenueView {
  name: string;
  index: number;
  isActive: boolean;
  authority: string;
  activationEtaSlot: bigint;
}

export interface PriceView {
  assetId: number;
  priceMicro: bigint;
  confMicro: bigint;
  publishTs: bigint;
}

export interface ClockView {
  slot: bigint;
  unixTimestamp: bigint;
}

export interface BenchState {
  clock: ClockView;
  programDeployed: boolean;
  config: ConfigView | null;
  riskModel: RiskModelView | null;
  venues: VenueView[];
  consent: { isActive: boolean; authorizedVenuesBitmap: bigint } | null;
  portfolio: PortfolioView | null;
  snapshots: SnapshotView[];
  prices: PriceView[];
}

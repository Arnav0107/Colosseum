/**
 * Chain access for the test bench: one place that builds transactions and
 * decodes accounts into the plain views in ../model/views.ts.
 *
 * LOCALNET ONLY. The program must be built with
 * `--features mock-oracle,dev-venue-signer` (scripts/dev-localnet.sh); the
 * keys are deterministic dev keys (../devKeys.ts).
 */
import * as anchor from "@coral-xyz/anchor";
import type { IdlAccounts } from "@coral-xyz/anchor";
import { Connection, Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram, SYSVAR_CLOCK_PUBKEY } from "@solana/web3.js";
import { Buffer } from "buffer";
import idl from "../idl/ch_core.json";
import type { ChCore } from "../idl/ch_core";
import { DEV_KEYS } from "../devKeys";
import { pdas, PROGRAM_ID, venueIdBytes } from "./pdas";
import { corrIndex } from "../model/verify";
import type {
  BenchState,
  ConfigParamsView,
  ConfigView,
  LegStateView,
  PortfolioView,
  PriceView,
  RiskModelView,
  SnapshotView,
  VenueView,
} from "../model/views";
import { ASSETS } from "../model/format";

type Accounts = IdlAccounts<ChCore>;
const { BN } = anchor;
const big = (v: { toString(): string }): bigint => BigInt(v.toString());
const bn = (v: bigint | number) => new BN(v.toString());

export const VENUES = [
  { name: "venue-perp-a", index: 0, key: DEV_KEYS.venueAuthA },
  { name: "venue-perp-b", index: 1, key: DEV_KEYS.venueAuthB },
];

/** Max correlation change per update (ch_core::governance::MAX_CORR_STEP_PPM). */
export const MAX_CORR_STEP_PPM = 250_000;

/** Localnet parameters (timelock 0 is accepted only by mock-oracle builds). */
export const LOCALNET_PARAMS = {
  maxVenues: 16,
  haircutBps: 2_000,
  creditTtlSlots: bn(3_000),
  snapshotMaxAgeSlots: bn(10_000),
  maxCreditPerUser: bn(100_000_000_000n),
  maxCreditBpsOfIm: 7_500,
  maxCreditPerVenue: bn(1_000_000_000_000n),
  maxCreditPerAsset: bn(1_000_000_000_000n),
  globalCreditCap: bn(10_000_000_000_000n),
  riskModelMaxAgeSlots: bn(100_000),
  maxPriceAgeSecs: bn(300),
  maxConfBps: 100,
  maxMoveBps: 300,
  timelockSlots: bn(0),
  hedgeBreakBps: 5_000,
  // A5: report sanity bounds (localnet defaults).
  maxLegNotional: bn(10_000_000_000_000n),
  maxVenueNotionalPerUser: bn(20_000_000_000_000n),
  maxNotionalGrowthBps: 10_000,
  notionalGrowthWindowSlots: bn(150),
  notionalGrowthFloor: bn(2_000_000_000_000n),
  // B3: regime throttle.
  throttleMoveStartBps: 50,
  throttleMoveFullBps: 150,
  throttleConfStartBps: 50,
};

/** Per-asset (shock, gap, basis) in bps, as used by the Rust test fixture. */
export const DEFAULT_ASSET_RISK: Record<number, [number, number, number]> = {
  0: [1_000, 1_000, 100],
  1: [800, 800, 100],
  2: [600, 600, 100],
};

function paramsView(p: Accounts["globalConfig"]["params"]): ConfigParamsView {
  return {
    maxVenues: p.maxVenues,
    haircutBps: p.haircutBps,
    creditTtlSlots: big(p.creditTtlSlots),
    snapshotMaxAgeSlots: big(p.snapshotMaxAgeSlots),
    maxCreditPerUser: big(p.maxCreditPerUser),
    maxCreditBpsOfIm: p.maxCreditBpsOfIm,
    maxCreditPerVenue: big(p.maxCreditPerVenue),
    maxCreditPerAsset: big(p.maxCreditPerAsset),
    globalCreditCap: big(p.globalCreditCap),
    riskModelMaxAgeSlots: big(p.riskModelMaxAgeSlots),
    maxPriceAgeSecs: big(p.maxPriceAgeSecs),
    maxConfBps: p.maxConfBps,
    maxMoveBps: p.maxMoveBps,
    timelockSlots: big(p.timelockSlots),
    hedgeBreakBps: p.hedgeBreakBps,
    maxLegNotional: big(p.maxLegNotional),
    maxVenueNotionalPerUser: big(p.maxVenueNotionalPerUser),
    maxNotionalGrowthBps: p.maxNotionalGrowthBps,
    notionalGrowthWindowSlots: big(p.notionalGrowthWindowSlots),
    notionalGrowthFloor: big(p.notionalGrowthFloor),
    throttleMoveStartBps: p.throttleMoveStartBps,
    throttleMoveFullBps: p.throttleMoveFullBps,
    throttleConfStartBps: p.throttleConfStartBps,
  };
}

function configView(c: Accounts["globalConfig"]): ConfigView {
  return {
    admin: c.admin.toBase58(),
    riskAuthority: c.riskAuthority.toBase58(),
    venueManager: c.venueManager.toBase58(),
    guardian: c.guardian.toBase58(),
    riskEpoch: big(c.riskEpoch),
    params: paramsView(c.params),
    usedVenuesBitmap: big(c.usedVenuesBitmap),
    activeVenuesBitmap: big(c.activeVenuesBitmap),
    globalOutstandingCredit: big(c.globalOutstandingCredit),
    venueOutstanding: c.venueOutstanding.map(big),
    assetOutstanding: c.assetOutstanding.map(big),
    venueAccepts: c.venueAccepts.map(big),
    paused: c.paused,
  };
}

function riskModelView(m: Accounts["riskModel"]): RiskModelView {
  return {
    version: big(m.version),
    updatedSlot: big(m.updatedSlot),
    corr: m.corr,
    assets: m.assets.map((a) => ({ shockBps: a.shockBps, gapBps: a.gapBps, basisBps: a.basisBps })),
    corrStressPpm: m.corrStressPpm,
    basisDecorrPpm: m.basisDecorrPpm,
    scenarioKind: m.scenarioKind,
    scenarios: m.scenarios,
  };
}

function portfolioView(p: Accounts["portfolio"]): PortfolioView {
  const legs: LegStateView[] = p.legs.slice(0, p.numLegs).map((l) => ({
    venueIndex: l.venueIndex,
    assetId: l.assetId,
    isLong: l.isLong,
    snapshotVersion: big(l.snapshotVersion),
    snapshotSlot: big(l.snapshotSlot),
    notional: big(l.notional),
    initialMargin: big(l.initialMargin),
    maintenanceMargin: big(l.maintenanceMargin),
    baselinePrice: big(l.baselinePrice),
    baselineConf: big(l.baselineConf),
    baselinePublishTs: big(l.baselinePublishTs),
    credit: big(l.credit),
    viewMask: big(l.viewMask),
    baselineEma: big(l.baselineEma),
  }));
  return {
    status: p.status,
    creditKind: p.creditKind,
    version: big(p.version),
    riskEpoch: big(p.riskEpoch),
    riskModelVersion: big(p.riskModelVersion),
    computedSlot: big(p.computedSlot),
    expiresSlot: big(p.expiresSlot),
    totalCredit: big(p.totalCredit),
    counted: p.counted,
    countedEpoch: big(p.countedEpoch),
    sumStandaloneRisk: big(p.sumStandaloneRisk),
    portfolioRisk: big(p.portfolioRisk),
    hedgeBenefitPpm: p.hedgeBenefitPpm,
    registered: p.registered.slice(0, p.numRegistered).map((k) => ({ venueIndex: k.venueIndex, assetId: k.assetId })),
    legs,
  };
}

function snapshotView(s: Accounts["posSnapshot"]): SnapshotView {
  return {
    venueIndex: s.venueIndex,
    assetId: s.assetId,
    version: big(s.version),
    isLong: s.isLong,
    notional: big(s.notional),
    initialMargin: big(s.initialMargin),
    maintenanceMargin: big(s.maintenanceMargin),
    slot: big(s.slot),
    timestamp: big(s.timestamp),
  };
}

export interface SnapshotInput {
  venueIndex: number;
  assetId: number;
  isLong: boolean;
  notional: bigint;
  initialMargin: bigint;
  maintenanceMargin: bigint;
}

export class Bench {
  readonly connection: Connection;
  readonly program: anchor.Program<ChCore>;
  readonly admin: Keypair = DEV_KEYS.admin;
  readonly trader: Keypair = DEV_KEYS.trader;
  /**
   * Refresh and invalidation need no role and no signer besides the fee payer
   * (here the provider wallet). The keeper key exists to show that any funded
   * key can pay for them; see services/keeper.
   */
  readonly keeper: Keypair = DEV_KEYS.keeper;

  constructor(rpcUrl: string) {
    this.connection = new Connection(rpcUrl, "confirmed");
    const provider = new anchor.AnchorProvider(this.connection, new anchor.Wallet(this.admin), {
      commitment: "confirmed",
    });
    this.program = new anchor.Program<ChCore>(idl as ChCore, provider);
  }

  devKeys(): { label: string; key: PublicKey }[] {
    return [
      { label: "admin (all roles)", key: this.admin.publicKey },
      { label: "keeper (refresh)", key: this.keeper.publicKey },
      { label: "venue A authority", key: VENUES[0].key.publicKey },
      { label: "venue B authority", key: VENUES[1].key.publicKey },
      { label: "trader", key: this.trader.publicKey },
    ];
  }

  async balances(): Promise<Map<string, number>> {
    const keys = this.devKeys().map((k) => k.key);
    const infos = await this.connection.getMultipleAccountsInfo(keys);
    return new Map(keys.map((k, i) => [k.toBase58(), (infos[i]?.lamports ?? 0) / LAMPORTS_PER_SOL]));
  }

  async airdropAll(): Promise<void> {
    for (const { key } of this.devKeys()) {
      const sig = await this.connection.requestAirdrop(key, 10 * LAMPORTS_PER_SOL);
      await this.connection.confirmTransaction(sig, "confirmed");
    }
  }

  /** On-chain clock (slot and unix time); the UI never uses the browser clock for ages. */
  async clock(): Promise<{ slot: bigint; unixTimestamp: bigint }> {
    const info = await this.connection.getAccountInfo(SYSVAR_CLOCK_PUBKEY);
    if (!info) throw new Error("clock sysvar unavailable");
    const d = Buffer.from(info.data);
    return { slot: d.readBigUInt64LE(0), unixTimestamp: d.readBigInt64LE(32) };
  }

  async fetchState(): Promise<BenchState> {
    const a = this.program.account;
    const trader = this.trader.publicKey;
    const [clock, programInfo, config, riskModel, consent, portfolio] = await Promise.all([
      this.clock(),
      this.connection.getAccountInfo(PROGRAM_ID),
      a.globalConfig.fetchNullable(pdas.config()),
      a.riskModel.fetchNullable(pdas.riskModel()),
      a.userConsent.fetchNullable(pdas.consent(trader)),
      a.portfolio.fetchNullable(pdas.portfolio(trader)),
    ]);
    const venueAccounts = await a.venueRegistration.fetchMultiple(VENUES.map((v) => pdas.venue(venueIdBytes(v.name))));
    const venues: VenueView[] = [];
    venueAccounts.forEach((v, i) => {
      if (v)
        venues.push({
          name: VENUES[i].name,
          index: v.venueIndex,
          isActive: v.isActive,
          authority: v.venueAuthority.toBase58(),
          activationEtaSlot: big(v.activationEtaSlot),
        });
    });
    const combos = VENUES.flatMap((v) => ASSETS.map((asset) => ({ v, asset: asset.id })));
    const snaps = await a.posSnapshot.fetchMultiple(
      combos.map(({ v, asset }) => pdas.snapshot(trader, venueIdBytes(v.name), asset))
    );
    const priceAccounts = await a.mockPrice.fetchMultiple(ASSETS.map((x) => pdas.mockPrice(x.id)));
    const prices: PriceView[] = [];
    priceAccounts.forEach((p) => {
      if (p)
        prices.push({ assetId: p.assetId, priceMicro: big(p.priceMicro), confMicro: big(p.confMicro), publishTs: big(p.publishTs) });
    });
    return {
      clock,
      programDeployed: programInfo?.executable ?? false,
      config: config ? configView(config) : null,
      riskModel: riskModel ? riskModelView(riskModel) : null,
      venues,
      consent: consent ? { isActive: consent.isActive, authorizedVenuesBitmap: big(consent.authorizedVenuesBitmap) } : null,
      portfolio: portfolio ? portfolioView(portfolio) : null,
      snapshots: snaps.flatMap((s) => (s ? [snapshotView(s)] : [])),
      prices,
    };
  }

  // ------------------------------------------------------------------ setup

  private riskGov() {
    return {
      config: pdas.config(),
      riskModel: pdas.riskModel(),
      pendingChange: pdas.pendingChange(),
      authority: this.admin.publicKey,
    };
  }

  initialize(): Promise<string> {
    return this.program.methods
      .initialize(LOCALNET_PARAMS, this.admin.publicKey, this.admin.publicKey, this.admin.publicKey)
      .accountsStrict({
        config: pdas.config(),
        riskModel: pdas.riskModel(),
        pendingChange: pdas.pendingChange(),
        admin: this.admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([this.admin])
      .rpc();
  }

  /**
   * Dev venue signer: the venue authority is a plain keypair. The program id
   * is recorded but not checked in this build (production requires the venue
   * program's PDA; see docs/VENUE_INTEGRATION.md).
   */
  registerVenue(i: number): Promise<string> {
    const v = VENUES[i];
    const id = venueIdBytes(v.name);
    return this.program.methods
      .registerVenue([...id], v.key.publicKey, v.key.publicKey, v.index)
      .accountsStrict({
        config: pdas.config(),
        venueRegistration: pdas.venue(id),
        authority: this.admin.publicKey,
        payer: this.admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([this.admin])
      .rpc();
  }

  /**
   * A1: venue `i` allowlists the venues in `acceptsMask` (own bit implied).
   * Dev venue signer: signed by the venue's keypair.
   */
  setVenueAcceptance(i: number, acceptsMask: bigint): Promise<string> {
    const v = VENUES[i];
    const id = venueIdBytes(v.name);
    return this.program.methods
      .setVenueAcceptance(bn(acceptsMask))
      .accountsStrict({
        config: pdas.config(),
        venueRegistration: pdas.venue(id),
        venueAuthority: v.key.publicKey,
      })
      .signers([v.key])
      .rpc();
  }

  setAssetRisk(assetId: number, shockBps: number, gapBps: number, basisBps: number): Promise<string> {
    return this.program.methods
      .setAssetRisk(assetId, { shockBps, gapBps, basisBps })
      .accountsStrict(this.riskGov())
      .signers([this.admin])
      .rpc();
  }

  /** Walk the on-chain correlation to `target` in steps the program accepts. Returns tx count. */
  async setCorrelation(a: number, b: number, target: number, current: number): Promise<number> {
    if (corrIndex(a, b) === null) throw new Error("correlation needs two different assets");
    let cur = current;
    let n = 0;
    while (cur !== target) {
      const step = Math.max(-MAX_CORR_STEP_PPM, Math.min(MAX_CORR_STEP_PPM, target - cur));
      cur += step;
      await this.program.methods
        .updateCorrelations([{ a, b, rho: cur }])
        .accountsStrict(this.riskGov())
        .signers([this.admin])
        .rpc();
      n += 1;
    }
    return n;
  }

  setConsent(isActive: boolean, bitmap: bigint): Promise<string> {
    return this.program.methods
      .updateUserConsent(isActive, bn(bitmap))
      .accountsStrict({
        userConsent: pdas.consent(this.trader.publicKey),
        portfolio: pdas.portfolio(this.trader.publicKey),
        user: this.trader.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([this.trader])
      .rpc();
  }

  /** MOCK ORACLE: the admin writes a price stamped with the on-chain clock. */
  /** B3: `emaMicro` defaults to the price (no throttle). */
  async setMockPrice(assetId: number, priceMicro: bigint, confMicro: bigint, emaMicro: bigint = priceMicro): Promise<string> {
    const { unixTimestamp } = await this.clock();
    return this.program.methods
      .setMockPrice(assetId, bn(priceMicro), bn(confMicro), bn(unixTimestamp), bn(emaMicro))
      .accountsStrict({
        config: pdas.config(),
        mockPrice: pdas.mockPrice(assetId),
        admin: this.admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([this.admin])
      .rpc();
  }

  // ------------------------------------------------------------- positions

  submitSnapshot(s: SnapshotInput): Promise<string> {
    const v = VENUES[s.venueIndex];
    const id = venueIdBytes(v.name);
    const user = this.trader.publicKey;
    return this.program.methods
      .submitPositionSnapshot([...id], s.assetId, bn(s.notional), s.isLong, bn(s.initialMargin), bn(s.maintenanceMargin), bn(0))
      .accountsStrict({
        config: pdas.config(),
        venueRegistration: pdas.venue(id),
        userConsent: pdas.consent(user),
        portfolio: pdas.portfolio(user),
        posSnapshot: pdas.snapshot(user, id, s.assetId),
        user,
        venueAuthority: v.key.publicKey,
        payer: v.key.publicKey,
        systemProgram: SystemProgram.programId,
            riskModel: pdas.riskModel(),
      })
      .signers([v.key])
      .rpc();
  }

  closeSnapshot(venueIndex: number, assetId: number): Promise<string> {
    const v = VENUES[venueIndex];
    const id = venueIdBytes(v.name);
    const user = this.trader.publicKey;
    return this.program.methods
      .closePositionSnapshot()
      .accountsStrict({
        venueRegistration: pdas.venue(id),
        userConsent: pdas.consent(user),
        portfolio: pdas.portfolio(user),
        posSnapshot: pdas.snapshot(user, id, assetId),
        rentReceiver: v.key.publicKey,
        closer: v.key.publicKey,
      })
      .signers([v.key])
      .rpc();
  }

  // ------------------------------------------------------------- portfolio

  private venueName(index: number): string {
    const v = VENUES.find((x) => x.index === index);
    if (!v) throw new Error(`no dev venue with index ${index}`);
    return v.name;
  }

  /** Remaining accounts: every registered snapshot, then one oracle per leg. */
  refreshRemaining(pf: PortfolioView): PublicKey[] {
    const user = this.trader.publicKey;
    const snaps = pf.registered.map((k) => pdas.snapshot(user, venueIdBytes(this.venueName(k.venueIndex)), k.assetId));
    return [...snaps, ...pf.registered.map((k) => pdas.mockPrice(k.assetId))];
  }

  refresh(pf: PortfolioView): Promise<string> {
    const user = this.trader.publicKey;
    return this.program.methods
      .refreshPortfolio()
      .accountsStrict({
        config: pdas.config(),
        riskModel: pdas.riskModel(),
        userConsent: pdas.consent(user),
        portfolio: pdas.portfolio(user),
      })
      .remainingAccounts(this.refreshRemaining(pf).map((pubkey) => ({ pubkey, isSigner: false, isWritable: false })))
      .rpc();
  }

  invalidate(pf: PortfolioView, withOracles: boolean): Promise<string> {
    const user = this.trader.publicKey;
    const oracles = withOracles ? pf.legs.map((l) => pdas.mockPrice(l.assetId)) : [];
    return this.program.methods
      .invalidatePortfolio()
      .accountsStrict({
        config: pdas.config(),
        riskModel: pdas.riskModel(),
        userConsent: pdas.consent(user),
        portfolio: pdas.portfolio(user),
      })
      .remainingAccounts(oracles.map((pubkey) => ({ pubkey, isSigner: false, isWritable: false })))
      .rpc();
  }
}

/** The current on-chain correlation between two assets (1e6 scale). */
export function currentRho(rm: RiskModelView | null, a: number, b: number): number {
  const k = corrIndex(a, b);
  return rm && k !== null ? rm.corr[k] ?? 0 : 0;
}

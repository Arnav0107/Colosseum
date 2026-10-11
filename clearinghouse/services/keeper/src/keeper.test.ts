import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyFailure, decide, PortfolioState, ProtocolState } from "./decide";
import { ChainAdapter, Keeper, KeeperTxError, PortfolioRef } from "./keeper";

const proto = (o: Partial<ProtocolState> = {}): ProtocolState => ({ slot: 1_000n, configEpoch: 3n, paused: false, ...o });
const pf = (o: Partial<PortfolioState> = {}): PortfolioState => ({
  version: 5n,
  status: 1,
  numRegistered: 2,
  riskEpoch: 3n,
  expiresSlot: 4_000n,
  counted: true,
  ...o,
});

test("decide: refresh only when needed, release when credit must stop counting", () => {
  const M = 300n;
  assert.equal(decide(pf(), proto(), M).kind, "none");
  assert.equal(decide(pf({ expiresSlot: 1_300n }), proto(), M).kind, "refresh"); // within margin
  assert.equal(decide(pf({ status: 0, counted: false }), proto(), M).kind, "refresh"); // never computed
  assert.equal(decide(pf({ status: 2 }), proto(), M).kind, "refresh"); // invalidated by an input change
  assert.equal(decide(pf({ riskEpoch: 2n }), proto(), M).kind, "refresh"); // epoch bumped
  assert.equal(decide(pf({ numRegistered: 0, status: 2, counted: false }), proto(), M).kind, "none");
  assert.equal(decide(pf({ numRegistered: 0, status: 2, counted: true }), proto(), M).kind, "release");
  // Paused: no refresh, but expired / stale / invalid counted credit is released.
  assert.equal(decide(pf(), proto({ paused: true }), M).kind, "none");
  assert.equal(decide(pf({ expiresSlot: 999n }), proto({ paused: true }), M).kind, "release");
  assert.equal(decide(pf({ status: 2 }), proto({ paused: true }), M).kind, "release");
  assert.equal(decide(pf({ status: 2, counted: false }), proto({ paused: true }), M).kind, "none");
});

test("classifyFailure", () => {
  assert.equal(classifyFailure("InvalidationNotJustified"), "benign-race");
  assert.equal(classifyFailure("SnapshotStale"), "inputs-not-ready");
  assert.equal(classifyFailure("PriceStale"), "inputs-not-ready");
  assert.equal(classifyFailure("Unauthorized"), "unexpected");
  assert.equal(classifyFailure(null), "unexpected");
});

/** A chain that applies the program's rules for refresh and invalidation. */
class FakeChain implements ChainAdapter {
  slot = 1_000n;
  epoch = 3n;
  paused = false;
  ttl = 3_000n;
  refreshes = 0;
  releases = 0;
  readonly book = new Map<string, PortfolioState & { inputsReady: boolean }>();

  /** Lets concurrent callers interleave between reads and writes. */
  private yieldNow = () => new Promise<void>((r) => setImmediate(r));

  outstanding(): number {
    return [...this.book.values()].filter((p) => p.counted).length;
  }
  async protocol(): Promise<ProtocolState> {
    await this.yieldNow();
    return { slot: this.slot, configEpoch: this.epoch, paused: this.paused };
  }
  async portfolios(): Promise<PortfolioRef[]> {
    await this.yieldNow();
    return [...this.book.entries()].map(([address, s]) => ({ address, user: address, state: { ...s } }));
  }
  async refresh(p: PortfolioRef): Promise<string> {
    await this.yieldNow();
    const s = this.book.get(p.address)!;
    if (this.paused) throw new KeeperTxError("ProgramPaused", "paused");
    if (!s.inputsReady) throw new KeeperTxError("SnapshotStale", "stale");
    // release() then recount: a repeated refresh never double-counts.
    Object.assign(s, { status: 1, riskEpoch: this.epoch, expiresSlot: this.slot + this.ttl, counted: true, version: s.version + 1n });
    this.refreshes += 1;
    return `refresh-${this.refreshes}`;
  }
  async release(p: PortfolioRef): Promise<string> {
    await this.yieldNow();
    const s = this.book.get(p.address)!;
    const justified = s.status !== 1 ? s.counted : s.riskEpoch !== this.epoch || this.slot > s.expiresSlot;
    if (!justified) throw new KeeperTxError("InvalidationNotJustified", "no reason");
    Object.assign(s, { status: s.status === 1 ? 2 : s.status, counted: false, version: s.version + 1n });
    this.releases += 1;
    return `release-${this.releases}`;
  }
}

const OPTS = { refreshMarginSlots: 300n, maxActionsPerTick: 100 };

function seeded(): FakeChain {
  const c = new FakeChain();
  c.book.set("fresh", { ...pf(), inputsReady: true });
  c.book.set("expiring", { ...pf({ expiresSlot: 1_200n }), inputsReady: true });
  c.book.set("invalidated", { ...pf({ status: 2 }), inputsReady: true });
  c.book.set("stale-inputs", { ...pf({ expiresSlot: 900n }), inputsReady: false });
  c.book.set("empty", { ...pf({ numRegistered: 0, status: 2, counted: false }), inputsReady: true });
  return c;
}

test("one keeper: refreshes what needs it and releases what cannot be refreshed", async () => {
  const c = seeded();
  const s = await new Keeper(c, OPTS).tick();
  assert.equal(s.unexpected, 0);
  assert.equal(c.book.get("expiring")!.expiresSlot, 4_000n);
  assert.equal(c.book.get("invalidated")!.status, 1);
  const stale = c.book.get("stale-inputs")!;
  assert.equal(stale.counted, false, "expired credit with stale inputs stops counting");
  assert.equal(stale.status, 2);
  assert.equal(c.book.get("fresh")!.expiresSlot, 4_000n, "fresh credit untouched");
});

test("two keepers at once converge to the same state as one, with no unexpected failures", async () => {
  const single = seeded();
  await new Keeper(single, OPTS).tick();

  const shared = seeded();
  const k1 = new Keeper(shared, OPTS);
  const k2 = new Keeper(shared, OPTS);
  const [a, b] = await Promise.all([k1.tick(), k2.tick()]);
  assert.equal(a.unexpected + b.unexpected, 0);
  // Same economic state. (`version` may differ: a duplicate refresh is a
  // second, identical computation and the program counts it.)
  const econ = ({ version: _v, ...rest }: PortfolioState & { inputsReady: boolean }) => rest;
  for (const [k, v] of single.book) assert.deepEqual(econ(shared.book.get(k)!), econ(v), `portfolio ${k}`);
  assert.equal(shared.outstanding(), single.outstanding(), "counters identical: nothing double-counted");
  // The duplicate release of stale credit is recognised as a race, not an error.
  assert.ok(a.benignRaces + b.benignRaces + a.inputsNotReady + b.inputsNotReady >= 1);

  // Next round: nothing to do except the stale-input portfolio, which both
  // instances now back off from instead of paying a failed fee every tick.
  const [a2, b2] = await Promise.all([k1.tick(), k2.tick()]);
  assert.equal(a2.attempted + b2.attempted, 0);
  assert.equal(a2.backedOff + b2.backedOff, 2);
});

test("backoff ends as soon as the portfolio's inputs change", async () => {
  const c = seeded();
  const k = new Keeper(c, OPTS);
  await k.tick();
  assert.equal((await k.tick()).backedOff, 1);
  // A venue re-reports (the program bumps the version) and inputs are fresh again.
  const s = c.book.get("stale-inputs")!;
  Object.assign(s, { inputsReady: true, version: s.version + 1n });
  const t = await k.tick();
  assert.equal(t.backedOff, 0);
  assert.equal(c.book.get("stale-inputs")!.status, 1);
});

test("paused: never refreshes, still releases expired credit", async () => {
  const c = seeded();
  c.paused = true;
  c.slot = 5_000n; // everything expired
  const s = await new Keeper(c, OPTS).tick();
  assert.equal(c.refreshes, 0);
  assert.equal(s.unexpected, 0);
  assert.equal(c.outstanding(), 0);
});

test("respects the per-tick action budget and refuses overlapping ticks", async () => {
  const c = seeded();
  const k = new Keeper(c, { ...OPTS, maxActionsPerTick: 1 });
  const first = k.tick();
  await assert.rejects(k.tick(), /already running/);
  assert.equal((await first).attempted, 1);
});

test("hooks receive every action and tick; metrics accumulate", async () => {
  const c = seeded();
  const actions: string[] = [];
  let ticks = 0;
  const k = new Keeper(c, OPTS, { onAction: (r) => actions.push(`${r.portfolio}:${r.outcome}`), onTick: () => ticks++ });
  await k.tick();
  assert.equal(ticks, 1);
  assert.ok(actions.includes("expiring:ok"));
  assert.ok(actions.includes("stale-inputs:released-after-failure"));
  assert.equal(k.metrics().ticks, 1);
  assert.equal(k.metrics().actions, actions.length);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { Candidate, classifyFailure, decide, orderCandidates, PortfolioState, ProtocolState, shardOf } from "./decide";
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
  /** Every attempt: who (instance tag), on what, at which on-chain version it read. */
  readonly attempts: { by: string; address: string; kind: string; readVersion: bigint; ok: boolean }[] = [];
  /** Active portfolios observed past their expiry (credit lapsed before a refresh). */
  readonly lapses: { address: string; slot: bigint; expiresSlot: bigint }[] = [];

  checkLapses(): void {
    for (const [address, s] of this.book) {
      if (s.status === 1 && this.slot > s.expiresSlot) this.lapses.push({ address, slot: this.slot, expiresSlot: s.expiresSlot });
    }
  }
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
  async refresh(p: PortfolioRef, by = "?"): Promise<string> {
    await this.yieldNow();
    const s = this.book.get(p.address)!;
    const rec = { by, address: p.address, kind: "refresh", readVersion: p.state.version, ok: false };
    this.attempts.push(rec);
    if (this.paused) throw new KeeperTxError("ProgramPaused", "paused");
    if (!s.inputsReady) throw new KeeperTxError("SnapshotStale", "stale");
    rec.ok = true;
    // release() then recount: a repeated refresh never double-counts.
    Object.assign(s, { status: 1, riskEpoch: this.epoch, expiresSlot: this.slot + this.ttl, counted: true, version: s.version + 1n });
    this.refreshes += 1;
    return `refresh-${this.refreshes}`;
  }
  async release(p: PortfolioRef, by = "?"): Promise<string> {
    await this.yieldNow();
    const s = this.book.get(p.address)!;
    const rec = { by, address: p.address, kind: "release", readVersion: p.state.version, ok: false };
    this.attempts.push(rec);
    const justified = s.status !== 1 ? s.counted : s.riskEpoch !== this.epoch || this.slot > s.expiresSlot;
    if (!justified) throw new KeeperTxError("InvalidationNotJustified", "no reason");
    rec.ok = true;
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

/** The same chain seen through one keeper instance, so attempts are attributed. */
const viewAs = (c: FakeChain, by: string): ChainAdapter => ({
  protocol: () => c.protocol(),
  portfolios: () => c.portfolios(),
  refresh: (p) => c.refresh(p, by),
  release: (p) => c.release(p, by),
});

const cand = (address: string, kind: "refresh" | "release", expiresSlot: bigint): Candidate<string> => ({
  ref: address,
  address,
  expiresSlot,
  action: { kind, why: "test" },
});

test("orderCandidates: releases first, then refreshes by expiresSlot, ties by address", () => {
  const got = orderCandidates([
    cand("d", "refresh", 500n),
    cand("z", "release", 9_999n),
    cand("b", "refresh", 100n),
    cand("a", "refresh", 500n),
    cand("c", "release", 1n),
  ]).map((c) => c.address);
  assert.deepEqual(got, ["c", "z", "b", "a", "d"]);
  // Input order never matters.
  const rev = orderCandidates([cand("a", "refresh", 500n), cand("d", "refresh", 500n), cand("b", "refresh", 100n)]);
  assert.deepEqual(
    rev.map((c) => c.address),
    ["b", "a", "d"]
  );
});

/**
 * N = 3 × cap portfolios. Expiry order is the reverse of address order, and
 * the earliest group expires before an address-order keeper would reach it:
 * it would service p00–p03 at slot 1000, p04–p07 at 1100 and p08–p11 at 1200,
 * after p08–p11 lapse at 1150.
 */
function starvationBook(cap: number): FakeChain {
  const c = new FakeChain();
  for (let i = 0; i < 3 * cap; i++) {
    const expiresSlot = [1_290n, 1_250n, 1_150n][Math.floor(i / cap)];
    c.book.set(`p${String(i).padStart(2, "0")}`, { ...pf({ expiresSlot }), inputsReady: true });
  }
  return c;
}

test("N = 3 × cap over 100 ticks: every portfolio is refreshed before expiry and none starves", async () => {
  const cap = 4;
  const c = starvationBook(cap);
  const k = new Keeper(c, { refreshMarginSlots: 300n, maxActionsPerTick: cap });
  let overBudgetSeen = 0;
  for (let t = 0; t < 100; t++) {
    c.checkLapses();
    const s = await k.tick();
    overBudgetSeen += s.overBudget;
    assert.equal(s.unexpected, 0);
    assert.ok(s.attempted <= cap, "budget respected");
    c.slot += 100n;
  }
  c.checkLapses();
  assert.ok(overBudgetSeen > 0, "the budget actually bound (otherwise the test proves nothing)");
  assert.deepEqual(c.lapses, [], "no active credit ever passed its expiry");
  const per = new Map<string, number>();
  for (const a of c.attempts) per.set(a.address, (per.get(a.address) ?? 0) + 1);
  const counts = [...c.book.keys()].map((a) => per.get(a) ?? 0);
  // 10,000 slots at a 3,000-slot TTL: each portfolio needs at least 3 refreshes.
  assert.ok(Math.min(...counts) >= 3, `every portfolio serviced repeatedly: ${counts}`);
  assert.ok(Math.max(...counts) - Math.min(...counts) <= 1, `even service, no starvation: ${counts}`);
});

test("N = 3 × cap with every expiry synchronised (worst case) never lapses", async () => {
  const cap = 4;
  const c = new FakeChain();
  for (let i = 0; i < 3 * cap; i++) {
    c.book.set(`q${String(i).padStart(2, "0")}`, { ...pf({ expiresSlot: 1_300n }), inputsReady: true });
  }
  const k = new Keeper(c, { refreshMarginSlots: 300n, maxActionsPerTick: cap });
  for (let t = 0; t < 60; t++) {
    c.checkLapses();
    await k.tick();
    c.slot += 100n;
  }
  assert.deepEqual(c.lapses, []);
});

function concurrentBook(): FakeChain {
  const c = new FakeChain();
  for (let i = 0; i < 12; i++) {
    c.book.set(`r${String(i).padStart(2, "0")}`, { ...pf({ expiresSlot: 1_100n + BigInt(i) * 150n }), inputsReady: true });
  }
  c.book.set("invalidated", { ...pf({ status: 2 }), inputsReady: true });
  c.book.set("epoch-old", { ...pf({ riskEpoch: 2n }), inputsReady: true });
  c.book.set("empty-counted", { ...pf({ numRegistered: 0, status: 2, counted: true }), inputsReady: true });
  return c;
}

const sharded = (index: number, takeoverTicks: number) => ({
  refreshMarginSlots: 300n,
  maxActionsPerTick: 5,
  instance: { index, count: 2, takeoverTicks },
});

test("two sharded keepers running concurrently: no double actions and no failures", async () => {
  const c = concurrentBook();
  const k0 = new Keeper(viewAs(c, "k0"), sharded(0, 2));
  const k1 = new Keeper(viewAs(c, "k1"), sharded(1, 2));
  // Both shards must own something, or this test proves nothing.
  assert.deepEqual([...new Set([...c.book.keys()].map((a) => shardOf(a, 2)))].sort(), [0, 1]);
  for (let t = 0; t < 80; t++) {
    c.checkLapses();
    const [a, b] = await Promise.all([k0.tick(), k1.tick()]);
    assert.equal(a.unexpected + b.unexpected, 0);
    assert.equal(a.benignRaces + b.benignRaces, 0, "no instance lost a race");
    assert.equal(a.inputsNotReady + b.inputsNotReady, 0);
    c.slot += 100n;
  }
  assert.deepEqual(c.lapses, []);
  assert.ok(c.attempts.length > 20, "the run did real work");
  assert.ok(c.attempts.every((x) => x.ok), "every attempt succeeded");
  const seen = new Map<string, string>();
  for (const x of c.attempts) {
    const key = `${x.address}@${x.readVersion}`;
    assert.ok(!seen.has(key), `double action on ${key} by ${seen.get(key)} and ${x.by}`);
    seen.set(key, x.by);
  }
  assert.equal(new Set(c.attempts.map((x) => x.by)).size, 2, "both instances did work");
  assert.equal(c.outstanding(), [...c.book.values()].filter((p) => p.status === 1).length, "counters consistent");
});

test("sharded keepers: if one instance dies, the other takes over before anything lapses", async () => {
  const c = concurrentBook();
  const live = new Keeper(viewAs(c, "k0"), sharded(0, 1));
  for (let t = 0; t < 80; t++) {
    c.checkLapses();
    assert.equal((await live.tick()).unexpected, 0);
    c.slot += 100n;
  }
  assert.deepEqual(c.lapses, []);
  const orphaned = [...c.book.keys()].filter((a) => shardOf(a, 2) === 1);
  assert.ok(orphaned.length > 0);
  for (const a of orphaned) assert.ok(c.attempts.some((x) => x.address === a && x.ok), `${a} taken over`);
});

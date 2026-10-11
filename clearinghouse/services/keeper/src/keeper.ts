import { Action, Candidate, classifyFailure, decide, FailureClass, orderCandidates, PortfolioState, ProtocolState, shardOf } from "./decide";

export interface PortfolioRef {
  /** Portfolio account address (base58). */
  address: string;
  /** Owner of the portfolio (base58). */
  user: string;
  state: PortfolioState;
}

/** A failed transaction, carrying the program error name when there is one. */
export class KeeperTxError extends Error {
  constructor(
    readonly code: string | null,
    message: string
  ) {
    super(message);
    this.name = "KeeperTxError";
  }
}

/** Everything the keeper needs from chain. Implemented over Anchor in chain.ts. */
export interface ChainAdapter {
  protocol(): Promise<ProtocolState>;
  portfolios(): Promise<PortfolioRef[]>;
  refresh(p: PortfolioRef): Promise<string>;
  release(p: PortfolioRef): Promise<string>;
}

export interface ActionResult {
  portfolio: string;
  user: string;
  action: Action["kind"];
  why: string;
  outcome: "ok" | FailureClass | "released-after-failure";
  signature?: string;
  error?: string;
}

export interface TickStats {
  slot: string;
  portfolios: number;
  attempted: number;
  ok: number;
  benignRaces: number;
  inputsNotReady: number;
  unexpected: number;
  backedOff: number;
  /** Needed an action but belongs to another instance that is not yet overdue. */
  deferredToOwner: number;
  /** Needed an action but over `maxActionsPerTick`; first in line next tick. */
  overBudget: number;
  durationMs: number;
}

/** Monitoring hooks. The defaults (see index.ts) write one JSON line per event. */
export interface KeeperHooks {
  onTick?(s: TickStats): void;
  onAction?(r: ActionResult): void;
  onError?(context: string, e: unknown): void;
}

export interface KeeperOptions {
  /** Refresh credit this many slots before it expires. */
  refreshMarginSlots: bigint;
  /** Bound on transactions per tick (rate limit / fee budget). */
  maxActionsPerTick: number;
  /**
   * After a failed attempt, skip that portfolio for this many ticks unless its
   * on-chain version changes (any new report, consent change or computation
   * bumps it). Stops paying a failed fee every tick for stale inputs.
   */
  backoffTicks?: number;
  /**
   * Running several instances: each portfolio has one owner
   * (`shardOf(address, count) === index`), which acts at once. Another
   * instance acts only after it has seen the same portfolio, at the same
   * on-chain version, still needing an action for `takeoverTicks` ticks,
   * i.e. when the owner looks dead. Healthy instances never duplicate an
   * action; a dead one is covered after `takeoverTicks`. Keep
   * `takeoverTicks × interval` well below `refreshMarginSlots`.
   * Omitted: this instance owns every portfolio.
   */
  instance?: { index: number; count: number; takeoverTicks: number };
}

export interface Metrics {
  ticks: number;
  actions: number;
  ok: number;
  benignRaces: number;
  inputsNotReady: number;
  unexpected: number;
  lastTickSlot: string | null;
  lastTickAt: string | null;
}

const describe = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export class Keeper {
  private readonly metricsState: Metrics = {
    ticks: 0,
    actions: 0,
    ok: 0,
    benignRaces: 0,
    inputsNotReady: 0,
    unexpected: 0,
    lastTickSlot: null,
    lastTickAt: null,
  };
  private running = false;
  private tickNo = 0;
  /**
   * Portfolios whose last attempt failed. `version` is the on-chain version
   * first seen *after* the failure (our own release may have bumped it), so
   * only a later input change ends the backoff early. A change landing in
   * that window waits at most `backoffTicks`; meanwhile the credit stays
   * invalid, which is the conservative direction.
   */
  private readonly backoff = new Map<string, { version: bigint | null; untilTick: number }>();
  /** Non-owned portfolios seen needing an action: since which tick, at which version. */
  private readonly pendingSince = new Map<string, { version: bigint; tick: number }>();

  constructor(
    private readonly chain: ChainAdapter,
    private readonly opts: KeeperOptions,
    private readonly hooks: KeeperHooks = {}
  ) {}

  metrics(): Metrics {
    return { ...this.metricsState };
  }

  private async attempt(p: PortfolioRef, action: Action): Promise<ActionResult> {
    const base = { portfolio: p.address, user: p.user, action: action.kind, why: action.why };
    try {
      const signature = action.kind === "refresh" ? await this.chain.refresh(p) : await this.chain.release(p);
      return { ...base, outcome: "ok", signature };
    } catch (e) {
      const code = e instanceof KeeperTxError ? e.code : null;
      const cls = classifyFailure(code);
      if (cls === "inputs-not-ready" && action.kind === "refresh") {
        // Cannot recompute now: at least make sure stale credit stops counting.
        try {
          const signature = await this.chain.release(p);
          return { ...base, outcome: "released-after-failure", signature, error: code ?? describe(e) };
        } catch (e2) {
          const code2 = e2 instanceof KeeperTxError ? e2.code : null;
          const cls2 = classifyFailure(code2);
          // Nothing to release (credit already invalid and uncounted) is fine.
          return { ...base, outcome: cls2 === "benign-race" ? "inputs-not-ready" : cls2, error: code2 ?? describe(e2) };
        }
      }
      return { ...base, outcome: cls, error: code ?? describe(e) };
    }
  }

  /**
   * Whether this instance should act on `p` now (see `KeeperOptions.instance`).
   * Records non-owned portfolios that need an action in `seen`.
   */
  private owns(p: PortfolioRef, seen: Set<string>): boolean {
    const inst = this.opts.instance;
    if (!inst || shardOf(p.address, inst.count) === inst.index) return true;
    seen.add(p.address);
    const since = this.pendingSince.get(p.address);
    if (!since || since.version !== p.state.version) {
      this.pendingSince.set(p.address, { version: p.state.version, tick: this.tickNo });
      return inst.takeoverTicks === 0;
    }
    return this.tickNo - since.tick >= inst.takeoverTicks;
  }

  /** One pass over every portfolio. Never runs concurrently with itself. */
  async tick(): Promise<TickStats> {
    if (this.running) throw new Error("tick already running");
    this.running = true;
    const started = Date.now();
    try {
      const protocol = await this.chain.protocol();
      const portfolios = await this.chain.portfolios();
      const stats: TickStats = {
        slot: protocol.slot.toString(),
        portfolios: portfolios.length,
        attempted: 0,
        ok: 0,
        benignRaces: 0,
        inputsNotReady: 0,
        unexpected: 0,
        backedOff: 0,
        deferredToOwner: 0,
        overBudget: 0,
        durationMs: 0,
      };
      this.tickNo += 1;
      const candidates: Candidate<PortfolioRef>[] = [];
      const stillPending = new Set<string>();
      for (const p of portfolios) {
        const action = decide(p.state, protocol, this.opts.refreshMarginSlots);
        if (action.kind === "none") continue;
        const b = this.backoff.get(p.address);
        if (b && this.tickNo < b.untilTick) {
          if (b.version === null) b.version = p.state.version;
          if (b.version === p.state.version) {
            stats.backedOff += 1;
            continue;
          }
        }
        if (!this.owns(p, stillPending)) {
          stats.deferredToOwner += 1;
          continue;
        }
        candidates.push({ ref: p, address: p.address, expiresSlot: p.state.expiresSlot, action });
      }
      for (const k of this.pendingSince.keys()) if (!stillPending.has(k)) this.pendingSince.delete(k);

      const ordered = orderCandidates(candidates);
      stats.overBudget = Math.max(0, ordered.length - this.opts.maxActionsPerTick);
      for (const { ref: p, action } of ordered.slice(0, this.opts.maxActionsPerTick)) {
        stats.attempted += 1;
        const r = await this.attempt(p, action);
        if (r.outcome === "ok" || r.outcome === "benign-race") this.backoff.delete(p.address);
        else this.backoff.set(p.address, { version: null, untilTick: this.tickNo + 1 + (this.opts.backoffTicks ?? 10) });
        if (r.outcome === "ok" || r.outcome === "released-after-failure") stats.ok += 1;
        else if (r.outcome === "benign-race") stats.benignRaces += 1;
        else if (r.outcome === "inputs-not-ready") stats.inputsNotReady += 1;
        else stats.unexpected += 1;
        this.hooks.onAction?.(r);
      }
      stats.durationMs = Date.now() - started;
      const m = this.metricsState;
      m.ticks += 1;
      m.actions += stats.attempted;
      m.ok += stats.ok;
      m.benignRaces += stats.benignRaces;
      m.inputsNotReady += stats.inputsNotReady;
      m.unexpected += stats.unexpected;
      m.lastTickSlot = stats.slot;
      m.lastTickAt = new Date().toISOString();
      this.hooks.onTick?.(stats);
      return stats;
    } finally {
      this.running = false;
    }
  }

  /** Tick every `intervalMs` until `signal` aborts. A failed tick is reported and retried next time. */
  async run(intervalMs: number, signal: AbortSignal): Promise<void> {
    while (!signal.aborted) {
      try {
        await this.tick();
      } catch (e) {
        this.hooks.onError?.("tick", e);
      }
      await new Promise<void>((resolve) => {
        const t = setTimeout(resolve, intervalMs);
        signal.addEventListener("abort", () => {
          clearTimeout(t);
          resolve();
        });
      });
    }
  }
}

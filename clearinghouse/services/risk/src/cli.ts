/**
 * Offline CLI. Reads committed CSVs and a params JSON; makes no network calls
 * and needs no keys.
 *
 *   node dist/services/risk/src/cli.js calibrate [--data DIR] [--current FILE] [--out FILE]
 *   node dist/services/risk/src/cli.js backtest  [--data DIR] [--current FILE] [--out FILE]
 *   node dist/services/risk/src/cli.js frontier  [--data DIR] [--current FILE] [--out FILE]
 *
 * Defaults are relative to services/risk (where the npm scripts run).
 */
import { mkdirSync, readFileSync, writeFileSync } from "fs";
import * as path from "path";
import { DEFAULT_OPTIONS, effectiveFrom, hypothetical, realizedLoss, runBacktest, summarize, DayResult, DEFAULT_BOOK, creditFor } from "./backtest";
import { calibrate, CurrentParams, renderReport } from "./calibrate";
import { Bar, BAR_MS, DAY_MS, describeKlines, loadDaily, loadKlines20m, REPO_ROOT } from "./data";
import { frontier, paretoFront } from "./frontier";

function arg(name: string, fallback: string): string {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

const REPO = REPO_ROOT;
const json = (o: unknown) => JSON.stringify(o, (_k, v) => (typeof v === "bigint" ? v.toString() : v), 2) + "\n";

/** Months of committed 20-minute data (B2 adds 2021-05, 2022-05, 2022-06). `--months a,b` overrides. */
export const ALL_MONTHS = ["2021-05", "2022-05", "2022-06", "2022-11", "2026-09"];
export const MONTHS = (() => {
  const i = process.argv.indexOf("--months");
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1].split(",") : ALL_MONTHS;
})();

export function loadSeries(dataDir: string, months: string[] = MONTHS): { sol: Bar[]; eth: Bar[]; inputs: ReturnType<typeof describeKlines>[] } {
  const inputs: ReturnType<typeof describeKlines>[] = [];
  const load = (sym: string) =>
    months.flatMap((m) => {
      const f = path.join(dataDir, "klines_20m", `${sym}-${m}.csv`);
      const bars = loadKlines20m(f);
      inputs.push(describeKlines(f, bars, REPO));
      return bars;
    });
  return { sol: load("SOLUSDT"), eth: load("ETHUSDT"), inputs };
}

function cmdCalibrate(): void {
  const dataDir = path.resolve(arg("data", path.join(REPO, "crates/ch_math/tests/data")));
  const current = JSON.parse(readFileSync(path.resolve(arg("current", "params/localnet.json")), "utf8")) as CurrentParams;
  const out = path.resolve(arg("out", "params/calibration.json"));
  const { sol, eth, inputs } = loadSeries(dataDir);
  const c = calibrate(
    [
      { name: "SOL", assetId: 0, bars: sol },
      { name: "ETH", assetId: 1, bars: eth },
    ],
    current
  );
  writeFileSync(out, json({ inputs, current: current.label, ...c }));
  console.log(renderReport(c));
  console.log(`\nwrote ${path.relative(process.cwd(), out)}`);
}

const pct = (x: number) => `${(x * 100).toFixed(2)}%`;
const usd = (x: number) => `$${x.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

function cmdBacktest(): void {
  const dataDir = path.resolve(arg("data", path.join(REPO, "crates/ch_math/tests/data")));
  const current = JSON.parse(readFileSync(path.resolve(arg("current", "params/localnet.json")), "utf8")) as CurrentParams;
  const out = path.resolve(arg("out", path.join(REPO, "docs/backtest/results.json")));
  const { sol, eth, inputs } = loadSeries(dataDir);

  // Primary: H = 1 day (conservative proxy for a ~20-minute credit lifetime).
  const daily = runBacktest(sol, eth, current, DEFAULT_OPTIONS);
  const byMonth = (rows: DayResult[], m: string) => rows.filter((r) => r.date.startsWith(m));
  const dailySummary = [summarize("all days", daily), ...MONTHS.map((m) => summarize(m, byMonth(daily, m)))];

  // Supplementary: H = one 20-minute window, every window (bar data, not minute-level).
  const bar = runBacktest(sol, eth, current, { ...DEFAULT_OPTIONS, horizonMs: BAR_MS, stepMs: BAR_MS });
  const barSummary = [summarize("all windows", bar), ...MONTHS.map((m) => summarize(m, byMonth(bar, m)))];
  const barBreaches = bar.filter((r) => r.breachWithCh || r.breachBaseline);

  // Historical replay: the book carried through 2–15 Nov 2022 (one cumulative window).
  const from = Date.UTC(2022, 10, 2);
  const to = Date.UTC(2022, 10, 16);
  const replayLoss = realizedLoss(sol, eth, from, to, DEFAULT_BOOK);
  const replayParams = daily.find((r) => r.date === "2022-11-02")!;
  const replay = {
    label: "HISTORICAL REPLAY: book opened 2022-11-02 00:00 UTC, held to 2022-11-15 24:00 UTC (14 days; NOT one margin horizon)",
    paramsInForce: replayParams.paramsSource,
    sumIm: replayParams.sumIm,
    credit: replayParams.credit,
    postCredit: replayParams.postCredit,
    loss: replayLoss,
    breachWithCh: replayLoss.pathLoss > replayParams.postCredit,
    breachBaseline: replayLoss.pathLoss > replayParams.sumIm,
  };
  // The quoted figures (SOL ≈ −54%, ETH ≈ −20% for 2–15 Nov) checked against both CSVs.
  const dailyCsv = loadDaily(path.join(dataDir, "binance_nov2022_daily.csv"));
  const close = (d: string) => dailyCsv.find((r) => r.date === d)!;
  const verify = {
    daily_csv_close_2022_11_01_to_11_15: { sol: close("2022-11-15").solClose / close("2022-11-01").solClose - 1, eth: close("2022-11-15").ethClose / close("2022-11-01").ethClose - 1 },
    daily_csv_close_2022_11_02_to_11_15: { sol: close("2022-11-15").solClose / close("2022-11-02").solClose - 1, eth: close("2022-11-15").ethClose / close("2022-11-02").ethClose - 1 },
    klines_20m_open_2022_11_02_to_close_11_15: { sol: replayLoss.solMove, eth: replayLoss.ethMove },
  };

  // HYPOTHETICAL decorrelation: SOL −30% and ETH +15% within one day (chosen, not observed).
  const last = daily[daily.length - 1];
  const hypo = [
    hypothetical("HYPOTHETICAL decorrelation, initial params", -0.3, 0.15, effectiveFrom(current), current.label, current),
    hypothetical(`HYPOTHETICAL decorrelation, params in force on ${last.date}`, -0.3, 0.15, last.params, `calibrated, in force ${last.date}`, current),
  ];

  mkdirSync(path.dirname(out), { recursive: true });
  writeFileSync(
    out,
    json({
      command: "cd services/risk && npm run backtest",
      inputs,
      book: DEFAULT_BOOK,
      horizonCaveat: "Credit lives ~20 minutes (credit_ttl_slots ≤ 3,000). The primary horizon H = 1 day is a conservative proxy. The supplementary H = 20 min run uses 20-minute bars (high/low/close), not minute data.",
      method: DEFAULT_OPTIONS.method,
      initialParams: current,
      daily: { horizon: "1 day", summary: dailySummary, rows: daily },
      bar20m: { horizon: "20 minutes", summary: barSummary, breaches: barBreaches },
      replay,
      verify,
      hypothetical: hypo,
      sanity: { creditAtInitialParams: creditFor(effectiveFrom(current), DEFAULT_BOOK, current.credit, false) },
    })
  );

  const table = (rows: ReturnType<typeof summarize>[]) => {
    const l = ["| sample | evaluations | breaches with CH | pre-A3 (no hedge break) | baseline (Σ IM only) | worst margin of safety (with CH) | pre-A2 reference (no floor) | B3 EVAL: with throttle (evaluations throttled) |", "| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |"];
    for (const s of rows) l.push(`| ${s.label} | ${s.days} | ${s.breachesWithCh} | ${s.breachesWithChNoHedgeBreak} | ${s.breachesBaseline} | ${usd(s.worstMarginOfSafety)} (${s.worstMarginDate}) | ${s.breachesNoFloor} | ${s.breachesThrottled} (${s.throttled}) |`);
    return l.join("\n");
  };
  console.log("## H = 1 day (conservative proxy)\n");
  console.log(table(dailySummary));
  console.log("\n## H = 20 minutes (bar-level supplementary)\n");
  console.log(table(barSummary));
  console.log("\n## Breach days (H = 1 day)\n");
  console.log("| date | params (data cutoff) | SOL / ETH shock | R_p | credit | post-credit req | realized loss (path) | loss (closes) | SOL | ETH | with CH | baseline |");
  console.log("| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |");
  for (const r of daily.filter((x) => x.breachWithCh || x.breachBaseline || x.breachWithChNoHedgeBreak)) {
    console.log(
      `| ${r.date} | ${r.paramsSource} (${r.dataCutoff.slice(0, 10)}) | ${r.params.sol.shock_bps} / ${r.params.eth.shock_bps} bps | ${usd(r.portfolioRisk)} | ${usd(r.credit)} | ${usd(r.postCredit)} | ${usd(r.loss.pathLoss)} | ${usd(r.loss.closeLoss)} | ${pct(r.loss.solMove)} | ${pct(r.loss.ethMove)} | ${r.breachWithCh ? "BREACH" : "ok"} | ${r.breachBaseline ? "BREACH" : "ok"} |`
    );
  }
  // Information only (NOT applied): the smallest haircut that would have removed each breach.
  for (const r of daily.filter((x) => x.breachWithCh)) {
    let lo = current.credit.haircut_bps;
    let hi = 10_000;
    while (lo < hi) {
      const mid = Math.floor((lo + hi) / 2);
      const c = Number(creditFor(r.params, DEFAULT_BOOK, { ...current.credit, haircut_bps: mid }, false)) / 1e6;
      if (r.sumIm - c >= r.loss.pathLoss) hi = mid;
      else lo = mid + 1;
    }
    console.log(`\n${r.date}: haircut that would have removed this breach (NOT applied): >= ${lo} bps (in force: ${current.credit.haircut_bps} bps)`);
  }
  console.log("\n## Historical replay\n");
  console.log(`${replay.label}: Σ IM ${usd(replay.sumIm)}, credit ${usd(replay.credit)}, post-credit ${usd(replay.postCredit)}, worst loss ${usd(replay.loss.pathLoss)} (closes ${usd(replay.loss.closeLoss)}); with CH ${replay.breachWithCh ? "BREACH" : "ok"}, baseline ${replay.breachBaseline ? "BREACH" : "ok"}`);
  console.log("\n## Verification of quoted moves\n");
  for (const [k, v] of Object.entries(verify)) console.log(`${k}: SOL ${pct(v.sol)}, ETH ${pct(v.eth)}`);
  console.log("\n## HYPOTHETICAL\n");
  for (const h of hypo) {
    console.log(`${h.label}: SOL ${pct(h.solMove)}, ETH ${pct(h.ethMove)}; loss ${usd(h.loss)}; Σ IM ${usd(h.sumIm)}; credit ${usd(h.credit)} -> post-credit ${usd(h.postCredit)} (${h.breachWithCh ? "BREACH" : "ok"}); pre-A3 (no hedge break) credit ${usd(h.creditNoHedgeBreak)} -> ${usd(h.postCreditNoHedgeBreak)} (${h.breachWithChNoHedgeBreak ? "BREACH" : "ok"}); baseline ${h.breachBaseline ? "BREACH" : "ok"}`);
  }
  console.log(`\nwrote ${path.relative(process.cwd(), out)}`);
}


function cmdFrontier(): void {
  const dataDir = path.resolve(arg("data", path.join(REPO, "crates/ch_math/tests/data")));
  const current = JSON.parse(readFileSync(path.resolve(arg("current", "params/localnet.json")), "utf8")) as CurrentParams;
  const out = path.resolve(arg("out", path.join(REPO, "docs/backtest/frontier.json")));
  const { sol, eth, inputs } = loadSeries(dataDir);
  const daily = runBacktest(sol, eth, current, DEFAULT_OPTIONS);
  const bar = runBacktest(sol, eth, current, { ...DEFAULT_OPTIONS, horizonMs: BAR_MS, stepMs: BAR_MS });
  const points = frontier(daily, bar, current, DEFAULT_BOOK);
  const front = paretoFront(points);
  mkdirSync(path.dirname(out), { recursive: true });
  writeFileSync(
    out,
    json({
      command: "cd services/risk && npm run frontier",
      inputs,
      book: DEFAULT_BOOK,
      note: "Calibrated risk parameters in force each day are those of the backtest; only haircut_bps and hedge_break_bps are swept. Main columns include the floor credit <= (sum IM - R_p)+ (A2, on chain); NoFloor fields are the pre-A2 reference.",
      points,
      pareto: front.map((p) => ({ haircutBps: p.haircutBps, hedgeBreakBps: p.hedgeBreakBps })),
    })
  );
  const f = (x: number) => x.toFixed(2);
  console.log("| haircut | hedge_break | freed % ΣIM (H=1d) | breaches H=1d (/day) | freed % (H=20m) | breaches H=20m (/day) | pre-A2: freed % (H=1d) | pre-A2: breaches H=1d | pre-A2: breaches H=20m | Pareto |");
  console.log("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
  for (const p of points) {
    const onFront = front.includes(p) ? "yes" : "";
    console.log(
      `| ${p.haircutBps} | ${p.hedgeBreakBps} | ${f(p.freedPctDaily)} | ${p.breachesDaily} (${p.breachRateDaily.toFixed(4)}) | ${f(p.freedPct20m)} | ${p.breaches20m} (${p.breachRate20m.toFixed(4)}) | ${f(p.freedPctDailyNoFloor)} | ${p.breachesDailyNoFloor} | ${p.breaches20mNoFloor} | ${onFront} |`
    );
  }
  console.log(`
${points[0].daysDaily} daily evaluations, ${points[0].windows20m} 20-minute windows`);
  console.log(`
wrote ${path.relative(process.cwd(), out)}`);
}

function cmdThrottle(): void {
  // B3 evaluation: sweep the throttle thresholds; report freed margin and breaches.
  const dataDir = path.resolve(arg("data", path.join(REPO, "crates/ch_math/tests/data")));
  const current = JSON.parse(readFileSync(path.resolve(arg("current", "params/localnet.json")), "utf8")) as CurrentParams;
  const out = path.resolve(arg("out", path.join(REPO, "docs/backtest/throttle.json")));
  const { sol, eth } = loadSeries(dataDir);
  const settings: [number, number][] = [[0, 0], [100, 300], [50, 300], [50, 200], [50, 150], [25, 125], [10, 110], [0, 0]].slice(0, 7) as [number, number][];
  const results = settings.map(([startBps, fullBps]) => {
    const t = { startBps, fullBps };
    const daily = runBacktest(sol, eth, current, { ...DEFAULT_OPTIONS, throttle: t });
    const bar = runBacktest(sol, eth, current, { ...DEFAULT_OPTIONS, horizonMs: BAR_MS, stepMs: BAR_MS, throttle: t });
    const freed = (rows: DayResult[]) => (100 * rows.reduce((a, r) => a + r.creditThrottled / r.sumIm, 0)) / rows.length;
    const d = summarize("all", daily);
    const b = summarize("all", bar);
    const nov9 = daily.find((r) => r.date === "2022-11-09")!;
    return {
      startBps, fullBps,
      freedPctDaily: freed(daily), freedPct20m: freed(bar),
      breachesDaily: d.breachesThrottled, breaches20m: b.breachesThrottled,
      throttledDays: d.throttled, throttledWindows: b.throttled,
      nov9ScalePpm: nov9.throttleScalePpm, nov9Credit: nov9.creditThrottled, nov9Loss: nov9.loss.pathLoss,
    };
  });
  mkdirSync(path.dirname(out), { recursive: true });
  writeFileSync(out, json({ command: "cd services/risk && npm run throttle", note: "B3 evaluation. EMA = exponential average of 20-minute closes, 1-hour time constant (stand-in for Pyth ema_price). Confidence not modelled. startBps 0 = throttle off.", results }));
  console.log("| start bps | full bps | freed % (H=1d) | breaches H=1d | freed % (H=20m) | breaches H=20m | days throttled | windows throttled | 2022-11-09 scale | 2022-11-09 credit |");
  console.log("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
  for (const r of results) {
    console.log(`| ${r.startBps || "off"} | ${r.fullBps || "-"} | ${r.freedPctDaily.toFixed(2)} | ${r.breachesDaily} | ${r.freedPct20m.toFixed(2)} | ${r.breaches20m} | ${r.throttledDays} | ${r.throttledWindows} | ${(r.nov9ScalePpm / 1e4).toFixed(1)}% | ${usd(r.nov9Credit)} |`);
  }
  console.log(`
2022-11-09 realized loss ${usd(results[0].nov9Loss)}; credit must be <= ${usd(20_000 - results[0].nov9Loss)} to avoid the breach`);
  console.log(`
wrote ${path.relative(process.cwd(), out)}`);
}

function cmdLeverage(): void {
  // B1 extension: the same backtest at 2x, 3x and 5x leverage (IM = 1/L, MM = IM/2).
  const dataDir = path.resolve(arg("data", path.join(REPO, "crates/ch_math/tests/data")));
  const current = JSON.parse(readFileSync(path.resolve(arg("current", "params/localnet.json")), "utf8")) as CurrentParams;
  const out = path.resolve(arg("out", path.join(REPO, "docs/backtest/leverage.json")));
  const { sol, eth } = loadSeries(dataDir);
  const books = [
    { leverage: 2, book: { notionalUsd: 50_000, imBps: 5_000, mmBps: 2_500 } },
    { leverage: 3, book: { notionalUsd: 50_000, imBps: 3_334, mmBps: 1_667 } },
    { leverage: 5, book: { notionalUsd: 50_000, imBps: 2_000, mmBps: 1_000 } },
  ];
  const results = books.map(({ leverage, book }) => {
    const daily = runBacktest(sol, eth, current, { ...DEFAULT_OPTIONS, book });
    const bar = runBacktest(sol, eth, current, { ...DEFAULT_OPTIONS, book, horizonMs: BAR_MS, stepMs: BAR_MS });
    const freed = (rows: DayResult[]) => (100 * rows.reduce((a, r) => a + r.credit / r.sumIm, 0)) / rows.length;
    const withCredit = (rows: DayResult[]) => rows.filter((r) => r.credit > 0).length;
    const meanCredit = (rows: DayResult[]) => rows.reduce((a, r) => a + r.credit, 0) / rows.length;
    const extraSize = (rows: DayResult[]) => meanCredit(rows) / (book.imBps / 10_000);
    const byMonth = (rows: DayResult[], m: string) => rows.filter((r) => r.date.startsWith(m));
    const d = summarize("all", daily);
    const b = summarize("all", bar);
    return {
      leverage,
      book,
      sumIm: daily[0].sumIm,
      freedPctDaily: freed(daily),
      freedPct20m: freed(bar),
      daysWithCredit: withCredit(daily),
      days: daily.length,
      meanCreditDaily: meanCredit(daily),
      meanExtraNotionalDaily: extraSize(daily),
      freedPctByMonth: Object.fromEntries(MONTHS.map((m) => [m, freed(byMonth(daily, m))])),
      breachesDaily: d.breachesWithCh,
      breachesBaselineDaily: d.breachesBaseline,
      breaches20m: b.breachesWithCh,
      breachesBaseline20m: b.breachesBaseline,
      worstMarginDaily: d.worstMarginOfSafety,
      worstMarginDate: d.worstMarginDate,
    };
  });
  mkdirSync(path.dirname(out), { recursive: true });
  writeFileSync(out, json({ command: "cd services/risk && npm run leverage", note: "Engine as on chain (floor A2, hedge break A3 at the governed value); haircut in force; long SOL / short ETH $50k per leg; IM = 1/leverage, MM = IM/2.", results }));
  console.log("| leverage | IM per leg | sum IM | freed % of sum IM (H=1d) | days with credit | mean credit | mean extra notional | breaches H=1d (baseline) | breaches H=20m (baseline) | worst margin H=1d |");
  console.log("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
  for (const r of results) {
    console.log(`| ${r.leverage}x | ${(r.book.imBps / 100).toFixed(2)}% | ${usd(r.sumIm)} | ${r.freedPctDaily.toFixed(2)} | ${r.daysWithCredit} of ${r.days} | ${usd(r.meanCreditDaily)} | ${usd(r.meanExtraNotionalDaily)} | ${r.breachesDaily} (${r.breachesBaselineDaily}) | ${r.breaches20m} (${r.breachesBaseline20m}) | ${usd(r.worstMarginDaily)} (${r.worstMarginDate}) |`);
  }
  for (const r of results) console.log(`${r.leverage}x freed by month: ${Object.entries(r.freedPctByMonth).map(([m, v]) => `${m} ${(v as number).toFixed(2)}%`).join(", ")}`);
  console.log(`
wrote ${path.relative(process.cwd(), out)}`);
}

if (require.main === module) {
  const cmd = process.argv[2];
  if (cmd === "calibrate") cmdCalibrate();
  else if (cmd === "backtest") cmdBacktest();
  else if (cmd === "frontier") cmdFrontier();
  else if (cmd === "throttle") cmdThrottle();
  else if (cmd === "leverage") cmdLeverage();
  else {
    console.error("usage: cli.js calibrate|backtest|frontier|throttle|leverage [--data DIR] [--current FILE] [--out FILE]");
    process.exit(2);
  }
}

export { DAY_MS };

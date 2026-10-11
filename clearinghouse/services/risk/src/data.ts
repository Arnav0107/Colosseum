/** Loaders for the committed CSVs in crates/ch_math/tests/data (no network). */
import { createHash } from "crypto";
import { readFileSync } from "fs";
import * as path from "path";

/** One 20-minute window. Prices in micro-USD; `t` = window start (ms, UTC). */
export interface Bar {
  t: number;
  o: number;
  h: number;
  l: number;
  c: number;
}

/** clearinghouse/ (compiled layout: services/risk/dist/services/risk/src). */
export const REPO_ROOT = path.resolve(__dirname, "../../../../../..");

export const BAR_MS = 20 * 60 * 1000;
export const DAY_MS = 24 * 60 * 60 * 1000;
export const BARS_PER_DAY = DAY_MS / BAR_MS;

export interface InputFile {
  file: string;
  sha256: string;
  rows: number;
  first: string;
  last: string;
}

const iso = (ms: number) => new Date(ms).toISOString();

export function sha256(file: string): string {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

/** `window_start_ms,open,high,low,close`; rejects anything malformed or out of order. */
export function loadKlines20m(file: string): Bar[] {
  const lines = readFileSync(file, "utf8").split(/\r?\n/).filter((l) => l.trim() !== "");
  if (lines[0] !== "window_start_ms,open,high,low,close") throw new Error(`${file}: unexpected header ${lines[0]}`);
  const bars = lines.slice(1).map((l, i) => {
    const f = l.split(",").map(Number);
    if (f.length !== 5 || f.some((x) => !Number.isFinite(x))) throw new Error(`${file}:${i + 2}: malformed row`);
    const [t, o, h, lo, c] = f;
    if (!(lo <= Math.min(o, c) && h >= Math.max(o, c) && lo > 0)) throw new Error(`${file}:${i + 2}: inconsistent OHLC`);
    return { t, o, h, l: lo, c };
  });
  for (let i = 1; i < bars.length; i++) {
    if (bars[i].t <= bars[i - 1].t) throw new Error(`${file}: timestamps not increasing at row ${i + 2}`);
  }
  return bars;
}

export interface DailyRow {
  date: string;
  solClose: number;
  solLow: number;
  ethClose: number;
  ethLow: number;
}

/** binance_nov2022_daily.csv: `#` comments, then `date,sol_close,sol_low,eth_close,eth_low`. */
export function loadDaily(file: string): DailyRow[] {
  const lines = readFileSync(file, "utf8")
    .split(/\r?\n/)
    .filter((l) => l.trim() !== "" && !l.startsWith("#"));
  if (lines[0] !== "date,sol_close,sol_low,eth_close,eth_low") throw new Error(`${file}: unexpected header`);
  return lines.slice(1).map((l) => {
    const [date, ...rest] = l.split(",");
    const [solClose, solLow, ethClose, ethLow] = rest.map(Number);
    return { date, solClose, solLow, ethClose, ethLow };
  });
}

export function describeKlines(file: string, bars: Bar[], root: string): InputFile {
  return {
    file: path.relative(root, file).split(path.sep).join("/"),
    sha256: sha256(file),
    rows: bars.length,
    first: iso(bars[0].t),
    last: iso(bars[bars.length - 1].t),
  };
}

/** Two series aligned on identical window starts (intersection). */
export interface Aligned {
  t: number[];
  a: Bar[];
  b: Bar[];
}

export function align(a: Bar[], b: Bar[]): Aligned {
  const byT = new Map(b.map((x) => [x.t, x]));
  const out: Aligned = { t: [], a: [], b: [] };
  for (const x of a) {
    const y = byT.get(x.t);
    if (y) {
      out.t.push(x.t);
      out.a.push(x);
      out.b.push(y);
    }
  }
  return out;
}

/** Index ranges [start, end) of runs of consecutive 20-minute windows (no gaps). */
export function segments(t: number[]): [number, number][] {
  const out: [number, number][] = [];
  let s = 0;
  for (let i = 1; i <= t.length; i++) {
    if (i === t.length || t[i] - t[i - 1] !== BAR_MS) {
      if (i > s) out.push([s, i]);
      s = i;
    }
  }
  return out;
}

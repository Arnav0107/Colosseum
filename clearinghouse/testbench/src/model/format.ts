/** Display helpers. Amounts on chain are micro-USD (1e-6 USD) in u64. */

export const ASSETS: { id: number; symbol: string }[] = [
  { id: 0, symbol: "SOL" },
  { id: 1, symbol: "ETH" },
  { id: 2, symbol: "BTC" },
];

export const assetSymbol = (id: number): string => ASSETS.find((a) => a.id === id)?.symbol ?? `asset ${id}`;

export function formatUsd(micro: bigint): string {
  const neg = micro < 0n;
  const abs = neg ? -micro : micro;
  const dollars = abs / 1_000_000n;
  const cents = (abs % 1_000_000n) / 10_000n;
  const grouped = dollars.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return `${neg ? "−" : ""}$${grouped}.${cents.toString().padStart(2, "0")}`;
}

/** Parse a user-entered dollar amount ("1,234.5") into micro-USD. Null if invalid. */
export function parseUsd(text: string): bigint | null {
  const t = text.replace(/[,$\s]/g, "");
  if (!/^\d+(\.\d{0,6})?$/.test(t)) return null;
  const [whole, frac = ""] = t.split(".");
  return BigInt(whole) * 1_000_000n + BigInt(frac.padEnd(6, "0"));
}

export const formatBps = (bps: number): string => `${(bps / 100).toFixed(2)}%`;
export const formatPpm = (ppm: number): string => `${(ppm / 10_000).toFixed(2)}%`;

/** Rough wall time for a slot count at 400 ms per slot. */
export function slotsToText(slots: bigint): string {
  const secs = Number(slots) * 0.4;
  if (secs < 90) return `${Math.round(secs)} s`;
  if (secs < 5400) return `${Math.round(secs / 60)} min`;
  return `${(secs / 3600).toFixed(1)} h`;
}

export const shortKey = (k: string): string => (k.length > 10 ? `${k.slice(0, 4)}…${k.slice(-4)}` : k);

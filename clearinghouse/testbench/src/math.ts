export interface MathLeg {
  asset: number;
  signedRequiredMargin: bigint;
}

/**
 * Integer square root floor for BigInt:
 * Finds the largest integer r such that r*r <= n.
 */
export function isqrtFloor(n: bigint): bigint {
  if (n <= 0n) return 0n;
  let res = 0n;
  let one = 1n << 126n;
  while (one > n) {
    one >>= 2n;
  }
  let op = n;
  while (one !== 0n) {
    const sum = res + one;
    if (op >= sum) {
      op -= sum;
      res = (res >> 1n) + one;
    } else {
      res >>= 1n;
    }
    one >>= 2n;
  }
  return res;
}

/**
 * Integer square root ceiling for BigInt:
 * Finds the smallest integer r such that r*r >= n.
 */
export function isqrtCeil(n: bigint): bigint {
  const floor = isqrtFloor(n);
  if (floor * floor === n) {
    return floor;
  }
  return floor + 1n;
}

/**
 * Computes combined portfolio risk for signed legs.
 * Uses integer math with 1e6 scaling for rho.
 */
export function combinedRisk(
  legs: MathLeg[],
  rho: (a: number, b: number) => bigint
): bigint {
  if (legs.length === 0) return 0n;

  let totalCov = 0n;
  for (let i = 0; i < legs.length; i++) {
    const ri = legs[i].signedRequiredMargin;
    for (let j = 0; j < legs.length; j++) {
      const rj = legs[j].signedRequiredMargin;
      const rProd = ri * rj;
      const corr =
        legs[i].asset === legs[j].asset
          ? 1_000_000n
          : rho(legs[i].asset, legs[j].asset);
      const term = rProd * corr;
      totalCov += term;
    }
  }

  if (totalCov < 0n) {
    throw new Error("NegativeVariance");
  }
  if (totalCov === 0n) {
    return 0n;
  }

  const div = totalCov / 1_000_000n;
  const rem = totalCov % 1_000_000n;
  const variance = rem > 0n ? div + 1n : div;
  return isqrtCeil(variance);
}

/**
 * Computes allowable total credit:
 * (sumRequired - combined) * (10_000 - haircutBps) / 10_000, rounded DOWN.
 */
export function creditTotal(
  sumRequired: bigint,
  combined: bigint,
  haircutBps: number
): bigint {
  if (sumRequired <= combined || haircutBps >= 10_000) {
    return 0n;
  }
  const diff = sumRequired - combined;
  const factor = 10_000n - BigInt(haircutBps);
  return (diff * factor) / 10_000n;
}

/**
 * Floor each share, remainder to the last entry.
 */
export function splitProRata(total: bigint, required: bigint[]): bigint[] {
  const N = required.length;
  if (N === 0 || total === 0n) return new Array(N).fill(0n);
  let sumRequired = 0n;
  for (const r of required) {
    sumRequired += r;
  }
  if (sumRequired === 0n) return new Array(N).fill(0n);

  const result = new Array(N).fill(0n);
  let allocated = 0n;
  for (let i = 0; i < N - 1; i++) {
    const share = (total * required[i]) / sumRequired;
    result[i] = share;
    allocated += share;
  }
  result[N - 1] = total >= allocated ? total - allocated : 0n;
  return result;
}

/**
 * Default fund cap = fundBalance * 10_000 / minRatioBps.
 */
export function fundCap(fundBalance: bigint, minRatioBps: number): bigint {
  if (minRatioBps <= 0) return 0n;
  return (fundBalance * 10_000n) / BigInt(minRatioBps);
}

/**
 * Liquidation drop tolerance in bps:
 * triggerBps * collateral / position, rounded down.
 */
export function liquidationDropBps(
  collateral: bigint,
  position: bigint,
  triggerBps: number
): bigint {
  if (position <= 0n) return 0n;
  return (BigInt(triggerBps) * collateral) / position;
}

/**
 * Price move in bps:
 * |new - old| * 10_000 / old, rounded UP; old == 0 returns u64::MAX.
 */
export function priceMoveBps(oldPrice: bigint, newPrice: bigint): bigint {
  if (oldPrice === 0n) {
    return 18446744073709551615n; // u64::MAX
  }
  const diff = newPrice >= oldPrice ? newPrice - oldPrice : oldPrice - newPrice;
  const num = diff * 10_000n;
  const div = num / oldPrice;
  const rem = num % oldPrice;
  return rem > 0n ? div + 1n : div;
}

/**
 * Formats micro-USD BigInt as a dollar string with 2 decimal places.
 */
export function formatMicroUSD(micro: bigint): string {
  const isNeg = micro < 0n;
  const abs = isNeg ? -micro : micro;
  const dollars = abs / 1_000_000n;
  const cents = (abs % 1_000_000n) / 10_000n;
  const formattedCents = cents.toString().padStart(2, "0");
  const formattedDollars = dollars.toLocaleString("en-US");
  return `${isNeg ? "-" : ""}$${formattedDollars}.${formattedCents}`;
}

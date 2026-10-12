/**
 * Headless end-to-end check of the test bench's chain layer against a local
 * validator running ch_core built with `--features mock-oracle,dev-venue-signer`
 * (see scripts/dev-localnet.sh). Exits non-zero on the first failed check.
 *
 *   npm run smoke            # RPC_URL defaults to http://127.0.0.1:8899
 *
 * Runs under vitest (vitest.smoke.config.ts) because Anchor's ESM build does
 * not load under plain Node ESM; it is not part of `npm test`.
 *
 * Idempotent on a fresh validator; on a reused one it skips setup that exists.
 * It uses exactly the code paths the UI uses (src/chain/bench.ts) and the same
 * independent verification (src/model/verify.ts).
 */
import { expect, it } from "vitest";
import { Bench, currentRho, DEFAULT_ASSET_RISK, VENUES } from "../src/chain/bench";
import { describeError } from "../src/chain/errors";
import { isLocalRpc } from "../src/model/rpc";
import { formatUsd } from "../src/model/format";
import { effectiveInitialMargin, usableCredit, verifyPortfolio } from "../src/model/verify";
import { STATUS } from "../src/model/views";

const rpc = process.env.RPC_URL ?? "http://127.0.0.1:8899";
const IM = 10_000_000_000n;

function check(cond: boolean, msg: string): void {
  expect(cond, msg).toBe(true);
  console.log(`ok   ${msg}`);
}

async function expectFailure(p: Promise<unknown>, code: string, msg: string): Promise<void> {
  try {
    await p;
  } catch (e) {
    const d = describeError(e);
    check(d.startsWith(`${code}:`), `${msg} (got ${d})`);
    return;
  }
  check(false, `${msg} (transaction unexpectedly succeeded)`);
}

it("localnet smoke: setup, credit, verification, invalidation", async () => {
  check(isLocalRpc(rpc), `RPC ${rpc} is loopback`);
  const bench = new Bench(rpc);
  let s = await bench.fetchState();
  check(s.programDeployed, "ch_core is loaded at its declare_id address");

  await bench.airdropAll();
  const bal = await bench.balances();
  check([...bal.values()].every((v) => v >= 1), "dev keys funded");

  if (!s.config) await bench.initialize();
  for (let i = 0; i < VENUES.length; i++) {
    s = await bench.fetchState();
    if (!s.venues.some((v) => v.name === VENUES[i].name)) await bench.registerVenue(i);
  }
  // A1: venues accept only their own reports by default; A and B allowlist each other.
  await bench.setVenueAcceptance(0, 0b10n);
  await bench.setVenueAcceptance(1, 0b01n);
  for (const [a, [sh, g, b]] of Object.entries(DEFAULT_ASSET_RISK)) await bench.setAssetRisk(Number(a), sh, g, b);
  s = await bench.fetchState();
  await bench.setCorrelation(0, 1, 800_000, currentRho(s.riskModel, 0, 1));
  await bench.setConsent(true, 3n);
  await bench.setMockPrice(0, 150_000_000n, 15_000n);
  await bench.setMockPrice(1, 3_000_000_000n, 300_000n);
  s = await bench.fetchState();
  check(s.venues.length === 2 && s.venues.every((v) => v.isActive), "venues A and B active");
  check(currentRho(s.riskModel, 0, 1) === 800_000, "SOL/ETH correlation is 0.8 on chain");

  await bench.submitSnapshot({ venueIndex: 0, assetId: 0, isLong: true, notional: 5n * IM, initialMargin: IM, maintenanceMargin: IM / 2n });
  await bench.submitSnapshot({ venueIndex: 1, assetId: 1, isLong: false, notional: 5n * IM, initialMargin: IM, maintenanceMargin: IM / 2n });
  s = await bench.fetchState();
  check(s.portfolio?.registered.length === 2, "two legs registered");

  await bench.refresh(s.portfolio!);
  s = await bench.fetchState();
  const pf = s.portfolio!;
  check(pf.status === STATUS.ACTIVE, "refresh made credit active");
  const verdict = verifyPortfolio(s.config!, s.riskModel!, pf);
  check(verdict.kind === "match", `independent verification: ${verdict.kind}`);
  console.log(`     total credit ${pf.totalCredit} (worked example on a fresh validator: 7111111110)`);
  // What each venue would apply right now (ch_client rule, read from chain).
  const slotActive = (await bench.clock()).slot;
  const withCredit = pf.legs.map((l) => usableCredit(s.config!, pf, slotActive, l.venueIndex, l.assetId));

  await expectFailure(bench.invalidate(pf, false), "InvalidationNotJustified", "invalidation without a reason is rejected");

  // A newer price that moved 6% (> 3% default guard) is evidence; wait for the clock to pass the baseline.
  const baseline = pf.legs[0].baselinePublishTs;
  for (let i = 0; i < 30 && (await bench.clock()).unixTimestamp <= baseline; i++) await new Promise((r) => setTimeout(r, 500));
  await bench.setMockPrice(0, 159_000_000n, 15_900n);
  await bench.invalidate(pf, true);
  s = await bench.fetchState();
  check(s.portfolio!.status === STATUS.INVALIDATED, "price breach newer than the baseline invalidates");
  check(s.portfolio!.totalCredit === 0n, "invalidation released the credit");
  const slotAfter = (await bench.clock()).slot;
  const afterCredit = pf.legs.map((l) => usableCredit(s.config!, s.portfolio!, slotAfter, l.venueIndex, l.assetId));
  check(afterCredit.every((c) => c === 0n), "venues see no usable credit after invalidation");
  printBeforeAfter(pf.legs, withCredit, afterCredit);

  // Leave the bench usable: close the snapshots again.
  await bench.closeSnapshot(0, 0);
  await bench.closeSnapshot(1, 1);
  console.log("SMOKE PASSED");
});

/**
 * Before/after table printed by `npm run demo` (and every smoke run): the
 * initial-margin requirement each venue applies without credit, with the
 * usable credit read from chain, and after the hedge changed (invalidation).
 * The maintenance margin, and so the liquidation threshold, never moves.
 */
function printBeforeAfter(
  legs: { venueIndex: number; assetId: number; isLong: boolean; initialMargin: bigint; maintenanceMargin: bigint }[],
  withCredit: bigint[],
  afterCredit: bigint[]
): void {
  const name = ["SOL", "ETH"];
  const rows = legs.map((l, i) => [
    `venue ${VENUES[l.venueIndex]?.name ?? l.venueIndex} ${l.isLong ? "long" : "short"} ${name[l.assetId] ?? l.assetId}`,
    formatUsd(l.initialMargin),
    formatUsd(withCredit[i]),
    formatUsd(effectiveInitialMargin(l.initialMargin, l.maintenanceMargin, withCredit[i])),
    formatUsd(effectiveInitialMargin(l.initialMargin, l.maintenanceMargin, afterCredit[i])),
    formatUsd(l.maintenanceMargin),
  ]);
  const sum = (f: (i: number) => bigint) => legs.reduce((a, _l, i) => a + f(i), 0n);
  rows.push([
    "total",
    formatUsd(sum((i) => legs[i].initialMargin)),
    formatUsd(sum((i) => withCredit[i])),
    formatUsd(sum((i) => effectiveInitialMargin(legs[i].initialMargin, legs[i].maintenanceMargin, withCredit[i]))),
    formatUsd(sum((i) => effectiveInitialMargin(legs[i].initialMargin, legs[i].maintenanceMargin, afterCredit[i]))),
    formatUsd(sum((i) => legs[i].maintenanceMargin)),
  ]);
  const head = ["leg", "IM, no credit (before)", "usable credit", "IM with credit (after)", "IM after hedge change", "MM (all columns)"];
  console.log("");
  console.log("BEFORE / AFTER (initial-margin requirement a venue applies; max(IM - credit, MM))");
  console.log(`| ${head.join(" | ")} |`);
  console.log(`| ${head.map((_, i) => (i === 0 ? "---" : "---:")).join(" | ")} |`);
  for (const r of rows) console.log(`| ${r.join(" | ")} |`);
}

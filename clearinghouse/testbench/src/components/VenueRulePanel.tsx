import { useState } from "react";
import { configFixture, riskModelFixture } from "../model/fixtures";
import { formatUsd, parseUsd } from "../model/format";
import { checkOpen, maxOpen, referenceScenario, USD } from "../model/venueRule";
import type { ConfigView, RiskModelView } from "../model/views";
import { TableScroll, TextField } from "./fields";

/**
 * C3: before/after at the reference venue. Same collateral on venue A, with
 * and without a $50,000 ETH short on venue B: the largest SOL long venue A
 * accepts under max(IM − usable credit, MM). Computed by model/venueRule.ts
 * from the loaded config and risk model (or the worked-example parameters
 * when no chain state is loaded).
 */
export function VenueRulePanel({ config, riskModel }: { config: ConfigView | null; riskModel: RiskModelView | null }) {
  const [text, setText] = useState("8000");
  const collateral = parseUsd(text);
  const error = collateral === null || collateral <= 0n ? "Enter a positive dollar amount." : collateral > 100_000n * USD ? "At most $100,000 (the probe runs in the browser)." : null;
  const cfg = config ?? configFixture();
  const rm = riskModel ?? riskModelFixture();
  const fromChain = config !== null && riskModel !== null;

  const rows =
    error || collateral === null
      ? []
      : ([false, true] as const).map((hedged) => {
          const s = { ...referenceScenario(hedged), collateral };
          const max = maxOpen(cfg, rm, s);
          const at = max > 0n ? checkOpen(cfg, rm, s, max) : null;
          return { hedged, max, at };
        });
  const gain = rows.length === 2 ? rows[1].max - rows[0].max : 0n;

  return (
    <section className="panel" aria-labelledby="vr-h">
      <h2 id="vr-h">What the credit buys at a venue</h2>
      <p className="hint">
        The reference venue (<code>programs/mock_perps_engine</code>) holds the trader's collateral and accepts a new position
        only if equity ≥ max(IM − usable credit, MM), with IM 20% and MM 10%. Two traders put the same collateral on venue A
        and go long SOL at $150; one of them is also short $50,000 ETH on venue B. Parameters:{" "}
        {fromChain ? "the loaded config and risk model" : "the worked example (no chain state loaded)"}; calm market
        (no throttle), empty credit budgets.
      </p>
      <TextField label="Collateral on venue A (USD)" value={text} onChange={setText} error={error} />
      <TableScroll label="Largest SOL long venue A accepts">
        <table>
          <caption>Largest SOL long venue A accepts ($500 steps)</caption>
          <thead>
            <tr>
              <th scope="col">Trader</th>
              <th scope="col">Max position</th>
              <th scope="col">IM at max</th>
              <th scope="col">Credit at max</th>
              <th scope="col">Required max(IM − credit, MM)</th>
              <th scope="col">MM (liquidation)</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={String(r.hedged)}>
                <td>{r.hedged ? "hedged (ETH short on B)" : "unhedged"}</td>
                <td className="num">{formatUsd(r.max)}</td>
                <td className="num">{r.at ? formatUsd(r.at.leg.initialMargin) : "—"}</td>
                <td className="num">{r.at ? formatUsd(r.at.credit) : "—"}</td>
                <td className="num">{r.at ? formatUsd(r.at.required) : "—"}</td>
                <td className="num">{r.at ? formatUsd(r.at.leg.maintenanceMargin) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableScroll>
      {rows.length === 2 && (
        <p role="status">
          Credit lets the hedged trader open <strong>{formatUsd(gain)}</strong> more with the same collateral. Liquidation
          still uses MM only, so the liquidation price of a given position is the same for both.
        </p>
      )}
      <p className="hint">
        On chain, the compiled reference venue gives the same numbers for $8,000 ($40,000 unhedged, $59,000 hedged):{" "}
        <code>cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test engine</code>.
      </p>
    </section>
  );
}

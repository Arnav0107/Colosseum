import { useState } from "react";
import { ASSETS, formatUsd, parseUsd } from "../model/format";
import { TableScroll, TextField } from "./fields";
import type { PanelProps } from "./types";

const DEFAULT_PRICES: Record<number, string> = { 0: "150", 1: "3000", 2: "65000" };

export function PricesPanel({ bench, state, run, busy }: PanelProps) {
  const [text, setText] = useState<Record<number, string>>(DEFAULT_PRICES);
  const now = state.clock.unixTimestamp;
  const maxAge = state.config?.params.maxPriceAgeSecs ?? 0n;
  const ready = state.config !== null;

  const publish = (assetId: number, price: bigint) =>
    run(`Publish ${ASSETS[assetId].symbol} = ${formatUsd(price)}`, () => bench.setMockPrice(assetId, price, price / 10_000n));

  return (
    <section className="panel" aria-labelledby="prices-h">
      <h2 id="prices-h">Mock prices</h2>
      <p className="hint">
        MOCK ORACLE: the admin writes prices, stamped with the on-chain clock. Confidence is set to 1 bp. Ages below use
        the on-chain clock, not your browser's.
      </p>
      <TableScroll label="Mock prices">
        <table>
          <thead>
            <tr>
              <th scope="col">Asset</th>
              <th scope="col">On-chain price</th>
              <th scope="col">Age</th>
              <th scope="col">New price (USD)</th>
              <th scope="col">
                <span className="sr-only">Action</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {ASSETS.map((a) => {
              const p = state.prices.find((x) => x.assetId === a.id);
              const age = p ? now - p.publishTs : null;
              const stale = age !== null && age > maxAge;
              const parsed = parseUsd(text[a.id] ?? "");
              return (
                <tr key={a.id}>
                  <th scope="row">{a.symbol}</th>
                  <td className="num">{p ? formatUsd(p.priceMicro) : "not set"}</td>
                  <td className="num">
                    {age === null ? "—" : `${age.toString()} s`} {stale && <span className="badge warn">stale</span>}
                  </td>
                  <td>
                    <TextField
                      label={`New ${a.symbol} price`}
                      value={text[a.id] ?? ""}
                      onChange={(v) => setText({ ...text, [a.id]: v })}
                      error={parsed === null || parsed === 0n ? "Enter a positive amount." : null}
                    />
                  </td>
                  <td>
                    <button disabled={busy || !ready || !parsed} onClick={() => parsed && publish(a.id, parsed)}>
                      Publish {a.symbol}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableScroll>
      <button
        disabled={busy || !ready || state.prices.length === 0}
        onClick={() =>
          run("Re-publish all prices at the current clock", async () => {
            for (const p of state.prices) await bench.setMockPrice(p.assetId, p.priceMicro, p.confMicro);
          })
        }
      >
        Re-publish current prices (keep them fresh)
      </button>
    </section>
  );
}

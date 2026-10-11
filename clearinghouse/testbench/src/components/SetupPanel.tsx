import { ReactNode, useEffect, useState } from "react";
import { currentRho, DEFAULT_ASSET_RISK, VENUES } from "../chain/bench";
import { assetSymbol, formatBps, formatUsd, shortKey, slotsToText } from "../model/format";
import { Stat, TableScroll, TextField } from "./fields";
import type { PanelProps } from "./types";

/** Parse a correlation in [-1, 1] ("0.8") into the on-chain 1e6 scale. */
export function parseRho(text: string): number | null {
  const v = Number(text.trim());
  if (!Number.isFinite(v) || text.trim() === "" || v < -1 || v > 1) return null;
  return Math.round(v * 1_000_000);
}

function Step(props: { n: number; title: string; done: boolean; children: ReactNode }) {
  return (
    <li className="step">
      <h3>
        {props.n}. {props.title} <span className={props.done ? "badge ok" : "badge"}>{props.done ? "done" : "to do"}</span>
      </h3>
      {props.children}
    </li>
  );
}

export function SetupPanel({ bench, state, run, busy }: PanelProps) {
  const [balances, setBalances] = useState<Map<string, number>>(new Map());
  const [rhoText, setRhoText] = useState("0.8");
  useEffect(() => {
    bench.balances().then(setBalances, () => setBalances(new Map()));
  }, [bench, state]);

  const funded = bench.devKeys().every((k) => (balances.get(k.key.toBase58()) ?? 0) >= 1);
  const cfg = state.config;
  const venuesDone = VENUES.every((v) => state.venues.some((x) => x.name === v.name && x.isActive));
  const riskDone = [0, 1, 2].every((a) => (state.riskModel?.assets[a]?.shockBps ?? 0) > 0);
  const rhoNow = currentRho(state.riskModel, 0, 1);
  const rhoTarget = parseRho(rhoText);
  const consentDone = state.consent?.isActive === true && (state.consent.authorizedVenuesBitmap & 3n) === 3n;

  return (
    <section className="panel" aria-labelledby="setup-h">
      <h2 id="setup-h">Setup</h2>
      <ol className="steps">
        <Step n={1} title="Fund dev keys" done={funded}>
          <TableScroll label="Dev key balances">
            <table>
              <thead>
                <tr>
                  <th scope="col">Key</th>
                  <th scope="col">Address</th>
                  <th scope="col">SOL</th>
                </tr>
              </thead>
              <tbody>
                {bench.devKeys().map((k) => (
                  <tr key={k.label}>
                    <td>{k.label}</td>
                    <td className="mono">{shortKey(k.key.toBase58())}</td>
                    <td className="num">{(balances.get(k.key.toBase58()) ?? 0).toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TableScroll>
          <button disabled={busy} onClick={() => run("Airdrop 10 SOL to each dev key", () => bench.airdropAll())}>
            Airdrop 10 SOL to each dev key
          </button>
        </Step>

        <Step n={2} title="Initialize the clearinghouse" done={cfg !== null}>
          {cfg ? (
            <dl className="stats">
              <Stat label="Haircut">{formatBps(cfg.params.haircutBps)}</Stat>
              <Stat label="Credit TTL">
                {cfg.params.creditTtlSlots.toString()} slots (~{slotsToText(cfg.params.creditTtlSlots)})
              </Stat>
              <Stat label="Max credit per user">{formatUsd(cfg.params.maxCreditPerUser)}</Stat>
              <Stat label="Max credit / IM">{formatBps(cfg.params.maxCreditBpsOfIm)}</Stat>
              <Stat label="Price guard">{formatBps(cfg.params.maxMoveBps)} move</Stat>
              <Stat label="Max price age">{cfg.params.maxPriceAgeSecs.toString()} s</Stat>
              <Stat label="Timelock">{cfg.params.timelockSlots.toString()} slots (localnet)</Stat>
              <Stat label="Risk epoch">{cfg.riskEpoch.toString()}</Stat>
              <Stat label="Paused">{cfg.paused ? "yes" : "no"}</Stat>
              <Stat label="Admin / risk / venues / guardian">
                <span className="mono">
                  {[cfg.admin, cfg.riskAuthority, cfg.venueManager, cfg.guardian].map(shortKey).join(" / ")}
                </span>
              </Stat>
            </dl>
          ) : (
            <button disabled={busy || !funded} onClick={() => run("Initialize", () => bench.initialize())}>
              Initialize with localnet parameters
            </button>
          )}
        </Step>

        <Step n={3} title="Register venues A and B (dev signer keys)" done={venuesDone}>
          <ul className="plain">
            {VENUES.map((v, i) => {
              const onChain = state.venues.find((x) => x.name === v.name);
              return (
                <li key={v.name}>
                  {v.name} (index {v.index}):{" "}
                  {onChain ? (onChain.isActive ? "registered, active" : "registered, inactive") : "not registered"}{" "}
                  {!onChain && (
                    <button disabled={busy || !cfg} onClick={() => run(`Register ${v.name}`, () => bench.registerVenue(i))}>
                      Register {v.name}
                    </button>
                  )}
                </li>
              );
            })}
          </ul>
        </Step>

        <Step n={4} title="Asset risk (shock, gap, basis)" done={riskDone}>
          <p className="hint">
            {Object.entries(DEFAULT_ASSET_RISK)
              .map(([a, [s, g, b]]) => `${assetSymbol(Number(a))} ${formatBps(s)} / ${formatBps(g)} / ${formatBps(b)}`)
              .join(" · ")}
          </p>
          <button
            disabled={busy || !cfg}
            onClick={() =>
              run("Set default asset risk", async () => {
                for (const [a, [s, g, b]] of Object.entries(DEFAULT_ASSET_RISK)) {
                  await bench.setAssetRisk(Number(a), s, g, b);
                }
              })
            }
          >
            Set default asset risk
          </button>
        </Step>

        <Step n={5} title="SOL/ETH correlation" done={rhoNow !== 0}>
          <p>
            On chain: <span className="mono">{(rhoNow / 1_000_000).toFixed(2)}</span>. The program accepts at most ±0.25 per
            update, so larger changes take several transactions.
          </p>
          <TextField
            label="Target correlation (−1 to 1)"
            value={rhoText}
            onChange={setRhoText}
            error={rhoTarget === null ? "Enter a number between −1 and 1." : null}
          />
          <button
            disabled={busy || !cfg || rhoTarget === null || rhoTarget === rhoNow}
            onClick={() =>
              rhoTarget !== null &&
              run(`Set SOL/ETH correlation to ${rhoText}`, () => bench.setCorrelation(0, 1, rhoTarget, rhoNow))
            }
          >
            Apply correlation
          </button>
        </Step>

        <Step n={6} title="Trader consents to both venues" done={consentDone}>
          <p>
            Consent:{" "}
            {state.consent
              ? `${state.consent.isActive ? "active" : "inactive"}, venues bitmap ${state.consent.authorizedVenuesBitmap.toString(2)}`
              : "none"}
            . Any consent change invalidates existing credit in the same transaction.
          </p>
          <div className="row">
            <button disabled={busy || !cfg} onClick={() => run("Consent to venues A and B", () => bench.setConsent(true, 3n))}>
              Consent to A and B
            </button>
            <button
              disabled={busy || !state.consent}
              onClick={() => run("Withdraw consent", () => bench.setConsent(false, 0n))}
            >
              Withdraw consent
            </button>
          </div>
        </Step>
      </ol>
    </section>
  );
}

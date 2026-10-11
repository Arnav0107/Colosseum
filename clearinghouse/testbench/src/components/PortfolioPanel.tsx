import { VENUES } from "../chain/bench";
import { assetSymbol, formatBps, formatPpm, formatUsd, slotsToText } from "../model/format";
import { effectiveInitialMargin, legCap, usableCredit, verifyPortfolio, Verdict } from "../model/verify";
import { STATUS, STATUS_NAME } from "../model/views";
import { Stat, TableScroll } from "./fields";
import type { PanelProps } from "./types";

export function VerdictView({ verdict }: { verdict: Verdict }) {
  switch (verdict.kind) {
    case "match":
      return (
        <p className="verdict ok" role="status">
          <strong>MATCH.</strong> The off-chain engine, fed only on-chain inputs, reproduces every leg's credit, S, R_p and
          the hedge benefit exactly.
        </p>
      );
    case "mismatch":
      return (
        <div className="verdict err" role="status">
          <p>
            <strong>MISMATCH.</strong> The off-chain engine disagrees with the program:
          </p>
          <ul>
            {verdict.differences.map((d) => (
              <li key={d}>{d}</li>
            ))}
          </ul>
        </div>
      );
    default:
      return (
        <p className="verdict" role="status">
          <strong>{verdict.kind === "stale" ? "NOT COMPARABLE." : verdict.kind === "engine-error" ? "ENGINE ERROR." : "NOTHING TO VERIFY."}</strong>{" "}
          {verdict.reason}
        </p>
      );
  }
}

export function PortfolioPanel({ bench, state, run, busy }: PanelProps) {
  const { config: cfg, riskModel: rm, portfolio: pf, clock } = state;
  if (!cfg || !rm || !pf) {
    return (
      <section className="panel" aria-labelledby="pf-h">
        <h2 id="pf-h">Portfolio credit</h2>
        <p className="muted">Initialize, then give consent, to create the trader's portfolio.</p>
      </section>
    );
  }
  const live = pf.status === STATUS.ACTIVE && pf.riskEpoch === cfg.riskEpoch && clock.slot <= pf.expiresSlot;
  const slotsLeft = pf.expiresSlot > clock.slot ? pf.expiresSlot - clock.slot : 0n;
  const verdict = verifyPortfolio(cfg, rm, pf);
  const sumIm = pf.legs.reduce((a, l) => a + l.initialMargin, 0n);

  return (
    <section className="panel" aria-labelledby="pf-h">
      <h2 id="pf-h">Portfolio credit</h2>
      <p className="hint">
        Credit is <strong>trading power only</strong>: it lowers a venue's initial-margin requirement for new risk, never
        below maintenance margin. It is not collateral, cannot be withdrawn, and losing it never by itself liquidates.
      </p>
      <dl className="stats">
        <Stat label="Status">{STATUS_NAME[pf.status] ?? pf.status}</Stat>
        <Stat label="Usable by venues now">{live ? "yes" : "no"}</Stat>
        <Stat label="Expires in">
          {pf.status === STATUS.ACTIVE ? `${slotsLeft.toString()} slots (~${slotsToText(slotsLeft)})` : "—"}
        </Stat>
        <Stat label="Version / epoch">
          {pf.version.toString()} / {pf.riskEpoch.toString()} (config epoch {cfg.riskEpoch.toString()})
        </Stat>
        <Stat label="Registered legs">{pf.registered.length}</Stat>
        <Stat label="Σ standalone risk S">{formatUsd(pf.sumStandaloneRisk)}</Stat>
        <Stat label="Portfolio risk R_p">{formatUsd(pf.portfolioRisk)}</Stat>
        <Stat label="Hedge benefit (S − R_p)/S">{formatPpm(pf.hedgeBenefitPpm)}</Stat>
        <Stat label="Σ initial margin">{formatUsd(sumIm)}</Stat>
        <Stat label="Haircut">{formatBps(cfg.params.haircutBps)}</Stat>
        <Stat label="Total credit">{formatUsd(pf.totalCredit)}</Stat>
      </dl>

      <TableScroll label="Credit by leg">
        <table>
          <caption>Per-leg breakdown (as computed at the last refresh)</caption>
          <thead>
            <tr>
              <th scope="col">Venue</th>
              <th scope="col">Asset</th>
              <th scope="col">Side</th>
              <th scope="col">Notional</th>
              <th scope="col">IM</th>
              <th scope="col">MM</th>
              <th scope="col">Cap min(IM·{formatBps(cfg.params.maxCreditBpsOfIm)}, IM−MM)</th>
              <th scope="col">Credit</th>
              <th scope="col">Venue applies max(IM − credit, MM)</th>
              <th scope="col">Usable now</th>
              <th scope="col">Baseline price</th>
            </tr>
          </thead>
          <tbody>
            {pf.legs.length === 0 && (
              <tr>
                <td colSpan={11} className="muted">
                  Not computed yet.
                </td>
              </tr>
            )}
            {pf.legs.map((l) => (
              <tr key={`${l.venueIndex}-${l.assetId}`}>
                <td>{VENUES[l.venueIndex]?.name ?? l.venueIndex}</td>
                <td>{assetSymbol(l.assetId)}</td>
                <td>{l.isLong ? "long" : "short"}</td>
                <td className="num">{formatUsd(l.notional)}</td>
                <td className="num">{formatUsd(l.initialMargin)}</td>
                <td className="num">{formatUsd(l.maintenanceMargin)}</td>
                <td className="num">{formatUsd(legCap(l.initialMargin, l.maintenanceMargin, cfg.params.maxCreditBpsOfIm))}</td>
                <td className="num">{formatUsd(l.credit)}</td>
                <td className="num">{formatUsd(effectiveInitialMargin(l.initialMargin, l.maintenanceMargin, l.credit))}</td>
                <td className="num">{formatUsd(usableCredit(cfg, pf, clock.slot, l.venueIndex, l.assetId))}</td>
                <td className="num">{formatUsd(l.baselinePrice)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableScroll>

      <div className="row">
        <button
          disabled={busy || pf.registered.length === 0 || cfg.paused}
          onClick={() => run("Refresh portfolio (permissionless)", () => bench.refresh(pf))}
        >
          Refresh portfolio
        </button>
        <button disabled={busy} onClick={() => run("Invalidate (no oracle evidence)", () => bench.invalidate(pf, false))}>
          Invalidate
        </button>
        <button
          disabled={busy || pf.legs.length === 0}
          onClick={() => run("Invalidate with oracle evidence", () => bench.invalidate(pf, true))}
        >
          Invalidate with oracle evidence
        </button>
      </div>
      <p className="hint">
        Anyone may refresh or invalidate. Invalidation must give a reason (expired, epoch changed, venue inactive, consent
        withdrawn, stale snapshot, or a price newer than the baseline that moved more than {formatBps(cfg.params.maxMoveBps)}{" "}
        or is unusable); otherwise the program rejects it.
      </p>

      <h3>Independent verification</h3>
      <VerdictView verdict={verdict} />
    </section>
  );
}

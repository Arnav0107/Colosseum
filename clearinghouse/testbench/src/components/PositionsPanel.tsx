import { useState } from "react";
import { VENUES } from "../chain/bench";
import { ASSETS, assetSymbol, formatUsd, parseUsd } from "../model/format";
import { SelectField, TableScroll, TextField } from "./fields";
import type { PanelProps } from "./types";

export interface PositionForm {
  notional: string;
  im: string;
  mm: string;
}

/** Same rules as the program (and ch_math::validate_legs): 0 < MM ≤ IM ≤ notional. */
export function validatePosition(f: PositionForm): { notional: bigint; im: bigint; mm: bigint } | { error: string } {
  const notional = parseUsd(f.notional);
  const im = parseUsd(f.im);
  const mm = parseUsd(f.mm);
  if (notional === null || im === null || mm === null) return { error: "Amounts must be plain USD numbers." };
  if (notional === 0n) return { error: "Notional must be positive." };
  if (mm === 0n) return { error: "Maintenance margin must be positive." };
  if (mm > im) return { error: "Maintenance margin cannot exceed initial margin." };
  if (im > notional) return { error: "Initial margin cannot exceed notional." };
  return { notional, im, mm };
}

export function PositionsPanel({ bench, state, run, busy }: PanelProps) {
  const [venue, setVenue] = useState(0);
  const [asset, setAsset] = useState(0);
  const [isLong, setIsLong] = useState(true);
  const [form, setForm] = useState<PositionForm>({ notional: "50000", im: "10000", mm: "5000" });
  const checked = validatePosition(form);
  const ready = state.consent !== null && state.venues.length > 0;

  return (
    <section className="panel" aria-labelledby="pos-h">
      <h2 id="pos-h">Positions reported by venues</h2>
      <p className="hint">
        UNVERIFIED VENUES: each venue signs its own report with a dev key. In production only the venue program can report
        (via CPI). Every report invalidates the trader's credit in the same transaction.
      </p>
      <form
        className="form-grid"
        aria-describedby="pos-err"
        onSubmit={(e) => {
          e.preventDefault();
          if ("error" in checked) return;
          run(
            `${VENUES[venue].name} reports ${isLong ? "long" : "short"} ${assetSymbol(asset)} ${formatUsd(checked.notional)}`,
            () =>
              bench.submitSnapshot({
                venueIndex: venue,
                assetId: asset,
                isLong,
                notional: checked.notional,
                initialMargin: checked.im,
                maintenanceMargin: checked.mm,
              })
          );
        }}
      >
        <SelectField label="Venue" value={venue} onChange={setVenue} options={VENUES.map((v, i) => ({ value: i, label: v.name }))} />
        <SelectField label="Asset" value={asset} onChange={setAsset} options={ASSETS.map((a) => ({ value: a.id, label: a.symbol }))} />
        <SelectField
          label="Side"
          value={isLong ? "long" : "short"}
          onChange={(v) => setIsLong(v === "long")}
          options={[
            { value: "long", label: "Long" },
            { value: "short", label: "Short" },
          ]}
        />
        <TextField label="Notional (USD)" value={form.notional} onChange={(v) => setForm({ ...form, notional: v })} />
        <TextField label="Initial margin (USD)" value={form.im} onChange={(v) => setForm({ ...form, im: v })} />
        <TextField label="Maintenance margin (USD)" value={form.mm} onChange={(v) => setForm({ ...form, mm: v })} />
        <div className="form-actions">
          <span id="pos-err" className="field-error" aria-live="polite">
            {"error" in checked ? checked.error : ""}
          </span>
          <button type="submit" disabled={busy || !ready || "error" in checked}>
            Submit snapshot
          </button>
        </div>
      </form>

      <TableScroll label="Current position snapshots">
        <table>
          <caption>Snapshots on chain for the trader</caption>
          <thead>
            <tr>
              <th scope="col">Venue</th>
              <th scope="col">Asset</th>
              <th scope="col">Side</th>
              <th scope="col">Notional</th>
              <th scope="col">IM</th>
              <th scope="col">MM</th>
              <th scope="col">Version</th>
              <th scope="col">Age (slots)</th>
              <th scope="col">
                <span className="sr-only">Action</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {state.snapshots.length === 0 && (
              <tr>
                <td colSpan={9} className="muted">
                  No snapshots.
                </td>
              </tr>
            )}
            {state.snapshots.map((s) => (
              <tr key={`${s.venueIndex}-${s.assetId}`}>
                <td>{VENUES[s.venueIndex]?.name ?? s.venueIndex}</td>
                <td>{assetSymbol(s.assetId)}</td>
                <td>{s.isLong ? "long" : "short"}</td>
                <td className="num">{formatUsd(s.notional)}</td>
                <td className="num">{formatUsd(s.initialMargin)}</td>
                <td className="num">{formatUsd(s.maintenanceMargin)}</td>
                <td className="num">{s.version.toString()}</td>
                <td className="num">{(state.clock.slot - s.slot).toString()}</td>
                <td>
                  <button
                    disabled={busy}
                    onClick={() =>
                      run(`Close ${assetSymbol(s.assetId)} snapshot on ${VENUES[s.venueIndex]?.name}`, () =>
                        bench.closeSnapshot(s.venueIndex, s.assetId)
                      )
                    }
                  >
                    Close<span className="sr-only">
                      {" "}
                      {assetSymbol(s.assetId)} on {VENUES[s.venueIndex]?.name}
                    </span>
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableScroll>
    </section>
  );
}

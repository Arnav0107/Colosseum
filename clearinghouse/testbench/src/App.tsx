import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Bench } from "./chain/bench";
import { describeError } from "./chain/errors";
import { DEFAULT_RPC, isLocalRpc } from "./model/rpc";
import type { BenchState } from "./model/views";
import { Banner } from "./components/Banner";
import { ConnectionPanel } from "./components/ConnectionPanel";
import { LogEntry, LogPanel } from "./components/LogPanel";
import { PortfolioPanel } from "./components/PortfolioPanel";
import { PositionsPanel } from "./components/PositionsPanel";
import { PricesPanel } from "./components/PricesPanel";
import { SetupPanel } from "./components/SetupPanel";
import { VenueRulePanel } from "./components/VenueRulePanel";
import type { Run } from "./components/types";

const POLL_MS = 2_000;

export function App() {
  const [rpcUrl, setRpcUrl] = useState(DEFAULT_RPC);
  const bench = useMemo(() => (isLocalRpc(rpcUrl) ? new Bench(rpcUrl) : null), [rpcUrl]);
  const [state, setState] = useState<BenchState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<LogEntry[]>([]);
  const nextId = useRef(1);

  const reload = useCallback(async () => {
    if (!bench) {
      setState(null);
      return;
    }
    try {
      setState(await bench.fetchState());
      setError(null);
    } catch (e) {
      setState(null);
      setError(`Cannot read localnet state: ${describeError(e)}`);
    }
  }, [bench]);

  useEffect(() => {
    void reload();
    const t = setInterval(() => void reload(), POLL_MS);
    return () => clearInterval(t);
  }, [reload]);

  const push = (text: string, ok: boolean) =>
    setLog((l) => [{ id: nextId.current++, at: new Date().toLocaleTimeString(), text, ok }, ...l].slice(0, 100));

  const run: Run = async (label, action) => {
    setBusy(true);
    try {
      await action();
      push(`${label}: ok`, true);
    } catch (e) {
      push(`${label}: ${describeError(e)}`, false);
    } finally {
      setBusy(false);
      await reload();
    }
  };

  return (
    <>
      <Banner />
      <main className="container" aria-busy={busy}>
        <header>
          <h1>Clearinghouse test bench</h1>
          <p className="muted">
            Drive ch_core on a local validator: set up venues and prices, report positions, compute portfolio credit and
            check it against an independent off-chain computation.
          </p>
        </header>
        <ConnectionPanel rpcUrl={rpcUrl} setRpcUrl={setRpcUrl} state={state} error={error} />
        {bench && state && state.programDeployed && (
          <>
            <SetupPanel bench={bench} state={state} run={run} busy={busy} />
            <PricesPanel bench={bench} state={state} run={run} busy={busy} />
            <PositionsPanel bench={bench} state={state} run={run} busy={busy} />
            <PortfolioPanel bench={bench} state={state} run={run} busy={busy} />
          </>
        )}
        <VenueRulePanel config={state?.config ?? null} riskModel={state?.riskModel ?? null} />
        <LogPanel entries={log} />
      </main>
    </>
  );
}

export default App;

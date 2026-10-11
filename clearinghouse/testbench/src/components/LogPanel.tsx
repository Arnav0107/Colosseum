export interface LogEntry {
  id: number;
  at: string;
  text: string;
  ok: boolean;
}

export function LogPanel({ entries }: { entries: LogEntry[] }) {
  return (
    <section className="panel" aria-labelledby="log-h">
      <h2 id="log-h">Log</h2>
      <ol className="log" role="log" aria-live="polite" aria-relevant="additions">
        {entries.length === 0 && <li className="muted">No actions yet.</li>}
        {entries.map((e) => (
          <li key={e.id} className={e.ok ? "log-ok" : "log-err"}>
            <span className="mono muted">{e.at}</span> <span className="sr-only">{e.ok ? "Success:" : "Error:"}</span>
            {e.text}
          </li>
        ))}
      </ol>
    </section>
  );
}

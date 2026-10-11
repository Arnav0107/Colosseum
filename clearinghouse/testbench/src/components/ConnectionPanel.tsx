import { TextField, Stat } from "./fields";
import { isLocalRpc } from "../model/rpc";
import type { BenchState } from "../model/views";
import { PROGRAM_ID } from "../chain/pdas";

export function ConnectionPanel(props: {
  rpcUrl: string;
  setRpcUrl: (v: string) => void;
  state: BenchState | null;
  error: string | null;
}) {
  const local = isLocalRpc(props.rpcUrl);
  return (
    <section className="panel" aria-labelledby="conn-h">
      <h2 id="conn-h">Connection</h2>
      <TextField
        label="RPC URL (localhost only)"
        value={props.rpcUrl}
        onChange={props.setRpcUrl}
        inputMode="text"
        error={local ? null : "Refused: the bench signs with fixed dev keys, so only localhost RPC URLs are allowed."}
      />
      <dl className="stats">
        <Stat label="Validator">{props.state ? "reachable" : props.error ? "unreachable" : "connecting…"}</Stat>
        <Stat label="Program">
          <span className="mono">{PROGRAM_ID.toBase58()}</span>{" "}
          {props.state ? (props.state.programDeployed ? "(loaded)" : "(NOT loaded: run scripts/dev-localnet.sh)") : ""}
        </Stat>
        <Stat label="Slot (on-chain)">{props.state ? props.state.clock.slot.toString() : "—"}</Stat>
        <Stat label="Unix time (on-chain clock)">{props.state ? props.state.clock.unixTimestamp.toString() : "—"}</Stat>
      </dl>
      {props.error && (
        <p className="field-error" role="alert">
          {props.error}
        </p>
      )}
    </section>
  );
}

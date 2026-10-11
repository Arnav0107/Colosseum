/**
 * Minimal clearinghouse keeper: keeps portfolio credit fresh and stale credit
 * released. Liveness only; it holds no role and cannot create credit that any
 * other caller could not (F14).
 *
 * Environment (all required unless noted):
 *   KEEPER_RPC_URL        RPC endpoint
 *   KEEPER_KEYPAIR        path to a JSON secret key (fee payer; never logged)
 *   KEEPER_ORACLES        "mock" (localnet MockPrice PDAs) or a path to a JSON
 *                         map {"<asset id>": "<price account>"}
 *   KEEPER_IDL            optional; default ../../testbench/src/idl/ch_core.json
 *   KEEPER_INTERVAL_MS    optional; default 10000
 *   KEEPER_MARGIN_SLOTS   optional; refresh this many slots before expiry (default 300)
 *   KEEPER_MAX_ACTIONS    optional; transactions per tick (default 50)
 *   KEEPER_INSTANCE       optional; "<index>/<count>" when running several instances
 *   KEEPER_TAKEOVER_TICKS optional; ticks before covering another instance's portfolio (default 2)
 *
 * Output: one JSON object per line (tick, action, error). Pipe it to any log
 * shipper; alert on `"type":"tick"` lines with `unexpected > 0` or on missing ticks.
 */
import * as anchor from "@coral-xyz/anchor";
import { Connection, Keypair } from "@solana/web3.js";
import { readFileSync } from "fs";
import * as path from "path";
import { AnchorChain, mapOracleResolver, mockOracleResolver } from "./chain";
import { Keeper, KeeperHooks } from "./keeper";

function env(name: string, fallback?: string): string {
  const v = process.env[name] ?? fallback;
  if (v === undefined || v === "") throw new Error(`missing required environment variable ${name}`);
  return v;
}

const line = (o: Record<string, unknown>) =>
  console.log(JSON.stringify(o, (_k, v) => (typeof v === "bigint" ? v.toString() : v)));

export const jsonLineHooks: KeeperHooks = {
  onTick: (s) => line({ type: "tick", at: new Date().toISOString(), ...s }),
  onAction: (r) => line({ type: "action", at: new Date().toISOString(), ...r }),
  onError: (context, e) =>
    line({ type: "error", at: new Date().toISOString(), context, error: e instanceof Error ? e.message : String(e) }),
};

/** `KEEPER_INSTANCE="<index>/<count>"`, e.g. "0/2"; unset = this instance owns everything. */
export function parseInstance(spec: string | undefined, takeoverTicks: number) {
  if (spec === undefined || spec === "") return undefined;
  const m = /^(\d+)\/(\d+)$/.exec(spec);
  const index = m ? Number(m[1]) : NaN;
  const count = m ? Number(m[2]) : NaN;
  if (!(count >= 1 && index < count && Number.isInteger(takeoverTicks) && takeoverTicks >= 0)) {
    throw new Error(`KEEPER_INSTANCE must be "<index>/<count>" with index < count; got ${spec}`);
  }
  return { index, count, takeoverTicks };
}

export async function main(): Promise<void> {
  const rpc = env("KEEPER_RPC_URL");
  const secret = JSON.parse(readFileSync(env("KEEPER_KEYPAIR"), "utf8")) as number[];
  const payer = Keypair.fromSecretKey(Uint8Array.from(secret));
  const idlPath = env("KEEPER_IDL", path.resolve(__dirname, "../../../testbench/src/idl/ch_core.json"));
  const idl = JSON.parse(readFileSync(idlPath, "utf8")) as anchor.Idl;
  const oracles = env("KEEPER_ORACLES");
  const connection = new Connection(rpc, "confirmed");
  const programId = new anchor.web3.PublicKey(idl.address);
  const resolver =
    oracles === "mock"
      ? mockOracleResolver(programId)
      : mapOracleResolver(JSON.parse(readFileSync(oracles, "utf8")) as Record<string, string>);

  const keeper = new Keeper(
    new AnchorChain(connection, payer, idl, resolver),
    {
      refreshMarginSlots: BigInt(env("KEEPER_MARGIN_SLOTS", "300")),
      maxActionsPerTick: Number(env("KEEPER_MAX_ACTIONS", "50")),
      instance: parseInstance(process.env.KEEPER_INSTANCE, Number(env("KEEPER_TAKEOVER_TICKS", "2"))),
    },
    jsonLineHooks
  );
  const stop = new AbortController();
  process.on("SIGINT", () => stop.abort());
  process.on("SIGTERM", () => stop.abort());
  // Origin only: provider URLs often carry an API key in the path or query.
  line({ type: "start", rpc: new URL(rpc).origin, program: programId.toBase58(), payer: payer.publicKey.toBase58(), oracles });
  await keeper.run(Number(env("KEEPER_INTERVAL_MS", "10000")), stop.signal);
  line({ type: "stop", metrics: keeper.metrics() });
}

if (require.main === module) {
  main().catch((e) => {
    jsonLineHooks.onError?.("startup", e);
    process.exit(1);
  });
}

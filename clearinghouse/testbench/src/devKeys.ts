import { Keypair } from "@solana/web3.js";
import { createHash } from "crypto";

export function deriveDevKeypair(seedString: string): Keypair {
  const hash = createHash("sha256").update(seedString).digest();
  return Keypair.fromSeed(new Uint8Array(hash));
}

export const DEV_KEYS = {
  admin: deriveDevKeypair("clearinghouse-testbench-admin"),
  keeper: deriveDevKeypair("clearinghouse-testbench-keeper"),
  venueAuthA: deriveDevKeypair("clearinghouse-testbench-venue-a"),
  venueAuthB: deriveDevKeypair("clearinghouse-testbench-venue-b"),
  trader: deriveDevKeypair("clearinghouse-testbench-trader"),
};

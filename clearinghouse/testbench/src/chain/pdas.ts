import { PublicKey } from "@solana/web3.js";
import { Buffer } from "buffer";
import idl from "../idl/ch_core.json";

export const PROGRAM_ID = new PublicKey(idl.address);

const find = (...seeds: (Buffer | Uint8Array)[]): PublicKey =>
  PublicKey.findProgramAddressSync(seeds, PROGRAM_ID)[0];

/** 32-byte venue id: the UTF-8 name, zero-padded (names are at most 32 bytes). */
export function venueIdBytes(name: string): Buffer {
  const b = Buffer.alloc(32);
  const encoded = Buffer.from(name, "utf8");
  if (encoded.length > 32) throw new Error(`venue name longer than 32 bytes: ${name}`);
  encoded.copy(b);
  return b;
}

export const pdas = {
  config: () => find(Buffer.from("config")),
  riskModel: () => find(Buffer.from("risk_model")),
  pendingChange: () => find(Buffer.from("pending_change")),
  venue: (venueId: Buffer) => find(Buffer.from("venue"), venueId),
  consent: (user: PublicKey) => find(Buffer.from("consent"), user.toBuffer()),
  portfolio: (user: PublicKey) => find(Buffer.from("portfolio"), user.toBuffer()),
  snapshot: (user: PublicKey, venueId: Buffer, assetId: number) =>
    find(Buffer.from("snapshot"), user.toBuffer(), venueId, Buffer.from([assetId])),
  mockPrice: (assetId: number) => find(Buffer.from("mock_price"), Buffer.from([assetId])),
};

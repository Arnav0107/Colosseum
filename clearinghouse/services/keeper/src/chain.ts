/** ChainAdapter over Anchor + web3.js. The IDL is read at runtime (see index.ts). */
import * as anchor from "@coral-xyz/anchor";
import { Connection, Keypair, PublicKey, SYSVAR_CLOCK_PUBKEY } from "@solana/web3.js";
import type { ProtocolState } from "./decide";
import { ChainAdapter, KeeperTxError, PortfolioRef } from "./keeper";

/** Which price account to pass for an asset (mock PDA on localnet, Pyth update account otherwise). */
export type OracleResolver = (assetId: number) => PublicKey;

const big = (v: { toString(): string }): bigint => BigInt(v.toString());

export function mockOracleResolver(programId: PublicKey): OracleResolver {
  return (asset) => PublicKey.findProgramAddressSync([Buffer.from("mock_price"), Buffer.from([asset])], programId)[0];
}

/** `{"0": "<price account>", "1": "..."}`: unknown assets fail closed (no refresh). */
export function mapOracleResolver(map: Record<string, string>): OracleResolver {
  const keys = new Map(Object.entries(map).map(([a, k]) => [Number(a), new PublicKey(k)]));
  return (asset) => {
    const k = keys.get(asset);
    if (!k) throw new KeeperTxError("OracleNotConfigured", `no oracle account configured for asset ${asset}`);
    return k;
  };
}

function toTxError(e: unknown): KeeperTxError {
  if (e instanceof KeeperTxError) return e;
  if (e instanceof anchor.AnchorError) return new KeeperTxError(e.error.errorCode.code, e.message);
  const logs = (e as { logs?: string[] })?.logs;
  if (Array.isArray(logs)) {
    const parsed = anchor.AnchorError.parse(logs);
    if (parsed) return new KeeperTxError(parsed.error.errorCode.code, parsed.message);
  }
  return new KeeperTxError(null, e instanceof Error ? e.message : String(e));
}

export class AnchorChain implements ChainAdapter {
  readonly program: anchor.Program;
  private readonly pda = (...seeds: Buffer[]) => PublicKey.findProgramAddressSync(seeds, this.program.programId)[0];

  constructor(
    readonly connection: Connection,
    payer: Keypair,
    idl: anchor.Idl,
    private readonly oracleFor: OracleResolver
  ) {
    const provider = new anchor.AnchorProvider(connection, new anchor.Wallet(payer), { commitment: "confirmed" });
    this.program = new anchor.Program(idl, provider);
  }

  private get accounts(): Record<string, anchor.AccountClient> {
    return this.program.account as unknown as Record<string, anchor.AccountClient>;
  }

  async protocol(): Promise<ProtocolState> {
    const [cfg, clock] = await Promise.all([
      this.accounts.globalConfig.fetch(this.pda(Buffer.from("config"))),
      this.connection.getAccountInfo(SYSVAR_CLOCK_PUBKEY),
    ]);
    if (!clock) throw new Error("clock sysvar unavailable");
    const c = cfg as { riskEpoch: anchor.BN; paused: boolean };
    return { slot: Buffer.from(clock.data).readBigUInt64LE(0), configEpoch: big(c.riskEpoch), paused: c.paused };
  }

  async portfolios(): Promise<PortfolioRef[]> {
    const all = await this.accounts.portfolio.all();
    return all.map(({ publicKey, account }) => {
      const p = account as {
        user: PublicKey;
        version: anchor.BN;
        status: number;
        numRegistered: number;
        riskEpoch: anchor.BN;
        expiresSlot: anchor.BN;
        counted: boolean;
      };
      return {
        address: publicKey.toBase58(),
        user: p.user.toBase58(),
        state: {
          version: big(p.version),
          status: p.status,
          numRegistered: p.numRegistered,
          riskEpoch: big(p.riskEpoch),
          expiresSlot: big(p.expiresSlot),
          counted: p.counted,
        },
      };
    });
  }

  private baseAccounts(user: PublicKey) {
    return {
      config: this.pda(Buffer.from("config")),
      riskModel: this.pda(Buffer.from("risk_model")),
      userConsent: this.pda(Buffer.from("consent"), user.toBuffer()),
      portfolio: this.pda(Buffer.from("portfolio"), user.toBuffer()),
    };
  }

  async refresh(ref: PortfolioRef): Promise<string> {
    try {
      const user = new PublicKey(ref.user);
      // Re-read: the registered set may have changed since discovery.
      const p = (await this.accounts.portfolio.fetch(new PublicKey(ref.address))) as {
        numRegistered: number;
        registered: { venueIndex: number; assetId: number }[];
      };
      const venues = (await this.accounts.venueRegistration.all()) as {
        account: { venueIndex: number; venueId: number[] };
      }[];
      const venueId = new Map(venues.map((v) => [v.account.venueIndex, Buffer.from(v.account.venueId)]));
      const keys = p.registered.slice(0, p.numRegistered);
      const snaps = keys.map((k) => {
        const id = venueId.get(k.venueIndex);
        if (!id) throw new KeeperTxError("IncompletePortfolio", `no venue registered at index ${k.venueIndex}`);
        return this.pda(Buffer.from("snapshot"), user.toBuffer(), id, Buffer.from([k.assetId]));
      });
      const oracles = keys.map((k) => this.oracleFor(k.assetId));
      return await this.program.methods
        .refreshPortfolio()
        .accountsStrict(this.baseAccounts(user))
        .remainingAccounts([...snaps, ...oracles].map((pubkey) => ({ pubkey, isSigner: false, isWritable: false })))
        .rpc();
    } catch (e) {
      throw toTxError(e);
    }
  }

  async release(ref: PortfolioRef): Promise<string> {
    try {
      return await this.program.methods
        .invalidatePortfolio()
        .accountsStrict(this.baseAccounts(new PublicKey(ref.user)))
        .rpc();
    } catch (e) {
      throw toTxError(e);
    }
  }
}

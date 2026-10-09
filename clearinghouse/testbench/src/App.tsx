import React, { useState, useEffect, useCallback, useMemo } from "react";
import * as anchor from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import idl from "./idl/ch_core.json";
import { DEV_KEYS } from "./devKeys";
import {
  combinedRisk,
  creditTotal,
  splitProRata,
  priceMoveBps,
  formatMicroUSD,
  MathLeg,
} from "./math";

interface AssetPriceState {
  assetId: number;
  name: string;
  priceMicro: bigint;
  confMicro: bigint;
  publishTs: number;
}

interface VenuePositionState {
  assetId: number;
  isLong: boolean;
  notionalValue: bigint;
  postedMargin: bigint;
  requiredMargin: bigint;
}

interface CreditState {
  creditAmount: bigint;
  validUntilSlot: bigint;
}

interface WalletBalances {
  admin: number | null;
  keeper: number | null;
  venueAuthA: number | null;
  venueAuthB: number | null;
  trader: number | null;
}

export const DEFAULT_HAIRCUT_BPS = 2_000;

export function App() {
  const [rpcUrl, setRpcUrl] = useState("http://127.0.0.1:8899");
  const [currentSlot, setCurrentSlot] = useState<number | null>(null);
  const [isRpcReachable, setIsRpcReachable] = useState<boolean>(false);
  const [isProgramDeployed, setIsProgramDeployed] = useState<boolean>(false);
  const [balances, setBalances] = useState<WalletBalances>({
    admin: null,
    keeper: null,
    venueAuthA: null,
    venueAuthB: null,
    trader: null,
  });

  const [logs, setLogs] = useState<Array<{ id: number; text: string; isError?: boolean }>>([]);
  const [presetHint, setPresetHint] = useState<string | null>(null);

  // Refuse non-local RPC URLs
  const isRpcAllowed = useMemo(() => {
    try {
      const parsed = new URL(rpcUrl);
      return (
        parsed.hostname === "localhost" ||
        parsed.hostname === "127.0.0.1" ||
        parsed.hostname === "::1"
      );
    } catch {
      return false;
    }
  }, [rpcUrl]);

  // Deterministic localnet dev keypairs derived from fixed seed strings (sha256)
  const admin = DEV_KEYS.admin;
  const keeper = DEV_KEYS.keeper;
  const venueAuthA = DEV_KEYS.venueAuthA;
  const venueAuthB = DEV_KEYS.venueAuthB;
  const trader = DEV_KEYS.trader;

  // On-chain admin mismatch state
  const [isAdminMismatch, setIsAdminMismatch] = useState(false);
  const [onChainAdmin, setOnChainAdmin] = useState<string | null>(null);

  const venueIdA = useMemo(() => {
    const b = Buffer.alloc(32);
    b.write("venue-perp-a");
    return b;
  }, []);

  const venueIdB = useMemo(() => {
    const b = Buffer.alloc(32);
    b.write("venue-perp-b");
    return b;
  }, []);

  // Setup completion status
  const [isAirdropped, setIsAirdropped] = useState(false);
  const [isInitialized, setIsInitialized] = useState(false);
  const [isVenuesRegistered, setIsVenuesRegistered] = useState(false);
  const [isConsentSet, setIsConsentSet] = useState(false);
  const [isCorrSet, setIsCorrSet] = useState(false);

  // Snapshot submission status
  const [isSnapASubmitted, setIsSnapASubmitted] = useState(false);
  const [isSnapBSubmitted, setIsSnapBSubmitted] = useState(false);

  // Timestamps when mock prices were published on-chain
  const [pricePublishTimes, setPricePublishTimes] = useState<{ [assetId: number]: number }>({});

  // Correlation settings (scaled 1e6)
  const [rho01, setRho01] = useState("800000"); // 0.8

  // Mock prices (SOL: 0, ETH: 1, BTC: 2)
  const [prices, setPrices] = useState<AssetPriceState[]>([
    {
      assetId: 0,
      name: "SOL",
      priceMicro: 150_000_000n, // $150.00
      confMicro: 50_000n,       // $0.05
      publishTs: Math.floor(Date.now() / 1000),
    },
    {
      assetId: 1,
      name: "ETH",
      priceMicro: 3_000_000_000n, // $3,000.00
      confMicro: 1_000_000n,      // $1.00
      publishTs: Math.floor(Date.now() / 1000),
    },
    {
      assetId: 2,
      name: "BTC",
      priceMicro: 65_000_000_000n, // $65,000.00
      confMicro: 10_000_000n,      // $10.00
      publishTs: Math.floor(Date.now() / 1000),
    },
  ]);

  // Positions
  const [posA, setPosA] = useState<VenuePositionState>({
    assetId: 0,
    isLong: true,
    notionalValue: 50_000_000_000n, // $50,000.00
    postedMargin: 12_000_000_000n,  // $12,000.00
    requiredMargin: 10_000_000_000n, // $10,000.00
  });

  const [posB, setPosB] = useState<VenuePositionState>({
    assetId: 1,
    isLong: false,
    notionalValue: 50_000_000_000n,
    postedMargin: 12_000_000_000n,
    requiredMargin: 10_000_000_000n,
  });

  const [snapshotPriceA, setSnapshotPriceA] = useState<bigint | null>(null);
  const [snapshotPriceB, setSnapshotPriceB] = useState<bigint | null>(null);

  // On-chain credits
  const [creditA, setCreditA] = useState<CreditState | null>(null);
  const [creditB, setCreditB] = useState<CreditState | null>(null);

  // Price guard evaluation
  const [priceGuardStatus, setPriceGuardStatus] = useState<string>("unknown");

  const addLog = useCallback((text: string, isError = false) => {
    setLogs((prev) => [{ id: Date.now() + Math.random(), text, isError }, ...prev]);
  }, []);

  // Solana Connection & Program
  const connection = useMemo(() => new anchor.web3.Connection(rpcUrl, "confirmed"), [rpcUrl]);

  const program: any = useMemo(() => {
    const wallet = {
      publicKey: admin.publicKey,
      payer: admin,
      signTransaction: async (tx: any) => {
        tx.partialSign(admin);
        return tx;
      },
      signAllTransactions: async (txs: any[]) => {
        txs.forEach((tx) => tx.partialSign(admin));
        return txs;
      },
    };
    const provider = new anchor.AnchorProvider(connection, wallet as any, { commitment: "confirmed" });
    return new anchor.Program(idl as any, provider);
  }, [connection, admin]);

  // PDAs
  const programId = program.programId;
  const [configPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("config")], programId), [programId]);
  const [venueRegAPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdA], programId), [programId, venueIdA]);
  const [venueRegBPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdB], programId), [programId, venueIdB]);
  const [userConsentPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("consent"), trader.publicKey.toBuffer()], programId), [programId, trader]);
  const [snapshotAPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("snapshot"), trader.publicKey.toBuffer(), venueIdA], programId), [programId, trader, venueIdA]);
  const [snapshotBPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("snapshot"), trader.publicKey.toBuffer(), venueIdB], programId), [programId, trader, venueIdB]);
  const [creditAPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("credit"), trader.publicKey.toBuffer(), venueIdA], programId), [programId, trader, venueIdA]);
  const [creditBPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("credit"), trader.publicKey.toBuffer(), venueIdB], programId), [programId, trader, venueIdB]);
  const [corrMatrixPda] = useMemo(() => PublicKey.findProgramAddressSync([Buffer.from("correlations")], programId), [programId]);

  // Refresh status bar: slot, program deployed, balances
  const refreshStatus = useCallback(async () => {
    if (!isRpcAllowed) {
      setIsRpcReachable(false);
      return;
    }
    try {
      const slot = await connection.getSlot();
      setCurrentSlot(slot);
      setIsRpcReachable(true);

      const progAcc = await connection.getAccountInfo(programId);
      setIsProgramDeployed(progAcc?.executable === true);

      const [bAdmin, bKeeper, bA, bB, bTrader] = await Promise.all([
        connection.getBalance(admin.publicKey),
        connection.getBalance(keeper.publicKey),
        connection.getBalance(venueAuthA.publicKey),
        connection.getBalance(venueAuthB.publicKey),
        connection.getBalance(trader.publicKey),
      ]);

      setBalances({
        admin: bAdmin / LAMPORTS_PER_SOL,
        keeper: bKeeper / LAMPORTS_PER_SOL,
        venueAuthA: bA / LAMPORTS_PER_SOL,
        venueAuthB: bB / LAMPORTS_PER_SOL,
        trader: bTrader / LAMPORTS_PER_SOL,
      });

      // 1. Airdrop status: balances > 0
      const allFunded = bAdmin > 0 && bKeeper > 0 && bA > 0 && bB > 0 && bTrader > 0;
      setIsAirdropped(allFunded);

      // 2. GlobalConfig: account exists and config.admin equals our admin
      try {
        const cfg = await program.account.globalConfig.fetchNullable(configPda);
        if (cfg) {
          setOnChainAdmin(cfg.admin.toBase58());
          if (cfg.admin.equals(admin.publicKey)) {
            setIsInitialized(true);
            setIsAdminMismatch(false);
          } else {
            setIsInitialized(false);
            setIsAdminMismatch(true);
          }
        } else {
          setOnChainAdmin(null);
          setIsInitialized(false);
          setIsAdminMismatch(false);
        }
      } catch (e) {
        console.warn("Failed to check globalConfig:", e);
      }

      // 3. Venue registrations: venue registration accounts exist
      try {
        const [vA, vB] = await Promise.all([
          program.account.venueRegistration.fetchNullable(venueRegAPda),
          program.account.venueRegistration.fetchNullable(venueRegBPda),
        ]);
        setIsVenuesRegistered(vA !== null && vB !== null);
      } catch (e) {
        console.warn("Failed to check venueRegistration:", e);
      }

      // 4. User consent: consent account active with bitmap 3
      try {
        const consent = await program.account.userConsent.fetchNullable(userConsentPda);
        const consentActive =
          consent !== null &&
          consent.isActive === true &&
          BigInt(consent.authorizedVenuesBitmap.toString()) === 3n;
        setIsConsentSet(consentActive);
      } catch (e) {
        console.warn("Failed to check userConsent:", e);
      }

      // 5. Correlation matrix: correlation matrix account exists
      try {
        const corr = await program.account.correlationMatrix.fetchNullable(corrMatrixPda);
        setIsCorrSet(corr !== null);
      } catch (e) {
        console.warn("Failed to check correlationMatrix:", e);
      }
    } catch {
      setIsRpcReachable(false);
    }
  }, [
    connection,
    isRpcAllowed,
    programId,
    admin,
    keeper,
    venueAuthA,
    venueAuthB,
    trader,
    program,
    configPda,
    venueRegAPda,
    venueRegBPda,
    userConsentPda,
    corrMatrixPda,
  ]);

  // Periodic status poll
  useEffect(() => {
    refreshStatus();
    const interval = setInterval(refreshStatus, 3000);
    return () => clearInterval(interval);
  }, [refreshStatus]);

  // Price freshness helper (< 60s since published on-chain)
  const isPriceFresh = useCallback((assetId: number) => {
    const publishedAt = pricePublishTimes[assetId];
    if (!publishedAt) return false;
    const elapsed = Math.floor(Date.now() / 1000) - publishedAt;
    return elapsed >= 0 && elapsed <= 60;
  }, [pricePublishTimes]);

  // Enhanced error handler: prints program logs, IDL error name/code, and actionable hints
  const handleTxError = async (actionName: string, err: any) => {
    let txLogs: string[] = [];

    if (err && typeof err.getLogs === "function") {
      try {
        txLogs = await err.getLogs(connection);
      } catch {
        // ignore
      }
    }
    if ((!txLogs || txLogs.length === 0) && Array.isArray(err.logs)) {
      txLogs = err.logs;
    }
    if ((!txLogs || txLogs.length === 0) && Array.isArray(err.transactionLogs)) {
      txLogs = err.transactionLogs;
    }

    const errStr = String(err?.message || err || "");
    let anchorErrorName = "";
    let anchorErrorCode = "";
    let hint = "";

    // Match against IDL errors
    for (const idlErr of idl.errors || []) {
      const codeStr = idlErr.code.toString();
      const hexCode = "0x" + idlErr.code.toString(16);
      if (
        errStr.includes(idlErr.name) ||
        errStr.includes(codeStr) ||
        errStr.includes(hexCode) ||
        txLogs.some((l) => l.includes(idlErr.name) || l.includes(hexCode))
      ) {
        anchorErrorName = idlErr.name;
        anchorErrorCode = codeStr;
        break;
      }
    }

    // Determine actionable hint
    if (
      errStr.includes("Attempt to debit an account but found no record of a prior credit") ||
      txLogs.some((l) => l.includes("Attempt to debit an account"))
    ) {
      hint = "Fee-payer has 0 SOL: run Airdrop first.";
    } else if (
      errStr.includes("already in use") ||
      txLogs.some((l) => l.includes("already in use"))
    ) {
      hint = "Account already in use: page was refreshed or accounts already initialized; restart the validator if needed.";
    } else if (
      errStr.includes("AccountNotInitialized") ||
      txLogs.some((l) => l.includes("AccountNotInitialized"))
    ) {
      hint = "Account not initialized: prerequisite setup step was skipped.";
    } else if (
      anchorErrorName === "PriceStale" ||
      errStr.includes("PriceStale") ||
      errStr.includes("6013")
    ) {
      hint = "Oracle price is stale: click Set price again to publish a fresh timestamp.";
    } else if (
      anchorErrorName === "PriceMoved" ||
      errStr.includes("PriceMoved") ||
      errStr.includes("6014")
    ) {
      hint = "Oracle price moved beyond max tolerance: submit snapshots again with current prices.";
    } else if (
      anchorErrorName === "VenueNotAuthorized" ||
      anchorErrorName === "UserConsentMissing" ||
      anchorErrorName === "VenueUnauthorized" ||
      errStr.includes("UserConsentMissing") ||
      errStr.includes("VenueNotAuthorized")
    ) {
      hint = "Consent missing or venue not authorized: click Set consent first.";
    }

    // Format final message
    let displayMsg = `${actionName} failed`;
    if (anchorErrorName) {
      displayMsg += `: AnchorError ${anchorErrorName} (code ${anchorErrorCode})`;
    } else if (hint) {
      displayMsg += `: ${hint}`;
    } else {
      const cleaned = errStr.split("Catch the `SendTransactionError`")[0].trim();
      displayMsg += `: ${cleaned}`;
    }

    addLog(displayMsg, true);
    if (hint && !displayMsg.includes(hint)) {
      addLog(`💡 Hint: ${hint}`, true);
    }

    if (txLogs && txLogs.length > 0) {
      addLog("--- Program Logs ---", true);
      for (const logLine of txLogs) {
        addLog(`> ${logLine}`, true);
      }
      addLog("--------------------", true);
    }

    await refreshStatus();
  };

  // Evaluate price guard
  useEffect(() => {
    if (snapshotPriceA === null || snapshotPriceB === null) {
      setPriceGuardStatus("ok (awaiting snapshots)");
      return;
    }
    const currentPriceA = prices.find((p) => p.assetId === posA.assetId)?.priceMicro || 0n;
    const currentPriceB = prices.find((p) => p.assetId === posB.assetId)?.priceMicro || 0n;

    const moveA = priceMoveBps(snapshotPriceA, currentPriceA);
    const moveB = priceMoveBps(snapshotPriceB, currentPriceB);

    if (moveA > 500n) {
      setPriceGuardStatus(`tripped (Venue A price moved ${moveA} bps > 500 bps)`);
    } else if (moveB > 500n) {
      setPriceGuardStatus(`tripped (Venue B price moved ${moveB} bps > 500 bps)`);
    } else {
      setPriceGuardStatus("ok");
    }
  }, [snapshotPriceA, snapshotPriceB, prices, posA.assetId, posB.assetId]);

  // 1. Airdrop: polls up to 10s until balances > 0
  const handleAirdrop = async () => {
    try {
      addLog("Requesting 10 SOL airdrop for 5 in-memory test keypairs...");

      // Check if validator is reachable
      try {
        await connection.getSlot();
      } catch {
        throw new Error(
          `Solana localnet validator is unreachable at ${rpcUrl}. Run ./scripts/dev-localnet.sh or solana-test-validator first.`
        );
      }

      await Promise.all([
        connection.requestAirdrop(admin.publicKey, 10 * LAMPORTS_PER_SOL),
        connection.requestAirdrop(keeper.publicKey, 10 * LAMPORTS_PER_SOL),
        connection.requestAirdrop(venueAuthA.publicKey, 10 * LAMPORTS_PER_SOL),
        connection.requestAirdrop(venueAuthB.publicKey, 10 * LAMPORTS_PER_SOL),
        connection.requestAirdrop(trader.publicKey, 10 * LAMPORTS_PER_SOL),
      ]);

      addLog("Airdrop requests submitted. Awaiting balance confirmations...");

      const start = Date.now();
      let funded = false;
      while (Date.now() - start < 10000) {
        const [bAdmin, bKeeper, bA, bB, bTrader] = await Promise.all([
          connection.getBalance(admin.publicKey),
          connection.getBalance(keeper.publicKey),
          connection.getBalance(venueAuthA.publicKey),
          connection.getBalance(venueAuthB.publicKey),
          connection.getBalance(trader.publicKey),
        ]);
        if (bAdmin > 0 && bKeeper > 0 && bA > 0 && bB > 0 && bTrader > 0) {
          funded = true;
          break;
        }
        await new Promise((r) => setTimeout(r, 400));
      }

      if (!funded) {
        throw new Error("Airdrop confirmation timed out after 10s waiting for balances to become > 0.");
      }

      setIsAirdropped(true);
      addLog("Airdrop confirmed! All 5 test accounts funded.");
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Airdrop", err);
    }
  };

  // 2. Initialize
  const handleInitialize = async () => {
    try {
      // Idempotency: check if already initialized on-chain
      const existingConfig = await program.account.globalConfig.fetchNullable(configPda);
      if (existingConfig) {
        if (existingConfig.admin.equals(admin.publicKey)) {
          setIsInitialized(true);
          setIsAdminMismatch(false);
          addLog("Initialize: already done");
          return;
        } else {
          setIsAdminMismatch(true);
          addLog("Initialize: this validator was initialized by another client.", true);
          return;
        }
      }

      addLog("Initializing GlobalConfig...");
      const dummyFund = Keypair.generate().publicKey;
      const sig = await program.methods
        .initialize({
          keeperAuthority: keeper.publicKey,
          defaultFundProgram: dummyFund,
          maxVenues: 10,
          haircutBps: DEFAULT_HAIRCUT_BPS,
          creditTtlSlots: new anchor.BN(3_000),
          snapshotMaxAgeSlots: new anchor.BN(10_000),
          maxCreditPerUser: new anchor.BN("100000000000"), // 100B micro-USD
          maxCreditBpsOfRequired: 7500, // 75%
          corrMinIntervalSlots: new anchor.BN(0),
          corrMaxAgeSlots: new anchor.BN(10_000),
          maxPriceAgeSecs: new anchor.BN(60),
          maxConfBps: 100, // 1%
          maxMoveBps: 500, // 5%
        })
        .accounts({
          config: configPda,
          admin: admin.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([admin])
        .rpc();
      setIsInitialized(true);
      setIsAdminMismatch(false);
      addLog(`GlobalConfig initialized: ${sig}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Initialize", err);
    }
  };

  // 3. Register Venues
  const handleRegisterVenues = async () => {
    try {
      // Idempotency: check if both venues are already registered
      const [vA, vB] = await Promise.all([
        program.account.venueRegistration.fetchNullable(venueRegAPda),
        program.account.venueRegistration.fetchNullable(venueRegBPda),
      ]);
      if (vA !== null && vB !== null) {
        setIsVenuesRegistered(true);
        addLog("Register venues: already done");
        return;
      }

      addLog("Registering Venue A and Venue B...");
      const dummyProgA = Keypair.generate().publicKey;
      const dummyProgB = Keypair.generate().publicKey;

      let sigA = "";
      if (vA === null) {
        sigA = await program.methods
          .registerVenue(Array.from(venueIdA), dummyProgA, venueAuthA.publicKey, 0, 10_000)
          .accounts({
            config: configPda,
            venueRegistration: venueRegAPda,
            admin: admin.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([admin])
          .rpc();
      }

      let sigB = "";
      if (vB === null) {
        sigB = await program.methods
          .registerVenue(Array.from(venueIdB), dummyProgB, venueAuthB.publicKey, 1, 10_000)
          .accounts({
            config: configPda,
            venueRegistration: venueRegBPda,
            admin: admin.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([admin])
          .rpc();
      }

      setIsVenuesRegistered(true);
      addLog(`Venues registered. Venue A: ${sigA || "already registered"}, Venue B: ${sigB || "already registered"}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Register venues", err);
    }
  };

  // 4. Set Consent
  const handleSetConsent = async () => {
    try {
      // Idempotency: check if trader consent is already active with bitmap 3
      const consent = await program.account.userConsent.fetchNullable(userConsentPda);
      if (consent !== null && consent.isActive && BigInt(consent.authorizedVenuesBitmap.toString()) === 3n) {
        setIsConsentSet(true);
        addLog("Set consent: already done");
        return;
      }

      addLog("Setting trader consent for venues 0 and 1...");
      const sig = await program.methods
        .updateUserConsent(true, new anchor.BN(3)) // 1 | 2 = 3
        .accounts({
          userConsent: userConsentPda,
          user: trader.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([trader])
        .rpc();
      setIsConsentSet(true);
      addLog(`Consent granted: ${sig}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Set consent", err);
    }
  };

  // 5. Set Correlation
  const handleSetCorrelation = async () => {
    try {
      // Idempotency: check if correlation matrix already exists
      const corr = await program.account.correlationMatrix.fetchNullable(corrMatrixPda);
      if (corr !== null) {
        setIsCorrSet(true);
        addLog("Set correlation: already done");
        return;
      }

      addLog(`Setting correlation matrix (rho = ${rho01})...`);
      const val = parseInt(rho01, 10);
      const mat: anchor.BN[][] = [];
      for (let i = 0; i < 8; i++) {
        const row: anchor.BN[] = [];
        for (let j = 0; j < 8; j++) {
          if (i === j) {
            row.push(new anchor.BN(1_000_000));
          } else if ((i === 0 && j === 1) || (i === 1 && j === 0)) {
            row.push(new anchor.BN(val));
          } else {
            row.push(new anchor.BN(0));
          }
        }
        mat.push(row);
      }

      const sig = await program.methods
        .updateCorrelations(mat)
        .accounts({
          config: configPda,
          correlationMatrix: corrMatrixPda,
          oracleAuthority: keeper.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([keeper])
        .rpc();
      setIsCorrSet(true);
      addLog(`Correlation updated: ${sig}`);
      setPresetHint(null);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Set correlation", err);
    }
  };

  // Set Mock Price (fixed double dollar sign)
  const handleSetPrice = async (assetId: number) => {
    try {
      const p = prices.find((item) => item.assetId === assetId);
      if (!p) return;
      const [mockPricePda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([assetId])],
        programId
      );
      const nowTs = Math.floor(Date.now() / 1000);
      addLog(`Setting mock price for ${p.name} (${formatMicroUSD(p.priceMicro)})...`);
      const sig = await program.methods
        .setMockPrice(
          assetId,
          new anchor.BN(p.priceMicro.toString()),
          new anchor.BN(p.confMicro.toString()),
          new anchor.BN(nowTs)
        )
        .accounts({
          config: configPda,
          mockPrice: mockPricePda,
          admin: admin.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([admin])
        .rpc();

      setPrices((prev) =>
        prev.map((item) => (item.assetId === assetId ? { ...item, publishTs: nowTs } : item))
      );
      setPricePublishTimes((prev) => ({ ...prev, [assetId]: nowTs }));
      addLog(`Price updated for ${p.name}: ${sig}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Set price", err);
    }
  };

  // Submit Snapshot
  const handleSubmitSnapshot = async (venueKey: "A" | "B") => {
    try {
      const isA = venueKey === "A";
      const pos = isA ? posA : posB;
      const venueId = isA ? venueIdA : venueIdB;
      const venueReg = isA ? venueRegAPda : venueRegBPda;
      const snapshotPda = isA ? snapshotAPda : snapshotBPda;
      const auth = isA ? venueAuthA : venueAuthB;

      const [mockPricePda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([pos.assetId])],
        programId
      );

      addLog(`Submitting snapshot for Venue ${venueKey}...`);
      const sig = await program.methods
        .submitPositionSnapshot(
          Array.from(venueId),
          pos.assetId,
          new anchor.BN(pos.notionalValue.toString()),
          pos.isLong,
          new anchor.BN(pos.requiredMargin.toString())
        )
        .accounts({
          config: configPda,
          venueRegistration: venueReg,
          userConsent: userConsentPda,
          posSnapshot: snapshotPda,
          priceOracle: mockPricePda,
          user: trader.publicKey,
          venueAuthority: auth.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([auth])
        .rpc();

      const currentPrice = prices.find((p) => p.assetId === pos.assetId)?.priceMicro || 0n;
      if (isA) {
        setSnapshotPriceA(currentPrice);
        setIsSnapASubmitted(true);
      } else {
        setSnapshotPriceB(currentPrice);
        setIsSnapBSubmitted(true);
      }

      addLog(`Snapshot submitted for Venue ${venueKey}: ${sig}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Submit snapshot", err);
    }
  };

  // Compute Credit
  const handleComputeCredit = async () => {
    try {
      addLog("Keeper computing credit...");
      const [mockPriceAPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([posA.assetId])],
        programId
      );
      const [mockPriceBPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([posB.assetId])],
        programId
      );

      const sig = await program.methods
        .computeCredit()
        .accounts({
          config: configPda,
          user: trader.publicKey,
          userConsent: userConsentPda,
          venueRegA: venueRegAPda,
          venueRegB: venueRegBPda,
          snapshotA: snapshotAPda,
          snapshotB: snapshotBPda,
          correlationMatrix: corrMatrixPda,
          priceA: mockPriceAPda,
          priceB: mockPriceBPda,
          creditA: creditAPda,
          creditB: creditBPda,
          keeper: keeper.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([keeper])
        .rpc();

      // Fetch on-chain accounts
      const onChainA: any = await (program.account as any).marginCredit.fetch(creditAPda);
      const onChainB: any = await (program.account as any).marginCredit.fetch(creditBPda);

      setCreditA({
        creditAmount: BigInt(onChainA.creditAmount.toString()),
        validUntilSlot: BigInt(onChainA.validUntilSlot.toString()),
      });
      setCreditB({
        creditAmount: BigInt(onChainB.creditAmount.toString()),
        validUntilSlot: BigInt(onChainB.validUntilSlot.toString()),
      });

      addLog(`Credit computed successfully: ${sig}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Compute credit", err);
    }
  };

  // Revoke Credit Pair (Keeper)
  const handleRevokeCreditPair = async () => {
    try {
      addLog("Keeper revoking credit pair...");
      const sig = await program.methods
        .revokeCredit()
        .accounts({
          config: configPda,
          user: trader.publicKey,
          creditA: creditAPda,
          creditB: creditBPda,
          venueRegA: venueRegAPda,
          venueRegB: venueRegBPda,
          authority: keeper.publicKey,
        })
        .signers([keeper])
        .rpc();

      setCreditA((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      setCreditB((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      addLog(`Credit pair revoked successfully: ${sig}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Revoke credit pair", err);
    }
  };

  // Revoke If Unsafe (Permissionless Paired)
  const handleRevokeIfUnsafe = async () => {
    try {
      const [mockPriceAPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([posA.assetId])],
        programId
      );
      const [mockPriceBPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([posB.assetId])],
        programId
      );

      addLog("Calling permissionless revoke_if_unsafe for credit pair...");
      const sig = await program.methods
        .revokeIfUnsafe()
        .accounts({
          config: configPda,
          user: trader.publicKey,
          venueRegA: venueRegAPda,
          venueRegB: venueRegBPda,
          snapshotA: snapshotAPda,
          snapshotB: snapshotBPda,
          priceOracleA: mockPriceAPda,
          priceOracleB: mockPriceBPda,
          creditA: creditAPda,
          creditB: creditBPda,
        })
        .rpc();

      setCreditA((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      setCreditB((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      addLog(`Unsafe credit pair revoked successfully: ${sig}`);
      await refreshStatus();
    } catch (err: any) {
      await handleTxError("Revoke if unsafe", err);
    }
  };

  // Apply Presets: only fills the forms, prompts to click Set correlation
  const applyPresetHedged = () => {
    setPosA({
      assetId: 0,
      isLong: true,
      notionalValue: 50_000_000_000n,
      postedMargin: 12_000_000_000n,
      requiredMargin: 10_000_000_000n,
    });
    setPosB({
      assetId: 1,
      isLong: false,
      notionalValue: 50_000_000_000n,
      postedMargin: 12_000_000_000n,
      requiredMargin: 10_000_000_000n,
    });
    setRho01("800000");
    const hintMsg = "Preset loaded (Hedged pair, rho 0.8). Click Set correlation to send this rho on-chain.";
    setPresetHint(hintMsg);
    addLog(hintMsg);
  };

  const applyPresetSame = () => {
    setPosA({
      assetId: 0,
      isLong: true,
      notionalValue: 50_000_000_000n,
      postedMargin: 12_000_000_000n,
      requiredMargin: 10_000_000_000n,
    });
    setPosB({
      assetId: 1,
      isLong: true,
      notionalValue: 50_000_000_000n,
      postedMargin: 12_000_000_000n,
      requiredMargin: 10_000_000_000n,
    });
    setRho01("800000");
    const hintMsg = "Preset loaded (Same direction legs, rho 0.8). Click Set correlation to send this rho on-chain.";
    setPresetHint(hintMsg);
    addLog(hintMsg);
  };

  const applyPresetPerfect = () => {
    setPosA({
      assetId: 0,
      isLong: true,
      notionalValue: 50_000_000_000n,
      postedMargin: 12_000_000_000n,
      requiredMargin: 10_000_000_000n,
    });
    setPosB({
      assetId: 1,
      isLong: false,
      notionalValue: 50_000_000_000n,
      postedMargin: 12_000_000_000n,
      requiredMargin: 10_000_000_000n,
    });
    setRho01("1000000");
    const hintMsg = "Preset loaded (Perfect hedge, rho 1.0). Click Set correlation to send this rho on-chain.";
    setPresetHint(hintMsg);
    addLog(hintMsg);
  };

  // Pure TS Math Evaluation
  const tsMathResult = useMemo(() => {
    try {
      const legs: MathLeg[] = [
        {
          asset: posA.assetId,
          signedRequiredMargin: posA.isLong ? posA.requiredMargin : -posA.requiredMargin,
        },
        {
          asset: posB.assetId,
          signedRequiredMargin: posB.isLong ? posB.requiredMargin : -posB.requiredMargin,
        },
      ];
      const rhoVal = BigInt(rho01);
      const rho = (a: number, b: number) => {
        if ((a === 0 && b === 1) || (a === 1 && b === 0)) return rhoVal;
        return 0n;
      };

      const combined = combinedRisk(legs, rho);
      const sumReq = posA.requiredMargin + posB.requiredMargin;
      const total = creditTotal(sumReq, combined, 0);
      const split = splitProRata(total, [posA.requiredMargin, posB.requiredMargin]);

      return {
        combined,
        sumReq,
        totalCredit: total,
        creditA: split[0] ?? 0n,
        creditB: split[1] ?? 0n,
      };
    } catch (err: any) {
      return {
        combined: 0n,
        sumReq: 0n,
        totalCredit: 0n,
        creditA: 0n,
        creditB: 0n,
        error: err.message,
      };
    }
  }, [posA, posB, rho01]);

  // Risk Bar percentages
  const separateSum = posA.requiredMargin + posB.requiredMargin;
  const nettedRisk = tsMathResult.combined;
  const freedCapital = (creditA?.creditAmount || 0n) + (creditB?.creditAmount || 0n);

  const baseMax = separateSum > 0n ? separateSum : 1n;
  const separatePct = 100;
  const nettedPct = Math.min(100, Math.round(Number((nettedRisk * 100n) / baseMax)));
  const freedPct = Math.min(100, Math.round(Number((freedCapital * 100n) / baseMax)));

  const nowSecs = Math.floor(Date.now() / 1000);

  // Prerequisite evaluation for UI buttons
  const airdropDisabled = !isRpcAllowed || isAdminMismatch;
  const airdropHint = isAdminMismatch ? "Validator initialized by another client" : "";

  const initDisabled = !isAirdropped || isAdminMismatch;
  const initHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isAirdropped
    ? "do Airdrop first"
    : "";

  const venuesDisabled = !isInitialized || isAdminMismatch;
  const venuesHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isInitialized
    ? (!isAirdropped ? "do Airdrop first" : "Initialize first")
    : "";

  const consentDisabled = !isAirdropped || isAdminMismatch;
  const consentHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isAirdropped
    ? "do Airdrop first"
    : "";

  const corrDisabled = !isInitialized || isAdminMismatch;
  const corrHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isInitialized
    ? (!isAirdropped ? "do Airdrop first" : "Initialize first")
    : "";

  const priceDisabled = !isInitialized || isAdminMismatch;
  const priceHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isInitialized
    ? (!isAirdropped ? "do Airdrop first" : "Initialize first")
    : "";

  const snapADisabled = !isVenuesRegistered || !isConsentSet || !isPriceFresh(posA.assetId) || isAdminMismatch;
  const snapAHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isVenuesRegistered
    ? "Register venues first"
    : !isConsentSet
    ? "Set consent first"
    : !isPriceFresh(posA.assetId)
    ? `Set Asset ${posA.assetId} price first (<60s)`
    : "";

  const snapBDisabled = !isVenuesRegistered || !isConsentSet || !isPriceFresh(posB.assetId) || isAdminMismatch;
  const snapBHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isVenuesRegistered
    ? "Register venues first"
    : !isConsentSet
    ? "Set consent first"
    : !isPriceFresh(posB.assetId)
    ? `Set Asset ${posB.assetId} price first (<60s)`
    : "";

  const computeCreditDisabled = !isSnapASubmitted || !isSnapBSubmitted || !isCorrSet || isAdminMismatch;
  const computeCreditHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !isCorrSet
    ? "Set correlation first"
    : !isSnapASubmitted || !isSnapBSubmitted
    ? "Submit both snapshots first"
    : "";

  const hasActiveCredit = (creditA?.creditAmount || 0n) > 0n || (creditB?.creditAmount || 0n) > 0n;
  const revokeDisabled = !hasActiveCredit || isAdminMismatch;
  const revokeHint = isAdminMismatch
    ? "Validator initialized by another client"
    : !hasActiveCredit
    ? "Compute credit first"
    : "";

  return (
    <div className="container">
      {/* 1. Header & Live Status Bar */}
      <div className="header-row">
        <h1>Clearinghouse test bench</h1>
        <div className="status-line">
          <div className="status-item">
            RPC: <span>{rpcUrl}</span>{" "}
            {isRpcAllowed ? (
              isRpcReachable ? (
                <b style={{ color: "var(--ok)" }}>[Reachable]</b>
              ) : (
                <b style={{ color: "var(--error)" }}>[Unreachable]</b>
              )
            ) : (
              <b style={{ color: "var(--error)" }}>[Refused - localnet only]</b>
            )}
          </div>
          <div className="status-item">
            SLOT: <span>{currentSlot !== null ? currentSlot : "connecting..."}</span>
          </div>
          <div className="status-item">
            PROGRAM: <span>{programId.toBase58().slice(0, 8)}...</span>{" "}
            {isProgramDeployed ? (
              <b style={{ color: "var(--ok)" }}>[Deployed]</b>
            ) : (
              <b style={{ color: "var(--error)" }}>[Not Deployed]</b>
            )}
          </div>
        </div>

        {/* Dynamic Wallet Balances Bar */}
        <div className="status-grid">
          <div className="status-cell">
            <span className="status-cell-title">Admin Wallet</span>
            <span style={{ fontSize: "10px", color: "var(--text-muted)", fontFamily: "monospace" }}>
              {admin.publicKey.toBase58().slice(0, 6)}...{admin.publicKey.toBase58().slice(-4)}
            </span>
            <span className="status-cell-val">
              {balances.admin !== null ? `${balances.admin.toFixed(2)} SOL` : "0.00 SOL"}
            </span>
          </div>
          <div className="status-cell">
            <span className="status-cell-title">Keeper Wallet</span>
            <span style={{ fontSize: "10px", color: "var(--text-muted)", fontFamily: "monospace" }}>
              {keeper.publicKey.toBase58().slice(0, 6)}...{keeper.publicKey.toBase58().slice(-4)}
            </span>
            <span className="status-cell-val">
              {balances.keeper !== null ? `${balances.keeper.toFixed(2)} SOL` : "0.00 SOL"}
            </span>
          </div>
          <div className="status-cell">
            <span className="status-cell-title">Venue A Authority</span>
            <span style={{ fontSize: "10px", color: "var(--text-muted)", fontFamily: "monospace" }}>
              {venueAuthA.publicKey.toBase58().slice(0, 6)}...{venueAuthA.publicKey.toBase58().slice(-4)}
            </span>
            <span className="status-cell-val">
              {balances.venueAuthA !== null ? `${balances.venueAuthA.toFixed(2)} SOL` : "0.00 SOL"}
            </span>
          </div>
          <div className="status-cell">
            <span className="status-cell-title">Venue B Authority</span>
            <span style={{ fontSize: "10px", color: "var(--text-muted)", fontFamily: "monospace" }}>
              {venueAuthB.publicKey.toBase58().slice(0, 6)}...{venueAuthB.publicKey.toBase58().slice(-4)}
            </span>
            <span className="status-cell-val">
              {balances.venueAuthB !== null ? `${balances.venueAuthB.toFixed(2)} SOL` : "0.00 SOL"}
            </span>
          </div>
          <div className="status-cell">
            <span className="status-cell-title">Trader Wallet</span>
            <span style={{ fontSize: "10px", color: "var(--text-muted)", fontFamily: "monospace" }}>
              {trader.publicKey.toBase58().slice(0, 6)}...{trader.publicKey.toBase58().slice(-4)}
            </span>
            <span className="status-cell-val">
              {balances.trader !== null ? `${balances.trader.toFixed(2)} SOL` : "0.00 SOL"}
            </span>
          </div>
        </div>
      </div>

      <div className="safety-note">
        Notice: Localnet only. Keys are deterministic public dev keypairs derived from fixed seed strings (sha256). These keys are public and must never be used on any real network or funded with real assets. Non-localhost RPC URLs are strictly refused.
      </div>

      {!isRpcAllowed && (
        <div className="panel" style={{ borderColor: "var(--error)", marginBottom: 20 }}>
          <b style={{ color: "var(--error)" }}>Access restricted:</b> Only local test validator URLs (localhost / 127.0.0.1) are permitted.
        </div>
      )}

      {/* 2. Setup Flow */}
      <div className="section panel">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 12 }}>
          <h2>Setup Flow</h2>
          <button className="small" onClick={() => refreshStatus()}>
            Re-check chain state
          </button>
        </div>

        {isAdminMismatch && (
          <div
            className="admin-mismatch-banner"
            style={{
              marginBottom: 16,
              padding: "12px 16px",
              background: "#fee2e2",
              color: "#991b1b",
              border: "1px solid #f87171",
              borderRadius: "6px",
              display: "flex",
              flexDirection: "column",
              gap: 8,
            }}
          >
            <div style={{ fontWeight: 600 }}>
              This validator was initialized by another client. Stop it and run scripts/dev-localnet.sh again.
            </div>
            <div style={{ fontSize: "12px", color: "#b91c1c", fontFamily: "monospace" }}>
              On-chain admin: {onChainAdmin || "unknown"} | Expected dev admin: {admin.publicKey.toBase58()}
            </div>
            <div>
              <button className="small" onClick={() => refreshStatus()}>
                Re-check chain state
              </button>
            </div>
          </div>
        )}

        <div className="grid-setup">
          <div className="setup-step">
            <div>
              <button onClick={handleAirdrop} disabled={airdropDisabled}>
                1. Airdrop
              </button>
              {airdropHint && <span className="btn-hint">{airdropHint}</span>}
            </div>
            <span className={`badge ${isAirdropped ? "badge-done" : "badge-not-done"}`}>
              {isAirdropped ? "done" : "not done"}
            </span>
          </div>

          <div className="setup-step">
            <div>
              <button onClick={handleInitialize} disabled={initDisabled}>
                2. Initialize
              </button>
              {initHint && <span className="btn-hint">{initHint}</span>}
            </div>
            <span className={`badge ${isInitialized ? "badge-done" : "badge-not-done"}`}>
              {isInitialized ? "done" : "not done"}
            </span>
          </div>

          <div className="setup-step">
            <div>
              <button onClick={handleRegisterVenues} disabled={venuesDisabled}>
                3. Register venues
              </button>
              {venuesHint && <span className="btn-hint">{venuesHint}</span>}
            </div>
            <span className={`badge ${isVenuesRegistered ? "badge-done" : "badge-not-done"}`}>
              {isVenuesRegistered ? "done" : "not done"}
            </span>
          </div>

          <div className="setup-step">
            <div>
              <button onClick={handleSetConsent} disabled={consentDisabled}>
                4. Set consent
              </button>
              {consentHint && <span className="btn-hint">{consentHint}</span>}
            </div>
            <span className={`badge ${isConsentSet ? "badge-done" : "badge-not-done"}`}>
              {isConsentSet ? "done" : "not done"}
            </span>
          </div>

          <div className="setup-step">
            <input
              type="text"
              style={{ width: 90 }}
              value={rho01}
              onChange={(e) => setRho01(e.target.value)}
              placeholder="rho 1e6"
            />
            <div>
              <button onClick={handleSetCorrelation} disabled={corrDisabled}>
                5. Set correlation
              </button>
              {corrHint && <span className="btn-hint">{corrHint}</span>}
            </div>
            <span className={`badge ${isCorrSet ? "badge-done" : "badge-not-done"}`}>
              {isCorrSet ? "done" : "not done"}
            </span>
          </div>
        </div>
      </div>

      {/* 3. Mock Prices */}
      <div className="section panel">
        <h2>Mock Price Feeds</h2>
        <div className="grid-cols-3">
          {prices.map((p) => {
            const ageSecs = nowSecs - p.publishTs;
            const fresh = isPriceFresh(p.assetId);
            return (
              <div key={p.assetId} style={{ borderRight: "1px solid var(--hairline)", paddingRight: 10 }}>
                <div style={{ fontWeight: 600, marginBottom: 4 }}>
                  Asset {p.assetId} — {p.name}
                </div>
                <div className="label">Price (micro-USD)</div>
                <input
                  type="text"
                  style={{ width: "100%", marginBottom: 6 }}
                  value={p.priceMicro.toString()}
                  onChange={(e) => {
                    const val = BigInt(e.target.value || "0");
                    setPrices((prev) =>
                      prev.map((item) => (item.assetId === p.assetId ? { ...item, priceMicro: val } : item))
                    );
                  }}
                />
                <div className="muted-sub" style={{ marginBottom: 6 }}>
                  {formatMicroUSD(p.priceMicro)}
                </div>

                <div className="label">Confidence (micro-USD)</div>
                <input
                  type="text"
                  style={{ width: "100%", marginBottom: 6 }}
                  value={p.confMicro.toString()}
                  onChange={(e) => {
                    const val = BigInt(e.target.value || "0");
                    setPrices((prev) =>
                      prev.map((item) => (item.assetId === p.assetId ? { ...item, confMicro: val } : item))
                    );
                  }}
                />

                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    <button className="small" onClick={() => handleSetPrice(p.assetId)} disabled={priceDisabled}>
                      Set price
                    </button>
                    <span className="muted-sub">
                      Age: {ageSecs}s {fresh ? <span style={{ color: "var(--ok)" }}>(fresh)</span> : <span style={{ color: "var(--error)" }}>(stale)</span>}
                    </span>
                  </div>
                  {priceHint && <span className="btn-hint">{priceHint}</span>}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* 4. Positions & Presets */}
      <div className="section panel">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
          <h2>Trading Positions</h2>
          <div className="presets-row" style={{ margin: 0 }}>
            <span className="label" style={{ marginRight: 4 }}>Presets:</span>
            <button className="small" onClick={applyPresetHedged}>Hedged pair (rho 0.8)</button>
            <button className="small" onClick={applyPresetSame}>Same direction</button>
            <button className="small" onClick={applyPresetPerfect}>Perfect hedge</button>
          </div>
        </div>

        {presetHint && <div className="preset-hint-banner">{presetHint}</div>}

        <div className="grid-cols-2">
          {/* Venue A Column */}
          <div style={{ borderRight: "1px solid var(--hairline)", paddingRight: 12 }}>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>Venue A (Index 0)</div>
            <div className="flex-row" style={{ marginBottom: 8 }}>
              <div>
                <div className="label">Asset</div>
                <select
                  value={posA.assetId}
                  onChange={(e) => setPosA({ ...posA, assetId: parseInt(e.target.value, 10) })}
                >
                  <option value={0}>0 (SOL)</option>
                  <option value={1}>1 (ETH)</option>
                  <option value={2}>2 (BTC)</option>
                </select>
              </div>
              <div>
                <div className="label">Side</div>
                <select
                  value={posA.isLong ? "long" : "short"}
                  onChange={(e) => setPosA({ ...posA, isLong: e.target.value === "long" })}
                >
                  <option value="long">Long</option>
                  <option value="short">Short</option>
                </select>
              </div>
            </div>

            <div style={{ marginBottom: 6 }}>
              <div className="label">Notional Value</div>
              <input
                type="text"
                style={{ width: "100%" }}
                value={posA.notionalValue.toString()}
                onChange={(e) => setPosA({ ...posA, notionalValue: BigInt(e.target.value || "0") })}
              />
              <div className="muted-sub">{formatMicroUSD(posA.notionalValue)}</div>
            </div>

            <div style={{ marginBottom: 6 }}>
              <div className="label">Own Required Margin</div>
              <input
                type="text"
                style={{ width: "100%" }}
                value={posA.requiredMargin.toString()}
                onChange={(e) => setPosA({ ...posA, requiredMargin: BigInt(e.target.value || "0") })}
              />
              <div className="muted-sub">{formatMicroUSD(posA.requiredMargin)}</div>
            </div>

            <div>
              <button className="small primary" onClick={() => handleSubmitSnapshot("A")} disabled={snapADisabled}>
                Submit snapshot
              </button>
              {snapAHint && <span className="btn-hint">{snapAHint}</span>}
            </div>
          </div>

          {/* Venue B Column */}
          <div style={{ paddingLeft: 4 }}>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>Venue B (Index 1)</div>
            <div className="flex-row" style={{ marginBottom: 8 }}>
              <div>
                <div className="label">Asset</div>
                <select
                  value={posB.assetId}
                  onChange={(e) => setPosB({ ...posB, assetId: parseInt(e.target.value, 10) })}
                >
                  <option value={0}>0 (SOL)</option>
                  <option value={1}>1 (ETH)</option>
                  <option value={2}>2 (BTC)</option>
                </select>
              </div>
              <div>
                <div className="label">Side</div>
                <select
                  value={posB.isLong ? "long" : "short"}
                  onChange={(e) => setPosB({ ...posB, isLong: e.target.value === "long" })}
                >
                  <option value="long">Long</option>
                  <option value="short">Short</option>
                </select>
              </div>
            </div>

            <div style={{ marginBottom: 6 }}>
              <div className="label">Notional Value</div>
              <input
                type="text"
                style={{ width: "100%" }}
                value={posB.notionalValue.toString()}
                onChange={(e) => setPosB({ ...posB, notionalValue: BigInt(e.target.value || "0") })}
              />
              <div className="muted-sub">{formatMicroUSD(posB.notionalValue)}</div>
            </div>

            <div style={{ marginBottom: 6 }}>
              <div className="label">Own Required Margin</div>
              <input
                type="text"
                style={{ width: "100%" }}
                value={posB.requiredMargin.toString()}
                onChange={(e) => setPosB({ ...posB, requiredMargin: BigInt(e.target.value || "0") })}
              />
              <div className="muted-sub">{formatMicroUSD(posB.requiredMargin)}</div>
            </div>

            <div>
              <button className="small primary" onClick={() => handleSubmitSnapshot("B")} disabled={snapBDisabled}>
                Submit snapshot
              </button>
              {snapBHint && <span className="btn-hint">{snapBHint}</span>}
            </div>
          </div>
        </div>
      </div>

      {/* 5. Results & Actions */}
      <div className="section panel">
        <h2>Credit Allocation & Risk Ledger</h2>
        <div className="flex-row" style={{ marginBottom: 8, alignItems: "flex-start", gap: 12 }}>
          <div>
            <button className="primary" onClick={handleComputeCredit} disabled={computeCreditDisabled}>
              Compute credit
            </button>
            {computeCreditHint && <span className="btn-hint">{computeCreditHint}</span>}
          </div>
          <div>
            <button onClick={handleRevokeCreditPair} disabled={revokeDisabled}>
              Revoke credit pair
            </button>
            {revokeHint && <span className="btn-hint">{revokeHint}</span>}
          </div>
          <div>
            <button className="btn-error" onClick={handleRevokeIfUnsafe} disabled={revokeDisabled}>
              Revoke if unsafe
            </button>
            {revokeHint && <span className="btn-hint">{revokeHint}</span>}
          </div>
        </div>

        <div style={{ marginBottom: 10, fontSize: 12 }}>
          Price guard:{" "}
          <span style={{ fontWeight: 600, color: priceGuardStatus.startsWith("ok") ? "var(--ok)" : "var(--error)" }}>
            {priceGuardStatus}
          </span>
        </div>

        {/* Ledger Table */}
        <table>
          <thead>
            <tr>
              <th>Venue</th>
              <th className="num">Own Required</th>
              <th className="num">Credit Amount</th>
              <th className="num">Effective Required</th>
              <th className="num">Valid Until Slot</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>Venue A</td>
              <td className="num">
                <div>{formatMicroUSD(posA.requiredMargin)}</div>
                <div className="muted-sub">{posA.requiredMargin.toString()}</div>
              </td>
              <td className="num">
                <div>{formatMicroUSD(creditA?.creditAmount || 0n)}</div>
                <div className="muted-sub">{(creditA?.creditAmount || 0n).toString()}</div>
              </td>
              <td className="num">
                <div>{formatMicroUSD(posA.requiredMargin - (creditA?.creditAmount || 0n))}</div>
                <div className="muted-sub">{(posA.requiredMargin - (creditA?.creditAmount || 0n)).toString()}</div>
              </td>
              <td className="num">{creditA?.validUntilSlot.toString() || "—"}</td>
            </tr>
            <tr>
              <td>Venue B</td>
              <td className="num">
                <div>{formatMicroUSD(posB.requiredMargin)}</div>
                <div className="muted-sub">{posB.requiredMargin.toString()}</div>
              </td>
              <td className="num">
                <div>{formatMicroUSD(creditB?.creditAmount || 0n)}</div>
                <div className="muted-sub">{(creditB?.creditAmount || 0n).toString()}</div>
              </td>
              <td className="num">
                <div>{formatMicroUSD(posB.requiredMargin - (creditB?.creditAmount || 0n))}</div>
                <div className="muted-sub">{(posB.requiredMargin - (creditB?.creditAmount || 0n)).toString()}</div>
              </td>
              <td className="num">{creditB?.validUntilSlot.toString() || "—"}</td>
            </tr>
          </tbody>
        </table>

        {/* CSS-only Risk Visualizer Bars */}
        <div className="bars-container">
          <div className="bar-row">
            <div className="bar-label-line">
              <span>Each venue separately</span>
              <span>{formatMicroUSD(separateSum)} (100%)</span>
            </div>
            <div className="bar-track">
              <div className="bar-fill bar-fill-ink" style={{ width: `${separatePct}%` }} />
            </div>
          </div>

          <div className="bar-row">
            <div className="bar-label-line">
              <span>Netted portfolio risk</span>
              <span>{formatMicroUSD(nettedRisk)} ({nettedPct}%)</span>
            </div>
            <div className="bar-track">
              <div className="bar-fill bar-fill-secondary" style={{ width: `${nettedPct}%` }} />
            </div>
          </div>

          <div className="bar-row">
            <div className="bar-label-line">
              <span>Freed capital (credit issued)</span>
              <span>{formatMicroUSD(freedCapital)} ({freedPct}%)</span>
            </div>
            <div className="bar-track">
              <div className="bar-fill bar-fill-ok" style={{ width: `${freedPct}%` }} />
            </div>
          </div>
        </div>

        {/* Mathematical Invariant Verification */}
        <div className="checks-box">
          <div className="label" style={{ marginBottom: 6 }}>
            Mathematical Verification (On-Chain vs TypeScript BigInt Engine)
          </div>
          {creditA !== null && creditB !== null ? (
            <>
              <div className="check-line">
                <span>Venue A Credit:</span>
                <span>
                  On-chain {creditA.creditAmount.toString()} vs TS {tsMathResult.creditA.toString()} —{" "}
                  {creditA.creditAmount === tsMathResult.creditA ? (
                    <span className="match-ok">MATCH</span>
                  ) : (
                    <span className="match-err">MISMATCH</span>
                  )}
                </span>
              </div>
              <div className="check-line">
                <span>Venue B Credit:</span>
                <span>
                  On-chain {creditB.creditAmount.toString()} vs TS {tsMathResult.creditB.toString()} —{" "}
                  {creditB.creditAmount === tsMathResult.creditB ? (
                    <span className="match-ok">MATCH</span>
                  ) : (
                    <span className="match-err">MISMATCH</span>
                  )}
                </span>
              </div>
            </>
          ) : (
            <div className="muted-sub">Compute credit to execute live verification check.</div>
          )}
        </div>
      </div>

      {/* 6. Execution Log */}
      <div className="section">
        <h2>Execution Log</h2>
        <div className="log-box">
          {logs.length === 0 && <div className="muted-sub">Ready. Execute actions above.</div>}
          {logs.map((item) => (
            <div key={item.id} className={`log-entry ${item.isError ? "log-err" : "log-ok"}`}>
              {item.text}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default App;

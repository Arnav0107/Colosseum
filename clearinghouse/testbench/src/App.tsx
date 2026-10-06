import React, { useState, useEffect, useCallback, useMemo } from "react";
import * as anchor from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import idl from "./idl/ch_core.json";
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

export function App() {
  const [rpcUrl, setRpcUrl] = useState("http://127.0.0.1:8899");
  const [currentSlot, setCurrentSlot] = useState<number | null>(null);
  const [logs, setLogs] = useState<Array<{ id: number; text: string; isError?: boolean }>>([]);

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

  // Persistent in-memory test keypairs
  const admin = useMemo(() => Keypair.generate(), []);
  const keeper = useMemo(() => Keypair.generate(), []);
  const venueAuthA = useMemo(() => Keypair.generate(), []);
  const venueAuthB = useMemo(() => Keypair.generate(), []);
  const trader = useMemo(() => Keypair.generate(), []);

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

  // Fetch slot periodically
  useEffect(() => {
    if (!isRpcAllowed) return;
    const interval = setInterval(async () => {
      try {
        const slot = await connection.getSlot();
        setCurrentSlot(slot);
      } catch {
        // ignore connection drop
      }
    }, 2000);
    return () => clearInterval(interval);
  }, [connection, isRpcAllowed]);

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

  // 1. Airdrop
  const handleAirdrop = async () => {
    try {
      addLog("Requesting airdrops for test accounts...");
      const accounts = [admin, keeper, venueAuthA, venueAuthB, trader];
      for (const acc of accounts) {
        const sig = await connection.requestAirdrop(acc.publicKey, 5 * LAMPORTS_PER_SOL);
        const latestBlockhash = await connection.getLatestBlockhash();
        await connection.confirmTransaction({ signature: sig, ...latestBlockhash });
      }
      setIsAirdropped(true);
      addLog("Airdrop confirmed for 5 test accounts.");
    } catch (err: any) {
      addLog(`Airdrop error: ${err.message || err}`, true);
    }
  };

  // 2. Initialize
  const handleInitialize = async () => {
    try {
      addLog("Initializing GlobalConfig...");
      const dummyFund = Keypair.generate().publicKey;
      const sig = await program.methods
        .initialize({
          keeperAuthority: keeper.publicKey,
          defaultFundProgram: dummyFund,
          maxVenues: 10,
          haircutBps: 0,
          creditTtlSlots: new anchor.BN(10_000),
          snapshotMaxAgeSlots: new anchor.BN(10_000),
          maxCreditPerUser: new anchor.BN("100000000000"), // 100B micro-USD
          maxCreditBpsOfRequired: 7500, // 75%
          corrMinIntervalSlots: new anchor.BN(0),
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
      addLog(`GlobalConfig initialized: ${sig}`);
    } catch (err: any) {
      addLog(`Initialize error: ${err.message || err}`, true);
    }
  };

  // 3. Register Venues
  const handleRegisterVenues = async () => {
    try {
      addLog("Registering Venue A and Venue B...");
      const sigA = await program.methods
        .registerVenue(
          Array.from(venueIdA),
          Keypair.generate().publicKey,
          venueAuthA.publicKey,
          0,
          10000
        )
        .accounts({
          config: configPda,
          venueRegistration: venueRegAPda,
          admin: admin.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([admin])
        .rpc();

      const sigB = await program.methods
        .registerVenue(
          Array.from(venueIdB),
          Keypair.generate().publicKey,
          venueAuthB.publicKey,
          1,
          10000
        )
        .accounts({
          config: configPda,
          venueRegistration: venueRegBPda,
          admin: admin.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([admin])
        .rpc();

      setIsVenuesRegistered(true);
      addLog(`Venues registered: A(${sigA.slice(0, 10)}...) B(${sigB.slice(0, 10)}...)`);
    } catch (err: any) {
      addLog(`Register venues error: ${err.message || err}`, true);
    }
  };

  // 4. Set Consent
  const handleSetConsent = async () => {
    try {
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
    } catch (err: any) {
      addLog(`Set consent error: ${err.message || err}`, true);
    }
  };

  // 5. Set Correlation
  const handleSetCorrelation = async () => {
    try {
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
    } catch (err: any) {
      addLog(`Set correlation error: ${err.message || err}`, true);
    }
  };

  // Set Mock Price
  const handleSetPrice = async (assetId: number) => {
    try {
      const p = prices.find((item) => item.assetId === assetId);
      if (!p) return;
      const [mockPricePda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([assetId])],
        programId
      );
      const nowTs = Math.floor(Date.now() / 1000);
      addLog(`Setting mock price for ${p.name} ($${formatMicroUSD(p.priceMicro)})...`);
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
      addLog(`Price updated for ${p.name}: ${sig}`);
    } catch (err: any) {
      addLog(`Set price error: ${err.message || err}`, true);
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
      if (isA) setSnapshotPriceA(currentPrice);
      else setSnapshotPriceB(currentPrice);

      addLog(`Snapshot submitted for Venue ${venueKey}: ${sig}`);
    } catch (err: any) {
      addLog(`Submit snapshot error: ${err.message || err}`, true);
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
      const onChainA: any = await program.account.marginCredit.fetch(creditAPda);
      const onChainB: any = await program.account.marginCredit.fetch(creditBPda);

      setCreditA({
        creditAmount: BigInt(onChainA.creditAmount.toString()),
        validUntilSlot: BigInt(onChainA.validUntilSlot.toString()),
      });
      setCreditB({
        creditAmount: BigInt(onChainB.creditAmount.toString()),
        validUntilSlot: BigInt(onChainB.validUntilSlot.toString()),
      });

      addLog(`Credit computed successfully: ${sig}`);
    } catch (err: any) {
      addLog(`Compute credit error: ${err.message || err}`, true);
    }
  };

  // Revoke Credit
  const handleRevokeCredit = async (venueKey: "A" | "B") => {
    try {
      const isA = venueKey === "A";
      const venueId = isA ? venueIdA : venueIdB;
      const creditPda = isA ? creditAPda : creditBPda;

      addLog(`Keeper revoking credit for Venue ${venueKey}...`);
      const sig = await program.methods
        .revokeCredit(Array.from(venueId))
        .accounts({
          config: configPda,
          user: trader.publicKey,
          marginCredit: creditPda,
          authority: keeper.publicKey,
        })
        .signers([keeper])
        .rpc();

      if (isA) {
        setCreditA((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      } else {
        setCreditB((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      }
      addLog(`Credit revoked for Venue ${venueKey}: ${sig}`);
    } catch (err: any) {
      addLog(`Revoke credit error: ${err.message || err}`, true);
    }
  };

  // Revoke If Unsafe (Permissionless)
  const handleRevokeIfUnsafe = async (venueKey: "A" | "B") => {
    try {
      const isA = venueKey === "A";
      const venueId = isA ? venueIdA : venueIdB;
      const snapshotPda = isA ? snapshotAPda : snapshotBPda;
      const creditPda = isA ? creditAPda : creditBPda;
      const assetId = isA ? posA.assetId : posB.assetId;
      const [mockPricePda] = PublicKey.findProgramAddressSync(
        [Buffer.from("mock_price"), Buffer.from([assetId])],
        programId
      );

      addLog(`Calling permissionless revoke_if_unsafe for Venue ${venueKey}...`);
      const sig = await program.methods
        .revokeIfUnsafe(Array.from(venueId))
        .accounts({
          config: configPda,
          user: trader.publicKey,
          posSnapshot: snapshotPda,
          priceOracle: mockPricePda,
          marginCredit: creditPda,
        })
        .rpc();

      if (isA) {
        setCreditA((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      } else {
        setCreditB((prev) => (prev ? { ...prev, creditAmount: 0n, validUntilSlot: 0n } : null));
      }
      addLog(`Unsafe credit revoked successfully: ${sig}`);
    } catch (err: any) {
      addLog(`Revoke if unsafe error: ${err.message || err}`, true);
    }
  };

  // Apply Presets
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
    addLog("Applied preset: Hedged pair (rho 0.8)");
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
    addLog("Applied preset: Same direction legs");
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
    addLog("Applied preset: Perfect hedge (rho 1.0)");
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
      const cappedTotal = total > 100_000_000_000n ? 100_000_000_000n : total;
      const shares = splitProRata(cappedTotal, [posA.requiredMargin, posB.requiredMargin]);

      const capA = (posA.requiredMargin * 7500n) / 10000n;
      const capB = (posB.requiredMargin * 7500n) / 10000n;

      const shareA = shares[0] < capA ? shares[0] : capA;
      const shareB = shares[1] < capB ? shares[1] : capB;

      return {
        combined,
        sumReq,
        totalCredit: shareA + shareB,
        creditA: shareA,
        creditB: shareB,
        error: null,
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

  return (
    <div className="container">
      {/* 1. Header */}
      <div className="header-row">
        <h1>Clearinghouse test bench</h1>
        <div className="status-line">
          <div className="status-item">
            RPC: <span>{rpcUrl}</span> {!isRpcAllowed && <b style={{ color: "var(--error)" }}>(Refused)</b>}
          </div>
          <div className="status-item">
            SLOT: <span>{currentSlot !== null ? currentSlot : "connecting..."}</span>
          </div>
          <div className="status-item">
            PROGRAM: <span>{programId.toBase58().slice(0, 8)}...</span>
          </div>
        </div>
      </div>

      <div className="safety-note">
        Notice: Localnet only. Keys are ephemeral in-memory keypairs generated in the browser for local testing. Non-localhost RPC URLs are strictly refused.
      </div>

      {!isRpcAllowed && (
        <div className="panel" style={{ borderColor: "var(--error)", marginBottom: 20 }}>
          <b style={{ color: "var(--error)" }}>Access restricted:</b> Only local test validator URLs (localhost / 127.0.0.1) are permitted.
        </div>
      )}

      {/* 2. Setup Flow */}
      <div className="section panel">
        <h2>Setup Flow</h2>
        <div className="grid-setup">
          <div className="setup-step">
            <button onClick={handleAirdrop} disabled={!isRpcAllowed}>
              1. Airdrop
            </button>
            <span className={`badge ${isAirdropped ? "badge-done" : "badge-not-done"}`}>
              {isAirdropped ? "done" : "not done"}
            </span>
          </div>

          <div className="setup-step">
            <button onClick={handleInitialize} disabled={!isAirdropped}>
              2. Initialize
            </button>
            <span className={`badge ${isInitialized ? "badge-done" : "badge-not-done"}`}>
              {isInitialized ? "done" : "not done"}
            </span>
          </div>

          <div className="setup-step">
            <button onClick={handleRegisterVenues} disabled={!isInitialized}>
              3. Register venues
            </button>
            <span className={`badge ${isVenuesRegistered ? "badge-done" : "badge-not-done"}`}>
              {isVenuesRegistered ? "done" : "not done"}
            </span>
          </div>

          <div className="setup-step">
            <button onClick={handleSetConsent} disabled={!isVenuesRegistered}>
              4. Set consent
            </button>
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
            <button onClick={handleSetCorrelation} disabled={!isVenuesRegistered}>
              5. Set correlation
            </button>
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

                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <button className="small" onClick={() => handleSetPrice(p.assetId)}>
                    Set price
                  </button>
                  <span className="muted-sub">Age: {ageSecs}s</span>
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

            <button className="small primary" onClick={() => handleSubmitSnapshot("A")}>
              Submit snapshot
            </button>
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

            <button className="small primary" onClick={() => handleSubmitSnapshot("B")}>
              Submit snapshot
            </button>
          </div>
        </div>
      </div>

      {/* 5. Results & Actions */}
      <div className="section panel">
        <h2>Credit Allocation & Risk Ledger</h2>
        <div className="flex-row" style={{ marginBottom: 12 }}>
          <button className="primary" onClick={handleComputeCredit}>
            Compute credit
          </button>
          <button onClick={() => handleRevokeCredit("A")}>Revoke Venue A</button>
          <button onClick={() => handleRevokeCredit("B")}>Revoke Venue B</button>
          <button className="btn-error" onClick={() => handleRevokeIfUnsafe("A")}>
            Revoke if unsafe (A)
          </button>
          <button className="btn-error" onClick={() => handleRevokeIfUnsafe("B")}>
            Revoke if unsafe (B)
          </button>
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

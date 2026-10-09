import * as anchor from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import idl from "../src/idl/ch_core.json" with { type: "json" };
import { DEV_KEYS } from "../src/devKeys.ts";
import {
  combinedRisk,
  creditTotal,
  splitProRata,
  type MathLeg,
} from "../src/math.ts";

function assert(condition: boolean, msg: string) {
  if (!condition) {
    console.error(`❌ ASSERTION FAILED: ${msg}`);
    process.exit(1);
  }
}

async function main() {
  console.log("=== RUNNING HEADLESS SMOKE TEST ===");
  const connection = new anchor.web3.Connection("http://127.0.0.1:8899", "confirmed");

  // Verify connection
  try {
    const slot = await connection.getSlot();
    console.log(`Connected to localnet at slot ${slot}`);
  } catch (err) {
    console.error("Localnet validator is unreachable at http://127.0.0.1:8899");
    process.exit(1);
  }

  // 1. Deterministic dev keypairs
  const admin = DEV_KEYS.admin;
  const keeper = DEV_KEYS.keeper;
  const venueAuthA = DEV_KEYS.venueAuthA;
  const venueAuthB = DEV_KEYS.venueAuthB;
  const trader = DEV_KEYS.trader;

  const wallet = {
    publicKey: admin.publicKey,
    payer: admin,
    signTransaction: async (tx: any) => {
      tx.partialSign(admin);
      return tx;
    },
    signAllTransactions: async (txs: any[]) => {
      txs.forEach((t) => t.partialSign(admin));
      return txs;
    },
  };

  const provider = new anchor.AnchorProvider(connection, wallet as any, { commitment: "confirmed" });
  const program: any = new anchor.Program(idl as any, provider);
  const programId = program.programId;

  // 2. Airdrop
  console.log("Requesting airdrop for 5 test accounts...");
  await Promise.all([
    connection.requestAirdrop(admin.publicKey, 10 * LAMPORTS_PER_SOL),
    connection.requestAirdrop(keeper.publicKey, 10 * LAMPORTS_PER_SOL),
    connection.requestAirdrop(venueAuthA.publicKey, 10 * LAMPORTS_PER_SOL),
    connection.requestAirdrop(venueAuthB.publicKey, 10 * LAMPORTS_PER_SOL),
    connection.requestAirdrop(trader.publicKey, 10 * LAMPORTS_PER_SOL),
  ]);

  // Poll balances until > 0
  const start = Date.now();
  let funded = false;
  while (Date.now() - start < 10000) {
    const [b1, b2, b3, b4, b5] = await Promise.all([
      connection.getBalance(admin.publicKey),
      connection.getBalance(keeper.publicKey),
      connection.getBalance(venueAuthA.publicKey),
      connection.getBalance(venueAuthB.publicKey),
      connection.getBalance(trader.publicKey),
    ]);
    if (b1 > 0 && b2 > 0 && b3 > 0 && b4 > 0 && b5 > 0) {
      funded = true;
      break;
    }
    await new Promise((r) => setTimeout(r, 400));
  }
  assert(funded, "All accounts must be funded with > 0 SOL");
  console.log("✅ All accounts funded.");

  // PDAs
  const [configPda] = PublicKey.findProgramAddressSync([Buffer.from("config")], programId);
  const venueIdA = Buffer.alloc(32);
  venueIdA.write("venue-perp-a");
  const venueIdB = Buffer.alloc(32);
  venueIdB.write("venue-perp-b");

  const [venueRegAPda] = PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdA], programId);
  const [venueRegBPda] = PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdB], programId);
  const [userConsentPda] = PublicKey.findProgramAddressSync([Buffer.from("consent"), trader.publicKey.toBuffer()], programId);
  const [corrMatrixPda] = PublicKey.findProgramAddressSync([Buffer.from("correlations")], programId);
  const [snapshotAPda] = PublicKey.findProgramAddressSync([Buffer.from("snapshot"), trader.publicKey.toBuffer(), venueIdA], programId);
  const [snapshotBPda] = PublicKey.findProgramAddressSync([Buffer.from("snapshot"), trader.publicKey.toBuffer(), venueIdB], programId);
  const [mockPriceAPda] = PublicKey.findProgramAddressSync([Buffer.from("mock_price"), Buffer.from([0])], programId);
  const [mockPriceBPda] = PublicKey.findProgramAddressSync([Buffer.from("mock_price"), Buffer.from([1])], programId);
  const [creditAPda] = PublicKey.findProgramAddressSync([Buffer.from("credit"), trader.publicKey.toBuffer(), venueIdA], programId);
  const [creditBPda] = PublicKey.findProgramAddressSync([Buffer.from("credit"), trader.publicKey.toBuffer(), venueIdB], programId);

  // 3. Initialize GlobalConfig with DEFAULT_HAIRCUT_BPS = 2000
  console.log("Initializing or updating GlobalConfig (haircut 2000 bps)...");
  const existingConfig = await program.account.globalConfig.fetchNullable(configPda);
  if (!existingConfig) {
    const dummyFund = Keypair.generate().publicKey;
    await program.methods
      .initialize({
        keeperAuthority: keeper.publicKey,
        defaultFundProgram: dummyFund,
        maxVenues: 10,
        haircutBps: 2000,
        creditTtlSlots: new anchor.BN(3_000),
        snapshotMaxAgeSlots: new anchor.BN(10_000),
        maxCreditPerUser: new anchor.BN("100000000000"),
        maxCreditBpsOfRequired: 7500,
        corrMinIntervalSlots: new anchor.BN(0),
        corrMaxAgeSlots: new anchor.BN(10_000),
        maxPriceAgeSecs: new anchor.BN(60),
        maxConfBps: 100,
        maxMoveBps: 500,
      })
      .accounts({
        config: configPda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();
    console.log("✅ GlobalConfig initialized.");
  } else {
    assert(existingConfig.admin.equals(admin.publicKey), "GlobalConfig admin must match dev admin keypair");
    if (existingConfig.haircutBps !== 2000) {
      await program.methods
        .updateParams({
          maxVenues: existingConfig.maxVenues,
          haircutBps: 2000,
          creditTtlSlots: existingConfig.creditTtlSlots,
          snapshotMaxAgeSlots: existingConfig.snapshotMaxAgeSlots,
          maxCreditPerUser: existingConfig.maxCreditPerUser,
          maxCreditBpsOfRequired: existingConfig.maxCreditBpsOfRequired,
          corrMinIntervalSlots: existingConfig.corrMinIntervalSlots,
          corrMaxAgeSlots: existingConfig.corrMaxAgeSlots,
          maxPriceAgeSecs: existingConfig.maxPriceAgeSecs,
          maxConfBps: existingConfig.maxConfBps,
          maxMoveBps: existingConfig.maxMoveBps,
        })
        .accounts({ config: configPda, admin: admin.publicKey })
        .signers([admin])
        .rpc();
      console.log("✅ GlobalConfig haircut updated to 2000 bps.");
    } else {
      console.log("✅ GlobalConfig already initialized with 2000 bps haircut.");
    }
  }

  // 4. Register Venues
  console.log("Registering venues A and B...");
  const [vA, vB] = await Promise.all([
    program.account.venueRegistration.fetchNullable(venueRegAPda),
    program.account.venueRegistration.fetchNullable(venueRegBPda),
  ]);
  if (!vA) {
    await program.methods
      .registerVenue(Array.from(venueIdA), Keypair.generate().publicKey, venueAuthA.publicKey, 0, 10_000)
      .accounts({ config: configPda, venueRegistration: venueRegAPda, admin: admin.publicKey, systemProgram: SystemProgram.programId })
      .signers([admin])
      .rpc();
  }
  if (!vB) {
    await program.methods
      .registerVenue(Array.from(venueIdB), Keypair.generate().publicKey, venueAuthB.publicKey, 1, 10_000)
      .accounts({ config: configPda, venueRegistration: venueRegBPda, admin: admin.publicKey, systemProgram: SystemProgram.programId })
      .signers([admin])
      .rpc();
  }
  console.log("✅ Venues registered.");

  // Helper function to setup a position pair and compute credit
  async function setupAndComputeCredit(
    posA_isLong: boolean,
    posB_isLong: boolean,
    rhoVal: number,
    marginA: bigint = 10_000_000_000n,
    marginB: bigint = 10_000_000_000n
  ) {
    // Ensure active consent
    await program.methods
      .updateUserConsent(true, new anchor.BN(3))
      .accounts({ userConsent: userConsentPda, user: trader.publicKey, systemProgram: SystemProgram.programId })
      .signers([trader])
      .rpc();

    // Set correlation matrix
    const mat: anchor.BN[][] = [];
    for (let i = 0; i < 8; i++) {
      const row: anchor.BN[] = [];
      for (let j = 0; j < 8; j++) {
        if (i === j) row.push(new anchor.BN(1_000_000));
        else if ((i === 0 && j === 1) || (i === 1 && j === 0)) row.push(new anchor.BN(rhoVal));
        else row.push(new anchor.BN(0));
      }
      mat.push(row);
    }
    await program.methods
      .updateCorrelations(mat)
      .accounts({ config: configPda, correlationMatrix: corrMatrixPda, oracleAuthority: keeper.publicKey, systemProgram: SystemProgram.programId })
      .signers([keeper])
      .rpc();

    // Set fresh prices: SOL $150, ETH $3000
    const nowTs = Math.floor(Date.now() / 1000);
    await program.methods
      .setMockPrice(0, new anchor.BN("150000000"), new anchor.BN("50000"), new anchor.BN(nowTs))
      .accounts({ config: configPda, mockPrice: mockPriceAPda, admin: admin.publicKey, systemProgram: SystemProgram.programId })
      .signers([admin])
      .rpc();

    await program.methods
      .setMockPrice(1, new anchor.BN("3000000000"), new anchor.BN("1000000"), new anchor.BN(nowTs))
      .accounts({ config: configPda, mockPrice: mockPriceBPda, admin: admin.publicKey, systemProgram: SystemProgram.programId })
      .signers([admin])
      .rpc();

    // Submit snapshots
    await program.methods
      .submitPositionSnapshot(
        Array.from(venueIdA),
        0,
        new anchor.BN("50000000000"),
        posA_isLong,
        new anchor.BN(marginA.toString())
      )
      .accounts({
        config: configPda,
        venueRegistration: venueRegAPda,
        userConsent: userConsentPda,
        posSnapshot: snapshotAPda,
        priceOracle: mockPriceAPda,
        user: trader.publicKey,
        venueAuthority: venueAuthA.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([venueAuthA])
      .rpc();

    await program.methods
      .submitPositionSnapshot(
        Array.from(venueIdB),
        1,
        new anchor.BN("50000000000"),
        posB_isLong,
        new anchor.BN(marginB.toString())
      )
      .accounts({
        config: configPda,
        venueRegistration: venueRegBPda,
        userConsent: userConsentPda,
        posSnapshot: snapshotBPda,
        priceOracle: mockPriceBPda,
        user: trader.publicKey,
        venueAuthority: venueAuthB.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([venueAuthB])
      .rpc();

    // Compute credit
    await program.methods
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
  }

  // ==========================================
  // FLOW 1: Happy path (hedged pair: SOL Long, ETH Short, rho = 0.8)
  // ==========================================
  console.log("\n--- [FLOW 1/7] Happy Path: Hedged Pair (rho = 0.8) ---");
  await setupAndComputeCredit(true, false, 800_000);

  let creditA = await program.account.marginCredit.fetch(creditAPda);
  let creditB = await program.account.marginCredit.fetch(creditBPda);
  let valA = BigInt(creditA.creditAmount.toString());
  let valB = BigInt(creditB.creditAmount.toString());

  console.log(`On-chain credits: A = ${valA}, B = ${valB}`);
  assert(valA > 0n && valB > 0n, "Flow 1: Both credits must be positive");

  const legsFlow1: MathLeg[] = [
    { asset: 0, signedRequiredMargin: 10_000_000_000n },
    { asset: 1, signedRequiredMargin: -10_000_000_000n },
  ];
  const comb1 = combinedRisk(legsFlow1, () => 800_000n);
  const total1 = creditTotal(20_000_000_000n, comb1, 2000);
  const expected1 = splitProRata(total1, [10_000_000_000n, 10_000_000_000n]);
  assert(valA === expected1[0], `Flow 1: Credit A matches math.ts (${expected1[0]})`);
  assert(valB === expected1[1], `Flow 1: Credit B matches math.ts (${expected1[1]})`);
  console.log("✅ Flow 1 Passed: Hedged pair credits exactly match math.ts engine.");

  // ==========================================
  // FLOW 2: Same direction (SOL Long, ETH Long, rho = 0.8)
  // ==========================================
  console.log("\n--- [FLOW 2/7] Same Direction Legs (SOL Long, ETH Long, rho = 0.8) ---");
  await setupAndComputeCredit(true, true, 800_000);

  creditA = await program.account.marginCredit.fetch(creditAPda);
  creditB = await program.account.marginCredit.fetch(creditBPda);
  valA = BigInt(creditA.creditAmount.toString());
  valB = BigInt(creditB.creditAmount.toString());

  console.log(`On-chain credits: A = ${valA}, B = ${valB}`);
  const legsFlow2: MathLeg[] = [
    { asset: 0, signedRequiredMargin: 10_000_000_000n },
    { asset: 1, signedRequiredMargin: 10_000_000_000n },
  ];
  const comb2 = combinedRisk(legsFlow2, () => 800_000n);
  const total2 = creditTotal(20_000_000_000n, comb2, 2000);
  const expected2 = splitProRata(total2, [10_000_000_000n, 10_000_000_000n]);
  assert(valA === expected2[0], `Flow 2: Credit A matches math.ts (${expected2[0]})`);
  assert(valB === expected2[1], `Flow 2: Credit B matches math.ts (${expected2[1]})`);
  assert(valA < expected1[0], "Flow 2: Same-direction credit must be substantially lower than hedged pair");
  console.log("✅ Flow 2 Passed: Same direction netting properly calculated and lower than hedge.");

  // ==========================================
  // FLOW 3: Perfect hedge (SOL Long, ETH Short, rho = 1.0)
  // ==========================================
  console.log("\n--- [FLOW 3/7] Perfect Hedge (SOL Long, ETH Short, rho = 1.0) ---");
  await setupAndComputeCredit(true, false, 1_000_000);

  creditA = await program.account.marginCredit.fetch(creditAPda);
  creditB = await program.account.marginCredit.fetch(creditBPda);
  valA = BigInt(creditA.creditAmount.toString());
  valB = BigInt(creditB.creditAmount.toString());

  console.log(`On-chain credits: A = ${valA}, B = ${valB}`);
  const legsFlow3: MathLeg[] = [
    { asset: 0, signedRequiredMargin: 10_000_000_000n },
    { asset: 1, signedRequiredMargin: -10_000_000_000n },
  ];
  const comb3 = combinedRisk(legsFlow3, () => 1_000_000n);
  assert(comb3 === 0n, "Flow 3: Perfect hedge combined risk must be 0");
  const rawTotal3 = creditTotal(20_000_000_000n, comb3, 2000);
  const maxAllowed3 = (20_000_000_000n * 7500n) / 10_000n; // 75% cap = 15B
  const total3 = rawTotal3 < maxAllowed3 ? rawTotal3 : maxAllowed3;
  const expected3 = splitProRata(total3, [10_000_000_000n, 10_000_000_000n]);
  assert(valA === expected3[0] && valA === 7_500_000_000n, `Flow 3: Credit A is exactly 7.5B (got ${valA})`);
  assert(valB === expected3[1] && valB === 7_500_000_000n, `Flow 3: Credit B is exactly 7.5B (got ${valB})`);
  console.log("✅ Flow 3 Passed: Perfect hedge combined risk is 0, capped at max credit cap (15B total).");

  // ==========================================
  // FLOW 4: Price move then pair revoke (revoke_if_unsafe)
  // ==========================================
  console.log("\n--- [FLOW 4/7] Price Move (+10%) then Pair Revoke (revoke_if_unsafe) ---");
  // Ensure fresh pair first
  await setupAndComputeCredit(true, false, 800_000);

  // Bump ETH price by +10% ($3000 -> $3300, moves 1000 bps > maxMoveBps 500)
  const nowTs = Math.floor(Date.now() / 1000);
  await program.methods
    .setMockPrice(1, new anchor.BN("3300000000"), new anchor.BN("1000000"), new anchor.BN(nowTs))
    .accounts({ config: configPda, mockPrice: mockPriceBPda, admin: admin.publicKey, systemProgram: SystemProgram.programId })
    .signers([admin])
    .rpc();

  console.log("Calling paired revoke_if_unsafe...");
  await program.methods
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

  creditA = await program.account.marginCredit.fetch(creditAPda);
  creditB = await program.account.marginCredit.fetch(creditBPda);
  valA = BigInt(creditA.creditAmount.toString());
  valB = BigInt(creditB.creditAmount.toString());
  console.log(`Post-revoke credits: A = ${valA}, B = ${valB}`);
  assert(valA === 0n && valB === 0n, "Flow 4: Both credits must be zeroed by paired revoke_if_unsafe on price move");
  console.log("✅ Flow 4 Passed: Paired credits zeroed upon price move breach.");

  // ==========================================
  // FLOW 5: Stale price then pair revoke (revoke_if_unsafe)
  // ==========================================
  console.log("\n--- [FLOW 5/7] Stale Price (>60s) then Pair Revoke (revoke_if_unsafe) ---");
  await setupAndComputeCredit(true, false, 800_000);

  // Set ETH price timestamp 120s in the past (> 60s max price age)
  await program.methods
    .setMockPrice(1, new anchor.BN("3000000000"), new anchor.BN("1000000"), new anchor.BN(nowTs - 120))
    .accounts({ config: configPda, mockPrice: mockPriceBPda, admin: admin.publicKey, systemProgram: SystemProgram.programId })
    .signers([admin])
    .rpc();

  console.log("Calling paired revoke_if_unsafe on stale price...");
  await program.methods
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

  creditA = await program.account.marginCredit.fetch(creditAPda);
  creditB = await program.account.marginCredit.fetch(creditBPda);
  valA = BigInt(creditA.creditAmount.toString());
  valB = BigInt(creditB.creditAmount.toString());
  console.log(`Post-revoke credits: A = ${valA}, B = ${valB}`);
  assert(valA === 0n && valB === 0n, "Flow 5: Both credits must be zeroed on stale price");
  console.log("✅ Flow 5 Passed: Paired credits zeroed upon stale price breach.");

  // ==========================================
  // FLOW 6: Invalidate snapshot then revoke_if_basis_gone
  // ==========================================
  console.log("\n--- [FLOW 6/7] Invalidate Snapshot then revoke_if_basis_gone ---");
  await setupAndComputeCredit(true, false, 800_000);

  console.log("Venue authority invalidating snapshot for Venue A...");
  await program.methods
    .invalidateSnapshot()
    .accounts({
      venueRegistration: venueRegAPda,
      posSnapshot: snapshotAPda,
      user: trader.publicKey,
      venueAuthority: venueAuthA.publicKey,
    })
    .signers([venueAuthA])
    .rpc();

  console.log("Calling permissionless revoke_if_basis_gone...");
  await program.methods
    .revokeIfBasisGone()
    .accounts({
      config: configPda,
      user: trader.publicKey,
      userConsent: userConsentPda,
      venueRegA: venueRegAPda,
      venueRegB: venueRegBPda,
      snapshotA: snapshotAPda,
      snapshotB: snapshotBPda,
      creditA: creditAPda,
      creditB: creditBPda,
    })
    .rpc();

  creditA = await program.account.marginCredit.fetch(creditAPda);
  creditB = await program.account.marginCredit.fetch(creditBPda);
  valA = BigInt(creditA.creditAmount.toString());
  valB = BigInt(creditB.creditAmount.toString());
  console.log(`Post-revoke credits: A = ${valA}, B = ${valB}`);
  assert(valA === 0n && valB === 0n, "Flow 6: Both credits must be zeroed after snapshot invalidation");
  console.log("✅ Flow 6 Passed: Snapshot invalidated and paired credit zeroed by revoke_if_basis_gone.");

  // ==========================================
  // FLOW 7: Consent withdrawn then revoke_if_basis_gone
  // ==========================================
  console.log("\n--- [FLOW 7/7] Consent Withdrawn then revoke_if_basis_gone ---");
  await setupAndComputeCredit(true, false, 800_000);

  console.log("Trader withdrawing consent (isActive = false)...");
  await program.methods
    .updateUserConsent(false, new anchor.BN(0))
    .accounts({ userConsent: userConsentPda, user: trader.publicKey, systemProgram: SystemProgram.programId })
    .signers([trader])
    .rpc();

  console.log("Calling permissionless revoke_if_basis_gone...");
  await program.methods
    .revokeIfBasisGone()
    .accounts({
      config: configPda,
      user: trader.publicKey,
      userConsent: userConsentPda,
      venueRegA: venueRegAPda,
      venueRegB: venueRegBPda,
      snapshotA: snapshotAPda,
      snapshotB: snapshotBPda,
      creditA: creditAPda,
      creditB: creditBPda,
    })
    .rpc();

  creditA = await program.account.marginCredit.fetch(creditAPda);
  creditB = await program.account.marginCredit.fetch(creditBPda);
  valA = BigInt(creditA.creditAmount.toString());
  valB = BigInt(creditB.creditAmount.toString());
  console.log(`Post-revoke credits: A = ${valA}, B = ${valB}`);
  assert(valA === 0n && valB === 0n, "Flow 7: Both credits must be zeroed after consent withdrawn");
  console.log("✅ Flow 7 Passed: Consent withdrawn and paired credit zeroed by revoke_if_basis_gone.");

  console.log("\n🎉 ALL 7 HEADLESS SMOKE TEST FLOWS PASSED SUCCESSFULLY! 🎉\n");
}

main().catch((err) => {
  console.error("❌ Smoke test failed with exception:", err);
  process.exit(1);
});

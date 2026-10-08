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

  // 3. Initialize GlobalConfig
  console.log("Initializing GlobalConfig...");
  const existingConfig = await program.account.globalConfig.fetchNullable(configPda);
  if (!existingConfig) {
    const dummyFund = Keypair.generate().publicKey;
    await program.methods
      .initialize({
        keeperAuthority: keeper.publicKey,
        defaultFundProgram: dummyFund,
        maxVenues: 10,
        haircutBps: 0,
        creditTtlSlots: new anchor.BN(10_000),
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
    console.log("✅ GlobalConfig already initialized with our admin.");
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

  // 5. Trader Consent
  console.log("Registering trader consent...");
  const existingConsent = await program.account.userConsent.fetchNullable(userConsentPda);
  if (!existingConsent || !existingConsent.isActive || BigInt(existingConsent.authorizedVenuesBitmap.toString()) !== 3n) {
    await program.methods
      .updateUserConsent(true, new anchor.BN(3)) // 1 | 2
      .accounts({ userConsent: userConsentPda, user: trader.publicKey, systemProgram: SystemProgram.programId })
      .signers([trader])
      .rpc();
  }
  console.log("✅ Trader consent granted.");

  // 6. Set Correlation Matrix (rho = 800_000)
  console.log("Setting correlation matrix (rho = 800,000)...");
  const mat: anchor.BN[][] = [];
  for (let i = 0; i < 8; i++) {
    const row: anchor.BN[] = [];
    for (let j = 0; j < 8; j++) {
      if (i === j) row.push(new anchor.BN(1_000_000));
      else if ((i === 0 && j === 1) || (i === 1 && j === 0)) row.push(new anchor.BN(800_000));
      else row.push(new anchor.BN(0));
    }
    mat.push(row);
  }
  await program.methods
    .updateCorrelations(mat)
    .accounts({ config: configPda, correlationMatrix: corrMatrixPda, oracleAuthority: keeper.publicKey, systemProgram: SystemProgram.programId })
    .signers([keeper])
    .rpc();
  console.log("✅ Correlation matrix updated.");

  // 7. Set Mock Prices
  console.log("Setting mock prices (SOL = $150, ETH = $3000)...");
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
  console.log("✅ Mock prices set.");

  // 8. Submit Position Snapshots
  console.log("Submitting position snapshots...");
  await program.methods
    .submitPositionSnapshot(
      Array.from(venueIdA),
      0, // SOL
      new anchor.BN("50000000000"), // 50B notional
      true, // long
      new anchor.BN("10000000000") // 10B req margin
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
      1, // ETH
      new anchor.BN("50000000000"), // 50B notional
      false, // short
      new anchor.BN("10000000000") // 10B req margin
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
  console.log("✅ Both position snapshots submitted.");

  // 9. Compute Credit
  console.log("Calling compute_credit...");
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
  console.log("✅ compute_credit executed.");

  // 10. Assert Credit Amounts
  const onChainCreditA = await program.account.marginCredit.fetch(creditAPda);
  const onChainCreditB = await program.account.marginCredit.fetch(creditBPda);

  const creditValA = BigInt(onChainCreditA.creditAmount.toString());
  const creditValB = BigInt(onChainCreditB.creditAmount.toString());

  console.log(`On-chain Credit A: ${creditValA}`);
  console.log(`On-chain Credit B: ${creditValB}`);

  assert(creditValA === 6837722339n, `Credit A must be 6837722339n, got ${creditValA}`);
  assert(creditValB === 6837722340n, `Credit B must be 6837722340n, got ${creditValB}`);

  // Cross-check with src/math.ts
  const legs: MathLeg[] = [
    { asset: 0, signedRequiredMargin: 10_000_000_000n },
    { asset: 1, signedRequiredMargin: -10_000_000_000n },
  ];
  const combined = combinedRisk(legs, () => 800_000n);
  const total = creditTotal(20_000_000_000n, combined, 0);
  const expectedSplit = splitProRata(total, [10_000_000_000n, 10_000_000_000n]);

  assert(creditValA === expectedSplit[0], `Credit A matches math.ts (${expectedSplit[0]})`);
  assert(creditValB === expectedSplit[1], `Credit B matches math.ts (${expectedSplit[1]})`);
  console.log("✅ On-chain credits exactly match math.ts calculations: [6837722339, 6837722340]!");

  // 11. Test Revocation via Price Guard
  console.log("\nTesting Price Guard Revocation (raise ETH price by 10%)...");
  const higherEthPrice = new anchor.BN("3300000000"); // $3300 (+10% move, exceeds maxMoveBps 500 = 5%)
  await program.methods
    .setMockPrice(1, higherEthPrice, new anchor.BN("1000000"), new anchor.BN(Math.floor(Date.now() / 1000)))
    .accounts({ config: configPda, mockPrice: mockPriceBPda, admin: admin.publicKey, systemProgram: SystemProgram.programId })
    .signers([admin])
    .rpc();

  console.log("Calling revoke_if_unsafe for Venue B...");
  await program.methods
    .revokeIfUnsafe(Array.from(venueIdB))
    .accounts({
      config: configPda,
      user: trader.publicKey,
      posSnapshot: snapshotBPda,
      priceOracle: mockPriceBPda,
      marginCredit: creditBPda,
    })
    .rpc();

  const revokedCreditB = await program.account.marginCredit.fetch(creditBPda);
  const revokedValB = BigInt(revokedCreditB.creditAmount.toString());
  console.log(`Revoked Credit B: ${revokedValB}`);

  assert(revokedValB === 0n, `Revoked Credit B must be 0n, got ${revokedValB}`);
  console.log("✅ revoke_if_unsafe successfully revoked credit to 0 upon price move breach!");

  console.log("\n🎉 ALL SMOKE TESTS PASSED SUCCESSFULLY! 🎉\n");
}

main().catch((err) => {
  console.error("❌ Smoke test failed with exception:", err);
  process.exit(1);
});

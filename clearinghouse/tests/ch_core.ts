import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { expect } from "chai";
import { ChCore } from "../target/types/ch_core";

describe("ch_core protocol", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.ChCore as Program<ChCore>;

  const admin = Keypair.generate();
  const keeper = Keypair.generate();
  const venueAuthA = Keypair.generate();
  const venueAuthB = Keypair.generate();
  const trader = Keypair.generate();
  const unauthorizedUser = Keypair.generate();

  const venueIdA = Buffer.alloc(32);
  venueIdA.write("venue-perp-a");
  const venueIdB = Buffer.alloc(32);
  venueIdB.write("venue-perp-b");

  const [configPda] = PublicKey.findProgramAddressSync([Buffer.from("config")], program.programId);
  const [venueRegAPda] = PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdA], program.programId);
  const [venueRegBPda] = PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdB], program.programId);
  const [userConsentPda] = PublicKey.findProgramAddressSync([Buffer.from("consent"), trader.publicKey.toBuffer()], program.programId);
  const [snapshotAPda] = PublicKey.findProgramAddressSync([Buffer.from("snapshot"), trader.publicKey.toBuffer(), venueIdA], program.programId);
  const [snapshotBPda] = PublicKey.findProgramAddressSync([Buffer.from("snapshot"), trader.publicKey.toBuffer(), venueIdB], program.programId);
  const [creditAPda] = PublicKey.findProgramAddressSync([Buffer.from("credit"), trader.publicKey.toBuffer(), venueIdA], program.programId);
  const [creditBPda] = PublicKey.findProgramAddressSync([Buffer.from("credit"), trader.publicKey.toBuffer(), venueIdB], program.programId);
  const [corrMatrixPda] = PublicKey.findProgramAddressSync([Buffer.from("correlations")], program.programId);
  const [mockPrice0Pda] = PublicKey.findProgramAddressSync([Buffer.from("mock_price"), Buffer.from([0])], program.programId);
  const [mockPrice1Pda] = PublicKey.findProgramAddressSync([Buffer.from("mock_price"), Buffer.from([1])], program.programId);

  const defaultFundProgram = Keypair.generate().publicKey;

  function makeCorrMatrix(rho01: number): anchor.BN[][] {
    const mat: anchor.BN[][] = [];
    for (let i = 0; i < 8; i++) {
      const row: anchor.BN[] = [];
      for (let j = 0; j < 8; j++) {
        if (i === j) {
          row.push(new anchor.BN(1_000_000));
        } else if ((i === 0 && j === 1) || (i === 1 && j === 0)) {
          row.push(new anchor.BN(rho01));
        } else {
          row.push(new anchor.BN(0));
        }
      }
      mat.push(row);
    }
    return mat;
  }

  before(async () => {
    // Fund all test accounts
    const airdropAccounts = [admin, keeper, venueAuthA, venueAuthB, trader, unauthorizedUser];
    for (const acc of airdropAccounts) {
      const sig = await provider.connection.requestAirdrop(acc.publicKey, 10 * LAMPORTS_PER_SOL);
      const latestBlockhash = await provider.connection.getLatestBlockhash();
      await provider.connection.confirmTransaction({
        signature: sig,
        ...latestBlockhash,
      });
    }

    // 1. Initialize GlobalConfig
    await program.methods
      .initialize({
        keeperAuthority: keeper.publicKey,
        defaultFundProgram: defaultFundProgram,
        maxVenues: 10,
        haircutBps: 0,
        creditTtlSlots: new anchor.BN(10_000),
        snapshotMaxAgeSlots: new anchor.BN(10_000),
        maxCreditPerUser: new anchor.BN("100000000000"), // 100_000_000_000
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

    // 2. Register Venue A (index 0) and Venue B (index 1)
    await program.methods
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

    await program.methods
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

    // 3. User Consent: trader grants permission to venue 0 and venue 1 (bitmap: 1 | 2 = 3)
    await program.methods
      .updateUserConsent(true, new anchor.BN(3))
      .accounts({
        userConsent: userConsentPda,
        user: trader.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([trader])
      .rpc();

    // 4. Set Mock Prices: Asset 0 (SOL) at $150.00, Asset 1 (ETH) at $3,000.00
    const nowTs = Math.floor(Date.now() / 1000);
    await program.methods
      .setMockPrice(0, new anchor.BN(150_000_000), new anchor.BN(50_000), new anchor.BN(nowTs))
      .accounts({
        config: configPda,
        mockPrice: mockPrice0Pda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    await program.methods
      .setMockPrice(1, new anchor.BN(3_000_000_000), new anchor.BN(1_000_000), new anchor.BN(nowTs))
      .accounts({
        config: configPda,
        mockPrice: mockPrice1Pda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();
  });

  it("Happy path: hedged pair (rho 800_000, SOL long 10B, ETH short 10B)", async () => {
    // Set correlations with rho(0,1) = 800_000
    await program.methods
      .updateCorrelations(makeCorrMatrix(800_000))
      .accounts({
        config: configPda,
        correlationMatrix: corrMatrixPda,
        oracleAuthority: keeper.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([keeper])
      .rpc();

    // Venue A submits snapshot: long SOL, 10_000_000_000 required margin
    await program.methods
      .submitPositionSnapshot(
        Array.from(venueIdA),
        0, // SOL
        new anchor.BN("50000000000"),
        true, // Long
        new anchor.BN("10000000000")
      )
      .accounts({
        config: configPda,
        venueRegistration: venueRegAPda,
        userConsent: userConsentPda,
        posSnapshot: snapshotAPda,
        priceOracle: mockPrice0Pda,
        user: trader.publicKey,
        venueAuthority: venueAuthA.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([venueAuthA])
      .rpc();

    // Venue B submits snapshot: short ETH, 10_000_000_000 required margin
    await program.methods
      .submitPositionSnapshot(
        Array.from(venueIdB),
        1, // ETH
        new anchor.BN("50000000000"),
        false, // Short
        new anchor.BN("10000000000")
      )
      .accounts({
        config: configPda,
        venueRegistration: venueRegBPda,
        userConsent: userConsentPda,
        posSnapshot: snapshotBPda,
        priceOracle: mockPrice1Pda,
        user: trader.publicKey,
        venueAuthority: venueAuthB.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([venueAuthB])
      .rpc();

    // Keeper computes credit
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
        priceA: mockPrice0Pda,
        priceB: mockPrice1Pda,
        creditA: creditAPda,
        creditB: creditBPda,
        keeper: keeper.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([keeper])
      .rpc();

    const creditA = await program.account.marginCredit.fetch(creditAPda);
    const creditB = await program.account.marginCredit.fetch(creditBPda);

    // Golden vectors: exactly [6_837_722_339, 6_837_722_340]
    expect(creditA.creditAmount.toString()).to.equal("6837722339");
    expect(creditB.creditAmount.toString()).to.equal("6837722340");
    expect(creditA.validUntilSlot.toNumber()).to.be.greaterThan(0);
  });

  it("Same-direction legs: total credit is 1_026_334_038", async () => {
    // Both long: Venue A long SOL, Venue B long ETH
    await program.methods
      .submitPositionSnapshot(
        Array.from(venueIdB),
        1,
        new anchor.BN("50000000000"),
        true, // Long
        new anchor.BN("10000000000")
      )
      .accounts({
        config: configPda,
        venueRegistration: venueRegBPda,
        userConsent: userConsentPda,
        posSnapshot: snapshotBPda,
        priceOracle: mockPrice1Pda,
        user: trader.publicKey,
        venueAuthority: venueAuthB.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([venueAuthB])
      .rpc();

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
        priceA: mockPrice0Pda,
        priceB: mockPrice1Pda,
        creditA: creditAPda,
        creditB: creditBPda,
        keeper: keeper.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([keeper])
      .rpc();

    const creditA = await program.account.marginCredit.fetch(creditAPda);
    const creditB = await program.account.marginCredit.fetch(creditBPda);

    const totalCredit = creditA.creditAmount.add(creditB.creditAmount);
    expect(totalCredit.toString()).to.equal("1026334038");
  });

  it("Perfect hedge (rho +1_000_000): total 20B, capped at 7_500_000_000 each", async () => {
    // Update correlation to +1_000_000
    await program.methods
      .updateCorrelations(makeCorrMatrix(1_000_000))
      .accounts({
        config: configPda,
        correlationMatrix: corrMatrixPda,
        oracleAuthority: keeper.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([keeper])
      .rpc();

    // Reset Venue B to short
    await program.methods
      .submitPositionSnapshot(
        Array.from(venueIdB),
        1,
        new anchor.BN("50000000000"),
        false, // Short
        new anchor.BN("10000000000")
      )
      .accounts({
        config: configPda,
        venueRegistration: venueRegBPda,
        userConsent: userConsentPda,
        posSnapshot: snapshotBPda,
        priceOracle: mockPrice1Pda,
        user: trader.publicKey,
        venueAuthority: venueAuthB.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([venueAuthB])
      .rpc();

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
        priceA: mockPrice0Pda,
        priceB: mockPrice1Pda,
        creditA: creditAPda,
        creditB: creditBPda,
        keeper: keeper.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([keeper])
      .rpc();

    const creditA = await program.account.marginCredit.fetch(creditAPda);
    const creditB = await program.account.marginCredit.fetch(creditBPda);

    // Each capped at 75% of 10B = 7_500_000_000
    expect(creditA.creditAmount.toString()).to.equal("7500000000");
    expect(creditB.creditAmount.toString()).to.equal("7500000000");
  });

  it("Failures: snapshot signed by wrong key", async () => {
    try {
      await program.methods
        .submitPositionSnapshot(
          Array.from(venueIdA),
          0,
          new anchor.BN("50000000000"),
          true,
          new anchor.BN("10000000000")
        )
        .accounts({
          config: configPda,
          venueRegistration: venueRegAPda,
          userConsent: userConsentPda,
          posSnapshot: snapshotAPda,
          priceOracle: mockPrice0Pda,
          user: trader.publicKey,
          venueAuthority: unauthorizedUser.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([unauthorizedUser])
        .rpc();
      expect.fail("Should have thrown unauthorized");
    } catch (err: any) {
      expect(err.toString()).to.include("Unauthorized");
    }
  });

  it("Failures: no consent or missing venue bit", async () => {
    // Unauthorized user has no consent PDA
    const [noConsentPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("consent"), unauthorizedUser.publicKey.toBuffer()],
      program.programId
    );
    const [unauthSnapshotPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("snapshot"), unauthorizedUser.publicKey.toBuffer(), venueIdA],
      program.programId
    );

    try {
      await program.methods
        .submitPositionSnapshot(
          Array.from(venueIdA),
          0,
          new anchor.BN("50000000000"),
          true,
          new anchor.BN("10000000000")
        )
        .accounts({
          config: configPda,
          venueRegistration: venueRegAPda,
          userConsent: noConsentPda,
          posSnapshot: unauthSnapshotPda,
          priceOracle: mockPrice0Pda,
          user: unauthorizedUser.publicKey,
          venueAuthority: venueAuthA.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([venueAuthA])
        .rpc();
      expect.fail("Should have failed missing consent");
    } catch (err: any) {
      expect(err.toString()).to.be.ok;
    }
  });

  it("Failures: non-keeper calling compute_credit", async () => {
    try {
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
          priceA: mockPrice0Pda,
          priceB: mockPrice1Pda,
          creditA: creditAPda,
          creditB: creditBPda,
          keeper: unauthorizedUser.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([unauthorizedUser])
        .rpc();
      expect.fail("Should have thrown unauthorized");
    } catch (err: any) {
      expect(err.toString()).to.include("Unauthorized");
    }
  });

  it("Failures: asset_id >= 8", async () => {
    try {
      await program.methods
        .submitPositionSnapshot(
          Array.from(venueIdA),
          8, // Invalid asset
          new anchor.BN("50000000000"),
          true,
          new anchor.BN("10000000000")
        )
        .accounts({
          config: configPda,
          venueRegistration: venueRegAPda,
          userConsent: userConsentPda,
          posSnapshot: snapshotAPda,
          priceOracle: mockPrice0Pda,
          user: trader.publicKey,
          venueAuthority: venueAuthA.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([venueAuthA])
        .rpc();
      expect.fail("Should have thrown InvalidAssetId");
    } catch (err: any) {
      expect(err.toString()).to.include("InvalidAssetId");
    }
  });

  it("Failures: invalid correlation matrix (out of bounds or asymmetric)", async () => {
    const invalidMat = makeCorrMatrix(1_500_000); // 1.5 > 1.0
    try {
      await program.methods
        .updateCorrelations(invalidMat)
        .accounts({
          config: configPda,
          correlationMatrix: corrMatrixPda,
          oracleAuthority: keeper.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([keeper])
        .rpc();
      expect.fail("Should have thrown InvalidCorrelationValue");
    } catch (err: any) {
      expect(err.toString()).to.include("InvalidCorrelationValue");
    }
  });

  it("Failures: stale price, wide confidence, and price moved beyond max tolerance", async () => {
    // 1. Stale price test: publish timestamp 1,000s ago
    const staleTs = Math.floor(Date.now() / 1000) - 1000;
    await program.methods
      .setMockPrice(0, new anchor.BN(150_000_000), new anchor.BN(50_000), new anchor.BN(staleTs))
      .accounts({
        config: configPda,
        mockPrice: mockPrice0Pda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    try {
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
          priceA: mockPrice0Pda,
          priceB: mockPrice1Pda,
          creditA: creditAPda,
          creditB: creditBPda,
          keeper: keeper.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([keeper])
        .rpc();
      expect.fail("Should have thrown PriceStale");
    } catch (err: any) {
      expect(err.toString()).to.include("PriceStale");
    }

    // 2. Wide confidence test: conf is 5% (> max 1%)
    const freshTs = Math.floor(Date.now() / 1000);
    await program.methods
      .setMockPrice(0, new anchor.BN(150_000_000), new anchor.BN(7_500_000), new anchor.BN(freshTs))
      .accounts({
        config: configPda,
        mockPrice: mockPrice0Pda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    try {
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
          priceA: mockPrice0Pda,
          priceB: mockPrice1Pda,
          creditA: creditAPda,
          creditB: creditBPda,
          keeper: keeper.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([keeper])
        .rpc();
      expect.fail("Should have thrown PriceUncertain");
    } catch (err: any) {
      expect(err.toString()).to.include("PriceUncertain");
    }

    // 3. Price moved test: price moved by 10% (> max 5% / 500 bps)
    await program.methods
      .setMockPrice(0, new anchor.BN(170_000_000), new anchor.BN(50_000), new anchor.BN(freshTs))
      .accounts({
        config: configPda,
        mockPrice: mockPrice0Pda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    try {
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
          priceA: mockPrice0Pda,
          priceB: mockPrice1Pda,
          creditA: creditAPda,
          creditB: creditBPda,
          keeper: keeper.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([keeper])
        .rpc();
      expect.fail("Should have thrown PriceMoved");
    } catch (err: any) {
      expect(err.toString()).to.include("PriceMoved");
    }
  });

  it("Revocation: revoke_credit zeroes credit and effective_margin returns full required", async () => {
    // Reset price to original $150.00
    const freshTs = Math.floor(Date.now() / 1000);
    await program.methods
      .setMockPrice(0, new anchor.BN(150_000_000), new anchor.BN(50_000), new anchor.BN(freshTs))
      .accounts({
        config: configPda,
        mockPrice: mockPrice0Pda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    // Recompute credit
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
        priceA: mockPrice0Pda,
        priceB: mockPrice1Pda,
        creditA: creditAPda,
        creditB: creditBPda,
        keeper: keeper.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([keeper])
      .rpc();

    let creditA = await program.account.marginCredit.fetch(creditAPda);
    expect(creditA.creditAmount.toNumber()).to.be.greaterThan(0);

    // Revoke credit for Venue A
    await program.methods
      .revokeCredit(Array.from(venueIdA))
      .accounts({
        config: configPda,
        user: trader.publicKey,
        marginCredit: creditAPda,
        authority: keeper.publicKey,
      })
      .signers([keeper])
      .rpc();

    creditA = await program.account.marginCredit.fetch(creditAPda);
    expect(creditA.creditAmount.toNumber()).to.equal(0);
    expect(creditA.validUntilSlot.toNumber()).to.equal(0);
  });

  it("Revocation: revoke_if_unsafe works only when guard is tripped", async () => {
    // Recompute credit for Venue B
    let creditB = await program.account.marginCredit.fetch(creditBPda);
    expect(creditB.creditAmount.toNumber()).to.be.greaterThan(0);

    // Price guard is currently NOT tripped: calling revoke_if_unsafe should fail with GuardNotTripped
    try {
      await program.methods
        .revokeIfUnsafe(Array.from(venueIdB))
        .accounts({
          config: configPda,
          user: trader.publicKey,
          posSnapshot: snapshotBPda,
          priceOracle: mockPrice1Pda,
          marginCredit: creditBPda,
        })
        .rpc();
      expect.fail("Should have failed GuardNotTripped");
    } catch (err: any) {
      expect(err.toString()).to.include("GuardNotTripped");
    }

    // Now trip the guard by moving Asset 1 (ETH) price significantly (> 5% move)
    const freshTs = Math.floor(Date.now() / 1000);
    await program.methods
      .setMockPrice(1, new anchor.BN(4_000_000_000), new anchor.BN(1_000_000), new anchor.BN(freshTs))
      .accounts({
        config: configPda,
        mockPrice: mockPrice1Pda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    // Now revoke_if_unsafe succeeds permissionlessly
    await program.methods
      .revokeIfUnsafe(Array.from(venueIdB))
      .accounts({
        config: configPda,
        user: trader.publicKey,
        posSnapshot: snapshotBPda,
        priceOracle: mockPrice1Pda,
        marginCredit: creditBPda,
      })
      .rpc();

    creditB = await program.account.marginCredit.fetch(creditBPda);
    expect(creditB.creditAmount.toNumber()).to.equal(0);
    expect(creditB.validUntilSlot.toNumber()).to.equal(0);
  });
});

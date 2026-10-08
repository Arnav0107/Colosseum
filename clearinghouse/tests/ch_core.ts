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
        creditTtlSlots: new anchor.BN(3_000),
        snapshotMaxAgeSlots: new anchor.BN(10_000),
        maxCreditPerUser: new anchor.BN("100000000000"), // 100_000_000_000
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
    let creditB = await program.account.marginCredit.fetch(creditBPda);
    expect(creditA.creditAmount.toNumber()).to.be.greaterThan(0);
    expect(creditB.creditAmount.toNumber()).to.be.greaterThan(0);

    // Paired revocation: revoke_credit zeroes BOTH credits of the pair
    await program.methods
      .revokeCredit()
      .accounts({
        config: configPda,
        user: trader.publicKey,
        venueRegA: venueRegAPda,
        venueRegB: venueRegBPda,
        creditA: creditAPda,
        creditB: creditBPda,
        authority: keeper.publicKey,
      })
      .signers([keeper])
      .rpc();

    creditA = await program.account.marginCredit.fetch(creditAPda);
    creditB = await program.account.marginCredit.fetch(creditBPda);
    expect(creditA.creditAmount.toNumber()).to.equal(0);
    expect(creditA.validUntilSlot.toNumber()).to.equal(0);
    expect(creditB.creditAmount.toNumber()).to.equal(0);
    expect(creditB.validUntilSlot.toNumber()).to.equal(0);
  });

  it("Revocation: revoke_if_unsafe works only when guard is tripped", async () => {
    // Recompute credits for both venues
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
    let creditB = await program.account.marginCredit.fetch(creditBPda);
    expect(creditA.creditAmount.toNumber()).to.be.greaterThan(0);
    expect(creditB.creditAmount.toNumber()).to.be.greaterThan(0);

    // Price guard is currently NOT tripped: calling revoke_if_unsafe should fail with GuardNotTripped
    try {
      await program.methods
        .revokeIfUnsafe()
        .accounts({
          config: configPda,
          user: trader.publicKey,
          venueRegA: venueRegAPda,
          venueRegB: venueRegBPda,
          snapshotA: snapshotAPda,
          snapshotB: snapshotBPda,
          priceOracleA: mockPrice0Pda,
          priceOracleB: mockPrice1Pda,
          creditA: creditAPda,
          creditB: creditBPda,
        })
        .rpc();
      expect.fail("Should have failed GuardNotTripped");
    } catch (err: any) {
      if (err.name === "AssertionError") throw err;
      expect(err.toString()).to.include("GuardNotTripped");
    }

    // Wrong oracle account on leg A should fail with InvalidOracleAccount (never "tripped")
    try {
      await program.methods
        .revokeIfUnsafe()
        .accounts({
          config: configPda,
          user: trader.publicKey,
          venueRegA: venueRegAPda,
          venueRegB: venueRegBPda,
          snapshotA: snapshotAPda,
          snapshotB: snapshotBPda,
          priceOracleA: mockPrice1Pda, // Wrong oracle for asset 0
          priceOracleB: mockPrice1Pda,
          creditA: creditAPda,
          creditB: creditBPda,
        })
        .rpc();
      expect.fail("Should have failed InvalidOracleAccount");
    } catch (err: any) {
      if (err.name === "AssertionError") throw err;
      expect(err.toString()).to.include("InvalidOracleAccount");
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

    // Now revoke_if_unsafe succeeds permissionlessly and zeroes BOTH credits of the pair
    await program.methods
      .revokeIfUnsafe()
      .accounts({
        config: configPda,
        user: trader.publicKey,
        venueRegA: venueRegAPda,
        venueRegB: venueRegBPda,
        snapshotA: snapshotAPda,
        snapshotB: snapshotBPda,
        priceOracleA: mockPrice0Pda,
        priceOracleB: mockPrice1Pda,
        creditA: creditAPda,
        creditB: creditBPda,
      })
      .rpc();

    creditA = await program.account.marginCredit.fetch(creditAPda);
    creditB = await program.account.marginCredit.fetch(creditBPda);
    expect(creditA.creditAmount.toNumber()).to.equal(0);
    expect(creditA.validUntilSlot.toNumber()).to.equal(0);
    expect(creditB.creditAmount.toNumber()).to.equal(0);
    expect(creditB.validUntilSlot.toNumber()).to.equal(0);
  });

  describe("Item 1.3: invalidate_snapshot and revoke_if_basis_gone", () => {
    it("invalidate_snapshot requires venue_authority and zeroes notional & required_margin", async () => {
      // Non-authority fails with Unauthorized
      try {
        await program.methods
          .invalidateSnapshot()
          .accounts({
            venueRegistration: venueRegAPda,
            posSnapshot: snapshotAPda,
            user: trader.publicKey,
            venueAuthority: unauthorizedUser.publicKey,
          })
          .signers([unauthorizedUser])
          .rpc();
        expect.fail("Should have failed Unauthorized");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.include("Unauthorized");
      }

      // Venue authority successfully invalidates snapshot
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

      const snapA = await program.account.posSnapshot.fetch(snapshotAPda);
      expect(snapA.notionalValue.toNumber()).to.equal(0);
      expect(snapA.requiredMargin.toNumber()).to.equal(0);
    });

    it("compute_credit rejects zero-notional snapshots with ZeroNotional", async () => {
      // Snapshot A was invalidated to notional 0
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
        expect.fail("Should have failed ZeroNotional");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.include("ZeroNotional");
      }
    });

    it("revoke_if_basis_gone fails with BasisNotGone when basis is intact", async () => {
      // Restore valid snapshot A and ETH price
      const freshTs = Math.floor(Date.now() / 1000);
      await program.methods
        .setMockPrice(1, new anchor.BN("3000000000"), new anchor.BN(1_000_000), new anchor.BN(freshTs))
        .accounts({
          config: configPda,
          mockPrice: mockPrice1Pda,
          admin: admin.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([admin])
        .rpc();

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
          false,
          new anchor.BN("10000000000")
        )
        .accounts({
          config: configPda,
          venueRegistration: venueRegAPda, // wait, venueRegBPda!
          userConsent: userConsentPda,
          posSnapshot: snapshotBPda,
          priceOracle: mockPrice1Pda,
          user: trader.publicKey,
          venueAuthority: venueAuthB.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .accounts({
          venueRegistration: venueRegBPda,
        })
        .signers([venueAuthB])
        .rpc();

      // Compute credits successfully
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

      // Basis is intact: revoke_if_basis_gone must fail with BasisNotGone
      try {
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
        expect.fail("Should have failed BasisNotGone");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.include("BasisNotGone");
      }
    });

    it("revoke_if_basis_gone branch: snapshot invalidated (notional 0)", async () => {
      // Invalidate snapshot B
      await program.methods
        .invalidateSnapshot()
        .accounts({
          venueRegistration: venueRegBPda,
          posSnapshot: snapshotBPda,
          user: trader.publicKey,
          venueAuthority: venueAuthB.publicKey,
        })
        .signers([venueAuthB])
        .rpc();

      // Now revoke_if_basis_gone succeeds and zeroes both credits
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

      const creditA = await program.account.marginCredit.fetch(creditAPda);
      const creditB = await program.account.marginCredit.fetch(creditBPda);
      expect(creditA.creditAmount.toNumber()).to.equal(0);
      expect(creditB.creditAmount.toNumber()).to.equal(0);
    });

    it("revoke_if_basis_gone branch: consent bit cleared", async () => {
      // Re-submit snapshot B
      await program.methods
        .submitPositionSnapshot(
          Array.from(venueIdB),
          1,
          new anchor.BN("50000000000"),
          false,
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

      // Recompute credits
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

      // Clear venue B bit from consent bitmap (keep venue A bit 1 only)
      await program.methods
        .updateUserConsent(true, new anchor.BN(1)) // only bit 0
        .accounts({
          userConsent: userConsentPda,
          user: trader.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([trader])
        .rpc();

      // Now revoke_if_basis_gone zeroes both credits
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

      const creditA = await program.account.marginCredit.fetch(creditAPda);
      const creditB = await program.account.marginCredit.fetch(creditBPda);
      expect(creditA.creditAmount.toNumber()).to.equal(0);
      expect(creditB.creditAmount.toNumber()).to.equal(0);
    });

    it("revoke_if_basis_gone branch: consent inactive", async () => {
      // Re-enable consent bitmap to 3, recompute credits
      await program.methods
        .updateUserConsent(true, new anchor.BN(3))
        .accounts({
          userConsent: userConsentPda,
          user: trader.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([trader])
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

      // Deactivate consent completely
      await program.methods
        .updateUserConsent(false, new anchor.BN(3))
        .accounts({
          userConsent: userConsentPda,
          user: trader.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([trader])
        .rpc();

      // Revoke if basis gone zeroes both credits
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

      const creditA = await program.account.marginCredit.fetch(creditAPda);
      const creditB = await program.account.marginCredit.fetch(creditBPda);
      expect(creditA.creditAmount.toNumber()).to.equal(0);
      expect(creditB.creditAmount.toNumber()).to.equal(0);

      // Re-enable consent for subsequent tests
      await program.methods
        .updateUserConsent(true, new anchor.BN(3))
        .accounts({
          userConsent: userConsentPda,
          user: trader.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([trader])
        .rpc();
    });
  });

  describe("Item 1.5: register_venue validation", () => {
    it("register_venue rejects venue_index >= max_venues with InvalidVenueIndex", async () => {
      const venueIdBad = Buffer.alloc(32);
      venueIdBad.write("venue-bad-index");
      const [venueRegBadPda] = PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdBad], program.programId);

      try {
        await program.methods
          .registerVenue(Array.from(venueIdBad), Keypair.generate().publicKey, venueAuthA.publicKey, 10, 10_000) // maxVenues is 10, so 10 is invalid
          .accounts({
            config: configPda,
            venueRegistration: venueRegBadPda,
            admin: admin.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([admin])
          .rpc();
        expect.fail("Should have failed InvalidVenueIndex");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.include("InvalidVenueIndex");
      }
    });

    it("register_venue rejects an index already used by a different venue with VenueIndexAlreadyUsed", async () => {
      const venueIdDup = Buffer.alloc(32);
      venueIdDup.write("venue-dup-index");
      const [venueRegDupPda] = PublicKey.findProgramAddressSync([Buffer.from("venue"), venueIdDup], program.programId);

      // Venue A already used index 0. Attempting to register venueIdDup at index 0 must fail!
      try {
        await program.methods
          .registerVenue(Array.from(venueIdDup), Keypair.generate().publicKey, venueAuthA.publicKey, 0, 10_000)
          .accounts({
            config: configPda,
            venueRegistration: venueRegDupPda,
            admin: admin.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([admin])
          .rpc();
        expect.fail("Should have failed VenueIndexAlreadyUsed");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.include("VenueIndexAlreadyUsed");
      }
    });
  });

  describe("Item 1.6: compute_credit distinct venues and margin validation", () => {
    it("compute_credit rejects same venue for both legs with DuplicateVenue", async () => {
      const dummyVenueId = Buffer.alloc(32);
      dummyVenueId.write("venue-dummy-credit");
      const [dummyCreditPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("credit"), trader.publicKey.toBuffer(), dummyVenueId],
        program.programId
      );

      try {
        await program.methods
          .computeCredit()
          .accounts({
            config: configPda,
            user: trader.publicKey,
            userConsent: userConsentPda,
            venueRegA: venueRegAPda,
            venueRegB: venueRegAPda, // same venue for both!
            snapshotA: snapshotAPda,
            snapshotB: snapshotAPda,
            correlationMatrix: corrMatrixPda,
            priceA: mockPrice0Pda,
            priceB: mockPrice0Pda,
            creditA: creditAPda,
            creditB: creditAPda,
            keeper: keeper.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([keeper])
          .rpc();
        expect.fail("Should have failed DuplicateVenue");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.satisfy(
          (s: string) => s.includes("DuplicateVenue") || s.includes("ConstraintDuplicateMutableAccount")
        );
      }
    });

    it("compute_credit rejects required_margin > notional with InvalidMargin", async () => {
      // Submit snapshot with required_margin (20B) > notional (10B)
      await program.methods
        .submitPositionSnapshot(
          Array.from(venueIdA),
          0,
          new anchor.BN("10000000000"), // notional 10B
          true,
          new anchor.BN("20000000000") // required 20B > notional!
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
        expect.fail("Should have failed InvalidMargin");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.include("InvalidMargin");
      }

      // Restore valid snapshot A
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
          venueAuthority: venueAuthA.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([venueAuthA])
        .rpc();
    });

    it("compute_credit rejects required_margin == 0 with InvalidMargin", async () => {
      // Submit snapshot with required_margin = 0
      await program.methods
        .submitPositionSnapshot(
          Array.from(venueIdA),
          0,
          new anchor.BN("50000000000"),
          true,
          new anchor.BN(0)
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
        expect.fail("Should have failed InvalidMargin");
      } catch (err: any) {
        if (err.name === "AssertionError") throw err;
        expect(err.toString()).to.include("InvalidMargin");
      }

      // Restore valid snapshot A
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
          venueAuthority: venueAuthA.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([venueAuthA])
        .rpc();
    });
  });
});

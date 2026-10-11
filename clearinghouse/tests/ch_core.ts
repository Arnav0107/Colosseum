// Integration tests for ch_core against a real solana-test-validator with the
// program loaded at its declare_id! address (see Anchor.toml [[test.genesis]]).
//
// Time-dependent behaviour (expiry, stale snapshots, epoch arithmetic under
// clock warps) is covered by the LiteSVM suite in crates/ch_svm_tests, which
// can move the clock. This suite checks the same program through the real
// RPC/validator path and the TypeScript client.
import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL, AccountMeta } from "@solana/web3.js";
import { expect } from "chai";
import { ChCore } from "../target/types/ch_core";
import { expectTxError } from "./helpers/expectTxError";

const IM = 10_000_000_000; // $10,000 in micro-USD
const SOL = 0;
const ETH = 1;
const STATUS_ACTIVE = 1;
const STATUS_INVALIDATED = 2;

describe("ch_core (portfolio model)", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.ChCore as Program<ChCore>;
  const pid = program.programId;

  const admin = Keypair.generate();
  const mallory = Keypair.generate();
  const venueAuths = [Keypair.generate(), Keypair.generate(), Keypair.generate(), Keypair.generate()];
  const venueIds = venueAuths.map((_, i) => {
    const b = Buffer.alloc(32);
    b.write(`venue-${i}`);
    return b;
  });

  const pda = (...seeds: (Buffer | Uint8Array)[]) => PublicKey.findProgramAddressSync(seeds, pid)[0];
  const configPda = pda(Buffer.from("config"));
  const riskModelPda = pda(Buffer.from("risk_model"));
  const venuePda = (i: number) => pda(Buffer.from("venue"), venueIds[i]);
  const consentPda = (u: PublicKey) => pda(Buffer.from("consent"), u.toBuffer());
  const portfolioPda = (u: PublicKey) => pda(Buffer.from("portfolio"), u.toBuffer());
  const snapshotPda = (u: PublicKey, venue: number, asset: number) =>
    pda(Buffer.from("snapshot"), u.toBuffer(), venueIds[venue], Buffer.from([asset]));
  const pricePda = (asset: number) => pda(Buffer.from("mock_price"), Buffer.from([asset]));

  const params = {
    maxVenues: 16,
    haircutBps: 2_000,
    creditTtlSlots: new BN(3_000),
    snapshotMaxAgeSlots: new BN(100_000),
    maxCreditPerUser: new BN("100000000000"),
    maxCreditBpsOfIm: 7_500,
    maxCreditPerVenue: new BN("1000000000000"),
    maxCreditPerAsset: new BN("1000000000000"),
    globalCreditCap: new BN("10000000000000"),
    riskModelMaxAgeSlots: new BN(1_000_000),
    // Generous: the validator clock and wall clock can drift in CI.
    maxPriceAgeSecs: new BN(600),
    maxConfBps: 100,
    maxMoveBps: 500,
  };

  async function airdrop(pk: PublicKey, sol = 10) {
    const sig = await provider.connection.requestAirdrop(pk, sol * LAMPORTS_PER_SOL);
    const bh = await provider.connection.getLatestBlockhash();
    await provider.connection.confirmTransaction({ signature: sig, ...bh });
  }

  async function setPrice(asset: number, price: number) {
    const now = Math.floor(Date.now() / 1000);
    await program.methods
      .setMockPrice(asset, new BN(price), new BN(Math.floor(price / 10_000)), new BN(now))
      .accountsStrict({
        config: configPda,
        mockPrice: pricePda(asset),
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();
  }

  async function newUser(bitmap = new BN("18446744073709551615")): Promise<Keypair> {
    const u = Keypair.generate();
    await airdrop(u.publicKey);
    await program.methods
      .updateUserConsent(true, bitmap)
      .accountsStrict({
        userConsent: consentPda(u.publicKey),
        portfolio: portfolioPda(u.publicKey),
        user: u.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([u])
      .rpc();
    return u;
  }

  function submitTx(
    u: PublicKey,
    venue: number,
    asset: number,
    isLong: boolean,
    im: number,
    notional = 5 * im,
    mm = Math.floor(im / 2)
  ) {
    return program.methods
      .submitPositionSnapshot(Array.from(venueIds[venue]), asset, new BN(notional), isLong, new BN(im), new BN(mm))
      .accountsStrict({
        config: configPda,
        venueRegistration: venuePda(venue),
        userConsent: consentPda(u),
        portfolio: portfolioPda(u),
        posSnapshot: snapshotPda(u, venue, asset),
        user: u,
        venueAuthority: venueAuths[venue].publicKey,
        payer: venueAuths[venue].publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([venueAuths[venue]]);
  }

  const meta = (pubkey: PublicKey): AccountMeta => ({ pubkey, isSigner: false, isWritable: false });

  async function refreshMetas(u: PublicKey): Promise<AccountMeta[]> {
    const p = await program.account.portfolio.fetch(portfolioPda(u));
    const keys = p.registered.slice(0, p.numRegistered);
    const snaps = keys.map((k) => snapshotPda(u, k.venueIndex, k.assetId));
    const oracles = keys.map((k) => pricePda(k.assetId));
    return [...snaps, ...oracles].map(meta);
  }

  function refreshTx(u: PublicKey, metas: AccountMeta[]) {
    return program.methods
      .refreshPortfolio()
      .accountsStrict({
        config: configPda,
        riskModel: riskModelPda,
        userConsent: consentPda(u),
        portfolio: portfolioPda(u),
      })
      .remainingAccounts(metas);
  }

  async function refresh(u: PublicKey) {
    return refreshTx(u, await refreshMetas(u)).rpc();
  }

  function invalidateTx(u: PublicKey, metas: AccountMeta[] = []) {
    return program.methods
      .invalidatePortfolio()
      .accountsStrict({ config: configPda, userConsent: consentPda(u), portfolio: portfolioPda(u) })
      .remainingAccounts(metas);
  }

  async function credits(u: PublicKey): Promise<string[]> {
    const p = await program.account.portfolio.fetch(portfolioPda(u));
    return p.legs.slice(0, p.numLegs).map((l) => l.credit.toString());
  }

  async function hedgedUser(): Promise<Keypair> {
    const u = await newUser();
    await submitTx(u.publicKey, 0, SOL, true, IM).rpc();
    await submitTx(u.publicKey, 1, ETH, false, IM).rpc();
    await refresh(u.publicKey);
    return u;
  }

  before(async () => {
    for (const k of [admin, mallory, ...venueAuths]) await airdrop(k.publicKey, 20);

    await program.methods
      .initialize(params)
      .accountsStrict({
        config: configPda,
        riskModel: riskModelPda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    for (let i = 0; i < venueAuths.length; i++) {
      await program.methods
        .registerVenue(Array.from(venueIds[i]), Keypair.generate().publicKey, venueAuths[i].publicKey, i)
        .accountsStrict({
          config: configPda,
          venueRegistration: venuePda(i),
          admin: admin.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([admin])
        .rpc();
    }

    await program.methods
      .updateCorrelations([{ a: SOL, b: ETH, rho: 800_000 }])
      .accountsStrict({ config: configPda, riskModel: riskModelPda, admin: admin.publicKey })
      .signers([admin])
      .rpc();
    // Assets start unsupported (refresh fails closed) until the risk role sets them.
    for (const [asset, shock] of [
      [SOL, 1_000],
      [ETH, 800],
    ]) {
      await program.methods
        .setAssetRisk(asset, { shockBps: shock, gapBps: shock, basisBps: 100 })
        .accountsStrict({ config: configPda, riskModel: riskModelPda, admin: admin.publicKey })
        .signers([admin])
        .rpc();
    }
    await setPrice(SOL, 150_000_000);
    await setPrice(ETH, 3_000_000_000);
  });

  describe("joint portfolio credit", () => {
    // Golden values: docs/RISK_MODEL.md worked example (S = $9,000, R_p = $5,000,
    // 20% haircut) and crates/ch_math/tests/vectors/portfolio.json.
    it("hedged pair: golden credits, TradingPowerOnly, counted once in the global counter", async () => {
      const before = (await program.account.globalConfig.fetch(configPda)).globalOutstandingCredit;
      const u = await hedgedUser();
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      expect(p.status).to.equal(STATUS_ACTIVE);
      expect(p.creditKind).to.equal(1);
      expect(await credits(u.publicKey)).to.deep.equal(["3555555555", "3555555555"]);
      expect(p.totalCredit.toString()).to.equal("7111111110");
      expect(p.sumStandaloneRisk.toString()).to.equal("9000000000");
      expect(p.portfolioRisk.toString()).to.equal("5000000000");
      const after = (await program.account.globalConfig.fetch(configPda)).globalOutstandingCredit;
      expect(after.sub(before).toString()).to.equal("7111111110");
      // Refreshing again does not double count.
      await refresh(u.publicKey);
      const again = (await program.account.globalConfig.fetch(configPda)).globalOutstandingCredit;
      expect(again.toString()).to.equal(after.toString());
    });

    it("single unhedged leg gets exactly zero", async () => {
      const u = await newUser();
      await submitTx(u.publicKey, 0, SOL, true, IM).rpc();
      await refresh(u.publicKey);
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      expect(p.totalCredit.toString()).to.equal("0");
    });

    it("HISTORICAL REPLAY (Nov 2022) in the scenario set removes the SOL/ETH hedge credit", async () => {
      const setScenario = (kind: number, moves: { asset: number; moveBps: number }[]) =>
        program.methods
          .setScenario(0, kind, moves)
          .accountsStrict({ config: configPda, riskModel: riskModelPda, admin: admin.publicKey })
          .signers([admin])
          .rpc();
      await setScenario(1, [
        { asset: SOL, moveBps: -5_351 },
        { asset: ETH, moveBps: -1_747 },
      ]);
      try {
        const u = await hedgedUser();
        const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
        expect(p.totalCredit.toString()).to.equal("0");
      } finally {
        await setScenario(0, []);
      }
    });

    it("assets without risk parameters fail closed", async () => {
      const u = await newUser();
      await setPrice(7, 1_000_000);
      await submitTx(u.publicKey, 0, 7, true, IM).rpc();
      await expectTxError(refresh(u.publicKey), "UnsupportedAsset");
    });

    it("F3: three hedge venues are netted jointly; margin held >= netted risk", async () => {
      const u = await newUser();
      await submitTx(u.publicKey, 0, SOL, true, IM).rpc();
      for (const v of [1, 2, 3]) await submitTx(u.publicKey, v, ETH, false, IM).rpc();
      await refresh(u.publicKey);
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      // Fraction of IM still held is at least R_p / S, and no leg below MM.
      const held = BigInt(4 * IM) - BigInt(p.totalCredit.toString());
      const s = BigInt(p.sumStandaloneRisk.toString());
      const rp = BigInt(p.portfolioRisk.toString());
      expect(held * s >= (rp < s ? rp : s) * BigInt(4 * IM)).to.equal(true);
      for (const l of p.legs.slice(0, p.numLegs)) {
        expect(BigInt(l.credit.toString()) <= BigInt(IM / 2)).to.equal(true);
      }
    });
  });

  describe("atomic invalidation (F2, F5)", () => {
    it("shrinking a leg invalidates the credit in the same instruction", async () => {
      const u = await hedgedUser();
      await submitTx(u.publicKey, 1, ETH, false, 200_000, 1_000_000, 100_000).rpc();
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      expect(p.status).to.equal(STATUS_INVALIDATED);
    });

    it("snapshot + refresh in one transaction recomputes from the new inputs", async () => {
      const u = await hedgedUser();
      const metas = await refreshMetas(u.publicKey);
      const flip = await submitTx(u.publicKey, 1, ETH, true, IM).instruction();
      await refreshTx(u.publicKey, metas).preInstructions([flip]).signers([venueAuths[1]]).rpc();
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      expect(p.status).to.equal(STATUS_ACTIVE);
      expect(p.legs[1].isLong).to.equal(true);
      expect(p.legs[1].snapshotVersion.toNumber()).to.equal(2);
    });

    it("consent withdrawal invalidates in the same instruction and blocks refresh", async () => {
      const u = await hedgedUser();
      await program.methods
        .updateUserConsent(false, new BN(0))
        .accountsStrict({
          userConsent: consentPda(u.publicKey),
          portfolio: portfolioPda(u.publicKey),
          user: u.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([u])
        .rpc();
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      expect(p.status).to.equal(STATUS_INVALIDATED);
      await expectTxError(refresh(u.publicKey), "UserConsentMissing");
      // The counter share is released by the permissionless invalidation, once.
      await invalidateTx(u.publicKey).rpc();
      await expectTxError(invalidateTx(u.publicKey).rpc(), "InvalidationNotJustified");
    });

    it("a price move beyond max_move_bps from the stored baseline allows invalidation", async () => {
      const u = await hedgedUser();
      const oracles = [SOL, ETH].map((a) => meta(pricePda(a)));
      await expectTxError(invalidateTx(u.publicKey, oracles).rpc(), "InvalidationNotJustified");
      await setPrice(SOL, 159_000_000); // +6%
      try {
        await invalidateTx(u.publicKey, oracles).rpc();
        const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
        expect(p.status).to.equal(STATUS_INVALIDATED);
        expect(p.legs[0].baselinePrice.toString()).to.equal("150000000");
      } finally {
        await setPrice(SOL, 150_000_000);
      }
    });
  });

  describe("refresh and snapshot input validation", () => {
    it("rejects a subset of legs, duplicates, and foreign snapshots", async () => {
      const u = await newUser();
      const v = await newUser();
      await submitTx(u.publicKey, 0, SOL, true, IM).rpc();
      await submitTx(u.publicKey, 1, ETH, false, IM).rpc();
      await submitTx(v.publicKey, 1, ETH, false, IM).rpc();
      const a = snapshotPda(u.publicKey, 0, SOL);
      const foreign = snapshotPda(v.publicKey, 1, ETH);
      await expectTxError(refreshTx(u.publicKey, [meta(a), meta(pricePda(SOL))]).rpc(), "IncompletePortfolio");
      await expectTxError(
        refreshTx(u.publicKey, [meta(a), meta(a), meta(pricePda(SOL)), meta(pricePda(SOL))]).rpc(),
        "DuplicateLeg"
      );
      await expectTxError(
        refreshTx(u.publicKey, [meta(a), meta(foreign), meta(pricePda(SOL)), meta(pricePda(ETH))]).rpc(),
        "InvalidSnapshotAccount"
      );
    });

    it("snapshot inputs must satisfy 0 < MM <= IM <= notional and notional > 0", async () => {
      const u = await newUser();
      await expectTxError(submitTx(u.publicKey, 0, SOL, true, 0, 0, 0).rpc(), "ZeroNotional");
      await expectTxError(submitTx(u.publicKey, 0, SOL, true, 50, 100, 60).rpc(), "InvalidMargin");
      await expectTxError(submitTx(u.publicKey, 0, SOL, true, 150, 100, 60).rpc(), "InvalidMargin");
      await expectTxError(submitTx(u.publicKey, 0, 32, true, 50, 100, 25).rpc(), "InvalidAssetId");
    });

    it("a venue not in the consent bitmap cannot submit", async () => {
      const u = await newUser(new BN(0b10));
      await expectTxError(submitTx(u.publicKey, 0, SOL, true, IM).rpc(), "VenueNotAuthorized");
    });

    it("only the registered venue authority can submit", async () => {
      const u = await newUser();
      await expectTxError(
        program.methods
          .submitPositionSnapshot(Array.from(venueIds[0]), SOL, new BN(5 * IM), true, new BN(IM), new BN(IM / 2))
          .accountsStrict({
            config: configPda,
            venueRegistration: venuePda(0),
            userConsent: consentPda(u.publicKey),
            portfolio: portfolioPda(u.publicKey),
            posSnapshot: snapshotPda(u.publicKey, 0, SOL),
            user: u.publicKey,
            venueAuthority: mallory.publicKey,
            payer: mallory.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([mallory])
          .rpc(),
        "Unauthorized"
      );
    });

    it("only the venue (or the user, once consent is withdrawn) can close a snapshot", async () => {
      const u = await newUser();
      await submitTx(u.publicKey, 1, ETH, false, IM).rpc();
      const close = (closer: Keypair) =>
        program.methods
          .closePositionSnapshot()
          .accountsStrict({
            venueRegistration: venuePda(1),
            userConsent: consentPda(u.publicKey),
            portfolio: portfolioPda(u.publicKey),
            posSnapshot: snapshotPda(u.publicKey, 1, ETH),
            rentReceiver: venueAuths[1].publicKey,
            closer: closer.publicKey,
          })
          .signers([closer])
          .rpc();
      await expectTxError(close(mallory), "SnapshotCloseNotAllowed");
      await expectTxError(close(u), "SnapshotCloseNotAllowed");
      await close(venueAuths[1]);
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      expect(p.numRegistered).to.equal(0);
    });
  });

  describe("governance and global invalidation", () => {
    it("F8: re-registering an existing venue fails and indices are never reused", async () => {
      // Venue PDA is `init`: system program "already in use" (custom error 0).
      await expectTxError(
        program.methods
          .registerVenue(Array.from(venueIds[0]), Keypair.generate().publicKey, mallory.publicKey, 1)
          .accountsStrict({
            config: configPda,
            venueRegistration: venuePda(0),
            admin: admin.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([admin])
          .rpc(),
        0
      );
      const newId = Buffer.alloc(32);
      newId.write("venue-new");
      await expectTxError(
        program.methods
          .registerVenue(Array.from(newId), Keypair.generate().publicKey, mallory.publicKey, 0)
          .accountsStrict({
            config: configPda,
            venueRegistration: pda(Buffer.from("venue"), newId),
            admin: admin.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([admin])
          .rpc(),
        "VenueIndexAlreadyUsed"
      );
    });

    it("correlation and parameter changes bump the risk epoch and reset the counters", async () => {
      const u = await hedgedUser();
      const e0 = (await program.account.globalConfig.fetch(configPda)).riskEpoch.toNumber();
      await program.methods
        .updateCorrelations([{ a: SOL, b: ETH, rho: 800_000 }])
        .accountsStrict({ config: configPda, riskModel: riskModelPda, admin: admin.publicKey })
        .signers([admin])
        .rpc();
      let c = await program.account.globalConfig.fetch(configPda);
      expect(c.riskEpoch.toNumber()).to.equal(e0 + 1);
      expect(c.globalOutstandingCredit.toString()).to.equal("0");
      const p = await program.account.portfolio.fetch(portfolioPda(u.publicKey));
      expect(p.riskEpoch.toNumber()).to.equal(e0); // stale epoch => unusable

      await program.methods
        .updateParams(params)
        .accountsStrict({ config: configPda, admin: admin.publicKey })
        .signers([admin])
        .rpc();
      c = await program.account.globalConfig.fetch(configPda);
      expect(c.riskEpoch.toNumber()).to.equal(e0 + 2);
    });

    it("venue deactivation bumps the epoch; refresh then fails with VenueInactive", async () => {
      const u = await newUser();
      await submitTx(u.publicKey, 2, SOL, true, IM).rpc();
      await submitTx(u.publicKey, 3, ETH, false, IM).rpc();
      await refresh(u.publicKey);
      const setActive = (active: boolean) =>
        program.methods
          .setVenueActive(active)
          .accountsStrict({ config: configPda, venueRegistration: venuePda(3), admin: admin.publicKey })
          .signers([admin])
          .rpc();
      await setActive(false);
      try {
        await expectTxError(refresh(u.publicKey), "VenueInactive");
        await expectTxError(submitTx(u.publicKey, 3, ETH, false, IM).rpc(), "VenueInactive");
      } finally {
        await setActive(true);
      }
      await refresh(u.publicKey);
    });

    it("admin-only instructions reject other signers", async () => {
      await expectTxError(
        program.methods
          .updateParams(params)
          .accountsStrict({ config: configPda, admin: mallory.publicKey })
          .signers([mallory])
          .rpc(),
        "Unauthorized"
      );
      await expectTxError(
        program.methods
          .updateCorrelations([{ a: SOL, b: ETH, rho: 1_000_000 }])
          .accountsStrict({ config: configPda, riskModel: riskModelPda, admin: mallory.publicKey })
          .signers([mallory])
          .rpc(),
        "Unauthorized"
      );
      await expectTxError(
        program.methods
          .setMockPrice(SOL, new BN(1), new BN(0), new BN(0))
          .accountsStrict({
            config: configPda,
            mockPrice: pricePda(SOL),
            admin: mallory.publicKey,
            systemProgram: SystemProgram.programId,
          })
          .signers([mallory])
          .rpc(),
        "Unauthorized"
      );
    });

    it("rejects out-of-range correlations and invalid parameters", async () => {
      await expectTxError(
        program.methods
          .updateCorrelations([{ a: SOL, b: ETH, rho: 1_000_001 }])
          .accountsStrict({ config: configPda, riskModel: riskModelPda, admin: admin.publicKey })
          .signers([admin])
          .rpc(),
        "InvalidCorrelationValue"
      );
      await expectTxError(
        program.methods
          .updateCorrelations([{ a: SOL, b: SOL, rho: 0 }])
          .accountsStrict({ config: configPda, riskModel: riskModelPda, admin: admin.publicKey })
          .signers([admin])
          .rpc(),
        "InvalidAssetId"
      );
      await expectTxError(
        program.methods
          .updateParams({ ...params, creditTtlSlots: new BN(3_001) })
          .accountsStrict({ config: configPda, admin: admin.publicKey })
          .signers([admin])
          .rpc(),
        "InvalidConfigParams"
      );
    });

    it("pause blocks refresh but not snapshot writes, consent changes or invalidation", async () => {
      const u = await hedgedUser();
      const setPaused = (paused: boolean) =>
        program.methods
          .setPaused(paused)
          .accountsStrict({ config: configPda, admin: admin.publicKey })
          .signers([admin])
          .rpc();
      await setPaused(true);
      try {
        await expectTxError(refresh(u.publicKey), "ProgramPaused");
        await submitTx(u.publicKey, 1, ETH, false, IM / 2).rpc();
        await invalidateTx(u.publicKey).rpc();
      } finally {
        await setPaused(false);
      }
    });

    it("two-step admin transfer", async () => {
      const next = Keypair.generate();
      await airdrop(next.publicKey);
      await expectTxError(
        program.methods
          .acceptAdmin()
          .accountsStrict({ config: configPda, proposedAdmin: next.publicKey })
          .signers([next])
          .rpc(),
        "Unauthorized"
      );
      await program.methods
        .proposeAdmin(next.publicKey)
        .accountsStrict({ config: configPda, admin: admin.publicKey })
        .signers([admin])
        .rpc();
      await program.methods
        .acceptAdmin()
        .accountsStrict({ config: configPda, proposedAdmin: next.publicKey })
        .signers([next])
        .rpc();
      // Hand it back so later suites keep working.
      await program.methods
        .proposeAdmin(admin.publicKey)
        .accountsStrict({ config: configPda, admin: next.publicKey })
        .signers([next])
        .rpc();
      await program.methods
        .acceptAdmin()
        .accountsStrict({ config: configPda, proposedAdmin: admin.publicKey })
        .signers([admin])
        .rpc();
      expect((await program.account.globalConfig.fetch(configPda)).admin.toBase58()).to.equal(
        admin.publicKey.toBase58()
      );
    });
  });
});

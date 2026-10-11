// Self-test for expectTxError. Runs without a validator.
//
// This exists because the original negative tests passed even when the
// transaction under test succeeded (F11): `expect.fail("…X…")` threw an
// AssertionError whose message contained "X", and the catch block accepted it.
import { AnchorError } from "@coral-xyz/anchor";
import { AssertionError, expect } from "chai";
import { expectTxError, identifyTxError } from "./expectTxError";

function anchorErr(code: string, number: number): AnchorError {
  return new AnchorError(
    { code, number },
    `${code} message`,
    [`Program log: AnchorError occurred. Error Code: ${code}. Error Number: ${number}.`],
    [
      "Program 2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg invoke [1]",
      `Program log: AnchorError occurred. Error Code: ${code}. Error Number: ${number}.`,
    ]
  );
}

async function rejectsWithAssertion(p: Promise<unknown>): Promise<string> {
  try {
    await p;
  } catch (e) {
    expect(e).to.be.instanceOf(AssertionError);
    return (e as Error).message;
  }
  throw new Error("helper did not reject");
}

describe("expectTxError helper (no validator)", () => {
  it("fails when the transaction succeeds, even if the fail message would contain the expected name", async () => {
    // The vacuous pattern: the success path used to produce
    // "Should have failed with VenueNotAuthorized", which matched.
    const msg = await rejectsWithAssertion(
      expectTxError(Promise.resolve("5igSig"), "VenueNotAuthorized")
    );
    expect(msg).to.contain("but it succeeded");
  });

  it("passes on the exact Anchor error name", async () => {
    const id = await expectTxError(Promise.reject(anchorErr("VenueInactive", 6001)), "VenueInactive");
    expect(id.name).to.equal("VenueInactive");
    expect(id.code).to.equal(6001);
  });

  it("passes on a numeric code", async () => {
    await expectTxError(Promise.reject(anchorErr("AccountNotInitialized", 3012)), 3012);
  });

  it("fails on a different error, and does not substring-match names", async () => {
    // "Unauthorized" must not match "UnauthorizedRole" or vice versa.
    const msg = await rejectsWithAssertion(
      expectTxError(Promise.reject(anchorErr("UnauthorizedRole", 6100)), "Unauthorized")
    );
    expect(msg).to.contain("got name=UnauthorizedRole");
    expect(msg).to.contain("logs:");
  });

  it("never swallows an AssertionError thrown inside the awaited promise", async () => {
    const inner = new AssertionError("Should have failed with Unauthorized");
    try {
      await expectTxError(Promise.reject(inner), "Unauthorized");
      throw new Error("helper swallowed the AssertionError");
    } catch (e) {
      expect(e).to.equal(inner);
    }
  });

  it("identifies errors from preflight logs and raw custom codes", () => {
    const fromLogs = identifyTxError({
      logs: ["Program log: AnchorError occurred. Error Code: PriceStale. Error Number: 6010. Error Message: Price oracle is stale."],
    });
    expect(fromLogs.name).to.equal("PriceStale");
    expect(fromLogs.code).to.equal(6010);

    const raw = identifyTxError(new Error("failed to send transaction: custom program error: 0x1771"));
    expect(raw.code).to.equal(0x1771);
    expect(raw.name).to.equal(undefined);
  });

  it("rejects an unidentifiable error rather than passing", async () => {
    const msg = await rejectsWithAssertion(expectTxError(Promise.reject(new Error("boom")), "PriceStale"));
    expect(msg).to.contain("got name=?");
  });
});

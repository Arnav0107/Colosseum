// TODO(owner): Dev A
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { expect } from "chai";

describe("ch_core", () => {
  // Configure the client to use the local cluster.
  anchor.setProvider(anchor.AnchorProvider.env());

  it("can initialize ch_core placeholder", async () => {
    // Placeholder test
    expect(true).to.be.true;
  });
});

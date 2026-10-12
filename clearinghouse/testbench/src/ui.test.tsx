// Server-rendered checks of the UI contract: the environment banner, labels
// for every form control, live regions, and the verification verdict. No DOM
// library needed: react-dom/server renders the same markup the browser gets.
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Bench } from "./chain/bench";
import { App } from "./App";
import { Banner, BANNER_TEXT } from "./components/Banner";
import { LogPanel } from "./components/LogPanel";
import { PortfolioPanel } from "./components/PortfolioPanel";
import { PositionsPanel, validatePosition } from "./components/PositionsPanel";
import { PricesPanel } from "./components/PricesPanel";
import { parseRho, SetupPanel } from "./components/SetupPanel";
import { VenueRulePanel } from "./components/VenueRulePanel";
import { stateFixture } from "./model/fixtures";
import type { PanelProps } from "./components/types";

const bench = new Bench("http://127.0.0.1:8899"); // never contacted: effects do not run in SSR
const props = (): PanelProps => ({ bench, state: stateFixture(), run: async () => {}, busy: false });

/** Every <input>/<select> has an id referenced by a <label for=…>. */
function unlabelledControls(html: string): string[] {
  const labels = new Set([...html.matchAll(/<label for="([^"]+)"/g)].map((m) => m[1]));
  const controls = [...html.matchAll(/<(input|select)\b[^>]*>/g)].map((m) => m[0]);
  return controls.filter((c) => {
    const id = /\bid="([^"]+)"/.exec(c)?.[1];
    return !id || !labels.has(id);
  });
}

describe("environment banner", () => {
  it("states localnet, mock oracle and unverified venues", () => {
    expect(BANNER_TEXT).toBe("LOCALNET · MOCK ORACLE · UNVERIFIED VENUES");
    const html = renderToStaticMarkup(<Banner />);
    expect(html).toContain(BANNER_TEXT);
    expect(html).toContain('role="note"');
  });

  it("is part of the app shell before any chain state loads", () => {
    const html = renderToStaticMarkup(<App />);
    expect(html.indexOf(BANNER_TEXT)).toBeGreaterThanOrEqual(0);
    expect(html.indexOf(BANNER_TEXT)).toBeLessThan(html.indexOf("<main"));
    expect(unlabelledControls(html)).toEqual([]);
  });
});

describe("panels", () => {
  const panels = {
    setup: <SetupPanel {...props()} />,
    prices: <PricesPanel {...props()} />,
    positions: <PositionsPanel {...props()} />,
    portfolio: <PortfolioPanel {...props()} />,
  };
  for (const [name, el] of Object.entries(panels)) {
    it(`${name}: every form control has a label; tables have column headers`, () => {
      const html = renderToStaticMarkup(el);
      expect(unlabelledControls(html)).toEqual([]);
      const tables = (html.match(/<table/g) ?? []).length;
      expect((html.match(/<th scope="col"/g) ?? []).length).toBeGreaterThanOrEqual(tables);
      expect(html).toMatch(/aria-labelledby="[^"]+"/);
    });
  }

  it("portfolio panel shows the breakdown, the MM floor and a MATCH verdict from on-chain inputs", () => {
    const html = renderToStaticMarkup(<PortfolioPanel {...props()} />);
    expect(html).toContain("$7,111.11"); // total credit
    expect(html).toContain("$3,555.55"); // per-leg credit
    expect(html).toContain("$6,444.44"); // IM − credit, above MM
    expect(html).toContain("44.44%"); // hedge benefit
    expect(html).toContain("MATCH.");
    expect(html).toContain('role="status"');
    expect(html).toContain("trading power only");
  });

  it("venue rule panel (C3): same $8,000, $40,000 unhedged vs $59,000 hedged, labelled controls", () => {
    const st = stateFixture();
    const html = renderToStaticMarkup(<VenueRulePanel config={st.config} riskModel={st.riskModel} />);
    expect(unlabelledControls(html)).toEqual([]);
    expect(html).toContain("$40,000.00");
    expect(html).toContain("$59,000.00");
    expect(html).toContain("$19,000.00"); // the difference
    expect(html).toContain("the loaded config and risk model");
    const noChain = renderToStaticMarkup(<VenueRulePanel config={null} riskModel={null} />);
    expect(noChain).toContain("the worked example (no chain state loaded)");
    expect(noChain).toContain("$59,000.00");
  });

  it("prices panel ages come from the on-chain clock and flag stale prices", () => {
    const html = renderToStaticMarkup(<PricesPanel {...props()} />);
    expect(html).toContain("40 s"); // SOL: 1_700_000_040 − 1_700_000_000
    expect(html).toContain("1040 s"); // ETH, older than max_price_age_secs = 300
    expect(html).toContain("stale");
  });

  it("log is a polite live region", () => {
    const html = renderToStaticMarkup(<LogPanel entries={[{ id: 1, at: "t", text: "x", ok: false }]} />);
    expect(html).toContain('role="log"');
    expect(html).toContain('aria-live="polite"');
    expect(html).toContain("Error:");
  });
});

describe("form validation mirrors the program", () => {
  it("position: 0 < MM ≤ IM ≤ notional", () => {
    expect(validatePosition({ notional: "50000", im: "10000", mm: "5000" })).toEqual({
      notional: 50_000_000_000n,
      im: 10_000_000_000n,
      mm: 5_000_000_000n,
    });
    expect("error" in validatePosition({ notional: "0", im: "0", mm: "0" })).toBe(true);
    expect("error" in validatePosition({ notional: "100", im: "10", mm: "0" })).toBe(true);
    expect("error" in validatePosition({ notional: "100", im: "10", mm: "11" })).toBe(true);
    expect("error" in validatePosition({ notional: "100", im: "101", mm: "1" })).toBe(true);
  });

  it("correlation input is bounded to [-1, 1]", () => {
    expect(parseRho("0.8")).toBe(800_000);
    expect(parseRho("-1")).toBe(-1_000_000);
    expect(parseRho("1.01")).toBeNull();
    expect(parseRho("")).toBeNull();
    expect(parseRho("x")).toBeNull();
  });
});

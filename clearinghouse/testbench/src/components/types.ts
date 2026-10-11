import type { Bench } from "../chain/bench";
import type { BenchState } from "../model/views";

/** Run an action, log its outcome, then reload on-chain state. */
export type Run = (label: string, action: () => Promise<unknown>) => Promise<void>;

export interface PanelProps {
  bench: Bench;
  state: BenchState;
  run: Run;
  busy: boolean;
}

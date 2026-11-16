// citrate-core: the FL network state as a React hook (see networkState.ts for the model and its
// sources). One read is shared by every FL view mounted at once; `refresh` reads again.
import { useEffect, useState } from "react";
import { bridge } from "../bridge";
import realBook from "../../src-tauri/addresses/40204.json";
import { bookOnlyState, readFlNetworkState, type FlBook, type FlNetworkState, type FlReads } from "./networkState";

const chainReads: FlReads = {
  codeSize: (a) => bridge.contracts.codeSize("citrate", a),
  viewCall: (a, data) => bridge.contracts.viewCall("citrate", a, data),
};

let shared: { book: FlBook; reads: FlReads; promise: Promise<FlNetworkState> } | null = null;

function sharedRead(book: FlBook, reads: FlReads, fresh: boolean): Promise<FlNetworkState> {
  if (fresh || !shared || shared.book !== book || shared.reads !== reads) {
    shared = { book, reads, promise: readFlNetworkState(book, reads) };
  }
  return shared.promise;
}

/** Test seam: forget the shared read. */
export function resetFlNetworkStateCache(): void {
  shared = null;
}

export interface UseFlNetworkStateOptions {
  /** Defaults to the generated address book; tests pass their own. */
  book?: FlBook;
  /** Defaults to core's code-size and view-call reads on the active network. */
  reads?: FlReads;
}

export function useFlNetworkState({ book = realBook, reads = chainReads }: UseFlNetworkStateOptions = {}): { state: FlNetworkState; refresh: () => void } {
  const [state, setState] = useState<FlNetworkState>(() => bookOnlyState(book));
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let live = true;
    void sharedRead(book, reads, tick > 0).then((s) => {
      if (live) setState(s);
    });
    return () => {
      live = false;
    };
  }, [book, reads, tick]);
  return { state, refresh: () => setTick((t) => t + 1) };
}

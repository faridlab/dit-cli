// The whole-root layout, off the main thread: thousands of nodes take a few
// seconds to settle, and the screen must keep panning while they do. The
// simulation itself lives in codegraphLayout.ts, where it is tested.

import { runLayout, type LayoutMessage, type LayoutRequest } from "./codegraphLayout";

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<LayoutRequest>) => void) | null;
  postMessage: (message: LayoutMessage, transfer: Transferable[]) => void;
};

scope.onmessage = (event) => {
  runLayout(event.data, (message) => scope.postMessage(message, [message.positions.buffer]));
};

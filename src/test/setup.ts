import { afterEach } from "vitest";

// jest-dom matchers are only meaningful under jsdom. `*.dom.test.tsx` files
// get that environment from vite.config.ts; pure tests run under node.
if (typeof document !== "undefined") {
  const [{ cleanup }, jestDom] = await Promise.all([
    import("@testing-library/react"),
    import("@testing-library/jest-dom/vitest"),
  ]);
  void jestDom;
  afterEach(() => cleanup());
}

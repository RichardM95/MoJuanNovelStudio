// @ts-expect-error Node Web Crypto is only used by Vitest.
import { webcrypto } from "node:crypto";

if (!globalThis.crypto.subtle)
  Object.defineProperty(globalThis.crypto, "subtle", {
    value: webcrypto.subtle,
  });

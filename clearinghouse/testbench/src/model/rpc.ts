/** The test bench signs with fixed dev keys, so it must never talk to a real cluster. */
export function isLocalRpc(url: string): boolean {
  try {
    const u = new URL(url);
    if (u.protocol !== "http:" && u.protocol !== "https:") return false;
    return u.hostname === "localhost" || u.hostname === "127.0.0.1" || u.hostname === "[::1]";
  } catch {
    return false;
  }
}

export const DEFAULT_RPC = "http://127.0.0.1:8899";

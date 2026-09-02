const SHA256_OUTPUT_PATTERN = /^(?<hash>[0-9a-f]{64})\s/u;

export function sha256FromOutput(result, peer) {
  const hash = result.match(SHA256_OUTPUT_PATTERN)?.groups.hash;
  if (!hash) {
    throw new Error(`diverse LAN ${peer} returned an invalid SHA-256`);
  }
  return hash;
}

import { existsSync, writeFileSync } from "node:fs";
if (!existsSync(process.env.AST_GREP ?? "")) {
  throw new Error("AST_GREP executable is missing");
}
writeFileSync("/tmp/quickshare-staged-contract-dSOUcl/plugin-test-ran", "ran\n");

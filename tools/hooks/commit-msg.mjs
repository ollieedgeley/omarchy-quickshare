import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const COMMIT_PATTERN =
  /^(feat|fix|test|refactor|perf|docs|build|chore)(\([a-z0-9][a-z0-9-]*\))?!?: [a-z0-9][^\n.]*[^\s.]$/;

export function validateCommitMessage(message) {
  const subject = message.split("\n", 1)[0];
  if (subject.length > 72) return "commit subject exceeds 72 characters";
  if (!COMMIT_PATTERN.test(subject)) {
    return "use Conventional Commits: type(scope): imperative description";
  }
  return undefined;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const path = process.argv[2];
  if (!path) throw new Error("COMMIT_MSG_FILE is required");
  const failure = validateCommitMessage(readFileSync(path, "utf8"));
  if (failure) {
    process.stderr.write(`${failure}\n`);
    process.exitCode = 1;
  }
}

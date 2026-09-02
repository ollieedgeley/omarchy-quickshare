import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const SUBJECT_LIMIT = 72;
const TYPES = "(?:feat|fix|test|refactor|perf|docs|build|chore)";
const SCOPE = "(?:\\([a-z0-9][a-z0-9-]*\\))?";

export const COMMIT_PATTERN = new RegExp(
  `^${TYPES}${SCOPE}!?: [a-z0-9][^\\n.]*[^\\s.]$`,
  "u",
);

export function validateCommitMessage(message) {
  const [subject] = message.split("\n", 1);
  if (subject.length > SUBJECT_LIMIT) {
    return "commit subject exceeds 72 characters";
  }
  if (!COMMIT_PATTERN.test(subject)) {
    return "use Conventional Commits: type(scope): imperative description";
  }
  return globalThis.undefined;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , path] = process.argv;
  if (!path) {
    throw new Error("COMMIT_MSG_FILE is required");
  }
  const failure = validateCommitMessage(readFileSync(path, "utf8"));
  if (failure) {
    process.stderr.write(`${failure}\n`);
    process.exitCode = 1;
  }
}

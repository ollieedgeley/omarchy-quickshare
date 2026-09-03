const COMMAND_BOUNDARY_PATTERN = /&&|\|\||[;|()\n]/u;
const WHITESPACE_PATTERN = /\s+/u;
const ASSIGNMENT_PATTERN = /^[A-Za-z_]\w*=/u;
const MAKE_EXECUTABLE_PATTERN = /(?:^|\/)g?make$/u;
const QUOTED_WORD_PATTERN = /^(?<quote>["'])(?<content>.*)\k<quote>$/u;
const PROTECTED_TARGETS = new Set([
  "pre-commit",
  "pre-push",
  "verify",
  "build",
]);

function bareWord(word) {
  return word.match(QUOTED_WORD_PATTERN)?.groups?.content ?? word;
}

function makeArguments(segment) {
  const words = segment.trim().split(WHITESPACE_PATTERN).filter(Boolean);
  let index = 0;
  while (ASSIGNMENT_PATTERN.test(words[index] ?? "")) {
    index += 1;
  }
  if (words[index] === "env") {
    index += 1;
    while (
      (words[index] ?? "").startsWith("-") ||
      ASSIGNMENT_PATTERN.test(words[index] ?? "")
    ) {
      index += 1;
    }
  }
  if (words[index] === "command") {
    index += 1;
  }
  while (ASSIGNMENT_PATTERN.test(words[index] ?? "")) {
    index += 1;
  }
  if (!MAKE_EXECUTABLE_PATTERN.test(words[index] ?? "")) {
    return [];
  }
  return words.slice(index + 1).map(bareWord);
}

function protectedTarget(command) {
  for (const segment of command.split(COMMAND_BOUNDARY_PATTERN)) {
    const target = makeArguments(segment).find((word) =>
      PROTECTED_TARGETS.has(word),
    );
    if (target) {
      return target;
    }
  }
  return null;
}

export default function blockMainGates(pi) {
  pi.on("tool_call", (event) => {
    if (event.toolName !== "bash") {
      return null;
    }
    const target = protectedTarget(String(event.input?.command ?? ""));
    if (!target) {
      return null;
    }
    return {
      block: true,
      reason:
        `Direct \`make ${target}\` is blocked. ` +
        "Git hooks own aggregate gates; run a narrow target or use " +
        "`git commit`/`git push`.",
    };
  });
}

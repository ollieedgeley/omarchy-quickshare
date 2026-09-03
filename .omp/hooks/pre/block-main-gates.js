const COMMAND_BOUNDARY_PATTERN = /&&|\|\||[;|()\n]/u;
const WHITESPACE_PATTERN = /\s+/u;
const ASSIGNMENT_PATTERN = /^[A-Za-z_]\w*=/u;
const QUOTED_WORD_PATTERN = /^(?<quote>["'])(?<content>.*)\k<quote>$/u;
const PROTECTED_TARGETS = new Set([
  "pre-commit",
  "pre-push",
  "verify",
  "build",
]);
const MAKE_EXECUTABLES = new Set(["make", "gmake"]);
const ENV_OPTIONS_WITH_VALUE = new Set([
  "-C",
  "--chdir",
  "-S",
  "--split-string",
  "-u",
  "--unset",
]);
const TIMEOUT_OPTIONS_WITH_VALUE = new Set([
  "-k",
  "--kill-after",
  "-s",
  "--signal",
]);

function bareWord(word) {
  return word.match(QUOTED_WORD_PATTERN)?.groups?.content ?? word;
}

function executableName(word) {
  const unquoted = bareWord(word);
  return unquoted.slice(unquoted.lastIndexOf("/") + 1);
}

function skipOptions(words, start, optionsWithValue) {
  let index = start;
  while ((words[index] ?? "").startsWith("-")) {
    const option = bareWord(words[index]);
    if (optionsWithValue.has(option)) {
      index += 2;
    } else {
      index += 1;
    }
  }
  return index;
}

function skipExecutionWrappers(words) {
  let index = 0;
  while (index < words.length) {
    while (ASSIGNMENT_PATTERN.test(words[index] ?? "")) {
      index += 1;
    }
    const executable = executableName(words[index] ?? "");
    if (executable === "command") {
      index += 1;
    } else if (executable === "env") {
      index = skipOptions(words, index + 1, ENV_OPTIONS_WITH_VALUE);
    } else if (executable === "timeout") {
      index = skipOptions(words, index + 1, TIMEOUT_OPTIONS_WITH_VALUE) + 1;
    } else {
      return index;
    }
  }
  return index;
}

function makeArguments(segment) {
  const words = segment.trim().split(WHITESPACE_PATTERN).filter(Boolean);
  const index = skipExecutionWrappers(words);
  if (!MAKE_EXECUTABLES.has(executableName(words[index] ?? ""))) {
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

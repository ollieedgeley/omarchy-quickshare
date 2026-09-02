const RUN_PATTERN = /^\[ RUN {6}\] .+$/gmu;
const OK_PATTERN = /^\[ {7}OK \] .+$/gmu;
const SUMMARY_PATTERN = /^\[ {2}PASSED {2}\] (?<count>\d+) tests?\.$/mu;

export function assertGtestEvidence(output, expectedCount) {
  const ran = [...output.matchAll(RUN_PATTERN)].length;
  const passed = [...output.matchAll(OK_PATTERN)].length;
  const summary = Number(output.match(SUMMARY_PATTERN)?.groups?.count);
  if (!summary || !ran || passed !== ran || summary !== ran) {
    throw new Error(
      `GTest evidence is missing or incomplete: ran ${ran}, passed ${passed}`,
    );
  }
  if (ran !== expectedCount) {
    throw new Error(
      `expected ${expectedCount} selected cases, observed ${ran}`,
    );
  }
  return ran;
}

export function runSelectedGtest(docker, args, configuration) {
  const result = docker(args, { capture: true });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  process.stdout.write(output);
  const count = assertGtestEvidence(output, configuration.expectedCount);
  process.stdout.write(`${configuration.label} passed ${count} cases.\n`);
}

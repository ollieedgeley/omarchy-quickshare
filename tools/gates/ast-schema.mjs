import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import Ajv from "ajv";
import Ajv2020 from "ajv/dist/2020.js";
import { parseAllDocuments, parseDocument } from "yaml";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SCHEMAS = join(ROOT, "rules", "ast-grep", "schemas");
const expectedHashes = {
  "project.json":
    "2131295e01a760708d5249365120f97eba535448bda4f0b13fff077e2f5a4b2c",
  "rule.json":
    "f7bc52696200e7c75700ccbc95ca25993373c411f2b3ab2a1914b2a482b94d3c",
  "rust_rule.json":
    "7230b0a78356d2f06fdea94deb8fc09f5c8f443315620a85070b0e6bbc9398c7",
};

function checkedJson(name) {
  const source = readFileSync(join(SCHEMAS, name));
  const actual = createHash("sha256").update(source).digest("hex");
  if (actual !== expectedHashes[name]) {
    throw new Error(`${name} does not match the pinned ast-grep 0.45.3 schema`);
  }
  return JSON.parse(source);
}

function yamlValue(path) {
  const document = parseDocument(readFileSync(path, "utf8"));
  if (document.errors.length) {
    throw document.errors[0];
  }
  return document.toJS();
}

const projectAjv = new Ajv({
  allErrors: true,
  allowUnionTypes: true,
  strict: true,
});
projectAjv.addSchema(
  {
    $id: "rule.json",
    $defs: { SerializableRule: { type: "object" } },
  },
  "rule.json",
);
const projectValidator = projectAjv.compile(checkedJson("project.json"));
const project = yamlValue(join(ROOT, "sgconfig.yml"));
if (!projectValidator(project)) {
  throw new Error(
    `invalid sgconfig.yml: ${JSON.stringify(projectValidator.errors)}`,
  );
}

const ruleSchemaOptions = {
  allErrors: true,
  formats: { int32: true, uint: true },
  strict: true,
};
const ruleAjv = new Ajv2020(ruleSchemaOptions);
ruleAjv.addKeyword({ keyword: "example" });
const ruleValidator = ruleAjv.compile(checkedJson("rust_rule.json"));
ruleAjv.compile(checkedJson("rule.json"));
const ruleDirectory = join(ROOT, "rules", "ast-grep", "rules");
for (const name of readdirSync(ruleDirectory).filter((entry) =>
  entry.endsWith(".yml"),
)) {
  const path = join(ruleDirectory, name);
  const documents = parseAllDocuments(readFileSync(path, "utf8"));
  for (const document of documents) {
    if (document.errors.length) {
      throw document.errors[0];
    }
    if (!ruleValidator(document.toJS())) {
      throw new Error(
        `${name} violates the pinned schema: ${JSON.stringify(
          ruleValidator.errors,
        )}`,
      );
    }
  }
}

process.stdout.write("Pinned ast-grep schemas and configuration passed.\n");

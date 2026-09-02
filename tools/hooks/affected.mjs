import { dirname, relative, resolve, sep } from "node:path";

export function parseAffectedJson(value) {
  const paths = new Set();
  const visit = (node) => {
    if (typeof node === "string" && node.endsWith(".rs")) {
      paths.add(node);
    }
    if (Array.isArray(node)) {
      node.forEach(visit);
    }
    if (node && typeof node === "object") {
      Object.values(node).forEach(visit);
    }
  };
  visit(value);
  return [...paths];
}

export function packageSelection(metadata, paths, workspaceRoot) {
  const workspaceIds = new Set(metadata.workspace_members);
  const packages = metadata.packages.filter((pkg) => workspaceIds.has(pkg.id));
  const owners = new Set();
  for (const path of paths) {
    const absolute = resolve(workspaceRoot, path);
    const candidates = packages
      .filter((pkg) => {
        const packageRoot = dirname(pkg.manifest_path);
        return (
          absolute === packageRoot ||
          absolute.startsWith(`${packageRoot}${sep}`)
        );
      })
      .sort(
        (left, right) => right.manifest_path.length - left.manifest_path.length,
      );
    if (candidates[0]) {
      owners.add(candidates[0].id);
    }
  }

  const reverse = new Map();
  for (const node of metadata.resolve?.nodes ?? []) {
    for (const dependency of node.dependencies ?? []) {
      const dependents = reverse.get(dependency) ?? new Set();
      dependents.add(node.id);
      reverse.set(dependency, dependents);
    }
  }
  const selected = new Set(owners);
  const queue = [...owners];
  while (queue.length) {
    const current = queue.shift();
    for (const dependent of reverse.get(current) ?? []) {
      if (workspaceIds.has(dependent) && !selected.has(dependent)) {
        selected.add(dependent);
        queue.push(dependent);
      }
    }
  }

  return packages
    .filter((pkg) => selected.has(pkg.id))
    .map((pkg) => ({
      hasLibrary: pkg.targets.some((target) => target.kind.includes("lib")),
      name: pkg.name,
      root: relative(workspaceRoot, dirname(pkg.manifest_path)),
    }))
    .sort((left, right) => left.name.localeCompare(right.name));
}

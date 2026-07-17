import { spawnSync } from "node:child_process";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const result = spawnSync(
  "cargo",
  ["metadata", "--locked", "--format-version", "1", "--no-deps"],
  { cwd: root, encoding: "utf8" },
);

if (result.error) {
  process.stderr.write(
    `Failed to run cargo metadata: ${result.error.message}\n`,
  );
  process.exit(1);
}
if (result.status !== 0) {
  process.stderr.write(result.stderr);
  process.exit(result.status ?? 1);
}

const metadata = JSON.parse(result.stdout);
const members = new Set(metadata.workspace_members);
const packages = metadata.packages.filter(({ id }) => members.has(id));
const packageByPath = new Map(
  packages.map((pkg) => [resolve(dirname(pkg.manifest_path)), pkg.name]),
);
const violations = [];

function dependencyName(dependency) {
  return dependency.path == null
    ? dependency.name
    : (packageByPath.get(resolve(dependency.path)) ?? dependency.name);
}

function dependencyKind(dependency) {
  return dependency.kind ?? "normal";
}

function isWorkspaceDependency(dependency) {
  return dependency.path != null && packageByPath.has(resolve(dependency.path));
}

for (const pkg of packages) {
  const manifest = relative(root, pkg.manifest_path).split(sep).join("/");
  const isExample = manifest.startsWith("examples/");

  if (
    isExample &&
    !pkg.dependencies.some(
      (dependency) =>
        isWorkspaceDependency(dependency) &&
        dependencyName(dependency) === "arborui" &&
        dependencyKind(dependency) === "normal",
    )
  ) {
    violations.push(`${pkg.name} must depend on the arborui facade`);
  }

  for (const dependency of pkg.dependencies) {
    const name = dependencyName(dependency);
    const kind = dependencyKind(dependency);

    if (isExample && isWorkspaceDependency(dependency)) {
      const allowed =
        (kind === "normal" && name === "arborui") ||
        (kind === "dev" && name === "arborui-test");
      if (!allowed) {
        violations.push(
          `${pkg.name} has forbidden ${kind} dependency ${name}; examples use arborui normally and arborui-test for tests`,
        );
      }
    }

    if (name === "crossterm" && pkg.name !== "arborui-backend-crossterm") {
      violations.push(
        `${pkg.name} depends on crossterm outside arborui-backend-crossterm`,
      );
    }
    if (name === "taffy" && pkg.name !== "arborui-layout") {
      violations.push(`${pkg.name} depends on taffy outside arborui-layout`);
    }
    if (
      pkg.name === "arborui-ui" &&
      [
        "arborui-backend-crossterm",
        "arborui-runtime",
        "arborui-terminal",
      ].includes(name)
    ) {
      violations.push(
        `arborui-ui has forbidden terminal/runtime dependency ${name}`,
      );
    }
    if (pkg.name === "arborui-runtime" && name === "arborui-widgets") {
      violations.push("arborui-runtime must not depend on arborui-widgets");
    }
  }
}

if (violations.length > 0) {
  process.stderr.write(
    `Architecture dependency check failed:\n${violations.map((violation) => `- ${violation}`).join("\n")}\n`,
  );
  process.exit(1);
}

console.log("Architecture dependency check passed.");

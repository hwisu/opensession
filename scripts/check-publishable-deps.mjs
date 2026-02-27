#!/usr/bin/env node

import { execFileSync } from "node:child_process";

function isPublishable(pkg) {
  if (pkg.publish === null) return true;
  if (Array.isArray(pkg.publish)) return pkg.publish.length > 0;
  return Boolean(pkg.publish);
}

function loadMetadata() {
  const raw = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    encoding: "utf8",
  });
  return JSON.parse(raw);
}

function main() {
  const metadata = loadMetadata();
  const workspaceMemberIds = new Set(metadata.workspace_members);
  const workspacePackages = metadata.packages.filter((pkg) => workspaceMemberIds.has(pkg.id));

  const publishByName = new Map(workspacePackages.map((pkg) => [pkg.name, isPublishable(pkg)]));
  const unpublishedWorkspaceNames = new Set(
    [...publishByName.entries()].filter(([, publish]) => !publish).map(([name]) => name),
  );

  const violations = [];
  for (const pkg of workspacePackages) {
    if (!isPublishable(pkg)) continue;
    for (const dep of pkg.dependencies) {
      // `cargo package` verification gate is driven by normal dependencies.
      if (dep.kind !== null) continue;
      if (!publishByName.has(dep.name)) continue;
      if (!unpublishedWorkspaceNames.has(dep.name)) continue;

      violations.push({
        packageName: pkg.name,
        dependencyName: dep.name,
        requirement: dep.req,
        kind: dep.kind ?? "normal",
      });
    }
  }

  if (violations.length > 0) {
    console.error(
      "[publishable-deps-check] FAIL: publishable workspace packages depend on publish=false packages:",
    );
    for (const violation of violations) {
      console.error(
        `  - ${violation.packageName} -> ${violation.dependencyName} (${violation.kind}, req ${violation.requirement})`,
      );
    }
    console.error(
      "\nFix by either publishing the dependency crate or removing/gating the dependency from publishable packages.",
    );
    process.exit(1);
  }

  console.log("[publishable-deps-check] OK");
}

main();

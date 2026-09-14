import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const contractPath = "contracts/palbeacon/usability-qa.v1.json";
const errors = [];

const readText = async (path) => readFile(resolve(root, path), "utf8");
const readJson = async (path) => JSON.parse(await readText(path));
const exists = async (path) => {
  try {
    await stat(resolve(root, path));
    return true;
  } catch {
    return false;
  }
};
const check = (condition, message) => {
  if (!condition) errors.push(message);
};

const contract = await readJson(contractPath);
check(contract.schema_version === 1, `${contractPath}: schema_version must be 1`);

const referencedPaths = [
  contract.authority.design,
  contract.authority.routes,
  contract.authority.skill,
  contract.authority.method,
  contract.tooling.test,
  ...contract.known_gaps.map(({ evidence }) => evidence),
];
for (const path of referencedPaths) {
  check(await exists(path), `Missing usability-contract path: ${path}`);
}

const packageJson = await readJson("package.json");
const uiPackage = await readJson("apps/palbeacon-ui/package.json");
check(
  packageJson.scripts?.["quality:usability-contract"] ===
    "node scripts/verify-palbeacon-usability-contract.mjs",
  "Workspace script quality:usability-contract is missing or incorrect",
);
check(
  packageJson.scripts?.["quality:usability"] ===
    "pnpm quality:usability-contract && pnpm ui:test:usability",
  "Workspace script quality:usability is missing or incorrect",
);
check(
  uiPackage.scripts?.["test:usability"] ===
    "pnpm build && node scripts/run-e2e.mjs e2e/usability.spec.ts",
  "UI script test:usability is missing or incorrect",
);
check(
  uiPackage.devDependencies?.["@playwright/test"] &&
    uiPackage.devDependencies?.["@axe-core/playwright"],
  "Usability QA requires the maintained Playwright and Axe packages",
);

const skill = await readText(contract.authority.skill);
check(skill.includes("@Browser"), "Usability skill must require the in-app @Browser");
check(skill.includes("Computer Use"), "Usability skill must explicitly exclude Computer Use");
check(
  skill.includes("pnpm quality:usability"),
  "Usability skill must document the usability quality gate",
);

const routes = await readJson(contract.authority.routes);
const implementedWebRoutes = routes.routes
  .filter(
    ({ availability, migration_status }) =>
      availability.includes("web") && migration_status === "implemented",
  )
  .map(({ path }) => path)
  .sort();
check(
  JSON.stringify([...contract.manual_route_coverage].sort()) ===
    JSON.stringify(implementedWebRoutes),
  "Manual usability coverage must match every implemented Web route",
);

const designContract = await readJson("contracts/palbeacon/design-qa.v1.json");
const availableProjects = new Set(designContract.viewports.map(({ project }) => project));
const journeyIds = contract.journeys.map(({ id }) => id);
check(new Set(journeyIds).size === journeyIds.length, "Usability journey IDs must be unique");
const journeyTest = await readText(contract.tooling.test);
for (const journey of contract.journeys) {
  check(journey.goal_ko.trim().length > 0, `Journey goal is empty: ${journey.id}`);
  check(
    Number.isSafeInteger(journey.max_actions) && journey.max_actions > 0,
    `Journey action budget is invalid: ${journey.id}`,
  );
  check(
    journey.projects.every((project) => availableProjects.has(project)),
    `Journey uses an unknown Playwright project: ${journey.id}`,
  );
  if (journey.automated) {
    check(
      journeyTest.includes(journey.id),
      `Automated journey is missing from the test: ${journey.id}`,
    );
  }
}
check(
  journeyTest.includes("usability-metrics.json"),
  "Journey tests must attach machine-readable usability metrics",
);

check(
  contract.thresholds.minimum_target_css_px === 24,
  "WCAG 2.2 minimum target threshold must remain 24 CSS px",
);
check(
  contract.thresholds.preferred_primary_target_css_px >= 44,
  "Primary touch target preference must be at least 44 CSS px",
);
check(contract.thresholds.reflow_width_css_px === 320, "Reflow width must remain 320 CSS px");
check(
  contract.tooling.visual_baseline_policy === "manual_review_only",
  "Visual and ARIA baselines must require manual review",
);

const gapIds = contract.known_gaps.map(({ id }) => id);
check(new Set(gapIds).size === gapIds.length, "Known-gap IDs must be unique");
for (const gap of contract.known_gaps) {
  for (const field of ["route", "viewport", "evidence", "smallest_fix_ko", "remove_when_ko"]) {
    check(String(gap[field] ?? "").trim().length > 0, `Known gap ${gap.id} is missing ${field}`);
  }
}

if (errors.length > 0) {
  console.error(`PalBeacon usability contract failed with ${errors.length} issue(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exitCode = 1;
} else {
  console.log(
    `PalBeacon usability contract passed: ${contract.journeys.length} task journeys, ${implementedWebRoutes.length} Web routes, ${contract.known_gaps.length} explicit known gaps.`,
  );
}

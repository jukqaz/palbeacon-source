import { readFile, readdir, stat } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const contractPath = "contracts/palbeacon/design-qa.v1.json";
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
  contract.authority.document,
  contract.stack.web_ui,
  contract.stack.windows_shell,
  contract.stack.tokens,
  contract.stack.runtime_tokens,
  contract.states.source,
  contract.route_audit.source,
  contract.route_audit.test,
  contract.visual_regression.decision_record,
  contract.browser_validation.route_test,
  ...contract.browser_validation.component_tests,
];
for (const path of referencedPaths) {
  check(await exists(path), `Missing design-contract path: ${path}`);
}

const design = await readText(contract.authority.document);
const start = design.indexOf(contract.authority.start_marker);
const end = design.indexOf(contract.authority.end_marker);
check(
  design.includes(contract.authority.history_heading),
  "DESIGN.md is missing the change-log heading",
);
check(start >= 0, "DESIGN.md is missing the canonical start marker");
check(end > start, "DESIGN.md canonical markers are missing or out of order");
const canonical = start >= 0 && end > start ? design.slice(start, end) : "";
for (const heading of [
  "## 1. 제품 인상",
  "## 4. 공통 셸",
  "## 12. 구현 및 검증 규칙",
  "## 13. 운영 route 디자인 검증 계약",
]) {
  check(canonical.includes(heading), `DESIGN.md canonical section missing: ${heading}`);
}
for (const stale of [
  "apps/pal-companion-widgetbook",
  "apps/pal-companion-win-ui",
  "Flutter 구현 방향",
  "Widgetbook 디자인 원장 계약",
  "Storybook 디자인 원장 계약",
]) {
  check(
    !canonical.includes(stale),
    `DESIGN.md canonical section still contains legacy text: ${stale}`,
  );
}

const tokenContract = await readJson(contract.stack.tokens);
const css = await readText(contract.stack.runtime_tokens);
const tokenNames = {
  void_black: "void",
  surface_raised: "surface-raised",
  surface_strong: "surface-strong",
  border_strong: "border-strong",
  text_soft: "text-soft",
  muted_strong: "muted-strong",
  accent_strong: "accent-strong",
  technology_normal: "tech-normal",
  technology_normal_surface: "tech-normal-surface",
  technology_ancient: "tech-ancient",
  technology_ancient_surface: "tech-ancient-surface",
};
for (const [name, value] of Object.entries(tokenContract.color)) {
  const cssName = tokenNames[name] ?? name.replaceAll("_", "-");
  const matcher = new RegExp(`--${cssName}\\s*:\\s*${value.replace("#", "#")}`, "iu");
  check(matcher.test(css), `CSS token --${cssName} does not match ${value}`);
}

const stateContract = await readJson(contract.states.source);
const actualStates = stateContract.states.map(({ id }) => id).sort();
const requiredStates = [...contract.states.required].sort();
check(
  JSON.stringify(actualStates) === JSON.stringify(requiredStates),
  "UI state contract and design QA state list differ",
);

const storybookPaths = [
  "apps/palbeacon-ui/.storybook/main.ts",
  "apps/palbeacon-ui/.storybook/preview.ts",
  "apps/palbeacon-ui/vitest.storybook.config.ts",
];
if (contract.browser_validation.storybook_forbidden === true) {
  for (const path of storybookPaths) {
    check(!(await exists(path)), `Removed Storybook surface still exists: ${path}`);
  }
  const uiPackage = await readJson("apps/palbeacon-ui/package.json");
  const packageEntries = {
    ...uiPackage.scripts,
    ...uiPackage.dependencies,
    ...uiPackage.devDependencies,
  };
  check(
    Object.keys(packageEntries).every((name) => !name.toLowerCase().includes("storybook")),
    "PalBeacon UI package still references Storybook",
  );
  const productFiles = await readdir(resolve(root, "apps/palbeacon-ui/src"), {
    recursive: true,
  });
  check(
    productFiles.every((path) => !path.endsWith(".stories.ts")),
    "PalBeacon UI source still contains Storybook stories",
  );
}

const playwright = await readText("apps/palbeacon-ui/playwright.config.ts");
for (const viewport of contract.viewports) {
  check(
    playwright.includes(`name: '${viewport.project}'`),
    `Playwright project missing: ${viewport.project}`,
  );
  const viewportPattern = new RegExp(
    `viewport:\\s*\\{\\s*width:\\s*${viewport.width},\\s*height:\\s*${viewport.height}\\s*\\}`,
    "u",
  );
  check(
    viewportPattern.test(playwright),
    `Playwright viewport missing: ${viewport.width}x${viewport.height}`,
  );
}

const routes = await readJson(contract.route_audit.source);
const routeTest = await readText(contract.route_audit.test);
check(
  JSON.stringify(routes.navigation.primary.map(({ label_ko }) => label_ko)) ===
    JSON.stringify(["홈", "지도", "도감", "계획", "내 게임"]),
  "Primary navigation does not match the lightweight menu contract",
);
check(
  JSON.stringify(routes.navigation.utilities.map(({ label_ko }) => label_ko)) ===
    JSON.stringify(["검색"]),
  "Utility navigation does not match the lightweight menu contract",
);
check(
  JSON.stringify(routes.navigation.primary.map(({ scope }) => scope)) ===
    JSON.stringify(["common", "common", "common", "common", "windows"]),
  "Primary navigation does not separate common and Windows scopes",
);
const secondaryLabels = Object.fromEntries(
  routes.navigation.secondary.map((group) => [
    group.group,
    group.items.map(({ label_ko }) => label_ko),
  ]),
);
check(
  JSON.stringify(secondaryLabels.catalog) ===
    JSON.stringify(["팰", "아이템", "스킬", "기술", "건축물"]),
  "Catalog navigation does not match the five-destination contract",
);
check(
  JSON.stringify(secondaryLabels.plan) === JSON.stringify(["팀 구성", "교배", "팰 비교", "재료"]),
  "Plan navigation does not match the four-destination contract",
);
check(
  JSON.stringify(secondaryLabels.pc) ===
    JSON.stringify(["내 데이터", "서버", "오버레이", "앱 설정"]),
  "Windows navigation does not match the four-destination contract",
);
const auditedRoutes = routes.routes.filter(
  (route) =>
    route.availability.includes(contract.route_audit.platform) &&
    route.migration_status === contract.route_audit.migration_status,
);
for (const route of auditedRoutes) {
  check(
    routeTest.includes(`path: '${route.path}'`),
    `Web route is missing from the menu audit: ${route.path}`,
  );
}

const ruleIds = contract.presentation_rules.map(({ id }) => id);
check(new Set(ruleIds).size === ruleIds.length, "Presentation rule IDs must be unique");
for (const rule of contract.presentation_rules) {
  check(
    ["P0", "P1", "P2"].includes(rule.severity),
    `Invalid severity for presentation rule: ${rule.id}`,
  );
  check(
    rule.rule.trim().length > 0 && rule.enforcement.trim().length > 0,
    `Incomplete presentation rule: ${rule.id}`,
  );
}
check(
  contract.visual_regression.update_policy === "manual_review_only",
  "Visual baseline updates must require manual review",
);

if (errors.length > 0) {
  console.error(`PalBeacon design contract failed with ${errors.length} issue(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exitCode = 1;
} else {
  console.log(
    `PalBeacon design contract passed: ${auditedRoutes.length} Web routes, ${contract.browser_validation.component_tests.length} component suites, ${contract.viewports.length} browser projects, ${requiredStates.length} data states, ${ruleIds.length} presentation rules.`,
  );
}

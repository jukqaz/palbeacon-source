import { mkdirSync } from "node:fs";
import { resolve } from "node:path";

const overlayArtifacts = resolve(process.cwd(), "test-results/overlay");
const platformArtifacts = resolve(process.cwd(), "test-results/navigation");

describe("PalBeacon Windows shell", () => {
  it("UQ-WINDOWS-NAV-01 separates common and PC-only navigation", async () => {
    mkdirSync(platformArtifacts, { recursive: true });
    await browser.setWindowSize(1440, 900);
    const mainWindow = await browser.getWindowHandle();
    await browser.switchToWindow(mainWindow);

    const hydrated = await $('[data-palbeacon-hydrated="true"]');
    await hydrated.waitForExist({ timeout: 20_000 });

    const brand = await $('a[aria-label="PalBeacon 홈"]');
    await brand.waitForDisplayed({ timeout: 20_000 });
    await expect(brand).toBeDisplayed();

    const commonNavigation = await $('nav[aria-label="주요 메뉴"]');
    const pcNavigation = await $('nav[aria-label="PC 메뉴"]');
    await expect(commonNavigation).toBeDisplayed();
    await expect(pcNavigation).toBeDisplayed();

    const privateGameLink = await $('nav[aria-label="PC 메뉴"] a[href*="/connection/profile"]');
    await expect(privateGameLink).toBeDisplayed();
    await expect(privateGameLink).toHaveText("내 게임");
    await expect(
      await $('nav[aria-label="주요 메뉴"] a[href*="/connection/profile"]'),
    ).not.toExist();
    await expect(await $('nav[aria-label="보조 메뉴"]')).not.toExist();
    const homeSearchLabel = await $('label[for="home-search"]');
    expect((await homeSearchLabel.getCSSProperty("clip-path")).value).toBe("inset(50%)");
    await browser.saveScreenshot(resolve(platformArtifacts, "windows-common-and-pc-menu.png"));

    const catalogLink = await $('nav[aria-label="주요 메뉴"] a[href*="/pals"]');
    await expect(catalogLink).toBeDisplayed();
    await expect(catalogLink).toBeClickable();
    await catalogLink.click();

    const catalogHeading = await $("h1=팰 도감");
    await expect(catalogHeading).toBeDisplayed();
    await expect(browser).toHaveUrl(expect.stringMatching(/\/pals\/?$/));

    const commonSearchLink = await $('nav[aria-label="보조 메뉴"] a[href*="/search"]');
    await expect(commonSearchLink).toBeDisplayed();
    await expect(await $(".global-search")).not.toExist();
    await commonSearchLink.click();
    await expect(await $("h1=통합 검색")).toBeDisplayed();
    await expect(await $('nav[aria-label="보조 메뉴"]')).not.toExist();

    await $('nav[aria-label="PC 메뉴"] a[href*="/connection/profile"]').click();
    await expect(await $("h1=내 데이터")).toBeDisplayed();
    const personalDataText = await $("body").getText();
    expect(personalDataText).not.toContain("현재 연결");
    expect(personalDataText).not.toContain("현재 위치를 사용할 수 없어 지도만 표시합니다.");
    await browser.saveScreenshot(resolve(platformArtifacts, "windows-pc-data.png"));

    const pcSubmenu = await $('nav[aria-label="내 게임 세부 메뉴"]');
    await expect(pcSubmenu).toBeDisplayed();
    const settingsLink = await $('nav[aria-label="내 게임 세부 메뉴"] a[href*="/settings"]');
    await expect(settingsLink).toBeDisplayed();
    await settingsLink.click();
    await expect(await $("h1=앱 설정")).toBeDisplayed();
    await expect(browser).toHaveUrl(expect.stringMatching(/\/settings\/?$/));
    await browser.saveScreenshot(resolve(platformArtifacts, "windows-pc-settings.png"));
  });

  it("renders and operates the minimap-focused overlay workshop", async () => {
    mkdirSync(overlayArtifacts, { recursive: true });
    await browser.setWindowSize(1440, 900);

    const privateGameLink = await $('nav[aria-label="PC 메뉴"] a[href*="/connection/profile"]');
    await privateGameLink.waitForClickable();
    await privateGameLink.click();

    const overlayLink = await $(
      'nav[aria-label="내 게임 세부 메뉴"] a[href*="/connection/overlay"]',
    );
    await overlayLink.waitForClickable();
    await overlayLink.click();

    const heading = await $("h1=게임 오버레이");
    await heading.waitForDisplayed();
    await expect(await $('[aria-labelledby="preview-title"]')).toBeDisplayed();
    await expect(await $('[role="slider"][aria-label="지도 크기"]')).toExist();
    await expect(await $('[role="slider"][aria-label="지도 선명도"]')).toExist();

    const documentText = await $("body").getText();
    expect(documentText).not.toContain("px");
    expect(documentText).not.toContain("%");
    expect(documentText).not.toContain("FPS");
    expect(documentText).not.toContain("입력 모드");

    await browser.execute(() => window.scrollTo(0, 0));
    await browser.saveScreenshot(resolve(overlayArtifacts, "overlay-desktop-1440x900.png"));

    const headingUp = await $('input[type="radio"][value="heading_up"]');
    await headingUp.click();
    const applyButton = await $("button=적용");
    await expect(applyButton).toBeEnabled();
    await browser.saveScreenshot(resolve(overlayArtifacts, "overlay-heading-up-1440x900.png"));

    const northUp = await $('input[type="radio"][value="north_up"]');
    await northUp.click();

    await browser.setWindowSize(1024, 720);
    const compactSize = await browser.getWindowSize();
    expect(compactSize.width).toBe(1024);
    expect(compactSize.height).toBe(720);
    await browser.execute(() => window.scrollTo(0, 0));
    await browser.saveScreenshot(resolve(overlayArtifacts, "overlay-compact-1024x720.png"));

    const workshop = await $('[aria-labelledby="workshop-title"]');
    await workshop.scrollIntoView({ block: "start" });
    await browser.saveScreenshot(
      resolve(overlayArtifacts, "overlay-compact-workshop-1024x720.png"),
    );

    await browser.execute(() => {
      document.documentElement.style.fontSize = "200%";
      window.scrollTo(0, 0);
    });
    const hasHorizontalOverflow = await browser.execute(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    );
    expect(hasHorizontalOverflow).toBe(false);
    await browser.saveScreenshot(
      resolve(overlayArtifacts, "overlay-compact-text-200-1024x720.png"),
    );
    await browser.execute(() => {
      document.documentElement.style.removeProperty("font-size");
    });
  });
});

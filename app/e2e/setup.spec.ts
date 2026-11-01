import { expect, test } from "@playwright/test";

test.describe("?demo=setup", () => {
  test("skips the wizard and reopens it from the capture banner", async ({
    page,
  }) => {
    await page.goto("/?demo=setup");

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole("heading", { name: "启用本地 OCR 引擎" }),
    ).toBeVisible();
    await expect(
      dialog.getByRole("button", { name: /下载模型/ }),
    ).toBeEnabled();

    await dialog.getByRole("button", { name: "稍后设置" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);

    // The blocking state still offers a primary exit on the capture page.
    await expect(
      page.getByRole("heading", { name: "先下载 OCR 模型" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "下载模型", exact: true })
      .first()
      .click();
    await expect(
      page
        .getByRole("dialog")
        .getByRole("heading", { name: "启用本地 OCR 引擎" }),
    ).toBeVisible();
  });

  test("starts the download, shows progress and keeps the resume hint on cancel", async ({
    page,
  }) => {
    await page.goto("/?demo=setup");

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: /下载模型/ }).click();

    await expect(
      dialog.getByRole("heading", { name: "正在下载模型" }),
    ).toBeVisible();
    await expect(dialog.getByRole("progressbar")).toBeVisible();

    await dialog.getByRole("button", { name: "取消下载" }).click();
    await expect(dialog.getByText("已取消，下次会接着下。")).toBeVisible();
  });
});

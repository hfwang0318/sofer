import { expect, test } from "@playwright/test";

test.describe("?demo=permission", () => {
  test("explains the permission and keeps exit paths available", async ({
    page,
  }) => {
    await page.goto("/?demo=permission");

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole("heading", { name: "允许 sofer 录制屏幕" }),
    ).toBeVisible();
    await expect(
      dialog.getByText("隐私与安全性 → 屏幕录制。"),
    ).toBeVisible();

    // "Later" dismisses the modal but leaves the blocking banner and its exit.
    await dialog.getByRole("button", { name: "稍后再说" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByRole("heading", { name: "先允许录制屏幕" }),
    ).toBeVisible();
    const grantButtons = page.getByRole("button", { name: "去授权" });
    await expect(grantButtons).toHaveCount(2);

    await grantButtons.first().click();
    await expect(
      page.getByRole("dialog").getByRole("heading", {
        name: "允许 sofer 录制屏幕",
      }),
    ).toBeVisible();

    // Granting switches the dialog to its ready branch instead of dead-ending.
    await page.getByRole("dialog").getByRole("button", { name: "授权" }).click();
    await expect(
      page.getByRole("dialog").getByRole("heading", { name: "权限已开启" }),
    ).toBeVisible();
  });
});

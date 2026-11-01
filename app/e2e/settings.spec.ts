import { expect, test } from "@playwright/test";

const SETTINGS_KEY = "sofer.settings.v1";

test.describe("?demo=settings", () => {
  test("shows the save bar after an edit and clears it after saving", async ({
    page,
  }) => {
    await page.goto("/?demo=settings");

    await expect(
      page.getByRole("heading", { level: 1, name: "设置" }),
    ).toBeVisible();
    await expect(page.getByText("已保存", { exact: true })).toBeVisible();

    await page.getByRole("switch", { name: "自动复制" }).click();
    await expect(
      page.getByText("有未保存的更改", { exact: true }),
    ).toBeVisible();
    const save = page.getByRole("button", { name: "保存更改" });
    await expect(save).toBeEnabled();

    await save.click();
    await expect(page.getByText("已保存", { exact: true })).toBeVisible();
    await expect(save).toBeDisabled();

    const stored = await page.evaluate((key) => {
      const raw = localStorage.getItem(key);
      return raw ? JSON.parse(raw) : null;
    }, SETTINGS_KEY);
    expect(stored?.autoCopy).toBe(false);
  });
});

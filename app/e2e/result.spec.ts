import { expect, test } from "@playwright/test";

test.describe("?demo=result", () => {
  test("shows the result, line count, duration and copy action", async ({
    page,
  }) => {
    await page.goto("/?demo=result");

    await expect(
      page.getByRole("heading", { level: 1, name: "截图" }),
    ).toBeVisible();

    // The selected SQL line only exists in the result text, not the hero art.
    await expect(
      page.getByText(
        'SELECT id, email, created_at FROM users WHERE id = $1',
      ),
    ).toBeVisible();

    await expect(page.getByText("10 行", { exact: true })).toBeVisible();
    await expect(page.getByText("864 毫秒", { exact: true })).toBeVisible();

    await page.getByRole("button", { name: "复制", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "已复制", exact: true }),
    ).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => navigator.clipboard.readText()))
      .toContain("SELECT id, email, created_at");
  });
});

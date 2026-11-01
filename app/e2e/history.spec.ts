import { expect, test } from "@playwright/test";

test.describe("?demo=history", () => {
  test("lists the demo entries and confirms destructive actions", async ({
    page,
  }) => {
    await page.goto("/?demo=history");

    await expect(
      page.getByRole("heading", { level: 1, name: "历史" }),
    ).toBeVisible();
    await expect(page.getByRole("article")).toHaveCount(3);
    await expect(page.getByText("cargo test --workspace")).toBeVisible();

    await page.getByRole("button", { name: "删除这条记录" }).first().click();
    await expect(page.getByRole("article")).toHaveCount(2);

    // The first click only arms the confirmation; nothing is cleared yet.
    await page.getByRole("button", { name: "清空记录" }).click();
    const confirm = page.getByRole("button", { name: "再点一次清空" });
    await expect(confirm).toBeVisible();
    await expect(page.getByRole("article")).toHaveCount(2);

    await confirm.click();
    await expect(
      page.getByRole("heading", { name: "还没有记录" }),
    ).toBeVisible();
    await expect(page.getByText("历史记录已清空")).toBeVisible();
  });

  test("filters by text and restores the full list", async ({ page }) => {
    await page.goto("/?demo=history");
    const search = page.getByRole("searchbox", { name: "搜索历史" });

    await search.fill("cargo");
    await expect(page.getByRole("article")).toHaveCount(1);
    await expect(page.getByText("cargo test --workspace")).toBeVisible();

    await search.fill("no-such-entry");
    await expect(page.getByRole("article")).toHaveCount(0);
    await expect(page.getByText(/换个关键词/)).toBeVisible();

    await page.getByRole("button", { name: "清空搜索" }).first().click();
    await expect(page.getByRole("article")).toHaveCount(3);
  });
});

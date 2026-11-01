import { expect, test } from "@playwright/test";

test.describe("?demo=error", () => {
  test("shows the cause, the next step and both recovery actions", async ({
    page,
  }) => {
    await page.goto("/?demo=error");

    const alert = page.getByRole("alert");
    await expect(alert).toBeVisible();
    await expect(
      alert.getByRole("heading", { name: "识别没有完成" }),
    ).toBeVisible();
    await expect(
      alert.getByText("本地 OCR 引擎没有响应。"),
    ).toBeVisible();
    await expect(
      alert.getByText(/稍等几秒再试，或到设置里检查 OCR 引擎状态。/),
    ).toBeVisible();

    // First-layer copy must not leak error codes.
    await expect(alert).not.toContainText(/\bE_[A-Z]/);

    await expect(
      alert.getByRole("button", { name: "重试截图" }),
    ).toBeVisible();
    await alert.getByRole("button", { name: "检查设置" }).click();
    await expect(
      page.getByRole("heading", { level: 1, name: "设置" }),
    ).toBeVisible();
  });
});

import { expect, test } from '@playwright/test';

test('landing lists all five personas', async ({ page }) => {
  await page.goto('/');
  for (const p of ['clinician', 'prior-auth', 'steward', 'patient', 'audit']) {
    await expect(page.getByTestId(`persona-${p}`)).toBeVisible();
  }
});

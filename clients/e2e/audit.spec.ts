import { expect, test } from '@playwright/test';

test.describe('audit', () => {
  test('KPIs + ledger render and filter works', async ({ page }) => {
    await page.goto('/audit');
    await expect(page.getByText('events in window')).toBeVisible();
    await page.getByTestId('filter-export').click();
    await expect(page.getByTestId('audit-row-x1')).toBeVisible();
    await page.getByTestId('audit-row-x1').click();
    await expect(page.getByText('Export after revocation')).toBeVisible();
  });

  test('link-decision shows §5.5 step evaluation', async ({ page }) => {
    await page.goto('/audit');
    await page.getByTestId('filter-linkdecision').click();
    await page.getByTestId('audit-row-ld1').click();
    await expect(page.getByText(/Link-chain evaluation/)).toBeVisible();
    await expect(page.getByText(/BLOCKED/)).toBeVisible();
  });

  test('report modal opens and closes', async ({ page }) => {
    await page.goto('/audit');
    await page.getByTestId('generate-report').click();
    await expect(page.getByText('Compliance report — review window')).toBeVisible();
    await page.getByTestId('modal-confirm').click();
    await expect(page.getByTestId('toast')).toContainText(/Report generated/);
  });
});

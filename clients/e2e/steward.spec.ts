import { expect, test } from '@playwright/test';

test.describe('steward', () => {
  test('queue + case detail renders link policy', async ({ page }) => {
    await page.goto('/steward');
    await expect(page.getByTestId('case-c1')).toContainText('Robert Chen');
    await expect(page.getByText(/Link policy/)).toBeVisible();
    await expect(page.getByText(/Deny-by-default/)).toBeVisible();
  });

  test('blocked-link case shows BLOCKED stamp', async ({ page }) => {
    await page.goto('/steward');
    await page.getByTestId('case-c5').click();
    await expect(page.getByRole('heading', { name: /BluePeak/i })).toBeVisible();
    await expect(page.getByText(/chain blocked/)).toBeVisible();
  });

  test('contest action appends a fresh DAG node', async ({ page }) => {
    await page.goto('/steward');
    await page.getByTestId('case-c1').click();
    await page.getByTestId('steward-action-1').click(); // Reject — different people
    await page.getByTestId('modal-confirm').click();
    await expect(page.getByTestId('toast')).toContainText(/Contest recorded/);
  });
});

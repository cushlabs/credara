import { expect, test } from '@playwright/test';

test('prior-auth flow advances CRD → DTR → PAS → Decision', async ({ page }) => {
  await page.goto('/prior-auth');
  await expect(page.getByText(/Coverage requirements/)).toBeVisible();
  await page.getByTestId('start-dtr').click();
  await page.getByTestId('gap-red').selectOption('None');
  await page.getByTestId('gap-just').fill('Persistent radicular symptoms despite 8 weeks of PT.');
  await page.getByTestId('attest-box').check();
  await page.getByTestId('submit-pas').click();
  await expect(page.getByText(/Sent to BlueChoice PPO/)).toBeVisible();
  await expect(page.getByText('APPROVED')).toBeVisible({ timeout: 6000 });
  await page.getByTestId('toggle-receipt').click();
  await expect(page.getByText('event_type')).toBeVisible();
});

test('generic order short-circuits at CRD with no auth required', async ({ page }) => {
  await page.goto('/prior-auth');
  await page.getByTestId('order-pick').selectOption('generic');
  await expect(page.getByText('No prior authorization required')).toBeVisible();
});

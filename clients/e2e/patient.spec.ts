import { expect, test } from '@playwright/test';

test.describe('patient consent', () => {
  test('lists active grants and revokes one', async ({ page }) => {
    await page.goto('/patient');
    await expect(page.getByTestId('grant-g-mercy')).toContainText('Mercy General Hospital');
    await page.getByTestId('grant-stop-g-northside').click();
    await page.getByTestId('modal-confirm').click();
    await expect(page.getByTestId('toast')).toContainText(/Sharing stopped/);
  });

  test('share tab grants a new institution', async ({ page }) => {
    await page.goto('/patient');
    await page.getByTestId('tab-share').click();
    await page.getByTestId('share-who').fill('Lakeside Hospital');
    await page.getByTestId('share-authorize').click();
    await page.getByTestId('modal-confirm').click();
    await expect(page.getByTestId('toast')).toContainText(/Access granted/);
  });
});

import { expect, test } from '@playwright/test';

test.describe('clinician', () => {
  test('worklist renders and patient detail loads', async ({ page }) => {
    await page.goto('/clinician');
    await expect(page.getByRole('heading', { name: /patient identity worklist/i })).toBeVisible();
    await expect(page.getByTestId('patient-card-p1')).toContainText('Maria Gonzalez');
    await page.getByTestId('patient-card-p2').click();
    await expect(page.getByRole('heading', { name: 'James Whitfield' })).toBeVisible();
    await expect(page.getByText(/Conflicting DOB/)).toBeVisible();
  });

  test('attest action records to the action log', async ({ page }) => {
    await page.goto('/clinician');
    await page.getByTestId('patient-card-p3').click();
    await page.getByTestId('challenge-opt-0').click(); // "Yes — same person"
    await page.getByTestId('modal-confirm').click();
    await expect(page.getByTestId('toast')).toContainText('Attest recorded');
    await expect(page.getByText('Actions taken this visit')).toBeVisible();
  });
});

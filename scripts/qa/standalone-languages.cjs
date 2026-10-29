// Standalone: every new exercise asks each role for its language in its own tab
// (the scenario's language is the highlighted proposal). QA scripts confirm all
// roles before pressing the EXCON start button; a resumed session asks nothing.

/** Confirm each role's language (`overrides[role]`, else the proposed one), then return to the EXCON tab. */
async function confirmLanguages(page, overrides = {}) {
  const roles = await page.locator('.role-tabs [data-role]').evaluateAll(tabs => tabs.map(t => t.dataset.role));
  for (const role of roles.filter(r => r !== 'master')) {
    await page.locator(`.role-tabs [data-role="${role}"]`).click();
    const gate = page.locator(`[data-panel="${role}"] #language-gate`);
    try { await gate.waitFor({ timeout: 5000 }); } catch { continue; }
    const lang = overrides[role];
    const choice = lang ? gate.locator(`[data-lang="${lang}"]`) : gate.locator('button.primary');
    await choice.first().click();
    await gate.waitFor({ state: 'detached', timeout: 10000 });
  }
  await page.locator('.role-tabs [data-role="master"]').click();
}

module.exports = { confirmLanguages };

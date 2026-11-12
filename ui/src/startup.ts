import './tokens.css';
import { readThemePreference, resolveThemePreference } from './lib/appearance';

document.documentElement.dataset.theme = resolveThemePreference(
  readThemePreference(),
  window.matchMedia('(prefers-color-scheme: light)').matches
);

async function loadWorkspace() {
  try {
    await import('./main');
  } catch (error) {
    const status = document.getElementById('startup-status');
    if (status) status.textContent = 'Could not start Catabase. Quit and reopen the app.';
    console.error('Could not start Catabase', error);
  }
}

// Let the native window display the lightweight startup page before loading Monaco.
if (document.readyState === 'complete') void loadWorkspace();
else window.addEventListener('load', loadWorkspace, { once: true });

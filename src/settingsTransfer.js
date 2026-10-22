import { openSettingsDialog, readTextFile, saveSettingsDialog, writeTextFile } from './tauriApi';

const EXPORT_KIND = 'libregene-settings';

/**
 * Dump every localStorage entry (all user preferences, My Primers, My
 * Enzymes, recent files, plugin settings, ...) to a JSON file chosen via a
 * save dialog. Returns the written path, or null when cancelled.
 */
export async function exportUserSettings() {
  const path = await saveSettingsDialog();
  if (!path) return null;
  const settings = {};
  for (let i = 0; i < localStorage.length; i += 1) {
    const key = localStorage.key(i);
    settings[key] = localStorage.getItem(key);
  }
  const { getVersion } = await import('@tauri-apps/api/app');
  const payload = {
    kind: EXPORT_KIND,
    appVersion: await getVersion(),
    exportedAt: new Date().toISOString(),
    settings,
  };
  const res = await writeTextFile(path, JSON.stringify(payload, null, 2));
  if (res?.error) throw new Error(res.error);
  return path;
}

/**
 * Pick an export file and validate it. Returns validated [key, value] entries,
 * or null when cancelled; throws on unreadable or malformed files.
 */
export async function readSettingsImport() {
  const path = await openSettingsDialog();
  if (!path) return null;
  const res = await readTextFile(path);
  if (res?.error) throw new Error(res.error);
  let data;
  try {
    data = JSON.parse(res?.contents ?? '');
  } catch {
    throw new Error('The selected file is not valid JSON');
  }
  if (
    !data ||
    data.kind !== EXPORT_KIND ||
    !data.settings ||
    typeof data.settings !== 'object' ||
    Array.isArray(data.settings)
  ) {
    throw new Error('The selected file is not a LibreGene settings export');
  }
  return Object.entries(data.settings).filter(([, v]) => typeof v === 'string');
}

/** Write the imported entries into localStorage and reload to apply them. */
export function applySettingsImport(entries) {
  for (const [key, value] of entries) {
    localStorage.setItem(key, value);
  }
  window.location.reload();
}

import { useCallback, useEffect, useState } from 'react';
import { isTauri, openUrl } from '../tauriApi';
import { useTranslation } from '../i18n';

const MANIFEST_URL = 'https://libregene.donglin.li/latest.json';
const RELEASES_URL = 'https://github.com/dl-li/LibreGene/releases/latest';
const CHECK_INTERVAL = 24 * 60 * 60 * 1000;
const IS_LINUX = typeof navigator !== 'undefined' && /Linux/i.test(navigator.platform);

export function compareVersions(a, b) {
  const parse = (value) => {
    const match = String(value || '')
      .trim()
      .replace(/^v/, '')
      .match(/^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/);
    if (!match) return null;
    return {
      numbers: match.slice(1, 4).map(Number),
      prerelease: match[4]?.split('.') || [],
    };
  };
  const left = parse(a);
  const right = parse(b);
  if (!left || !right) return 0;
  for (let i = 0; i < 3; i += 1) {
    if (left.numbers[i] !== right.numbers[i]) return left.numbers[i] > right.numbers[i] ? 1 : -1;
  }
  if (!left.prerelease.length || !right.prerelease.length) {
    if (left.prerelease.length === right.prerelease.length) return 0;
    return left.prerelease.length ? -1 : 1;
  }
  for (let i = 0; i < Math.max(left.prerelease.length, right.prerelease.length); i += 1) {
    const l = left.prerelease[i];
    const r = right.prerelease[i];
    if (l === undefined) return -1;
    if (r === undefined) return 1;
    if (l === r) continue;
    const ln = /^\d+$/.test(l);
    const rn = /^\d+$/.test(r);
    if (ln && rn) return Number(l) > Number(r) ? 1 : -1;
    if (ln !== rn) return ln ? -1 : 1;
    return l > r ? 1 : -1;
  }
  return 0;
}

function readAutoCheck() {
  try {
    const value = JSON.parse(localStorage.getItem('autoCheckUpdates'));
    return value === null ? true : !!value;
  } catch {
    return true;
  }
}

function errorText(error) {
  return error instanceof Error ? error.message : String(error);
}

export default function useUpdater({ hasUnsavedProjects = false } = {}) {
  const { t } = useTranslation();
  const [autoCheck, setAutoCheckState] = useState(readAutoCheck);
  const [currentVersion, setCurrentVersion] = useState('');
  const [state, setState] = useState({
    status: 'idle',
    update: null,
    error: '',
    downloaded: 0,
    total: 0,
  });

  const setAutoCheck = useCallback((enabled) => {
    const next = !!enabled;
    setAutoCheckState(next);
    try {
      localStorage.setItem('autoCheckUpdates', JSON.stringify(next));
    } catch {
      // The in-memory preference still applies.
    }
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    import('@tauri-apps/api/app')
      .then(({ getVersion }) => getVersion())
      .then(setCurrentVersion)
      .catch(() => {});
  }, []);

  const checkForUpdates = useCallback(async () => {
    if (!isTauri) {
      setState((prev) => ({ ...prev, status: 'error', error: 'Updates require the desktop app' }));
      return;
    }
    setState((prev) => ({ ...prev, status: 'checking', error: '' }));
    try {
      const { getVersion } = await import('@tauri-apps/api/app');
      const version = await getVersion();
      setCurrentVersion(version);
      if (IS_LINUX) {
        const response = await fetch(MANIFEST_URL, {
          cache: 'no-store',
          signal: window.AbortSignal.timeout(30000),
        });
        if (!response.ok) throw new Error(`Update server returned HTTP ${response.status}`);
        const manifest = await response.json();
        const available = compareVersions(manifest.version, version) > 0;
        setState({
          status: available ? 'available' : 'latest',
          update: available
            ? { version: manifest.version, body: manifest.notes || '', date: manifest.pub_date }
            : null,
          error: '',
          downloaded: 0,
          total: 0,
        });
        return;
      }
      const { check } = await import('@tauri-apps/plugin-updater');
      const result = await check({ timeout: 30000 });
      setState({
        status: result ? 'available' : 'latest',
        update: result
          ? {
              version: result.version,
              body: result.body || '',
              date: result.date,
              raw: result,
            }
          : null,
        error: '',
        downloaded: 0,
        total: 0,
      });
    } catch (error) {
      setState((prev) => ({
        ...prev,
        status: prev.update ? 'available' : 'error',
        error: errorText(error),
      }));
    }
  }, []);

  useEffect(() => {
    if (!isTauri || !autoCheck) return undefined;
    const initial = setTimeout(checkForUpdates, 1500);
    const interval = window.setInterval(checkForUpdates, CHECK_INTERVAL);
    return () => {
      clearTimeout(initial);
      window.clearInterval(interval);
    };
  }, [autoCheck, checkForUpdates]);

  const downloadAndInstall = useCallback(async () => {
    const update = state.update;
    if (!update) return;
    if (IS_LINUX) {
      await openUrl(RELEASES_URL);
      return;
    }
    if (hasUnsavedProjects) {
      const { message } = await import('@tauri-apps/plugin-dialog');
      await message(t('Save all open projects before installing the update.'), {
        title: t('Unsaved changes'),
        kind: 'warning',
      });
      return;
    }
    if (!update.raw) return;
    setState((prev) => ({
      ...prev,
      status: 'downloading',
      error: '',
      downloaded: 0,
      total: 0,
    }));
    try {
      await update.raw.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          setState((prev) => ({ ...prev, total: event.data.contentLength || 0 }));
        } else if (event.event === 'Progress') {
          setState((prev) => ({ ...prev, downloaded: prev.downloaded + event.data.chunkLength }));
        } else if (event.event === 'Finished') {
          setState((prev) => ({ ...prev, status: 'installing' }));
        }
      });
      setState((prev) => ({ ...prev, status: 'installing' }));
      const { relaunch } = await import('@tauri-apps/plugin-process');
      await relaunch();
    } catch (error) {
      setState((prev) => ({ ...prev, status: 'available', error: errorText(error) }));
    }
  }, [hasUnsavedProjects, state.update, t]);

  return {
    autoCheck,
    setAutoCheck,
    currentVersion,
    ...state,
    isLinux: IS_LINUX,
    checkForUpdates,
    downloadAndInstall,
  };
}

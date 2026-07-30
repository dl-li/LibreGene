/**
 * 最近打开文件的 localStorage 持久化。
 *
 * 存一个路径数组，最新的在最前，去重，最多 MAX 项。
 * 仅在 Tauri 桌面环境用（路径是文件系统绝对路径）。
 */

const STORAGE_KEY = 'recentFiles';
const MAX = 10;

function readList() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const arr = raw ? JSON.parse(raw) : [];
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

function writeList(list) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch {
    /* storage disabled / quota — ignore */
  }
}

export function getRecentFiles() {
  return readList();
}

/** 把一个路径加到列表最前（去重）。返回新列表。 */
export function addRecentFile(path) {
  if (!path) return readList();
  const next = [path, ...readList().filter((p) => p !== path)].slice(0, MAX);
  writeList(next);
  return next;
}

export function removeRecentFile(path) {
  const next = readList().filter((p) => p !== path);
  writeList(next);
  return next;
}

export function clearRecentFiles() {
  writeList([]);
  return [];
}

/** 最后打开的一个路径，或 null。 */
export function getLastOpenedFile() {
  const list = readList();
  return list.length > 0 ? list[0] : null;
}

/** 从路径里取短文件名用于展示。 */
export function fileNameOf(path) {
  if (!path) return '';
  const s = String(path).replace(/\\/g, '/');
  return s.split('/').pop() || s;
}

/** 取路径所在目录（用平台分隔符返回，给原生文件对话框当 defaultPath）。 */
export function dirOf(path) {
  if (!path) return '';
  const norm = String(path).replace(/\\/g, '/');
  const idx = norm.lastIndexOf('/');
  return idx > 0 ? norm.slice(0, idx) : '';
}

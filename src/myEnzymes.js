/**
 * My Enzymes — 本地酶库的 localStorage 持久化。
 *
 * 存一个酶名数组（去重、trim）。导入/导出格式：纯文本，逗号分隔。
 * 例如 "AfeI, AgeI, ApaI, AscI, AseI"。
 */

const STORAGE_KEY = 'myEnzymes';
const MAX = 200;

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

export function getMyEnzymes() {
  return readList();
}

/** 从纯文本解析酶名列表：支持逗号 / 换行 / 分号分隔。 */
export function parseEnzymeText(text) {
  if (!text) return [];
  return [
    ...new Set(
      String(text)
        .split(/[,;\n\r]+/)
        .map((s) => s.trim())
        .filter(Boolean),
    ),
  ];
}

export function setMyEnzymes(list) {
  const next = [...new Set((list || []).map((s) => String(s).trim()).filter(Boolean))].slice(
    0,
    MAX,
  );
  writeList(next);
  return next;
}

export function addMyEnzymes(list) {
  return setMyEnzymes([...readList(), ...(list || [])]);
}

export function removeMyEnzyme(name) {
  const next = readList().filter((e) => e !== name);
  writeList(next);
  return next;
}

export function clearMyEnzymes() {
  writeList([]);
  return [];
}

/** 导出文本：逗号分隔。 */
export function exportEnzymeText(list) {
  return (list || []).join(', ');
}

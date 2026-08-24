/**
 * My Primers — 本地引物库的 localStorage 持久化。
 *
 * 序列是引物唯一的标识符：同一序列（忽略大小写）只保留一条记录，
 * 但该记录可携带多个名称（来自不同文件/命名）。名称不作为去重依据——
 * 不同序列可以同名，仅在展示时提示重名。
 *
 * 记录格式：{ id, seq, names: [name, ...], type, color }
 */

const STORAGE_KEY = 'myPrimers';
const MAX = 500;

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

function makeId() {
  return `myprimer_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
}

/** 归一化旧格式 {id,name,primerSeq,type} 记录为新格式。 */
function normalize(entry) {
  if (entry && entry.seq) return entry;
  return {
    id: entry?.id || makeId(),
    seq: String(entry?.primerSeq || entry?.seq || '').toUpperCase(),
    names: entry?.names?.length ? [...entry.names] : [entry?.name || 'Unnamed'],
    type: entry?.type === 'rev' ? 'rev' : 'fwd',
  };
}

export function getMyPrimers() {
  return readList().map(normalize);
}

/**
 * 追加一组引物到库。
 * 序列相同（忽略大小写）→ 合并到已有记录，仅追加缺失的名称；
 * 序列不同 → 新建记录（即使名称与其他记录相同也保留）。
 */
export function addMyPrimers(primers) {
  if (!primers || !primers.length) return getMyPrimers();
  const next = getMyPrimers();
  for (const p of primers) {
    if (!p || (!p.primerSeq && !p.seq)) continue;
    const seq = String(p.primerSeq || p.seq).toUpperCase();
    const name = p.name || 'Unnamed';
    const existing = next.find((e) => e.seq === seq);
    if (existing) {
      if (name && !existing.names.includes(name)) existing.names.push(name);
    } else {
      next.push({
        id: p.id || makeId(),
        seq,
        names: [name],
        type: p.type === 'rev' ? 'rev' : 'fwd',
      });
    }
  }
  const trimmed = next.slice(0, MAX);
  writeList(trimmed);
  return trimmed;
}

export function removeMyPrimer(id) {
  const next = getMyPrimers().filter((e) => e.id !== id);
  writeList(next);
  return next;
}

export function clearMyPrimers() {
  writeList([]);
  return [];
}

/** 把库记录转成后端 Primer 对象（每条序列一个，取主名称）。 */
export function libraryToPrimers(lib) {
  return (lib || []).map((e) => ({
    id: e.id,
    name: e.names?.[0] || 'Unnamed',
    type: e.type === 'rev' ? 'rev' : 'fwd',
    primerSeq: e.seq,
  }));
}

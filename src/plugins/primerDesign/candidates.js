const COMPLEMENT = { A: 'T', C: 'G', G: 'C', T: 'A' };

export function revComp(s) {
  let out = '';
  for (let i = s.length - 1; i >= 0; i--) out += COMPLEMENT[s[i].toUpperCase()] || 'N';
  return out;
}

export function gcPercent(s) {
  if (!s.length) return 0;
  const gc = (s.match(/[GC]/gi) || []).length;
  return Math.round((gc / s.length) * 1000) / 10;
}

// Modulo indexing so anneal/tail regions can wrap across the origin of a circular sequence
function sliceWrap(seq, start, len, circular) {
  const n = seq.length;
  if (!circular) return seq.substring(Math.max(0, start), Math.min(n, start + len));
  let out = '';
  for (let i = 0; i < len; i++) out += seq[(((start + i) % n) + n) % n];
  return out;
}

async function coreLenForTm(buildAnneal, targetTm, tmOf) {
  let len = 18;
  while (len < 40) {
    const tm = await tmOf(buildAnneal(len));
    if (tm >= targetTm) break;
    len++;
  }
  return len;
}

async function buildVariants(buildPrimer, coreLen, targetTm, tmOf) {
  const lengths = [];
  for (let l = coreLen - 3; l <= coreLen + 3; l++) lengths.push(l);
  const candidates = [];
  for (const l of lengths) {
    const { seq, anneal, tailLen } = buildPrimer(l);
    const tm = await tmOf(anneal);
    candidates.push({
      seq,
      tailLen,
      annealLen: anneal.length,
      tm: Math.round(tm * 10) / 10,
      gc: gcPercent(seq),
    });
  }
  let defaultIndex = 0;
  candidates.forEach((c, i) => {
    if (Math.abs(c.tm - targetTm) < Math.abs(candidates[defaultIndex].tm - targetTm)) {
      defaultIndex = i;
    }
  });
  return { candidates, defaultIndex };
}

export async function buildAmplifyGroups({ seq, seg, name, targetTm, topology, tmOf }) {
  const circular = topology === 'circular';
  const fwdAnneal = (l) => sliceWrap(seq, seg.start, l, circular);
  const revAnneal = (l) => revComp(sliceWrap(seq, seg.end - l + 1, l, circular));
  const fwd = await buildVariants(
    (l) => ({ seq: fwdAnneal(l), anneal: fwdAnneal(l), tailLen: 0 }),
    await coreLenForTm(fwdAnneal, targetTm, tmOf),
    targetTm,
    tmOf,
  );
  const rev = await buildVariants(
    (l) => ({ seq: revAnneal(l), anneal: revAnneal(l), tailLen: 0 }),
    await coreLenForTm(revAnneal, targetTm, tmOf),
    targetTm,
    tmOf,
  );
  return [
    { name: `${name}-Fwd`, type: 'fwd', ...fwd },
    { name: `${name}-Rev`, type: 'rev', ...rev },
  ];
}

export async function buildOepcrGroups({
  seq,
  seg1,
  seg2,
  name1,
  name2,
  targetTm,
  overlapLen,
  topology,
  tmOf,
}) {
  const circular = topology === 'circular';
  const seg1FwdAnneal = (l) => sliceWrap(seq, seg1.start, l, circular);
  const seg1RevAnneal = (l) => revComp(sliceWrap(seq, seg1.end - l + 1, l, circular));
  const seg2FwdAnneal = (l) => sliceWrap(seq, seg2.start, l, circular);
  const seg2RevAnneal = (l) => revComp(sliceWrap(seq, seg2.end - l + 1, l, circular));
  const seg1EndTail = revComp(sliceWrap(seq, seg1.end - overlapLen + 1, overlapLen, circular));
  const seg2StartTail = sliceWrap(seq, seg2.start, overlapLen, circular);

  const make = async (primerName, type, annealFn, tail) => ({
    name: primerName,
    type,
    ...(await buildVariants(
      (l) => ({ seq: tail + annealFn(l), anneal: annealFn(l), tailLen: tail.length }),
      await coreLenForTm(annealFn, targetTm, tmOf),
      targetTm,
      tmOf,
    )),
  });

  return [
    await make(`${name1}-Fwd`, 'fwd', seg1FwdAnneal, ''),
    await make(`${name1}-Rev`, 'rev', seg1RevAnneal, seg2StartTail),
    await make(`${name2}-Fwd`, 'fwd', seg2FwdAnneal, seg1EndTail),
    await make(`${name2}-Rev`, 'rev', seg2RevAnneal, ''),
  ];
}

export async function buildMutagenesisGroups({
  seq,
  seg,
  siteName,
  mutSeq,
  targetTm,
  armLen,
  tmOf,
}) {
  const mut = (mutSeq || '').toUpperCase().replace(/[^ACGT]/g, '');
  const upArm = sliceWrap(seq, seg.start - armLen, armLen, true);
  const downArm = sliceWrap(seq, seg.end + 1, armLen, true);
  const fwdAnneal = (l) => sliceWrap(seq, seg.end + 1, l, true);
  const revAnneal = (l) => revComp(sliceWrap(seq, seg.start - l, l, true));
  const fwdTail = upArm + mut;
  const revTail = revComp(downArm + mut);

  const fwd = await buildVariants(
    (l) => ({ seq: fwdTail + fwdAnneal(l), anneal: fwdAnneal(l), tailLen: fwdTail.length }),
    await coreLenForTm(fwdAnneal, targetTm, tmOf),
    targetTm,
    tmOf,
  );
  const rev = await buildVariants(
    (l) => ({ seq: revTail + revAnneal(l), anneal: revAnneal(l), tailLen: revTail.length }),
    await coreLenForTm(revAnneal, targetTm, tmOf),
    targetTm,
    tmOf,
  );
  return [
    { name: `${siteName}-Fwd`, type: 'fwd', ...fwd },
    { name: `${siteName}-Rev`, type: 'rev', ...rev },
  ];
}

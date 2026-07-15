/**
 * LibreGene backend API client — lightweight fetch wrapper.
 * Default base URL matches the backend server (libregene serve).
 */
const BASE = "http://127.0.0.1:8765";

async function request(method, path, body) {
  const opts = { method, headers: { "Content-Type": "application/json" } };
  if (body) opts.body = JSON.stringify(body);
  const res = await fetch(`${BASE}${path}`, opts);
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status}`);
  return res.json();
}

// ── Project ───────────────────────────────────────────────────

export function getProject(filter = '', cpl = 60) { return request("GET", `/project?enzyme_filter=${filter}&cpl=${cpl}`); }
export function getProjectAll(cpl = 60) { return getProject('all', cpl); }
export function openFile(p)        { return request("POST", `/open?path=${encodeURIComponent(p)}`); }
export function saveFile(p)        { return request("POST", `/save?path=${encodeURIComponent(p)}`); }

// ── Sequence ──────────────────────────────────────────────────

export function updateSequence(seq) { return request("PUT", "/sequence", { sequence: seq }); }

// ── ROI ───────────────────────────────────────────────────────

export function setROI(s, e)       { return request("POST", `/roi?s=${s}&e=${e}`); }
export function clearROI()         { return request("POST", "/roi/clear"); }

// ── Features ──────────────────────────────────────────────────

export function getFeatures()      { return request("GET", "/features"); }
export function addFeature(f)      { return request("POST", "/features", f); }
export function deleteFeature(id)  { return request("DELETE", `/features/${encodeURIComponent(id)}`); }

// ── Primers ───────────────────────────────────────────────────

export function getPrimers()       { return request("GET", "/primers"); }
export function addPrimer(p)       { return request("POST", "/primers", p); }
export function deletePrimer(id)   { return request("DELETE", `/primers/${encodeURIComponent(id)}`); }

// ── Methylation ──────────────────────────────────────────────

export function setMethylation(systems, overlap = 2) {
  return request("POST", `/methylation?systems=${systems.join(',')}&overlap=${overlap}`);
}

// ── WebSocket (real-time push) ────────────────────────────────

export function connectWS(onProject) {
  let retryCount = 0;
  let retryTimeout = null;
  const MAX_RETRIES = 10;

  function connect() {
    if (retryCount >= MAX_RETRIES) return null;
    const ws = new WebSocket(`ws://127.0.0.1:8765/ws`);
    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data);
        if (msg.type === "project" && onProject) onProject(msg.data);
        retryCount = 0; // reset on successful message
      } catch {}
    };
    ws.onclose = () => {
      retryCount++;
      if (retryCount < MAX_RETRIES) {
        const delay = Math.min(1000 * Math.pow(2, retryCount - 1), 30000);
        retryTimeout = setTimeout(connect, delay);
      }
    };
    return ws;
  }

  const ws = connect();
  return {
    close: () => {
      if (retryTimeout) clearTimeout(retryTimeout);
      if (ws) ws.close();
    },
  };
}

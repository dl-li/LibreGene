"""FastAPI + WebSocket server for frontend-backend communication."""
from __future__ import annotations
import dataclasses
import json
from pathlib import Path
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from .project_manager import ProjectManager
from .plugin_manager import discover_plugins
from .file_io import parse_file, write_gbk

app = FastAPI(title="Geneie Backend")
app.add_middleware(CORSMiddleware, allow_origins=["*"], allow_methods=["*"], allow_headers=["*"])

pm = ProjectManager()
_ws_clients: list[WebSocket] = []

# Load plugins
_plugins = discover_plugins()
for plug in _plugins:
    plug.on_load(pm)
    print(f"[geneie] Plugin loaded: {plug.manifest.name} v{plug.manifest.version}")


# ── broadcast helpers ──────────────────────────────────────────

def _to_camel(s: str) -> str:
    """snake_case → camelCase."""
    parts = s.split("_")
    return parts[0] + "".join(p.title() for p in parts[1:])


def _camelize(obj):
    """Recursively convert dict keys from snake_case to camelCase."""
    if isinstance(obj, dict):
        return {_to_camel(k): _camelize(v) for k, v in obj.items()}
    elif isinstance(obj, list):
        return [_camelize(v) for v in obj]
    return obj


def _project_json() -> str:
    """Serialize project, converting to camelCase for frontend JS compat."""
    data = dataclasses.asdict(pm.project)
    # Inject computed primer fields from binding_sites[0]
    for p in data.get("primers", []):
        bs = p.get("binding_sites", [])
        if bs:
            p["match_start"] = bs[0]["match_start"]
            p["match_end"] = bs[0]["match_end"]
            p["match_str"] = bs[0]["match_str"]
    return json.dumps(_camelize(data))


async def _broadcast(msg: str) -> None:
    dead = []
    for ws in _ws_clients:
        try:
            await ws.send_text(msg)
        except Exception:
            dead.append(ws)
    for ws in dead:
        _ws_clients.remove(ws)


async def _push_project() -> None:
    if pm.project is None:
        return
    await _broadcast(json.dumps({"type": "project", "data": json.loads(_project_json())}))


# ── REST ───────────────────────────────────────────────────────

@app.get("/project")
def get_project():
    if pm.project is None:
        return {"error": "No project loaded"}
    return json.loads(_project_json())


@app.post("/open")
def open_file(path: str):
    p = parse_file(Path(path))
    pm.load(p)
    pm.notify()  # triggers plugin recomputation
    return {"status": "ok", "length": p.length, "features": len(p.features)}


@app.post("/save")
def save_file(path: str):
    if pm.project is None:
        return {"error": "No project loaded"}
    write_gbk(pm.project, Path(path))
    return {"status": "ok", "path": path}


@app.post("/roi")
def set_roi(s: int, e: int):
    pm.set_roi(s, e)
    return {"status": "ok", "roi": [s, e]}


@app.post("/roi/clear")
def clear_roi():
    pm.clear_roi()
    return {"status": "ok"}


# ── Sequence ───────────────────────────────────────────────────

@app.put("/sequence")
def update_sequence(data: dict):
    """Replace the full sequence. Triggers enzyme recompute."""
    if pm.project is None:
        return {"error": "No project loaded"}
    seq = data.get("sequence", "")
    pm.update_sequence(seq)
    pm.notify()
    return {"status": "ok", "length": len(seq)}


# ── Features CRUD ──────────────────────────────────────────────

@app.get("/features")
def list_features():
    if pm.project is None:
        return []
    return [dataclasses.asdict(f) for f in pm.project.features]


@app.post("/features")
def add_feature(data: dict):
    """Add a feature. If id matches existing, replace it."""
    if pm.project is None:
        return {"error": "No project loaded"}
    from .models import Feature
    f = Feature(**data)
    existing = [i for i, x in enumerate(pm.project.features) if x.id == f.id]
    if existing:
        pm.project.features[existing[0]] = f
    else:
        pm.project.features.append(f)
    pm.notify()
    return {"status": "ok", "id": f.id}


@app.delete("/features/{fid}")
def delete_feature(fid: str):
    if pm.project is None:
        return {"error": "No project loaded"}
    pm.project.features = [f for f in pm.project.features if f.id != fid]
    pm.notify()
    return {"status": "ok"}


# ── Primers CRUD ───────────────────────────────────────────────

@app.get("/primers")
def list_primers():
    if pm.project is None:
        return []
    return [dataclasses.asdict(p) for p in pm.project.primers]


@app.post("/primers")
def add_primer(data: dict):
    if pm.project is None:
        return {"error": "No project loaded"}
    from .models import Primer
    p = Primer(**data)
    existing = [i for i, x in enumerate(pm.project.primers) if x.id == p.id]
    if existing:
        pm.project.primers[existing[0]] = p
    else:
        pm.project.primers.append(p)
    pm.notify()
    return {"status": "ok", "id": p.id}


@app.delete("/primers/{pid}")
def delete_primer(pid: str):
    if pm.project is None:
        return {"error": "No project loaded"}
    pm.project.primers = [p for p in pm.project.primers if p.id != pid]
    pm.notify()
    return {"status": "ok"}


# ── WebSocket ──────────────────────────────────────────────────

@app.websocket("/ws")
async def ws_endpoint(ws: WebSocket):
    await ws.accept()
    _ws_clients.append(ws)
    if pm.project is not None:
        await ws.send_text(json.dumps({"type": "project", "data": json.loads(_project_json())}))
    try:
        while True:
            raw = await ws.receive_text()
            try:
                msg = json.loads(raw)
            except json.JSONDecodeError:
                await ws.send_text(json.dumps({"type": "error", "msg": "invalid json"}))
                continue
            await _handle_ws_msg(msg)
            await _push_project()
    except WebSocketDisconnect:
        _ws_clients.remove(ws)


async def _handle_ws_msg(msg: dict) -> None:
    t = msg.get("type", "")
    if t == "edit_sequence":
        pm.update_sequence(msg.get("sequence", ""))
    elif t == "edit_features":
        pm.update_features(msg.get("features", []))
    elif t == "edit_primers":
        pm.update_primers(msg.get("primers", []))
    elif t == "roi":
        s, e = msg.get("start", 0), msg.get("end", 0)
        pm.set_roi(s, e)
    elif t == "roi_clear":
        pm.clear_roi()
    else:
        pass  # unknown message type — silently ignored


def start(host: str = "127.0.0.1", port: int = 8765):
    import uvicorn
    uvicorn.run(app, host=host, port=port)


if __name__ == "__main__":
    start()

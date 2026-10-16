"""Exercise the fal and Meshy adapters against mocked APIs (no keys, no spend)."""
import json
from pathlib import Path

import httpx
import pytest

from ratmesh import engines

IMG = Path(__file__).resolve().parents[1] / "samples/input/rat.png"


@pytest.fixture
def mock(monkeypatch, tmp_path):
    calls = []
    state = {"polls": 0}

    def handler(req):
        calls.append((req.method, str(req.url), req.headers.get("authorization"), req.content))
        u = str(req.url)
        if "storage/upload/initiate" in u:
            return httpx.Response(200, json={"upload_url": "https://up/put", "file_url": "https://cdn/in.png"})
        if u == "https://up/put":
            return httpx.Response(200)
        if req.method == "POST" and "queue.fal.run" in u:
            return httpx.Response(200, json={"request_id": "r1", "status_url": "https://queue.fal.run/x/requests/r1/status", "response_url": "https://queue.fal.run/x/requests/r1"})
        if u.startswith("https://queue.fal.run/x/requests/r1/status"):
            state["polls"] += 1
            return httpx.Response(200, json={"status": "IN_QUEUE" if state["polls"] < 2 else "COMPLETED", "queue_position": 0})
        if u == "https://queue.fal.run/x/requests/r1":
            return httpx.Response(200, json={"model_glb": {"url": "https://cdn/model.glb"}})
        if req.method == "POST" and "api.meshy.ai" in u:
            return httpx.Response(202, json={"result": "t1"})
        if "api.meshy.ai" in u:
            state["polls"] += 1
            done = state["polls"] >= 2
            return httpx.Response(200, json={"status": "SUCCEEDED" if done else "IN_PROGRESS", "progress": 100 if done else 40, "model_urls": {"glb": "https://cdn/m.glb"}, "consumed_credits": 30})
        return httpx.Response(404)

    real = httpx.Client
    monkeypatch.setattr(engines.httpx, "Client", lambda **kw: real(transport=httpx.MockTransport(handler), **kw))
    monkeypatch.setattr(engines, "download", lambda url, out: Path(out).write_text(url))
    monkeypatch.setattr(engines.time, "sleep", lambda s: None)
    monkeypatch.setattr(engines, "load_keys", lambda: {"FAL_KEY": "fk", "MESHY_API_KEY": "mk"})
    return calls, tmp_path


OPT = {"polys": 30000, "texture": 2048, "quality": "standard", "pbr": True, "seed": 7, "views": {}}


def test_trellis2(mock):
    calls, tmp = mock
    meta = engines.trellis2(IMG, tmp / "o.glb", OPT, lambda m: None)
    sub = next(c for c in calls if "queue.fal.run" in c[1])
    body = json.loads(sub[3])
    assert sub[1] == "https://queue.fal.run/fal-ai/trellis-2" and sub[2] == "Key fk"
    assert calls[1][1] == "https://up/put" and calls[1][3] == IMG.read_bytes()
    assert body["image_url"] == "https://cdn/in.png" and body["resolution"] == "1024"
    assert body["decimation_target"] == 30000 and body["texture_size"] == "2048" and body["seed"] == 7
    assert (tmp / "o.glb").read_text() == "https://cdn/model.glb" and meta["engine"] == "trellis2"


def test_hunyuan3_views(mock):
    calls, tmp = mock
    engines.hunyuan3(IMG, tmp / "o.glb", dict(OPT, views={"back": str(IMG)}), lambda m: None)
    body = json.loads(next(c for c in calls if "queue.fal.run" in c[1])[3])
    assert "back_image_url" in body and "left_image_url" not in body and "face_count" not in body


def test_meshy_single_and_multi(mock):
    calls, tmp = mock
    meta = engines.meshy(IMG, tmp / "o.glb", dict(OPT, pose="a-pose"), lambda m: None)
    body = json.loads(calls[0][3])
    assert calls[0][1].endswith("/openapi/v1/image-to-3d") and calls[0][2] == "Bearer mk"
    assert body["target_polycount"] == 30000 and body["pose_mode"] == "a-pose" and body["should_remesh"] is True
    assert meta["credits"] == 30 and (tmp / "o.glb").read_text() == "https://cdn/m.glb"
    calls.clear()
    engines.meshy(IMG, tmp / "o.glb", dict(OPT, views={"back": str(IMG), "left": str(IMG)}), lambda m: None)
    body = json.loads(calls[0][3])
    assert calls[0][1].endswith("/multi-image-to-3d") and len(body["image_urls"]) == 3

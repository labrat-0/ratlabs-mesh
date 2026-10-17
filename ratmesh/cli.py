"""ratmesh: command line for RatLabs Mesh.

  ratmesh make art.png --kind character --engine trellis2 --variants 2
  ratmesh make scene.png --crop 0.3,0.1,0.7,1.0          # pick a character out of a scene
  ratmesh make front.png --back back.png --engine hunyuan3
  ratmesh import old.glb --kind critter --height 0.1      # finish an existing model
  ratmesh refinish <job> --polys 12000 --height 1.4       # re-finish, no GPU cost
  ratmesh rig <job>                                       # skeleton + 88 CC0 animations
  ratmesh engines
  ratmesh serve
"""
import argparse
import json

from . import engines, jobs
from .config import PRESETS


def main():
    ap = argparse.ArgumentParser(prog="ratmesh", description="RatLabs Mesh: image to finished 3D asset")
    sub = ap.add_subparsers(dest="cmd", required=True)

    def shape(p):
        p.add_argument("--name")
        p.add_argument("--kind", choices=list(PRESETS), default="character")
        p.add_argument("--height", type=float, help="metres, up axis")
        p.add_argument("--polys", type=int, help="triangle budget")
        p.add_argument("--texture", type=int, choices=[1024, 2048, 4096])
        p.add_argument("--origin", choices=["bottom", "center"])
        p.add_argument("--rotate", type=float, default=0, help="degrees about up, if the model faces the wrong way")

    m = sub.add_parser("make", help="generate from an image")
    m.add_argument("image")
    shape(m)
    m.add_argument("--engine", choices=list(engines.ENGINES), default="trellis2")
    m.add_argument("--variants", type=int, default=1, help="1-4 seeds to pick from")
    m.add_argument("--quality", choices=list(engines.QUALITY), default="standard")
    m.add_argument("--crop", help="x0,y0,x1,y1 as 0-1 fractions")
    m.add_argument("--back"), m.add_argument("--left"), m.add_argument("--right")
    m.add_argument("--pose", choices=["a-pose", "t-pose"], help="Meshy only")
    m.add_argument("--no-pbr", action="store_true")
    m.add_argument("--keep-background", action="store_true", help="skip cut-out (art already isolated)")
    m.add_argument("--keep-lighting", action="store_true", help="paint with the art's own lighting (no delight)")
    m.add_argument("--seed", type=int)

    i = sub.add_parser("import", help="finish an existing GLB")
    i.add_argument("glb")
    shape(i)

    r = sub.add_parser("refinish", help="re-run finishing on a job")
    r.add_argument("job")
    for k, t in (("height", float), ("polys", int), ("texture", int), ("rotate", float)):
        r.add_argument("--" + k, type=t)
    r.add_argument("--origin", choices=["bottom", "center"])

    g = sub.add_parser("rig", help="auto-rig a finished job and add the CC0 animation library")
    g.add_argument("job")

    sub.add_parser("engines", help="list engines and whether their keys are set")
    s = sub.add_parser("serve", help="run the studio UI")
    s.add_argument("--port", type=int, default=8771)

    a = ap.parse_args()
    if a.cmd == "engines":
        for k, e in engines.available().items():
            print(f"{'ready ' if e['ready'] else 'no key'}  {k:12} {e['label']:28} {e['cost']}" + ("" if e["ready"] or not e["key"] else f"   needs {e['key']}"))
        return
    if a.cmd == "serve":
        from .server import serve
        serve(a.port)
        return
    if a.cmd == "rig":
        job = jobs.rig(a.job)
        for r in job["results"]:
            if r.get("rigged"):
                print(f"  v{r['variant']}: {jobs.job_dir(job['id']) / r['rigged']}  {r['rig']['bones']} bones, {len(r['rig']['clips'])} clips")
        if job["error"]:
            print("  error:", job["error"])
            raise SystemExit(1)
        return
    common = dict(name=a.name, kind=a.kind, height=a.height, polys=a.polys, texture=a.texture, origin=a.origin, rotate=a.rotate) if a.cmd != "refinish" else {}
    if a.cmd == "make":
        views = {k: getattr(a, k) for k in ("back", "left", "right") if getattr(a, k)}
        crop = [float(x) for x in a.crop.split(",")] if a.crop else None
        job = jobs.create(a.image, engine=a.engine, variants=a.variants, quality=a.quality, crop=crop, views=views,
                          pbr=not a.no_pbr, pose=a.pose, seed=a.seed, keep_background=a.keep_background,
                          delight=not a.keep_lighting, **common)
    elif a.cmd == "import":
        job = jobs.create(None, glb=a.glb, **common)
    else:
        job = jobs.refinish(a.job, height=a.height, polys=a.polys, texture=a.texture, rotate=a.rotate, origin=a.origin)
        print(json.dumps({k: job[k] for k in ("id", "status", "error")}))
        return
    print("job", job["id"])
    job = jobs.run(job["id"])
    for r in job["results"]:
        s = r["stats"]
        print(f"  v{r['variant']}: {jobs.job_dir(job['id']) / r['glb']}  {s['tris']} tris  {s['dimensions_m']} m  sheet {r['renders']}/sheet.jpg")
    if job["error"]:
        print("  error:", job["error"])
        raise SystemExit(1)


if __name__ == "__main__":
    main()

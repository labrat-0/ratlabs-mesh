#!/usr/bin/env python3
"""Build starter-pack.zip: the 30 CC0 Poly Haven props finished by RatLabs Mesh, plus the
Labrat from Rat Underground as a showcase (RatLabs' own art: all rights reserved, usable in the studio).

Maintainer tool. Reads the finished studio jobs listed in the free-props package and
writes a zip that `ratmesh setup starter` (and the studio's "Load starter pack" button)
unpacks into a new user's library:

  pack.json                       name, version, credits for every item
  jobs/starter-<slug>/job.json    a normal job: finished GLB, stats, renders, licence/author/source
  jobs/starter-<slug>/v1/...      <name>.glb, <name>.json, renders/{three_quarter.png, sheet.jpg}

Raw originals are left out to keep the download small; refinish works from the finished GLB.

  python3 tools/build-starter-pack.py --package packages/ratlabs-underground-free-props \
      --jobs ~/.local/share/ratlabs-mesh/jobs --game ~/repos/ratlabs-underground --out starter-pack.zip
"""
import argparse, json, os, re, shutil, subprocess, sys, tempfile, time, zipfile
from pathlib import Path

VERSION = 1

RESERVED = ("© RatLabs, all rights reserved. Free to view, refinish and rig in RatLabs Mesh; "
            "not licensed for use outside it. See ART-LICENSE.md.")
# RatLabs' own characters shown off in the pack: the studio job they came from, and the
# game's rigged + animated version of it.
SHOWCASE = [
    {"job": "20261020-190923-labrat-main", "rigged": "assets/models/runtime/labrat-main.glb", "name": "Labrat",
     "author": "RatLabs (Rat Underground)", "licence": RESERVED, "source": "https://ratlabs.tech"},
]


def slug(s):
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")[:40] or "asset"


def finish_kit(glb, item, jobs_dir):
    """Props added to the game without a studio job (modular kits): finish them now, parts kept."""
    env = dict(os.environ, RATMESH_JOBS=str(jobs_dir))
    out = subprocess.run(["ratmesh", "import", str(glb), "--kind", "prop", "--name", item["name"],
                          "--height", "0", "--polys", str(item["tris"])], env=env, capture_output=True, text=True)
    m = re.search(r"^job (\S+)", out.stdout, re.M)
    if out.returncode or not m:
        sys.exit(f"finishing {item['name']} failed:\n{out.stdout}\n{out.stderr}")
    return jobs_dir / m.group(1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--package", type=Path, required=True)
    ap.add_argument("--jobs", type=Path, required=True)
    ap.add_argument("--game", type=Path, help="Rat Underground repo, for the showcase characters' rigs")
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args()
    manifest = json.loads((a.package / "manifest.json").read_text())
    job_ids = json.loads((a.package / "jobs.json").read_text())
    tmp = Path(tempfile.mkdtemp(prefix="starter-"))
    kit_jobs = tmp / "kit-jobs"
    items = []
    with zipfile.ZipFile(a.out, "w", zipfile.ZIP_STORED) as z:  # GLB/PNG/JPG are compressed already
        for it in manifest:
            if it.get("licence") != "CC0":
                sys.exit(f"{it['name']} is {it.get('licence')}, not CC0: refusing to pack it")
            src = a.jobs / job_ids[it["id"]] if job_ids.get(it["id"]) else finish_kit(a.package / "models" / it["file"], it, kit_jobs)
            job = json.loads((src / "job.json").read_text())
            r = job["results"][0]
            jid = "starter-" + slug(it["name"])
            glb = Path(r["glb"])
            renders = Path(r["renders"])
            base = f"jobs/{jid}/"
            z.write(src / glb, base + str(glb))
            z.write((src / glb).with_suffix(".json"), base + str(glb.with_suffix(".json")))
            z.write(src / renders / "sheet.jpg", base + str(renders / "sheet.jpg"))
            thumb = tmp / f"{jid}.png"
            if shutil.which("magick"):
                subprocess.run(["magick", str(src / renders / "three_quarter.png"), "-resize", "512x512", str(thumb)], check=True)
            else:
                shutil.copy(src / renders / "three_quarter.png", thumb)
            z.write(thumb, base + str(renders / "three_quarter.png"))
            r["raw"] = str(glb)  # no raw original in the pack: refinish starts from the finished model
            job.update(id=jid, status="done", stage="done", error=None, results=[r], pack="starter",
                       licence=it["licence"], author=it["author"], source=it["source"])
            job.pop("by", None)
            z.writestr(base + "job.json", json.dumps(job, indent=2))
            z.writestr(base + "job.log", f"{time.strftime('%H:%M:%S')} starter pack: {it['name']} by {it['author']} ({it['licence']}), {it['source']}\n")
            items.append({"id": jid, "name": it["name"], "author": it["author"], "licence": it["licence"], "source": it["source"]})
            print(f"  {jid:44} {r['stats'].get('tris', '?'):>6} tris")
        for sc in SHOWCASE if a.game else []:
            src = a.jobs / sc["job"]
            job = json.loads((src / "job.json").read_text())
            r = job["results"][0]
            jid = "starter-" + slug(sc["name"])
            glb, renders = Path(r["glb"]), Path(r["renders"])
            base = f"jobs/{jid}/"
            z.write(src / glb, base + str(glb))
            z.write((src / glb).with_suffix(".json"), base + str(glb.with_suffix(".json")))
            for f in ("sheet.jpg", "front.png", "three_quarter.png", "side.png", "back.png"):
                z.write(src / renders / f, base + str(renders / f))
            rigged = glb.with_name(glb.stem + "-rigged.glb")
            z.write(a.game / sc["rigged"], base + str(rigged))
            g = (a.game / sc["rigged"]).read_bytes()
            gj = json.loads(g[20:20 + int.from_bytes(g[12:16], "little")])
            r.update(raw=str(glb), rigged=str(rigged),
                     rig={"clips": [x.get("name") for x in gj.get("animations", [])], "bones": len(gj["skins"][0]["joints"]), "warnings": []})
            # newest in the pack, so it opens first after loading (still older than anything the user makes)
            job.update(id=jid, name=sc["name"], status="done", stage="done", error=None, results=[r], pack="starter", created=time.time(),
                       licence=sc["licence"], author=sc["author"], source=sc["source"])
            job.pop("by", None)
            z.writestr(base + "job.json", json.dumps(job, indent=2))
            z.writestr(base + "job.log", f"{time.strftime('%H:%M:%S')} starter pack: {sc['name']} from Rat Underground, rigged with the game's own animations\n")
            items.append({"id": jid, "name": sc["name"], "author": sc["author"], "licence": sc["licence"], "source": sc["source"]})
            print(f"  {jid:44} showcase, {len(r['rig']['clips'])} animations")
        z.writestr("pack.json", json.dumps({
            "name": "RatLabs Mesh starter pack", "version": VERSION,
            "note": "The props are CC0 from Poly Haven (polyhaven.com), finished by RatLabs Mesh: free to use for anything. "
                    "The Labrat is RatLabs' own character: all rights reserved, free to try in the studio only.",
            "items": items}, indent=1))
    shutil.rmtree(tmp, ignore_errors=True)
    print(f"{len(items)} items -> {a.out} ({a.out.stat().st_size / 1e6:.0f} MB)")


if __name__ == "__main__":
    main()

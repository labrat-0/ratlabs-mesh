"""TRELLIS.2 on the official Hugging Face Space, as a RatLabs Mesh command engine.

Prints one JSON line per stage ({"stage": ...}) for the job log; writes the GLB to `out`.
The token arrives in HF_TOKEN (the manifest's `key`).
"""
import argparse, json, os, shutil

from gradio_client import Client, handle_file

ap = argparse.ArgumentParser()
ap.add_argument("image"); ap.add_argument("out")
ap.add_argument("--seed", type=int, default=0)
ap.add_argument("--polys", type=int, default=30000)
ap.add_argument("--texture", type=int, default=2048)
ap.add_argument("--resolution", type=int, default=1024)
a = ap.parse_args()


def stage(s):
    print(json.dumps({"stage": s}), flush=True)


c = Client("microsoft/TRELLIS.2", hf_token=os.environ["HF_TOKEN"], verbose=False)
c.predict(api_name="/start_session")
stage("preprocessing")
pre = c.predict(handle_file(a.image), api_name="/preprocess_image")
stage("generating")
c.predict(handle_file(pre), a.seed, str(a.resolution), api_name="/image_to_3d")
stage("extracting GLB")
g = c.predict(max(10000, a.polys // 2), min(4096, a.texture), api_name="/extract_glb")
shutil.copy(g[0], a.out)

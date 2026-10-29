"""Make a lighter copy of a GLB, keeping node names, hierarchy, skins and clips.

Run by Blender:  blender -b --python lod_blender.py -- spec.json

spec: {"in": a.glb, "out": b.glb, "stats": out.json,
       "target": total triangles (optional), "ratio": per-mesh ratio (optional),
       "min_tris": floor per mesh (default 24), "clips": [names] to keep (optional)}

With "target", every mesh is cut by the same ratio so the whole file lands near it.
With "ratio", each mesh keeps that share of its triangles (kits: every piece cut alike).
"""
import json
import sys

import bpy

spec = json.load(open(sys.argv[sys.argv.index("--") + 1]))
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=spec["in"])


def tris(o):
    return sum(len(p.vertices) - 2 for p in o.data.polygons)


meshes = [o for o in bpy.data.objects if o.type == "MESH"]
before = {o.name: tris(o) for o in meshes}
total = sum(before.values())
ratio = spec.get("ratio") or min(1.0, spec["target"] / max(total, 1))
floor = spec.get("min_tris", 24)
done = set()
for o in meshes:
    if o.data in done:  # shared mesh data: decimate once
        continue
    done.add(o.data)
    r = max(ratio, min(1.0, floor / max(before[o.name], 1)))
    if r >= 0.999:
        continue
    bpy.context.view_layer.objects.active = o
    keys = o.data.shape_keys
    if keys:  # the decimate modifier refuses meshes with shape keys
        o.shape_key_clear()
    mod = o.modifiers.new("lod", "DECIMATE")
    mod.decimate_type = "COLLAPSE"
    mod.ratio = r
    mod.use_collapse_triangulate = True
    while o.modifiers[0] != mod:  # before the armature, so weights carry through
        bpy.ops.object.modifier_move_up(modifier=mod.name)
    bpy.ops.object.modifier_apply(modifier=mod.name)

acts = list(bpy.data.actions)
if spec.get("clips"):
    for a in acts:
        if a.name not in spec["clips"]:
            bpy.data.actions.remove(a)
    acts = list(bpy.data.actions)
for a in acts:
    a.use_fake_user = True
for o in bpy.data.objects:  # the importer stacks clips as NLA tracks; export actions instead
    if o.animation_data:
        for tr in list(o.animation_data.nla_tracks):
            o.animation_data.nla_tracks.remove(tr)
        o.animation_data.action = None

bpy.ops.export_scene.gltf(filepath=spec["out"], export_format="GLB", export_animations=bool(acts),
                          export_animation_mode="ACTIONS", export_skins=True,
                          export_image_format="AUTO", export_jpeg_quality=88)
after = {o.name: tris(o) for o in bpy.data.objects if o.type == "MESH"}
json.dump({"tris_before": total, "tris_after": sum(after.values()), "pieces": len(after),
           "names": sorted(after), "clips": sorted(a.name for a in acts),
           "skinned": any(o.find_armature() for o in bpy.data.objects if o.type == "MESH")},
          open(spec["stats"], "w"), indent=1)
print("LOD OK")

"""Stage 3 (runs inside Blender): make a raw generation look like a finished asset.

blender -b --python ratmesh/finish_blender.py -- spec.json

spec: {"in", "out", "height", "polys", "texture", "origin": "bottom"|"center",
       "rotate": degrees about up, "renders": dir or null, "stats": path}
"""
import bpy, bmesh, json, math, sys
import numpy as np
from mathutils import Matrix, Vector
from mathutils.kdtree import KDTree

spec = json.load(open(sys.argv[sys.argv.index("--") + 1]))
stats = {"warnings": []}

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=spec["in"])
for o in [o for o in bpy.context.scene.objects if o.type not in ("MESH",)]:
    bpy.data.objects.remove(o, do_unlink=True)
meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
if not meshes:
    raise SystemExit("no mesh in input")
bpy.ops.object.select_all(action="DESELECT")
for o in meshes:
    o.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
if len(meshes) > 1:
    bpy.ops.object.join()
body = bpy.context.view_layer.objects.active
body.name = spec.get("name", "ratmesh")
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
me = body.data
stats["raw_tris"] = sum(len(p.vertices) - 2 for p in me.polygons)


def bounds():
    vs = [v.co for v in body.data.vertices]
    mn = Vector([min(v[i] for v in vs) for i in range(3)])
    mx = Vector([max(v[i] for v in vs) for i in range(3)])
    return mn, mx


# --- cleanup: weld split vertices, drop floating specks ---
mn, mx = bounds()
diag = (mx - mn).length
bm = bmesh.new()
bm.from_mesh(me)
bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=diag * 1e-6)
bm.faces.ensure_lookup_table()
seen, parts = set(), []
for f in bm.faces:
    if f.index in seen:
        continue
    stack, comp = [f], []
    seen.add(f.index)
    while stack:
        g = stack.pop()
        comp.append(g)
        for e in g.edges:
            for h in e.link_faces:
                if h.index not in seen:
                    seen.add(h.index)
                    stack.append(h)
    parts.append(comp)
total = len(bm.faces)
parts.sort(key=len, reverse=True)
main_verts = {v for f in parts[0] for v in f.verts}
tree = KDTree(len(main_verts))
for i, v in enumerate(main_verts):
    tree.insert(v.co, i)
tree.balance()
drop, dropped = [], 0
for comp in parts[1:]:
    vs = list({v for f in comp for v in f.verts})
    lo = Vector([min(v.co[i] for v in vs) for i in range(3)])
    hi = Vector([max(v.co[i] for v in vs) for i in range(3)])
    speck = len(comp) < total * 0.002 and (hi - lo).length < diag * 0.03
    # Generators leave flakes hovering beside the body. Real separate pieces
    # (boots, goggles, a held prop) touch or cut into it; flakes keep a gap.
    gap = min(tree.find(v.co)[2] for v in vs[:: max(1, len(vs) // 400)])
    flake = gap > diag * 0.015 and len(comp) < len(parts[0]) * 0.25
    if speck or flake:
        drop.extend(comp)
        dropped += 1
if drop:
    bmesh.ops.delete(bm, geom=drop, context="FACES")
stats["parts"] = len(parts)
stats["floaters_removed"] = dropped
bm.to_mesh(me)
bm.free()

# --- orient: the reference front faces the camera at -Y ---
if spec.get("rotate"):
    body.rotation_euler.z = math.radians(spec["rotate"])
    bpy.ops.object.transform_apply(rotation=True)

# --- retopology budget ---
tris = sum(len(p.vertices) - 2 for p in me.polygons)
target = int(spec["polys"])
if tris > target * 1.05:
    m = body.modifiers.new("decimate", "DECIMATE")
    m.decimate_type = "COLLAPSE"
    m.ratio = target / tris
    m.use_collapse_triangulate = True
    bpy.ops.object.modifier_apply(modifier=m.name)
    stats["decimated_from"] = tris
try:
    bpy.ops.object.shade_smooth_by_angle(angle=math.radians(50))
except Exception:
    for p in me.polygons:
        p.use_smooth = True

# --- real-world size and pivot ---
mn, mx = bounds()
size = mx - mn
s = float(spec["height"]) / max(size.z, 1e-9)
for v in me.vertices:
    v.co *= s
mn, mx = bounds()
c = (mn + mx) / 2
shift = Vector((-c.x, -c.y, -mn.z if spec.get("origin", "bottom") == "bottom" else -c.z))
for v in me.vertices:
    v.co += shift
me.update()
mn, mx = bounds()
stats["dimensions_m"] = [round(x, 4) for x in (mx - mn)]

# --- materials and textures ---
def _filt(a, r, op):
    """Separable square min/max filter of radius r (numpy only; Blender has no scipy)."""
    for axis in (0, 1):
        p = np.pad(a, [(r, r) if i == axis else (0, 0) for i in range(2)], mode="edge")
        n = a.shape[axis]
        out = p.take(range(0, n), axis=axis)
        for k in range(1, 2 * r + 1):
            out = op(out, p.take(range(k, k + n), axis=axis))
        a = out
    return a


def clean_metal_rough(im, r=3):
    """Generated PBR maps sprinkle metal/gloss specks over cloth and skin, which render
    as white glitter. Keep only metal regions wider than ~2r+1 texels and give
    non-metal a sane minimum roughness."""
    w, h = im.size
    px = np.empty(w * h * 4, dtype=np.float32)
    im.pixels.foreach_get(px)
    px = px.reshape(h, w, 4)
    metal = px[..., 2]
    opened = _filt(_filt(metal, r, np.minimum), r, np.maximum)
    rough = np.where(opened < 0.5, np.maximum(px[..., 1], 0.45), px[..., 1])
    before = float((metal > 0.5).mean())
    px[..., 2], px[..., 1] = opened, rough
    im.pixels.foreach_set(px.ravel())
    im.update()
    im.pack()
    return {"metal_before": round(before, 3), "metal_after": round(float((opened > 0.5).mean()), 3)}


tex = int(spec["texture"])
has_alpha = False
stats["textures"] = []
for mat in me.materials:
    if not mat:
        continue
    if not mat.use_nodes:
        mat.use_nodes = True
    nt = mat.node_tree
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf:
        # glTF defaults an unspecified material to metallic 1 / roughness 1, which reads as chrome.
        if not bsdf.inputs["Roughness"].is_linked and bsdf.inputs["Roughness"].default_value > 0.99:
            bsdf.inputs["Roughness"].default_value = 0.8
        if not bsdf.inputs["Metallic"].is_linked and bsdf.inputs["Metallic"].default_value > 0.99:
            bsdf.inputs["Metallic"].default_value = 0.0
        if bsdf.inputs["Alpha"].is_linked:
            has_alpha = True
    for n in nt.nodes:
        if n.type == "TEX_IMAGE" and n.image:
            im = n.image
            w, h = im.size
            if max(w, h) > tex:
                im.scale(int(w * tex / max(w, h)), int(h * tex / max(w, h)))
            if "METALLIC" in (n.label or "").upper():
                stats["pbr_cleaned"] = clean_metal_rough(im)
            stats["textures"].append([n.label or im.name, list(im.size)])
if not me.materials or not any(me.materials):
    # Vertex-coloured input (TripoSR): show the colours through a real material.
    mat = bpy.data.materials.new("ratmesh_vertex_color")
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    bsdf.inputs["Roughness"].default_value = 0.8
    if me.color_attributes:
        attr = nt.nodes.new("ShaderNodeVertexColor")
        attr.layer_name = me.color_attributes[0].name
        nt.links.new(attr.outputs["Color"], bsdf.inputs["Base Color"])
    me.materials.append(mat)
    stats["vertex_colors"] = True

# --- normal map: keep the full-detail surface's sculpting on the game mesh ---
if spec.get("hipoly") and me.uv_layers:
    import numpy as np
    keep = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=spec["hipoly"])
    his = [o for o in bpy.data.objects if o not in keep and o.type == "MESH"]
    for o in [o for o in bpy.data.objects if o not in keep and o.type != "MESH"]:
        bpy.data.objects.remove(o, do_unlink=True)
    hi = his[0]
    if len(his) > 1:
        with bpy.context.temp_override(active_object=hi, selected_editable_objects=his):
            bpy.ops.object.join()
    hi.data.transform(hi.matrix_world)
    hi.matrix_world.identity()
    # Same rotate, scale and shift the game mesh received above.
    co = np.empty(len(hi.data.vertices) * 3)
    hi.data.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    if spec.get("rotate"):
        r = math.radians(spec["rotate"])
        co = co @ np.array([[math.cos(r), math.sin(r), 0], [-math.sin(r), math.cos(r), 0], [0, 0, 1]])
    co = co * s + np.array(shift)
    hi.data.vertices.foreach_set("co", co.ravel())
    hi.data.update()
    for p in hi.data.polygons:
        p.use_smooth = True
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = 1
    nimg = bpy.data.images.new("normal", tex, tex)
    nimg.colorspace_settings.name = "Non-Color"
    targets = []
    for mat in me.materials:
        nt = mat.node_tree
        node = nt.nodes.new("ShaderNodeTexImage")
        node.image = nimg
        node.interpolation = "Linear"
        nt.nodes.active = node
        targets.append((mat, node))
    bpy.ops.object.select_all(action="DESELECT")
    hi.select_set(True)
    body.select_set(True)
    bpy.context.view_layer.objects.active = body
    ext = max(mx - mn)
    bpy.ops.object.bake(type="NORMAL", normal_space="TANGENT", use_selected_to_active=True,
                        cage_extrusion=ext * 0.01, max_ray_distance=ext * 0.03, margin=16)
    for mat, node in targets:
        nt = mat.node_tree
        bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
        nm = nt.nodes.new("ShaderNodeNormalMap")
        nt.links.new(node.outputs["Color"], nm.inputs["Color"])
        nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    bpy.data.objects.remove(hi, do_unlink=True)
    stats["normal_map"] = [tex, tex]
    stats["hipoly_verts"] = len(co)

# --- quality report ---
bm = bmesh.new()
bm.from_mesh(me)
stats["tris"] = sum(len(f.verts) - 2 for f in bm.faces)
stats["verts"] = len(bm.verts)
stats["non_manifold_edges"] = sum(1 for e in bm.edges if not e.is_manifold)
bm.free()
stats["has_uv"] = bool(me.uv_layers)
stats["vertex_colors"] = stats.get("vertex_colors") or (bool(me.color_attributes) and not stats["textures"])
if not me.uv_layers and not stats["vertex_colors"]:
    stats["warnings"].append("mesh has no UVs")

kw = dict(filepath=spec["out"], export_format="GLB", use_selection=False, export_apply=True,
          export_image_format="AUTO" if has_alpha else "JPEG", export_yup=True)
try:
    bpy.ops.export_scene.gltf(**kw, export_jpeg_quality=92)
except TypeError:
    bpy.ops.export_scene.gltf(**kw)

# --- review renders: front, three-quarter, side, back on the RatLabs near-black ---
if spec.get("renders"):
    sc = bpy.context.scene
    engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    sc.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in engines else "BLENDER_EEVEE"
    sc.render.resolution_x = sc.render.resolution_y = int(spec.get("render_px", 768))
    sc.render.film_transparent = True  # transparent stills; the sheet is composited on near-black
    sc.view_settings.view_transform = "AgX" if "AgX" in [v.identifier for v in type(sc.view_settings).bl_rna.properties["view_transform"].enum_items] else "Filmic"
    w = bpy.data.worlds.new("w")
    sc.world = w
    w.use_nodes = True
    bg = w.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.05, 0.055, 0.065, 1)
    bg.inputs[1].default_value = 1.0
    mn, mx = bounds()
    ctr = (mn + mx) / 2
    ext = max(mx - mn)

    def light(name, kind, energy, color, loc):
        L = bpy.data.objects.new(name, bpy.data.lights.new(name, kind))
        L.data.energy = energy
        L.data.color = color
        sc.collection.objects.link(L)
        if kind == "AREA":
            L.data.size = ext * 1.2
        rig.append((L, Vector(loc)))
        return L

    def aim_lights(angle):
        # The rig turns with the camera so every view gets the same studio lighting.
        rot = Matrix.Rotation(angle, 3, "Z")
        for L, loc in rig:
            L.location = ctr + (rot @ loc) * ext
            L.rotation_euler = (ctr - L.location).to_track_quat("-Z", "Y").to_euler()

    rig = []
    k = ext * ext
    light("key", "AREA", 85 * k, (1, 0.96, 0.9), (-1.2, -2.0, 1.6))
    light("fill", "AREA", 32 * k, (0.85, 0.9, 1), (1.8, -1.4, 0.4))
    light("rim_c", "AREA", 14 * k, (0.2, 1, 0.85), (1.6, 1.8, 1.2))
    light("rim_m", "AREA", 9 * k, (1, 0.25, 0.8), (-1.8, 1.6, 0.8))
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    sc.collection.objects.link(cam)
    sc.camera = cam
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = ext * 1.18
    cam.data.clip_end = ext * 20
    import os
    os.makedirs(spec["renders"], exist_ok=True)
    for name, ang, el in (("front", 0, 0.08), ("three_quarter", 40, 0.18), ("side", 90, 0.05), ("back", 180, 0.08)):
        a = math.radians(ang)
        d = Vector((math.sin(a), -math.cos(a), el)).normalized()
        cam.location = ctr + d * ext * 4
        cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
        aim_lights(a)
        sc.render.filepath = os.path.join(spec["renders"], name + ".png")
        bpy.ops.render.render(write_still=True)

json.dump(stats, open(spec["stats"], "w"), indent=2)
print("RATMESH_STATS", json.dumps(stats))

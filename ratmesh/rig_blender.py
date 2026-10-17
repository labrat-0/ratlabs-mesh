"""Auto-rig a finished biped and give it the Quaternius CC0 animation libraries.

Run by Blender:  blender -b --python rig_blender.py -- spec.json

spec: {"in": finished.glb, "out": rigged.glb, "libs": [ual1.glb, ual2.glb],
       "stats": out.json, "renders": dir or null}

The character must stand upright on the origin, feet down, facing -Y (what the
finisher writes), arms away from the body (A- or T-pose). Steps:

1. find joints from the mesh itself (feet, crotch, armpits, hands, head),
2. build a 22-bone humanoid skeleton through them,
3. skin it: automatic weights on a watertight voxel copy, transferred back,
   so cracked or open generated meshes still bind,
4. retarget every library clip by world-space rotation deltas, so the library's
   T-pose rest and the character's A-pose rest don't fight.
"""
import bpy, json, math, sys
import numpy as np
from mathutils import Matrix, Vector

spec = json.load(open(sys.argv[sys.argv.index("--") + 1]))
stats = {"warnings": []}

# Target skeleton: name -> (parent, UAL1 source, UAL2 source)
BONES = {
    "root": (None, "root", "root"),
    "hips": ("root", "DEF-hips", "pelvis"),
    "spine1": ("hips", "DEF-spine.001", "spine_01"),
    "spine2": ("spine1", "DEF-spine.002", "spine_02"),
    "spine3": ("spine2", "DEF-spine.003", "spine_03"),
    "neck": ("spine3", "DEF-neck", "neck_01"),
    "head": ("neck", "DEF-head", "Head"),
}
for s, u, e in (("L", "L", "l"), ("R", "R", "r")):
    BONES.update({
        f"shoulder.{s}": ("spine3", f"DEF-shoulder.{u}", f"clavicle_{e}"),
        f"upper_arm.{s}": (f"shoulder.{s}", f"DEF-upper_arm.{u}", f"upperarm_{e}"),
        f"forearm.{s}": (f"upper_arm.{s}", f"DEF-forearm.{u}", f"lowerarm_{e}"),
        f"hand.{s}": (f"forearm.{s}", f"DEF-hand.{u}", f"hand_{e}"),
        f"thigh.{s}": ("hips", f"DEF-thigh.{u}", f"thigh_{e}"),
        f"shin.{s}": (f"thigh.{s}", f"DEF-shin.{u}", f"calf_{e}"),
        f"foot.{s}": (f"shin.{s}", f"DEF-foot.{u}", f"foot_{e}"),
        f"toe.{s}": (f"foot.{s}", f"DEF-toe.{u}", f"ball_{e}"),
    })
ORDER = list(BONES)  # parents always come first
TRUNK = {"root", "hips", "spine1", "spine2", "spine3", "neck", "head"}


def clear():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def play(obj, action):
    """Assign an action and its slot (Blender 5 actions animate nothing without a slot)."""
    obj.animation_data.action = action
    if action is not None and action.slots:
        obj.animation_data.action_slot = action.slots[0]


# ---------- load ----------
clear()
bpy.ops.import_scene.gltf(filepath=spec["in"])
char_objs = [o for o in bpy.context.scene.objects if o.type == "MESH"]
if not char_objs:
    raise SystemExit("no mesh in input")
bpy.ops.object.select_all(action="DESELECT")
for o in char_objs:
    o.select_set(True)
bpy.context.view_layer.objects.active = char_objs[0]
if len(char_objs) > 1:
    bpy.ops.object.join()
body = bpy.context.view_layer.objects.active
body.name = "Body"
bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
for o in list(bpy.context.scene.objects):
    if o is not body:
        bpy.data.objects.remove(o, do_unlink=True)

V = np.array([v.co[:] for v in body.data.vertices])
H = float(V[:, 2].max() - V[:, 2].min())
V[:, 2] -= V[:, 2].min()
x, y, z = V[:, 0], V[:, 1], V[:, 2]


# ---------- landmarks ----------
def band(z0, dz):
    return np.abs(z - z0) < dz


def find_landmarks():
    L = {}
    # feet: lowest 6% of height, split by side
    low = z < 0.06 * H
    for s, side in (("L", x > 0), ("R", x < 0)):
        f = low & side
        if f.sum() < 10:
            raise SystemExit("could not find both feet; is the model standing on the origin?")
        fx, fy = np.median(x[f]), np.median(y[f])
        heel_y = np.percentile(y[f], 90)
        toe_y = np.percentile(y[f], 3)
        L[f"ankle.{s}"] = Vector((fx, heel_y - 0.35 * (heel_y - toe_y), 0.075 * H))
        L[f"ball.{s}"] = Vector((fx, toe_y + 0.3 * (heel_y - toe_y), 0.02 * H))
        L[f"toe.{s}"] = Vector((fx, toe_y, 0.02 * H))
    ax = abs(L["ankle.L"].x - L["ankle.R"].x)
    legs_y = (L["ankle.L"].y + L["ankle.R"].y) / 2
    # crotch: first height where something fills the gap between the legs, near the legs' depth
    crotch = None
    gap = 0.18 * ax
    for zz in np.arange(0.2 * H, 0.65 * H, 0.004 * H):
        m = band(zz, 0.006 * H) & (np.abs(x) < gap) & (np.abs(y - legs_y) < 0.06 * H)
        if m.sum() >= 3:
            crotch = zz
            break
    if crotch is None or not (0.3 * H < crotch < 0.62 * H):
        stats["warnings"].append(f"crotch not found ({crotch}); using 0.47 H")
        crotch = 0.47 * H
    # hands: furthest points out to each side, above the knees
    mid = (z > 0.3 * H) & (z < 0.9 * H)
    for s, sign in (("L", 1), ("R", -1)):
        m = mid & (sign * x > 0)
        i = np.argmax(sign * x[m])
        L[f"hand_tip.{s}"] = Vector(V[m][i])
    top = np.percentile(z[np.abs(x) < 0.12 * H], 99.5)
    # shoulders: proportional height between crotch and crown, then the torso edge at that height
    sh_z = crotch + 0.66 * (top - crotch)
    chest = band(sh_z - 0.06 * H, 0.015 * H)
    half = 0.12 * H
    xs = np.sort(np.abs(x[chest]))
    if len(xs) > 10:
        # torso edge = first gap wider than 1.5% of height, walking outward from the middle
        steps = np.diff(xs)
        g = np.nonzero((steps > 0.015 * H) & (xs[:-1] > 0.05 * H))[0]
        half = float(xs[g[0]]) if len(g) else float(np.percentile(xs, 60))
    half = float(np.clip(half, 0.06 * H, 0.2 * H))
    for s, sign in (("L", 1), ("R", -1)):
        tc = band(sh_z, 0.02 * H) & (np.abs(x) < 0.1 * H)
        ty = float(np.median(y[tc])) if tc.sum() > 5 else legs_y
        L[f"shoulder.{s}"] = Vector((sign * half * 0.85, ty, sh_z))
        L[f"clavicle.{s}"] = Vector((sign * 0.02 * H, ty - 0.01 * H, sh_z + 0.005 * H))
    for name, zz in (("hips", crotch + 0.03 * H), ("neck", crotch + 0.71 * (top - crotch)),
                     ("chin", crotch + 0.77 * (top - crotch)), ("top", top)):
        c = band(zz, 0.02 * H) & (np.abs(x) < 0.08 * H)
        cy = float(np.median(y[c])) if c.sum() > 5 else legs_y
        L[name] = Vector((0.0, cy, zz))
    L["crotch"] = crotch
    return L


lm = find_landmarks()
stats["landmarks"] = {k: [round(c, 3) for c in v] if isinstance(v, Vector) else round(v, 3) for k, v in lm.items()}


# ---------- skeleton ----------
def lerp(a, b, t):
    return a + (b - a) * t


def joints():
    j = {}
    hips, neck = lm["hips"], lm["neck"]
    j["root"] = (Vector((0, 0, 0)), Vector((0, 0.1 * H, 0)))
    j["hips"] = (hips, lerp(hips, neck, 0.15))
    j["spine1"] = (lerp(hips, neck, 0.15), lerp(hips, neck, 0.42))
    j["spine2"] = (lerp(hips, neck, 0.42), lerp(hips, neck, 0.7))
    j["spine3"] = (lerp(hips, neck, 0.7), neck)
    j["neck"] = (neck, lm["chin"])
    j["head"] = (lm["chin"], lm["top"])
    for s in ("L", "R"):
        sh, tip = lm[f"shoulder.{s}"], lm[f"hand_tip.{s}"]
        elbow, wrist = lerp(sh, tip, 0.42), lerp(sh, tip, 0.8)
        elbow.y += 0.01 * H  # a slight bend back tells the solver which way elbows fold
        j[f"shoulder.{s}"] = (lm[f"clavicle.{s}"], sh)
        j[f"upper_arm.{s}"] = (sh, elbow)
        j[f"forearm.{s}"] = (elbow, wrist)
        j[f"hand.{s}"] = (wrist, tip)
        hip = Vector((lm[f"ankle.{s}"].x * 0.95, hips.y, lm["crotch"] + 0.02 * H))
        ankle = lm[f"ankle.{s}"]
        knee = lerp(hip, ankle, 0.5)
        knee.y -= 0.015 * H  # knees bend forward
        j[f"thigh.{s}"] = (hip, knee)
        j[f"shin.{s}"] = (knee, ankle)
        j[f"foot.{s}"] = (ankle, lm[f"ball.{s}"])
        j[f"toe.{s}"] = (lm[f"ball.{s}"], lm[f"toe.{s}"])
    return j


J = joints()
arm_data = bpy.data.armatures.new("Rig")
rig = bpy.data.objects.new("Rig", arm_data)
bpy.context.scene.collection.objects.link(rig)
bpy.context.view_layer.objects.active = rig
bpy.ops.object.mode_set(mode="EDIT")
for name in ORDER:
    eb = arm_data.edit_bones.new(name)
    eb.head, eb.tail = J[name]
    if (eb.tail - eb.head).length < 1e-4:
        eb.tail = eb.head + Vector((0, 0, 0.02 * H))
    parent = BONES[name][0]
    if parent:
        eb.parent = arm_data.edit_bones[parent]
        eb.use_connect = False
    eb.use_deform = name != "root"
    # roll so local Z points forward (-Y) for limbs and up/back for the spine: stable twist axes
    eb.align_roll(Vector((0, -1, 0)) if abs((eb.tail - eb.head).normalized().y) < 0.9 else Vector((0, 0, 1)))
bpy.ops.object.mode_set(mode="OBJECT")

# ---------- skin ----------
proxy = body.copy()
proxy.data = body.data.copy()
bpy.context.scene.collection.objects.link(proxy)
for m in list(proxy.modifiers):
    proxy.modifiers.remove(m)
rm = proxy.modifiers.new("remesh", "REMESH")
rm.mode = "VOXEL"
rm.voxel_size = H / 140
bpy.context.view_layer.objects.active = proxy
bpy.ops.object.select_all(action="DESELECT")
proxy.select_set(True)
bpy.ops.object.modifier_apply(modifier="remesh")
if len(proxy.data.polygons) > 60000:
    dm = proxy.modifiers.new("dec", "DECIMATE")
    dm.ratio = 60000 / len(proxy.data.polygons)
    bpy.ops.object.modifier_apply(modifier="dec")
stats["proxy_faces"] = len(proxy.data.polygons)

bpy.ops.object.select_all(action="DESELECT")
proxy.select_set(True)
rig.select_set(True)
bpy.context.view_layer.objects.active = rig
bpy.ops.object.parent_set(type="ARMATURE_AUTO")
groups = {g.name for g in proxy.vertex_groups}
unweighted = sum(1 for v in proxy.data.vertices if not v.groups)
stats["proxy_unweighted"] = unweighted
if unweighted > 0.05 * len(proxy.data.vertices):
    stats["warnings"].append(f"automatic weights left {unweighted} proxy verts unweighted; using envelope fallback")
    bpy.ops.object.parent_set(type="ARMATURE_ENVELOPE")

for name in ORDER:
    if BONES[name][0] is not None and name not in body.vertex_groups:
        body.vertex_groups.new(name=name)
dt = body.modifiers.new("weights", "DATA_TRANSFER")
dt.object = proxy
dt.use_vert_data = True
dt.data_types_verts = {"VGROUP_WEIGHTS"}
dt.vert_mapping = "POLYINTERP_NEAREST"
dt.layers_vgroup_select_src = "ALL"
dt.layers_vgroup_select_dst = "NAME"
bpy.context.view_layer.objects.active = body
bpy.ops.object.select_all(action="DESELECT")
body.select_set(True)
bpy.ops.object.modifier_apply(modifier="weights")
bpy.ops.object.vertex_group_limit_total(group_select_mode="ALL", limit=4)
bpy.ops.object.vertex_group_normalize_all(group_select_mode="ALL", lock_active=False)
bpy.data.objects.remove(proxy, do_unlink=True)
stats["body_unweighted"] = sum(1 for v in body.data.vertices if not v.groups)

body.parent = rig
am = body.modifiers.new("Armature", "ARMATURE")
am.object = rig

# ---------- retarget ----------
rest = {b.name: b.matrix_local.copy() for b in rig.data.bones}
k = lm["hips"].z / 0.917  # library hips height
clips = []
for li, path in enumerate(spec["libs"]):
    before = set(bpy.data.objects)
    before_act = set(bpy.data.actions)
    bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    lib = next(o for o in new if o.type == "ARMATURE")
    for o in new:
        if o.type == "MESH":
            bpy.data.objects.remove(o, do_unlink=True)
    lib_acts = [a for a in bpy.data.actions if a not in before_act]
    src = {t: BONES[t][1 + li] for t in ORDER}
    srest = {t: lib.data.bones[s].matrix_local.copy() for t, s in src.items()}
    # Reference pose: the library rest with each bone swung onto our rest direction
    # (T-pose arms lowered to our A-pose). Clip motion is measured from there, so a
    # hanging arm in the clip hangs on the character too.
    qinv = {}
    for t in ORDER:
        ds = (srest[t].to_3x3() @ Vector((0, 1, 0))).normalized()
        dt_ = (rest[t].to_3x3() @ Vector((0, 1, 0))).normalized()
        # Limbs copy the clip's bone directions (T-pose vs A-pose arms). The trunk and head
        # copy the change from rest instead: a head bone aimed at a hair crest or snout is
        # anatomically different, and forcing it to the mannequin's direction tips the face.
        swing = Matrix.Identity(3) if t in TRUNK else ds.rotation_difference(dt_).to_matrix()
        qinv[t] = (swing @ srest[t].to_3x3()).inverted()
    if not lib.animation_data:
        lib.animation_data_create()
    for tr in list(lib.animation_data.nla_tracks):  # the importer stacks every clip as NLA
        lib.animation_data.nla_tracks.remove(tr)
    if not rig.animation_data:
        rig.animation_data_create()
    for act in lib_acts:
        if act.name in clips:
            continue
        play(lib, act)
        out = bpy.data.actions.new(act.name)
        rig.animation_data.action = out
        f0, f1 = (int(round(v)) for v in act.frame_range)
        for f in range(f0, f1 + 1):
            bpy.context.scene.frame_set(f)
            full = {}
            for t in ORDER:
                sb = src[t]
                sp = lib.pose.bones[sb]
                Rt = rest[t]
                P = sp.matrix.to_3x3() @ qinv[t] @ Rt.to_3x3()  # clip change from the reference pose, on our rest
                parent = BONES[t][0]
                base = Rt if parent is None else full[parent] @ rest[parent].inverted() @ Rt
                rot = (base.to_3x3().inverted() @ P).to_quaternion()
                loc = Vector()
                if t in ("root", "hips"):
                    want = Rt.translation + (sp.head - lib.data.bones[sb].head_local) * k
                    loc = base.inverted() @ want
                full[t] = base @ Matrix.LocRotScale(loc, rot, None)
                pb = rig.pose.bones[t]
                pb.rotation_mode = "QUATERNION"
                pb.rotation_quaternion = rot
                pb.keyframe_insert("rotation_quaternion", frame=f - f0, group=t)
                if t in ("root", "hips"):
                    pb.location = loc
                    pb.keyframe_insert("location", frame=f - f0, group=t)
        clips.append(act.name)
    rig.animation_data.action = None
    bpy.data.objects.remove(lib, do_unlink=True)
    for a in lib_acts:
        bpy.data.actions.remove(a)

for a in bpy.data.actions:  # created while the library's same-named clips existed
    if a.name.endswith(".001"):
        a.name = a.name[:-4]
stats["clips"] = clips
for a in bpy.data.actions:
    a.use_fake_user = True

# ---------- review renders ----------
if spec.get("renders"):
    import os
    os.makedirs(spec["renders"], exist_ok=True)
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_EEVEE"
    sc.render.resolution_x = sc.render.resolution_y = 512
    sc.render.film_transparent = True
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    sc.collection.objects.link(cam)
    sc.camera = cam
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = H * 1.35
    cam.location = (H * 0.9, -H * 2.2, H * 0.6)
    cam.rotation_euler = (math.radians(84), 0, math.radians(22))
    for e, rot in ((3.0, (0.8, 0.2, -0.6)), (1.2, (0.9, 0, 2.6))):
        sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
        sun.data.energy = e
        sun.rotation_euler = rot
        sc.collection.objects.link(sun)
    world = bpy.data.worlds.new("w")
    world.color = (0.25, 0.25, 0.27)
    sc.world = world
    for name, frac in spec.get("poses", [("rest", None), ("Walk_Loop", 0.25), ("Punch_Cross", 0.4),
                                         ("Sitting_Idle_Loop", 0.5), ("Zombie_Walk_Fwd_Loop", 0.3)]):
        a = bpy.data.actions.get(name)
        play(rig, a)
        if a:
            sc.frame_set(int(a.frame_range[1] * frac))
        else:
            for pb in rig.pose.bones:
                pb.rotation_quaternion, pb.location = (1, 0, 0, 0), (0, 0, 0)
            sc.frame_set(0)
        sc.render.filepath = os.path.join(spec["renders"], f"{name}.png")
        bpy.ops.render.render(write_still=True)
    play(rig, bpy.data.actions.get("Idle_Loop"))

# ---------- export ----------
bpy.ops.object.select_all(action="DESELECT")
body.select_set(True)
rig.select_set(True)
bpy.ops.export_scene.gltf(filepath=spec["out"], use_selection=True, export_format="GLB",
                          export_animations=True, export_animation_mode="ACTIONS",
                          export_skins=True, export_image_format="AUTO", export_jpeg_quality=92)
stats["bones"] = len(rig.data.bones)
stats["height_m"] = round(H, 3)
json.dump(stats, open(spec["stats"], "w"), indent=1)
print("RIG OK", json.dumps({k: stats[k] for k in ("bones", "proxy_unweighted", "body_unweighted", "warnings")}))

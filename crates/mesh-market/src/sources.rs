//! The four Market sources: search and grab for each.
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};

use crate::{cached, download, get, item, merr, unzip};

pub type Search = fn(&str, &str, u32) -> Result<(Vec<Value>, u64)>;
pub type Grab = fn(&str, &str, &str, &Path) -> Result<Vec<PathBuf>>;

pub struct Source {
    pub id: &'static str,
    pub label: &'static str,
    pub kinds: &'static [&'static str],
    pub key: Option<&'static str>,
    /// What the key unlocks: "search" or "download".
    pub key_for: Option<&'static str>,
    pub licence: &'static str,
    pub note: &'static str,
    pub search: Search,
    pub grab: Grab,
}

pub static SOURCES: &[Source] = &[
    Source {
        id: "polyhaven",
        label: "Poly Haven",
        kinds: &["model", "material", "hdri"],
        key: None,
        key_for: None,
        licence: "CC0",
        note: "Photoscanned props, furniture, rocks; textures; HDRI skies. All CC0.",
        search: polyhaven_search,
        grab: polyhaven_grab,
    },
    Source {
        id: "ambientcg",
        label: "ambientCG",
        kinds: &["material", "hdri"],
        key: None,
        key_for: None,
        licence: "CC0",
        note: "Thousands of PBR materials and HDRIs. All CC0.",
        search: ambientcg_search,
        grab: ambientcg_grab,
    },
    Source {
        id: "sketchfab",
        label: "Sketchfab",
        kinds: &["model"],
        key: Some("SKETCHFAB_TOKEN"),
        key_for: Some("download"),
        licence: "per model",
        note: "The biggest free library. Licences vary (CC-BY needs credit); shown on every card.",
        search: sketchfab_search,
        grab: sketchfab_grab,
    },
    Source {
        id: "polypizza",
        label: "Poly Pizza",
        kinds: &["model"],
        key: Some("POLYPIZZA_KEY"),
        key_for: Some("search"),
        licence: "CC0 / CC-BY",
        note: "Low-poly game models, including Quaternius and Kenney packs.",
        search: polypizza_search,
        grab: polypizza_grab,
    },
];

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn file_name(url: &str) -> String {
    url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or("file").to_string()
}

// ---------- Poly Haven ----------

fn ph_type(kind: &str) -> &'static str {
    match kind {
        "model" => "models",
        "material" => "textures",
        _ => "hdris",
    }
}

fn polyhaven_search(q: &str, kind: &str, page: u32) -> Result<(Vec<Value>, u64)> {
    const PER: usize = 40;
    // The whole list for a kind, cached for a day (it's ~1 MB and rarely changes).
    let cat = cached(&format!("polyhaven-{kind}.json"), || get(&format!("https://api.polyhaven.com/assets?t={}", ph_type(kind)), &[], &[]))?;
    let words: Vec<String> = q.to_lowercase().split_whitespace().map(str::to_string).collect();
    let mut hits: Vec<(i64, &String, &Value)> = vec![];
    for (id, a) in cat.as_object().into_iter().flatten() {
        let mut hay = vec![id.clone(), s(&a["name"])];
        for k in ["tags", "categories"] {
            hay.extend(a[k].as_array().into_iter().flatten().map(s));
        }
        let hay = hay.join(" ").to_lowercase();
        if words.iter().all(|w| hay.contains(w.as_str())) {
            hits.push((a["download_count"].as_i64().unwrap_or(0), id, a));
        }
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.0));
    let total = hits.len() as u64;
    let out = hits
        .into_iter()
        .skip(page as usize * PER)
        .take(PER)
        .map(|(_, id, a)| {
            let authors: Vec<&String> = a["authors"].as_object().map(|m| m.keys().collect()).unwrap_or_default();
            item(
                "polyhaven",
                id,
                a["name"].as_str().unwrap_or(id),
                kind,
                &format!("https://cdn.polyhaven.com/asset_img/thumbs/{id}.png?width=256&height=256"),
                "CC0",
                &authors.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                &format!("https://polyhaven.com/a/{id}"),
                json!({"polycount": a["polycount"]}),
            )
        })
        .collect();
    Ok((out, total))
}

fn polyhaven_grab(id: &str, kind: &str, res: &str, d: &Path) -> Result<Vec<PathBuf>> {
    let files = get(&format!("https://api.polyhaven.com/files/{id}"), &[], &[])?;
    let mut got = vec![];
    match kind {
        "model" => {
            let gltf = &files["gltf"];
            let pick = if gltf[res].is_object() { &gltf[res] } else { gltf.as_object().and_then(|m| m.keys().min().map(|k| &gltf[k])).unwrap_or(&Value::Null) };
            let g = &pick["gltf"];
            let url = s(&g["url"]);
            let main = download(&url, &d.join(file_name(&url)), &[])?;
            for (rel, inc) in g["include"].as_object().into_iter().flatten() {
                if rel.contains("..") {
                    continue;
                }
                download(&s(&inc["url"]), &d.join(rel), &[])?;
            }
            got.push(main);
        }
        "material" => {
            for m in ["Diffuse", "nor_gl", "Rough", "AO", "Displacement", "arm", "Metal"] {
                let f = &files[m][res];
                let f = if f["jpg"].is_object() { &f["jpg"] } else { &f["png"] };
                if let Some(url) = f["url"].as_str() {
                    got.push(download(url, &d.join(file_name(url)), &[])?);
                }
            }
        }
        _ => {
            let h = if files["hdri"][res].is_object() { &files["hdri"][res] } else { &files["hdri"]["1k"] };
            let url = s(&h["hdr"]["url"]);
            got.push(download(&url, &d.join(file_name(&url)), &[])?);
        }
    }
    Ok(got)
}

// ---------- ambientCG ----------

fn acg_type(kind: &str) -> Option<&'static str> {
    match kind {
        "material" => Some("Material"),
        "hdri" => Some("HDRI"),
        _ => None,
    }
}

fn ambientcg_search(q: &str, kind: &str, page: u32) -> Result<(Vec<Value>, u64)> {
    const PER: u32 = 40;
    let Some(t) = acg_type(kind) else { return Ok((vec![], 0)) };
    let r = get(
        "https://ambientcg.com/api/v2/full_json",
        &[
            ("q", q.into()),
            ("type", t.into()),
            ("limit", PER.to_string()),
            ("offset", (page * PER).to_string()),
            ("sort", "Popular".into()),
            ("include", "previewData".into()),
        ],
        &[],
    )?;
    let out: Vec<Value> = r["foundAssets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["dataType"] == t)
        .map(|a| {
            let id = s(&a["assetId"]);
            let page = a["shortLink"].as_str().map(str::to_string).unwrap_or_else(|| format!("https://ambientcg.com/view?id={id}"));
            item("ambientcg", &id, a["displayName"].as_str().unwrap_or(&id), kind, &s(&a["previewImage"]["256-PNG"]), "CC0", "ambientCG", &page, json!({}))
        })
        .collect();
    let total = r["numberOfResults"].as_u64().unwrap_or(out.len() as u64);
    Ok((out, total))
}

fn ambientcg_grab(id: &str, _kind: &str, res: &str, d: &Path) -> Result<Vec<PathBuf>> {
    let r = get("https://ambientcg.com/api/v2/full_json", &[("id", id.into()), ("include", "downloadData".into())], &[])?;
    let a = &r["foundAssets"][0];
    let downloads: Vec<&Value> = a["downloadFolders"]["default"]["downloadFiletypeCategories"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .flat_map(|c| c["downloads"].as_array().into_iter().flatten())
        .collect();
    let up = res.to_uppercase();
    let want = [format!("{up}-JPG"), up.clone(), format!("{up}-HDR")];
    let pick = want
        .iter()
        .find_map(|w| downloads.iter().find(|x| x["attribute"] == w.as_str()))
        .or(downloads.first())
        .ok_or_else(|| merr("no downloads for this asset"))?;
    let z = download(&s(&pick["fullDownloadPath"]), &d.join(s(&pick["fileName"])), &[])?;
    if z.extension().is_some_and(|e| e == "zip") {
        unzip(&z, d)?;
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(d)?.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n != "meta.json")).collect();
    files.sort_by_key(|p| {
        let n = p.file_name().unwrap().to_string_lossy().into_owned();
        (!n.contains("Color"), n)
    });
    Ok(files)
}

// ---------- Sketchfab ----------

fn sketchfab_search(q: &str, kind: &str, page: u32) -> Result<(Vec<Value>, u64)> {
    const PER: u32 = 24;
    if kind != "model" {
        return Ok((vec![], 0));
    }
    let r = get(
        "https://api.sketchfab.com/v3/search",
        &[
            ("type", "models".into()),
            ("q", q.into()),
            ("downloadable", "true".into()),
            ("count", PER.to_string()),
            ("cursor", (page * PER).to_string()),
            ("sort_by", "-likeCount".into()),
        ],
        &[],
    )?;
    let out: Vec<Value> = r["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let thumb = m["thumbnails"]["images"]
                .as_array()
                .and_then(|imgs| imgs.iter().min_by_key(|i| (i["width"].as_i64().unwrap_or(0) - 256).abs()))
                .map(|i| s(&i["url"]))
                .unwrap_or_default();
            item(
                "sketchfab",
                &s(&m["uid"]),
                &s(&m["name"]),
                "model",
                &thumb,
                m["license"]["label"].as_str().unwrap_or("see page"),
                &s(&m["user"]["displayName"]),
                &s(&m["viewerUrl"]),
                json!({"polycount": m["faceCount"], "animated": m["animationCount"].as_i64().unwrap_or(0) > 0}),
            )
        })
        .collect();
    let total = r["totalCount"].as_u64().unwrap_or(out.len() as u64);
    Ok((out, total))
}

fn sketchfab_grab(id: &str, _kind: &str, _res: &str, d: &Path) -> Result<Vec<PathBuf>> {
    let token = mesh_core::keys::get("SKETCHFAB_TOKEN")
        .ok_or_else(|| merr("Sketchfab downloads need a free API token: sketchfab.com > Settings > Password & API. Add it under Keys."))?;
    let auth = [("Authorization", format!("Token {token}"))];
    let r = get(&format!("https://api.sketchfab.com/v3/models/{id}/download"), &[], &auth)?;
    if let Some(url) = r["glb"]["url"].as_str() {
        return Ok(vec![download(url, &d.join("model.glb"), &[])?]);
    }
    let z = download(&s(&r["gltf"]["url"]), &d.join("model.zip"), &[])?;
    unzip(&z, d)?;
    let mut found = vec![];
    walk(d, &mut |p| {
        if p.extension().is_some_and(|e| e == "gltf") {
            found.push(p.to_path_buf());
        }
    });
    found.sort();
    Ok(found)
}

fn walk(d: &Path, f: &mut dyn FnMut(&Path)) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() { walk(&p, f) } else { f(&p) }
    }
}

pub fn sketchfab_info(id: &str) -> Result<Value> {
    let m = get(&format!("https://api.sketchfab.com/v3/models/{id}"), &[], &[])?;
    Ok(json!({"name": m["name"], "author": m["user"]["displayName"], "page": m["viewerUrl"], "licence": m["license"]["label"].as_str().unwrap_or("see page")}))
}

// ---------- Poly Pizza ----------

fn pp_headers() -> Result<[(&'static str, String); 1]> {
    let key = mesh_core::keys::get("POLYPIZZA_KEY").ok_or_else(|| merr("Poly Pizza needs a free API key: poly.pizza/settings/api. Add it under Keys."))?;
    Ok([("x-auth-token", key)])
}

fn polypizza_search(q: &str, kind: &str, page: u32) -> Result<(Vec<Value>, u64)> {
    const PER: u32 = 32;
    if kind != "model" {
        return Ok((vec![], 0));
    }
    let q = if q.is_empty() { "prop" } else { q };
    let enc: String =
        q.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    let r = get(&format!("https://api.poly.pizza/v1.1/search/{enc}"), &[("Limit", PER.to_string()), ("Page", page.to_string())], &pp_headers()?)?;
    let out: Vec<Value> = r["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let id = m["ID"].as_str().map(str::to_string).unwrap_or_else(|| m["ID"].to_string());
            item(
                "polypizza",
                &id,
                &s(&m["Title"]),
                "model",
                &s(&m["Thumbnail"]),
                m["Licence"].as_str().unwrap_or("see page"),
                &s(&m["Creator"]["Username"]),
                &format!("https://poly.pizza/m/{id}"),
                json!({"polycount": m["Tri Count"], "animated": m["Animated"].as_bool().unwrap_or(false), "download": m["Download"]}),
            )
        })
        .collect();
    let total = r["total"].as_u64().unwrap_or(out.len() as u64);
    Ok((out, total))
}

fn polypizza_grab(id: &str, _kind: &str, _res: &str, d: &Path) -> Result<Vec<PathBuf>> {
    let m = get(&format!("https://api.poly.pizza/v1.1/model/{id}"), &[], &pp_headers()?)?;
    Ok(vec![download(&s(&m["Download"]), &d.join("model.glb"), &[])?])
}

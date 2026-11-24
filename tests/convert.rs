//! Native archives are read from EQ_ASSETS (default: ~/eq_assets/EQ_Files).
//! Run the ELF gate explicitly:
//! EQ_ASSETS=/path/to/assets cargo test --test convert elf_glb_has_head_regions_and_face_variants -- --ignored --exact --nocapture
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use sha2::Digest;
use std::path::PathBuf;

#[test]
#[ignore]
fn converts_humanoid_archive_to_glb() {
    let raw = PathBuf::from(
        std::env::var("EQ_ASSETS")
            .unwrap_or_else(|_| format!("{}/eq_assets/EQ_Files", std::env::var("HOME").unwrap())),
    );
    let src = raw.join("globalhum_chr.s3d");
    if !src.exists() {
        eprintln!("skip: {src:?} missing");
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let glb = out.path().join("humanoid.glb");
    eqoxide_asset_server::convert::s3d_to_glb(&src, &glb, true).unwrap();
    assert!(glb.metadata().unwrap().len() > 0);
}

/// ELF archive contract, matching the renderer's eq_face/eq_head_part selection:
/// every face has skin in regions 1/4/5 and painted scalp hair in region 1;
/// region 2 is fixed crown hair, region 6 is untinted ear skin, and all five
/// fixed regions remain visible. These are native region bindings, not generated
/// hair shells. Commit 2b7dafc replaced the obsolete hairstyle interpretation.
fn check_elf_head_contract(doc: &Value, bin: &[u8]) -> Result<()> {
    let prims = doc["meshes"][0]["primitives"]
        .as_array()
        .context("mesh primitives")?;
    let mats = doc["materials"].as_array().context("materials")?;
    for prim in prims {
        ensure!(
            prim["extras"]["eq_hairstyle"].is_null(),
            "obsolete eq_hairstyle tag"
        );
        if !prim["extras"]["eq_face"].is_null() {
            let face = prim["extras"]["eq_face"]
                .as_u64()
                .context("integer eq_face")?;
            let mi = prim["material"].as_u64().context("face material index")? as usize;
            let name = mats
                .get(mi)
                .and_then(|m| m["name"].as_str())
                .context("face material")?;
            ensure!(
                face <= 7
                    && [1, 4, 5]
                        .iter()
                        .any(|n| name == format!("elfhesk{face}{n}")),
                "face {face} bound to wrong region/material {name}"
            );
        }
    }

    let check_region = |name: &str, face: Option<u64>, parts: &[Option<&str>]| -> Result<()> {
        let matches: Vec<_> = mats
            .iter()
            .enumerate()
            .filter(|(_, m)| m["name"].as_str() == Some(name))
            .collect();
        ensure!(
            matches.len() == 1,
            "{name}: expected one material, found {}",
            matches.len()
        );
        let (mi, mat) = matches[0];
        let region: Vec<_> = prims
            .iter()
            .filter(|p| p["material"].as_u64() == Some(mi as u64))
            .collect();
        ensure!(
            region.len() == parts.len(),
            "{name}: expected {} primitives, found {}",
            parts.len(),
            region.len()
        );
        for part in parts {
            ensure!(
                region
                    .iter()
                    .filter(|p| {
                        let tag = &p["extras"]["eq_head_part"];
                        match part {
                            Some(part) => tag.as_str() == Some(*part),
                            None => tag.is_null(),
                        }
                    })
                    .count()
                    == 1,
                "{name}: missing/duplicated head part {part:?}"
            );
        }
        for prim in region {
            let ex = &prim["extras"];
            ensure!(
                match face {
                    Some(f) => ex["eq_face"].as_u64() == Some(f),
                    None => ex["eq_face"].is_null(),
                },
                "{name}: wrong face tag"
            );
            ensure!(
                if face.is_some_and(|f| f > 0) {
                    ex["eq_default_hidden"].as_bool() == Some(true)
                } else {
                    ex["eq_default_hidden"].is_null()
                },
                "{name}: wrong default visibility"
            );
            let ai = prim["indices"].as_u64().context("index accessor")? as usize;
            ensure!(
                doc["accessors"][ai]["count"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n % 3 == 0),
                "{name}: missing triangle indices"
            );
        }

        // Resolve the whole material -> texture -> named image -> embedded PNG
        // chain; an arbitrary numeric texture index must not satisfy the oracle.
        let ti = mat["pbrMetallicRoughness"]["baseColorTexture"]["index"]
            .as_u64()
            .with_context(|| format!("{name}: missing base-color texture"))?
            as usize;
        let ii = doc["textures"][ti]["source"]
            .as_u64()
            .context("texture source")? as usize;
        let image = &doc["images"][ii];
        ensure!(
            image["name"].as_str() == Some(name),
            "{name}: wrong texture image"
        );
        ensure!(
            image["mimeType"] == "image/png",
            "{name}: expected embedded PNG"
        );
        let vi = image["bufferView"].as_u64().context("image bufferView")? as usize;
        let view = &doc["bufferViews"][vi];
        ensure!(view["buffer"].as_u64() == Some(0), "{name}: image buffer");
        let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = view["byteLength"].as_u64().context("image byteLength")? as usize;
        let end = start.checked_add(len).context("image range overflow")?;
        let png = bin.get(start..end).context("image outside GLB buffer")?;
        ensure!(
            png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "{name}: missing PNG bytes"
        );
        Ok(())
    };
    for face in 0..=7 {
        check_region(
            &format!("elfhesk{face}1"),
            Some(face),
            &[None, Some("hair")],
        )?;
        for n in [4, 5] {
            check_region(&format!("elfhesk{face}{n}"), Some(face), &[None])?;
        }
    }
    check_region("elfhesk02", None, &[Some("hair")])?;
    for n in [3, 6, 7, 8] {
        check_region(&format!("elfhesk0{n}"), None, &[None])?;
    }
    Ok(())
}

/// Bake once, check every native region, and prove the oracle rejects malformed
/// copies of that same output. Selecting this ignored gate requires the archive.
#[test]
#[ignore = "requires globalelf_chr.s3d in EQ_ASSETS (or ~/eq_assets/EQ_Files)"]
fn elf_glb_has_head_regions_and_face_variants() {
    let raw = PathBuf::from(
        std::env::var("EQ_ASSETS")
            .unwrap_or_else(|_| format!("{}/eq_assets/EQ_Files", std::env::var("HOME").unwrap())),
    );
    let src = raw.join("globalelf_chr.s3d");
    assert!(
        src.is_file(),
        "required native archive {src:?} missing; set EQ_ASSETS"
    );
    let out = tempfile::tempdir().unwrap();
    let glb = out.path().join("elf.glb");
    eqoxide_asset_server::convert::s3d_to_glb(&src, &glb, true).unwrap();

    let glb_data = std::fs::read(&glb).unwrap();
    eprintln!(
        "native ELF GLB: {} bytes, sha256={:x}",
        glb_data.len(),
        sha2::Sha256::digest(&glb_data)
    );
    let parsed = gltf::binary::Glb::from_slice(&glb_data).unwrap();
    let doc: Value = serde_json::from_slice(&parsed.json).unwrap();
    let bin = parsed.bin.as_deref().expect("embedded GLB buffer");
    check_elf_head_contract(&doc, bin).unwrap();

    // Locate by material/part, never by incidental primitive order.
    let material = |name: &str| {
        doc["materials"]
            .as_array()
            .unwrap()
            .iter()
            .position(|m| m["name"] == name)
            .unwrap()
    };
    let primitive = |name: &str, hair: bool| {
        doc["meshes"][0]["primitives"]
            .as_array()
            .unwrap()
            .iter()
            .position(|p| {
                p["material"].as_u64() == Some(material(name) as u64)
                    && (p["extras"]["eq_head_part"] == "hair") == hair
            })
            .unwrap()
    };
    let skin = primitive("elfhesk01", false);
    let scalp = primitive("elfhesk01", true);
    let ear = primitive("elfhesk06", false);
    let reject = |label: &str, bad: &Value| {
        let err = check_elf_head_contract(bad, bin).expect_err(label);
        eprintln!("negative control {label}: rejected ({err})");
    };
    let mutate_primitive = |label: &str, pi: usize, key: &str, value: Value| {
        let mut bad = doc.clone();
        bad["meshes"][0]["primitives"][pi]["extras"][key] = value;
        reject(label, &bad);
    };
    mutate_primitive("wrong face ID", skin, "eq_face", 1.into());
    mutate_primitive(
        "visible nondefault face",
        primitive("elfhesk14", false),
        "eq_default_hidden",
        false.into(),
    );
    mutate_primitive(
        "hidden default face",
        scalp,
        "eq_default_hidden",
        true.into(),
    );
    mutate_primitive("untinted scalp", scalp, "eq_head_part", Value::Null);
    mutate_primitive("tinted facial skin", skin, "eq_head_part", "hair".into());
    mutate_primitive("tinted ear", ear, "eq_head_part", "hair".into());
    mutate_primitive("hidden fixed ear", ear, "eq_default_hidden", true.into());
    mutate_primitive("face-swapped fixed ear", ear, "eq_face", 0.into());
    mutate_primitive(
        "untinted crown",
        primitive("elfhesk02", true),
        "eq_head_part",
        Value::Null,
    );
    let mut bad = doc.clone();
    let ex = bad["meshes"][0]["primitives"][skin]["extras"]
        .as_object_mut()
        .unwrap();
    let face = ex.remove("eq_face").unwrap();
    ex.insert("eq_hairstyle".into(), face);
    reject("obsolete hairstyle schema", &bad);

    // Keep aggregate counts unchanged while losing one face/region binding.
    let mut bad = doc.clone();
    let pi = primitive("elfhesk74", false);
    bad["meshes"][0]["primitives"][pi] =
        doc["meshes"][0]["primitives"][primitive("elfhesk75", false)].clone();
    reject("missing region replaced by duplicate", &bad);
    let mut bad = doc.clone();
    bad["meshes"][0]["primitives"]
        .as_array_mut()
        .unwrap()
        .remove(ear);
    reject("missing ear", &bad);

    for (label, value) in [
        ("untextured head", Value::Null),
        ("invalid texture index", u64::MAX.into()),
    ] {
        let mut bad = doc.clone();
        bad["materials"][material("elfhesk01")]["pbrMetallicRoughness"]["baseColorTexture"]
            ["index"] = value;
        reject(label, &bad);
    }
    let ti = doc["materials"][material("elfhesk01")]["pbrMetallicRoughness"]["baseColorTexture"]
        ["index"]
        .as_u64()
        .unwrap() as usize;
    let ii = doc["textures"][ti]["source"].as_u64().unwrap() as usize;
    let vi = doc["images"][ii]["bufferView"].as_u64().unwrap() as usize;
    let mut bad = doc.clone();
    bad["textures"][ti]["source"] = u64::MAX.into();
    reject("invalid image index", &bad);
    let mut bad = doc.clone();
    bad["images"][ii]["name"] = "elfhesk11".into();
    reject("wrong face texture", &bad);
    let mut bad = doc.clone();
    bad["images"][ii]["bufferView"] = u64::MAX.into();
    reject("invalid image view", &bad);
    let mut bad = doc.clone();
    bad["bufferViews"][vi]["byteLength"] = (bin.len() as u64 + 1).into();
    reject("image outside embedded buffer", &bad);
    let mut bad = doc.clone();
    bad["bufferViews"][vi]["byteLength"] = 0.into();
    reject("empty image bytes", &bad);
    check_elf_head_contract(&doc, bin).unwrap(); // Restore/control: original still passes.
}

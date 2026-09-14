use libeq_pfs::PfsWriter;
use std::{io::Cursor, process::Command};

fn words(out: &mut Vec<u8>, values: impl IntoIterator<Item = u32>) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn string(table: &mut Vec<u8>, text: &str) -> u32 {
    let offset = table.len() as u32;
    table.extend(text.as_bytes());
    table.push(0);
    offset
}
fn mesh(terrain: bool) -> Vec<u8> {
    let mut strings = Vec::new();
    let mat = string(&mut strings, "material");
    let shader = string(&mut strings, "shader");
    let prop = string(&mut strings, "e_TextureDiffuse0");
    let texture = string(&mut strings, "Tex.PNG");
    let unused = string(&mut strings, "missing.png");
    let version = if terrain { 3 } else { 2 };
    let mut out = if terrain { b"EQGT" } else { b"EQGM" }.to_vec();
    words(&mut out, [version, strings.len() as u32, 2, 3, 2]);
    if !terrain {
        words(&mut out, [0]);
    }
    out.extend(strings);
    for tex in [texture, unused] {
        words(&mut out, [0, mat, shader, 1, prop, 2, tex]);
    }
    for p in [[1.0f32, 2.0, 3.0], [4.0, 2.0, 3.0], [1.0, 5.0, 6.0]] {
        words(
            &mut out,
            p.into_iter().chain([0.0, 1.0, 0.0]).map(f32::to_bits),
        );
        if version == 3 {
            words(&mut out, [0xff808080]);
        }
        words(&mut out, [0.25f32, 0.75].map(f32::to_bits));
        if version == 3 {
            words(&mut out, [0.0f32, 0.0].map(f32::to_bits));
        }
    }
    words(&mut out, [0, 1, 2, 0, 0, 2, 1, 0, u32::MAX, 0x10000]);
    if version == 2 {
        words(&mut out, [0]);
    }
    out
}
fn zone(version: u32) -> Vec<u8> {
    let mut strings = Vec::new();
    let terrain = string(&mut strings, "terrain.ter");
    let object = string(&mut strings, "object.mod");
    let name = string(&mut strings, "instance");
    let mut out = b"EQGZ".to_vec();
    words(&mut out, [version, strings.len() as u32, 2, 3, 0, 0]);
    out.extend(strings);
    words(&mut out, [terrain, object]);
    for (model, p, r, s) in [
        (0, [99.0f32, 99.0, 99.0], [0.0f32; 3], 1.0f32),
        (
            1,
            [10.0, 20.0, 30.0],
            [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
            2.0,
        ),
        (1, [-1.0, 0.0, 1.0], [0.0; 3], 1.0),
    ] {
        words(&mut out, [model, name]);
        words(
            &mut out,
            p.into_iter().chain(r).chain([s]).map(f32::to_bits),
        );
        if version == 2 {
            words(&mut out, [0]);
        }
    }
    out
}
fn fixture(
    version: u32,
    texture: bool,
    corrupt: bool,
    extra_terrain: bool,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zone.eqg");
    let mut pfs = PfsWriter::create(std::fs::File::create(&path).unwrap()).unwrap();
    pfs.insert("ZONE.ZON", Cursor::new(zone(version))).unwrap();
    pfs.insert("TERRAIN.TER", Cursor::new(mesh(true))).unwrap();
    pfs.insert("Object.Mod", Cursor::new(mesh(false))).unwrap();
    if extra_terrain {
        pfs.insert("another.ter", Cursor::new(mesh(true))).unwrap();
    }
    if texture {
        let bytes = if corrupt {
            b"broken image".to_vec()
        } else {
            let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
            let mut out = Cursor::new(Vec::new());
            image.write_to(&mut out, image::ImageFormat::Png).unwrap();
            out.into_inner()
        };
        pfs.insert("tEX.pNG", Cursor::new(bytes)).unwrap();
    }
    pfs.finish().unwrap();
    (dir, path)
}
fn export(archive: &std::path::Path, out: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
        .arg("export-eqg-preview")
        .arg("--archive")
        .arg(archive)
        .args(["--member", "zone.zon", "--out"])
        .arg(out)
        .output()
        .unwrap()
}
#[test]
fn preview_shares_meshes_and_places_objects_without_moving_terrain() {
    for version in [1, 2] {
        let (dir, archive) = fixture(version, true, false, false);
        let output = dir.path().join("preview.glb");
        let result = export(&archive, &output);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["terrain_nodes"], 1);
        assert_eq!(report["instance_nodes"], 2);
        assert_eq!(report["textures"], 1);
        assert_eq!(report["materials"], 2);
        assert_eq!(report["skipped_placements"].as_array().unwrap().len(), 1);
        assert!(!report["approximations"].as_array().unwrap().is_empty());
        let (doc, buffers, images) = gltf::import(&output).unwrap();
        assert_eq!(doc.meshes().len(), 2);
        assert_eq!(doc.nodes().len(), 3);
        assert_eq!(images.len(), 1);
        let nodes: Vec<_> = doc.nodes().collect();
        assert!(nodes.iter().all(|n| n.children().len() == 0));
        assert_ne!(
            nodes[0].mesh().unwrap().index(),
            nodes[1].mesh().unwrap().index()
        );
        assert_eq!(
            nodes[1].mesh().unwrap().index(),
            nodes[2].mesh().unwrap().index()
        );
        for (node, expected) in
            nodes
                .iter()
                .zip([[1.0, 3.0, -2.0], [6.0, 36.0, -22.0], [0.0, 4.0, -2.0]])
        {
            let primitive = node.mesh().unwrap().primitives().next().unwrap();
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));
            assert_eq!(reader.read_indices().unwrap().into_u32().count(), 3);
            assert_eq!(
                reader
                    .read_tex_coords(0)
                    .unwrap()
                    .into_f32()
                    .next()
                    .unwrap(),
                [0.25, 0.75]
            );
            let position = glam::Vec3::from_array(reader.read_positions().unwrap().next().unwrap());
            let transform = glam::Mat4::from_cols_array_2d(&node.transform().matrix());
            let actual = transform.transform_point3(position);
            assert!(
                (actual - glam::Vec3::from_array(expected)).length() < 0.0001,
                "{actual:?} != {expected:?}"
            );
        }
        let before = std::fs::read(&output).unwrap();
        let repeated = export(&archive, &output);
        assert!(repeated.status.success());
        assert_eq!(before, std::fs::read(&output).unwrap());
        assert_eq!(result.stdout, repeated.stdout);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
#[test]
fn preview_failures_preserve_existing_output_and_leave_no_temporary_files() {
    for (texture, corrupt, extra, context) in [
        (false, false, false, "Tex.PNG"),
        (true, true, false, "decode"),
        (true, false, true, "terrain"),
    ] {
        let (dir, archive) = fixture(2, texture, corrupt, extra);
        let out = dir.path().join("preview.glb");
        std::fs::write(&out, b"existing output").unwrap();
        let result = export(&archive, &out);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .to_ascii_lowercase()
                .contains(&context.to_ascii_lowercase()),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(std::fs::read(&out).unwrap(), b"existing output");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

#[test]
fn preview_rejects_unsupported_source_states_before_replacing_output() {
    use eqoxide_asset_server::eqg::{DescriptorSource, export::export_preview, load_binary_zone};
    for case in ["diffuse type", "null slot", "scale", "empty terrain"] {
        let (dir, archive) = fixture(2, true, false, false);
        let mut scene = load_binary_zone(&archive, DescriptorSource::Archive("zone.zon")).unwrap();
        match case {
            "diffuse type" => scene.meshes[1].materials[0].properties[0].kind = 0,
            "null slot" => scene.model_slots[1] = None,
            "scale" => scene.placements[1].scale = 0.0,
            "empty terrain" => {
                for tri in &mut scene.meshes[0].triangles {
                    tri.material_index = u32::MAX;
                }
            }
            _ => unreachable!(),
        }
        let out = dir.path().join("preview.glb");
        std::fs::write(&out, b"previous output").unwrap();
        assert!(export_preview(&scene, &out).is_err(), "{case}");
        assert_eq!(std::fs::read(&out).unwrap(), b"previous output", "{case}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

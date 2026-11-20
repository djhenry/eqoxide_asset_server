use eqoxide_asset_server::editable_visual::export_editable_visual;
use eqoxide_asset_server::static_scene::{
    write_static_visual_with_surface_profile, StaticAlphaMode, StaticInstance, StaticMaterial,
    StaticMaterialPolicy, StaticMaterialProfile, StaticMesh, StaticPrimitive, StaticScene,
    StaticTexture,
};
use glam::Mat4;
use serde_json::{json, value::RawValue, Value};
use std::{borrow::Cow, collections::BTreeMap, io::Cursor};

pub(crate) type RawObject = BTreeMap<String, Box<RawValue>>;

pub(crate) fn png(pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 1, image::Rgba(pixel))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

pub(crate) fn reference() -> Vec<u8> {
    let scene = StaticScene {
        meshes: vec![StaticMesh {
            name: "original mesh".into(),
            positions: vec![
                [0., 0., 0.],
                [1.25, 0., 0.],
                [0., 2.5, 0.],
                [3.5, 0., 0.],
                [4.75, 0., 0.],
                [3.5, 2.5, 0.],
            ],
            normals: vec![[0., 0., 1.]; 6],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.], [0., 0.], [1., 0.], [0., 1.]],
            primitives: vec![
                StaticPrimitive {
                    indices: vec![0, 1, 2],
                    material_index: 0,
                    colors: Some(vec![[0.125, 0.375, 0.625, 0.875]; 6]),
                },
                StaticPrimitive {
                    indices: vec![3, 4, 5],
                    material_index: 1,
                    colors: Some(vec![[0.75, 0.5, 0.25, 0.375]; 6]),
                },
            ],
        }],
        materials: vec![
            StaticMaterial {
                name: "shared name".into(),
                base_color: [0.25, 0.5, 0.75, 1.],
                texture_index: Some(0),
                alpha_mode: StaticAlphaMode::Opaque,
                double_sided: false,
            },
            StaticMaterial {
                name: "shared name".into(),
                base_color: [0.75, 0.5, 0.25, 0.625],
                texture_index: Some(0),
                alpha_mode: StaticAlphaMode::Mask { cutoff: 0.35 },
                double_sided: true,
            },
        ],
        textures: vec![StaticTexture {
            name: "shared PNG".into(),
            png_bytes: png([21, 93, 177, 221]),
        }],
        instances: vec![StaticInstance {
            mesh_index: 0,
            matrix: Mat4::IDENTITY.to_cols_array_2d(),
        }],
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reference.glb");
    write_static_visual_with_surface_profile(
        &scene,
        &StaticMaterialPolicy::default(),
        StaticMaterialProfile::ScalarPbrHdrBlendV1,
        &"5".repeat(64),
        &path,
    )
    .unwrap();
    let original = std::fs::read(path).unwrap();
    let mut packed = gltf::binary::Glb::from_slice(&original).unwrap();
    let mut root: RawObject = serde_json::from_slice(&packed.json).unwrap();
    root.insert("nodes".into(), RawValue::from_string(
        r#"[{"mesh":0,"matrix":[0.2796982,0,0,0,0,0.2796982,0,0,0,0,0.2796982,0,-0.10848464071750641,3.25000000000000000001,4,1]}]"#.into()).unwrap());
    packed.json = Cow::Owned(serde_json::to_vec(&root).unwrap());
    packed.to_vec().unwrap()
}

pub(crate) fn unpack(bytes: &[u8]) -> (Value, Vec<u8>) {
    let packed = gltf::binary::Glb::from_slice(bytes).unwrap();
    (
        serde_json::from_slice(&packed.json).unwrap(),
        packed.bin.unwrap().into_owned(),
    )
}

pub(crate) fn raw_root(bytes: &[u8]) -> RawObject {
    let packed = gltf::binary::Glb::from_slice(bytes).unwrap();
    serde_json::from_slice(&packed.json).unwrap()
}

pub(crate) fn compact(raw: &RawValue) -> String {
    raw.get()
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect()
}

pub(crate) fn editor(reference: &[u8], pixel: [u8; 4]) -> (Vec<u8>, Vec<u8>) {
    let bundle = export_editable_visual(reference, &Default::default()).unwrap();
    let mut root: Value = serde_json::from_slice(&bundle.files["scene.gltf"]).unwrap();
    root["materials"].as_array_mut().unwrap().swap(0, 1);
    root["materials"][0]["name"] = json!("renamed unselected");
    root["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"] = json!(0.13);
    root["materials"][1]["name"] = json!("renamed selected");
    root["materials"][1]["pbrMetallicRoughness"]["roughnessFactor"] = json!(0.42);
    root["meshes"] = json!([{"editorGeometryMustNeverBecomeOutput": true}]);
    root["nodes"] = json!([{"translation": [99999, 99999, 99999]}]);
    root["accessors"] = json!([]);
    let mut binary = bundle.files["scene.bin"].clone();
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let offset = binary.len();
    let replacement = png(pixel);
    binary.extend_from_slice(&replacement);
    let view = root["bufferViews"].as_array().unwrap().len();
    root["bufferViews"].as_array_mut().unwrap().push(json!({
        "buffer": 0, "byteOffset": offset, "byteLength": replacement.len()
    }));
    root["images"] = json!([{"bufferView": view, "mimeType": "image/png"}]);
    root["textures"] = json!([{"source": 0, "sampler": 0}]);
    root["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987}]);
    root["buffers"] = json!([{"byteLength": binary.len()}]);
    let packed = gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: Cow::Owned(serde_json::to_vec(&root).unwrap()),
        bin: Some(Cow::Owned(binary)),
    };
    (
        packed.to_vec().unwrap(),
        bundle.files["authoring.json"].clone(),
    )
}

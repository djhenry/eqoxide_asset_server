use eqoxide_asset_server::static_scene::*;
use glam::{Mat4, Quat, Vec3};

const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn scene() -> StaticScene {
    StaticScene {
        meshes: vec![StaticMesh { name: "authored".into(), positions: vec![[1.,2.,3.],[4.,2.,3.],[1.,6.,3.]], normals:vec![[0.,0.,1.];3], uvs:vec![[0.,0.],[1.,0.],[0.,1.]], primitives: vec![StaticPrimitive {indices:vec![0,1,2],material_index:0,colors:Some(vec![[0.2,0.4,0.6,0.8];3])},
                StaticPrimitive {indices:vec![0,1,2],material_index:0,colors:None}] }],
        materials: vec![StaticMaterial {name:"tinted".into(),base_color:[0.3,0.5,0.7,0.4],texture_index:None,alpha_mode:StaticAlphaMode::Mask{cutoff:0.371},double_sided:false}],
        textures:vec![],
        instances:vec![StaticInstance {mesh_index:0,matrix:Mat4::from_scale_rotation_translation(Vec3::splat(2.),Quat::from_rotation_x(0.7),Vec3::new(13.,17.,19.)).to_cols_array_2d()},StaticInstance {mesh_index:0,matrix:Mat4::IDENTITY.to_cols_array_2d()}],
    }
}
fn approx(a: [f32;3], b:[f32;3]) { for i in 0..3 { assert!((a[i]-b[i]).abs()<1e-4,"{a:?} != {b:?}"); } }
fn png() -> Vec<u8> { let mut out=std::io::Cursor::new(Vec::new()); image::DynamicImage::new_rgba8(1,1).write_to(&mut out,image::ImageFormat::Png).unwrap();out.into_inner() }

#[test]
fn asymmetric_instances_map_positions_normals_and_preserve_ccw() {
    let scene=scene();
    let dir=tempfile::tempdir().unwrap();
    let path=dir.path().join("scene.glb");
    write_static_visual(&scene,REVISION,&path).unwrap();
    let (doc,bufs,_)=gltf::import(&path).unwrap();
    assert_eq!(doc.meshes().count(),1);
    assert_eq!(doc.nodes().count(),2);
    let p=doc.meshes().next().unwrap().primitives().next().unwrap();
    let r=p.reader(|b|Some(&bufs[b.index()]));
    let positions:Vec<_>=r.read_positions().unwrap().collect();
    let normals:Vec<_>=r.read_normals().unwrap().collect();
    let indices:Vec<_>=r.read_indices().unwrap().into_u32().collect();
    assert_eq!(indices,vec![0,1,2]);
    approx(positions[0],[1.,3.,-2.]);
    approx(normals[0],[0.,1.,0.]);
    for (i,node) in doc.nodes().enumerate() {assert_eq!(node.mesh().unwrap().index(),0);
    let output=Mat4::from_cols_array_2d(&node.transform().matrix());
    let input=Mat4::from_cols_array_2d(&scene.instances[i].matrix);
        for (p,original) in positions.iter().zip(&scene.meshes[0].positions) {
        let v=input.transform_point3(Vec3::from_array(*original));
    approx(output.transform_point3(Vec3::from_array(*p)).to_array(),[v.x,v.z,-v.y]);}
        let n=input.transform_vector3(Vec3::Z).normalize();
    approx(output.transform_vector3(Vec3::from_array(normals[0])).normalize().to_array(),[n.x,n.z,-n.y]);
        let a=output.transform_point3(Vec3::from_array(positions[0]));
    let b=output.transform_point3(Vec3::from_array(positions[1]));
    let c=output.transform_point3(Vec3::from_array(positions[2]));
    assert!((b-a).cross(c-a).dot(output.transform_vector3(Vec3::from_array(normals[0])))>0.);
    }
}
#[test]
fn normalized_materials_colors_and_header_survive_real_glb() {
    let mut s=scene();s.textures.push(StaticTexture{name:"pixel".into(),png_bytes:png()});s.materials[0].texture_index=Some(0);
    s.materials.push(StaticMaterial {name:"blend".into(),base_color:[0.1,0.2,0.3,0.6],texture_index:None,alpha_mode:StaticAlphaMode::Blend,double_sided:true});
    s.meshes[0].primitives[1].material_index=1;
    let dir=tempfile::tempdir().unwrap();
    let path=dir.path().join("scene.glb");
    let req=write_static_visual(&s,REVISION,&path).unwrap();
    assert_eq!(req.reader_version,2);
    assert_eq!(req.capabilities,vec!["static-visual-v1","vertex-rgba-v1"]);
    let (doc,bufs,images)=gltf::import(&path).unwrap();
    assert_eq!(images.len(),1);
    let materials:Vec<_>=doc.materials().collect();
    assert_eq!(materials[0].pbr_metallic_roughness().base_color_factor(),s.materials[0].base_color);
    assert_eq!(materials[0].alpha_cutoff(),Some(0.371));
    assert_eq!(materials[0].alpha_mode(),gltf::material::AlphaMode::Mask);
    assert!(!materials[0].double_sided());
    assert!(materials[0].pbr_metallic_roughness().base_color_texture().is_some());
    assert_eq!(materials[1].pbr_metallic_roughness().base_color_factor(),s.materials[1].base_color);
    assert_eq!(materials[1].alpha_mode(),gltf::material::AlphaMode::Blend);
    assert!(materials[1].double_sided());
    let ps:Vec<_>=doc.meshes().next().unwrap().primitives().collect();
    let colors:Vec<_>=ps[0].reader(|b|Some(&bufs[b.index()])).read_colors(0).unwrap().into_rgba_f32().collect();
    assert_eq!(colors,s.meshes[0].primitives[0].colors.clone().unwrap());
    assert!(ps[1].get(&gltf::Semantic::Colors(0)).is_none());
    let glb=gltf::binary::Glb::from_slice(&std::fs::read(path).unwrap()).unwrap().json.into_owned();
    let json:serde_json::Value=serde_json::from_slice(&glb).unwrap();
    assert_eq!(json["asset"]["generator"], "eqoxide-static-visual-v1");
    let h=&json["extras"]["eqoxideAsset"];
    assert_eq!(h["schemaVersion"],1);
    assert_eq!(h["role"],"visual");
    assert_eq!(h["coordinateProfile"],"eqoxide-static-y-up-v1");
    assert_eq!(h["unitScale"],1.0);
    assert_eq!(h["bakeRevision"],REVISION);
    assert_eq!(h["requirements"],serde_json::to_value(req).unwrap());
}
#[test]
fn malformed_scenes_preserve_last_good_output() {
    let dir=tempfile::tempdir().unwrap();
    let path=dir.path().join("scene.glb");
    let base=scene();
    write_static_visual(&base,REVISION,&path).unwrap();
    let original=std::fs::read(&path).unwrap();
    let mutations:Vec<Box<dyn Fn(&mut StaticScene)>>=vec![
        Box::new(|s|s.instances.clear()),
        Box::new(|s|s.meshes.clear()),
        Box::new(|s|s.meshes[0].positions.clear()),
        Box::new(|s|s.meshes[0].normals.pop().map(|_|()).unwrap()),
        Box::new(|s|s.meshes[0].uvs[0][0]=f32::NAN),
        Box::new(|s|s.meshes[0].positions[0][0]=f32::INFINITY),
        Box::new(|s|s.meshes[0].normals[0]=[0.;3]),
        Box::new(|s|s.meshes[0].normals[0]=[0.,0.,2.]),
        Box::new(|s|s.meshes[0].primitives.clear()),
        Box::new(|s|s.meshes[0].primitives[0].indices.clear()),
        Box::new(|s|s.meshes[0].primitives[0].indices.push(0)),
        Box::new(|s|s.meshes[0].primitives[0].indices[0]=3),
        Box::new(|s|s.meshes[0].primitives[0].material_index=1),
        Box::new(|s|s.instances[0].mesh_index=1),
        Box::new(|s|s.materials[0].texture_index=Some(0)),
        Box::new(|s|s.materials[0].base_color[0]=1.1),
        Box::new(|s|s.materials[0].alpha_mode=StaticAlphaMode::Mask{cutoff:f32::NAN}),
        Box::new(|s|s.meshes[0].primitives[0].colors.as_mut().unwrap().pop().map(|_|()).unwrap()),
        Box::new(|s|s.meshes[0].primitives[0].colors.as_mut().unwrap()[0][0]=-0.1),
        Box::new(|s|s.textures.push(StaticTexture{name:"bad".into(),png_bytes:vec![1,2,3]})),
        Box::new(|s|s.instances[0].matrix[0][0]=f32::NAN),
        Box::new(|s|s.instances[0].matrix=Mat4::from_scale(Vec3::new(1.,2.,1.)).to_cols_array_2d()),
        Box::new(|s|s.instances[0].matrix=Mat4::from_scale(Vec3::new(-1.,1.,1.)).to_cols_array_2d()),
        Box::new(|s|s.instances[0].matrix=Mat4::ZERO.to_cols_array_2d()),
        Box::new(|s|s.instances[0].matrix[0][3]=0.1),
        Box::new(|s|{s.instances[0].matrix=Mat4::IDENTITY.to_cols_array_2d();s.instances[0].matrix[1][0]=0.1;}),
        Box::new(|s|{s.instances[0].matrix=Mat4::from_scale(Vec3::splat(1e30)).to_cols_array_2d();s.meshes[0].positions[0]=[1e30;3];}),
    ];
    for (i,mutate) in mutations.iter().enumerate() {
        let mut s=base.clone();mutate(&mut s);
    assert!(write_static_visual(&s,REVISION,&path).is_err(),"mutation {i} accepted");
    assert_eq!(std::fs::read(&path).unwrap(),original,"mutation {i} replaced output");}
    for revision in ["", "abc", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg"] {assert!(write_static_visual(&base,revision,&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(),original);}
}
#[test]
fn deterministic_authored_scene_without_colors_requires_only_baseline() {
    let mut s=scene();for p in &mut s.meshes[0].primitives {p.colors=None;}s.materials[0].alpha_mode=StaticAlphaMode::Opaque;
    let dir=tempfile::tempdir().unwrap();
    let a=dir.path().join("a.glb");
    let b=dir.path().join("b.glb");
    assert_eq!(write_static_visual(&s,REVISION,&a).unwrap().capabilities,vec!["static-visual-v1"]);
    write_static_visual(&s.clone(),REVISION,&b).unwrap();
    assert_eq!(std::fs::read(a).unwrap(),std::fs::read(b).unwrap());
}

#[test]
fn oversized_png_and_malformed_full_decode_preserve_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.glb");
    let mut scene = scene();
    let mut png_bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(8193, 1).write_to(&mut png_bytes, image::ImageFormat::Png).unwrap();
    scene.textures.push(StaticTexture { name: "wide".into(), png_bytes: png_bytes.into_inner() });
    std::fs::write(&path, b"last good").unwrap();
    assert!(write_static_visual(&scene, REVISION, &path).unwrap_err().to_string().contains("PNG"));
    assert_eq!(std::fs::read(&path).unwrap(), b"last good");
    // A valid IHDR alone is insufficient: validate the compressed pixel stream as well.
    scene.textures[0].png_bytes = png();
    scene.textures[0].png_bytes.truncate(40);
    assert!(write_static_visual(&scene, REVISION, &path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"last good");
}

#[test]
fn affine_uniform_scale_validation_is_stable_across_valid_magnitudes() {
    let dir = tempfile::tempdir().unwrap();
    for scale in [1e-6, 1.0, 1e6] {
        let mut scene = scene();
        scene.instances[0].matrix = Mat4::from_scale_rotation_translation(Vec3::splat(scale), Quat::from_rotation_y(1.1), Vec3::new(2., 3., 4.)).to_cols_array_2d();
        let path = dir.path().join("scale.glb");
        write_static_visual(&scene, REVISION, &path).unwrap();
        let glb = gltf::Gltf::open(path).unwrap();
        let matrix = Mat4::from_cols_array_2d(&glb.nodes().next().unwrap().transform().matrix());
        let lengths = [matrix.x_axis.truncate().length(), matrix.y_axis.truncate().length(), matrix.z_axis.truncate().length()];
        assert!(lengths.into_iter().all(|length| (length / scale - 1.).abs() < 1e-5));
    }
}

#[test]
fn converted_world_overflow_rejects_even_if_source_order_is_finite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.glb");
    std::fs::write(&path, b"last good").unwrap();
    let mut scene = scene();
    let a = Vec3::new(0.7, 0.02_f32.sqrt(), 0.7);
    let b = Vec3::new(1. / 2.0_f32.sqrt(), 0., -1. / 2.0_f32.sqrt());
    let c = a.cross(b);
    let matrix = Mat4::from_cols_array_2d(&[
        [a.x,b.x,c.x,0.], [a.y,b.y,c.y,0.], [a.z,b.z,c.z,0.], [0.,0.,0.,1.]
    ]);
    let k = 0.78 * f32::MAX;
    scene.meshes[0].positions[0] = [k,-k,k];
    scene.instances[0].matrix = matrix.to_cols_array_2d();
    assert!(matrix.transform_point3(Vec3::from_array(scene.meshes[0].positions[0])).is_finite());
    let error = write_static_visual(&scene,REVISION,&path).unwrap_err();
    assert!(error.to_string().contains("converted bounds"),"{error:#}");
    assert_eq!(std::fs::read(path).unwrap(), b"last good");
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32fast::hash(&out[4..]).to_be_bytes());
    out
}

fn animated_png(original: &[u8]) -> Vec<u8> {
    let control = |sequence: u32| {
        let mut data = Vec::new();
        for n in [sequence, 1, 1, 0, 0] { data.extend_from_slice(&n.to_be_bytes()); }
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&10u16.to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        png_chunk(b"fcTL", &data)
    };
    let mut out = original[..33].to_vec();
    let mut animation = 2u32.to_be_bytes().to_vec();
    animation.extend_from_slice(&0u32.to_be_bytes());
    out.extend(png_chunk(b"acTL", &animation));
    out.extend(control(0));
    out.extend_from_slice(&original[33..original.len() - 12]);
    out.extend(control(1));
    let mut frame_data = 2u32.to_be_bytes().to_vec();
    let mut offset = 33;
    while offset < original.len() - 12 {
        let len = u32::from_be_bytes(original[offset..offset + 4].try_into().unwrap()) as usize;
        if &original[offset + 4..offset + 8] == b"IDAT" {
            frame_data.extend_from_slice(&original[offset + 8..offset + 8 + len]);
        }
        offset += len + 12;
    }
    out.extend(png_chunk(b"fdAT", &frame_data));
    out.extend_from_slice(&original[original.len() - 12..]);
    out
}

#[test]
fn incomplete_crc_damaged_or_animated_png_preserves_last_good() {
    let original_png = png();
    let missing_end = original_png[..original_png.len() - 12].to_vec();
    let mut trailing = original_png.clone(); trailing.extend_from_slice(b"junk");
    let mut bad_crc = original_png.clone(); bad_crc[29] ^= 1;
    let animation = animated_png(&original_png);
    // This is a real two-frame APNG, not an arbitrary invalid image.
    let reader = image::codecs::png::PngDecoder::new(std::io::Cursor::new(&animation)).unwrap();
    assert!(reader.is_apng().unwrap());
    assert!(image::load_from_memory_with_format(&animation, image::ImageFormat::Png).is_ok());
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("scene.glb");
    write_static_visual(&scene(), REVISION, &path).unwrap();
    let last_good = std::fs::read(&path).unwrap();
    for (label, payload) in [("missing IEND", missing_end), ("trailing data", trailing),
        ("bad CRC", bad_crc), ("APNG", animation)] {
        let mut s = scene();
        s.textures.push(StaticTexture { name: "image".into(), png_bytes: payload });
        s.materials[0].texture_index = Some(0);
        assert!(write_static_visual(&s, REVISION, &path).is_err(), "accepted {label}");
        assert_eq!(std::fs::read(&path).unwrap(), last_good, "replaced output for {label}");
    }
}

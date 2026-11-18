use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage};
use libeq_pfs::PfsWriter;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn encode(s: &[u8]) -> Vec<u8> {
    let key = [0x95, 0x3a, 0xc5, 0x2a, 0x95, 0x7a, 0x95, 0x6a];
    s.iter().enumerate().map(|(i, b)| b ^ key[i % 8]).collect()
}
fn table(s: &mut Vec<u8>, v: &str) -> u32 {
    let i = s.len() as i32;
    s.extend(v.as_bytes());
    s.push(0);
    (-i) as u32
}
fn document(fragments: Vec<(u32, Vec<u8>)>, strings: Vec<u8>) -> Vec<u8> {
    let mut b = words(&[
        0x54503d02,
        0x15500,
        fragments.len() as u32,
        0,
        0,
        strings.len() as u32,
        0,
    ]);
    b.extend(encode(&strings));
    for (t, v) in fragments {
        b.extend(words(&[v.len() as u32, t]));
        b.extend(v)
    }
    b
}
fn model(object: bool, mode: u32, bad_group: bool) -> Vec<u8> {
    let mut strings = vec![0];
    let meshname = table(
        &mut strings,
        if object {
            "OBJ_DMSPRITEDEF"
        } else {
            "TERRAIN_DMSPRITEDEF"
        },
    );
    let actorname = table(&mut strings, "OBJ_ACTORDEF");
    let matname = table(&mut strings, "MAT_MDF");
    let name = encode(b"tex.png\0");
    let mut bitmap = words(&[0, 0]);
    bitmap.extend((name.len() as u16).to_le_bytes());
    bitmap.extend(name);
    let sprite = words(&[0, 0, 1, 1]);
    let reference = words(&[0, 2, 0x50]);
    let material = words(&[
        matname,
        1,
        mode,
        0x112233,
        1f32.to_bits(),
        1f32.to_bits(),
        3,
    ]);
    let palette = words(&[0, 0, 1, 4]);
    let mut mesh = words(&[meshname, 0x18003, 5, 0, 0, 0]);
    for f in if object { [0f32; 3] } else { [10f32, 20., 30.] } {
        mesh.extend(f.to_le_bytes())
    }
    mesh.extend(words(&[0; 10]));
    for x in [3u16, 3, 3, 0, 1, 0, 1, 0, 0, 1] {
        mesh.extend(x.to_le_bytes())
    }
    for p in [[2i16, 4, 6], [6, 4, 6], [2, 8, 6]] {
        for x in p {
            mesh.extend(x.to_le_bytes())
        }
    }
    for uv in [[64i16, 128], [128, 128], [64, 256]] {
        for x in uv {
            mesh.extend(x.to_le_bytes())
        }
    }
    for _ in 0..3 {
        mesh.extend([0, 0, 127])
    }
    mesh.extend(0u16.to_le_bytes());
    for x in [0u16, 2, 1] {
        mesh.extend(x.to_le_bytes())
    }
    mesh.extend(if bad_group { 2u16 } else { 1u16 }.to_le_bytes());
    mesh.extend(0u16.to_le_bytes());
    let mut frags = vec![
        (3, bitmap),
        (4, sprite),
        (5, reference),
        (0x30, material),
        (0x31, palette),
        (0x36, mesh),
    ];
    if object {
        frags.push((0x2d, words(&[0, 6, 0])));
        frags.push((0x14, words(&[actorname, 0, 0, 0, 1, 0, 7, 0])))
    }
    document(frags, strings)
}
fn actors(scale: f32) -> Vec<u8> {
    let mut strings = vec![0];
    let name = table(&mut strings, "OBJ_ACTORDEF");
    let mut frags = vec![];
    for (p, s) in [([10f32, 20., 30.], scale), ([-10f32, 5., 7.], 0.005)] {
        let mut b = words(&[0, name, 0x2a, 0]);
        for x in p.into_iter().chain([128., 0., 0.]) {
            b.extend(x.to_le_bytes())
        }
        b.extend(0u32.to_le_bytes());
        b.extend(s.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        frags.push((0x15, b))
    }
    document(frags, strings)
}
fn png(color: [u8; 4]) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba(color)))
        .write_to(&mut c, ImageFormat::Png)
        .unwrap();
    c.into_inner()
}
fn archive(path: &Path, entries: Vec<(&str, Vec<u8>)>) {
    let mut p = PfsWriter::create(std::fs::File::create(path).unwrap()).unwrap();
    for (n, b) in entries {
        p.insert(n, Cursor::new(b)).unwrap();
    }
    p.finish().unwrap();
}
fn fixture(mode: u32, bad_group: bool, scale: f32) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("sample.s3d");
    let o = d.path().join("sample_obj.s3d");
    archive(
        &a,
        vec![
            ("sample.wld", model(false, mode, bad_group)),
            ("objects.wld", actors(scale)),
            ("tex.png", png([255, 0, 0, 255])),
        ],
    );
    archive(
        &o,
        vec![
            ("sample_obj.wld", model(true, mode, false)),
            ("tex.png", png([0, 0, 255, 255])),
        ],
    );
    (d, a, o)
}

#[allow(dead_code)]
#[path = "../examples/wld_pbr_zone.rs"]
mod command;

#[test]
fn cli_child() {
    let Some(args) = std::env::var_os("PORTABLE_TEST_CHILD_ARGS") else {
        return;
    };
    let args: Vec<String> = serde_json::from_str(args.to_str().unwrap()).unwrap();
    match command::run(args.into_iter().map(Into::into)) {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::exit(1);
        }
    }
}
fn cli(args: &[&std::ffi::OsStr], cwd: &Path) -> Output {
    if let Some(binary) = std::env::var_os("WLD_PBR_ZONE_BIN") {
        return Command::new(binary)
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
    }
    // Exercise the production command in an isolated process without relying on
    // a prebuilt standalone example or launching Cargo from inside a test run.
    let encoded =
        serde_json::to_string(&args.iter().map(|a| a.to_str().unwrap()).collect::<Vec<_>>())
            .unwrap();
    let mut output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cli_child", "--nocapture"])
        .env("PORTABLE_TEST_CHILD_ARGS", encoded)
        .current_dir(cwd)
        .output()
        .unwrap();
    if let Some(report) = output
        .stdout
        .split(|&c| c == b'\n')
        .find(|line| line.starts_with(b"{"))
    {
        output.stdout = report.to_vec();
    }
    output
}

fn precise_fixture(mask: bool) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (d, a, o) = fixture(if mask { 0x80000013 } else { 0x80000001 }, false, 2.);
    let mut p = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(ImageBuffer::from_pixel(
        2,
        2,
        Rgba([129u16, 32768, 45678, 128]),
    ))
    .write_to(&mut p, ImageFormat::Png)
    .unwrap();
    archive(
        &a,
        vec![
            (
                "sample.wld",
                model(false, if mask { 0x80000013 } else { 0x80000001 }, false),
            ),
            ("objects.wld", actors(2.)),
            ("tex.png", p.into_inner()),
        ],
    );
    (d, a, o)
}
#[test]
fn source_aliases_refuse_before_parsing_and_preserve_both_archives() {
    for which in 0..2 {
        for kind in 0..4 {
            let (d, a, o) = fixture(0x80000001, false, 2.);
            let target = if which == 0 { &a } else { &o };
            let before_a = std::fs::read(&a).unwrap();
            let before_o = std::fs::read(&o).unwrap();
            let output = match kind {
                0 => target.clone(),
                1 => PathBuf::from(".").join(target.file_name().unwrap()),
                2 => {
                    let p = d.path().join("symlink.glb");
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(target, &p).unwrap();
                    p
                }
                _ => {
                    let p = d.path().join("hardlink.glb");
                    std::fs::hard_link(target, &p).unwrap();
                    p
                }
            };
            let result = cli(
                &[a.as_os_str(), output.as_os_str(), o.as_os_str()],
                d.path(),
            );
            println!("alias source={which} kind={kind} exit={:?} before_main={} after_main={} before_object={} after_object={}",result.status.code(),blake3::hash(&before_a),blake3::hash(&std::fs::read(&a).unwrap()),blake3::hash(&before_o),blake3::hash(&std::fs::read(&o).unwrap()));
            if let Some(e) = std::env::var_os("PORTABLE_ALIAS_EVIDENCE") {
                let e = PathBuf::from(e);
                std::fs::create_dir_all(&e).unwrap();
                for (name, bytes) in [
                    ("before-main", before_a.clone()),
                    ("before-object", before_o.clone()),
                    ("after-main", std::fs::read(&a).unwrap()),
                    ("after-object", std::fs::read(&o).unwrap()),
                ] {
                    std::fs::write(
                        e.join(format!("source{which}-kind{kind}-{name}.bin")),
                        bytes,
                    )
                    .unwrap();
                }
            }
            assert!(
                std::fs::read(&a).unwrap() == before_a,
                "main archive overwritten"
            );
            assert!(
                std::fs::read(&o).unwrap() == before_o,
                "object archive overwritten"
            );
            assert!(!result.status.success(), "source output alias accepted");
            assert!(String::from_utf8_lossy(&result.stderr).contains("source"));
        }
    }
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("invalid.s3d");
    std::fs::write(&a, b"not an archive").unwrap();
    let result = cli(&[a.as_os_str(), a.as_os_str()], d.path());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("source archive"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
fn portable_inspection_and_output_keep_source_and_emitted_records_separate() {
    let (d, a, o) = precise_fixture(false);
    let output = d.path().join("visual.glb");
    let args = [
        std::ffi::OsStr::new("--portable-png8"),
        std::ffi::OsStr::new("--inspect"),
        a.as_os_str(),
        o.as_os_str(),
    ];
    let inspected = cli(&args, d.path());
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let info: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(info["stage"], "cpu-portable-texture-inspection");
    assert_eq!(info["glb_written"], false);
    assert_eq!(
        info["texture_preparation"]["policy"],
        "portable-png8-nearest-v1"
    );
    assert_eq!(info["conversion_stage"], "source-import");
    let written = cli(
        &[
            std::ffi::OsStr::new("--portable-png8"),
            a.as_os_str(),
            output.as_os_str(),
            o.as_os_str(),
        ],
        d.path(),
    );
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    let info: serde_json::Value = serde_json::from_slice(&written.stdout).unwrap();
    assert_eq!(
        info["texture_preparation"]["images"][0]["input_bit_depth"],
        16
    );
    assert_eq!(
        info["texture_preparation"]["images"][0]["output_bit_depth"],
        8
    );
    let first = std::fs::read(&output).unwrap();
    let doc = gltf::Gltf::from_slice(&first).unwrap();
    let blob = doc.blob.as_ref().unwrap();
    for image in doc.images() {
        let gltf::image::Source::View { view, .. } = image.source() else {
            panic!("embedded image")
        };
        assert_eq!(blob[view.offset() + 24], 8);
    }
    let again = cli(
        &[
            std::ffi::OsStr::new("--portable-png8"),
            a.as_os_str(),
            output.as_os_str(),
            o.as_os_str(),
        ],
        d.path(),
    );
    assert!(again.status.success());
    assert_eq!(std::fs::read(&output).unwrap(), first);
    let elsewhere = tempfile::tempdir().unwrap();
    let aa = elsewhere.path().join("sample.s3d");
    let oo = elsewhere.path().join("sample_obj.s3d");
    let new_output = elsewhere.path().join("other.glb");
    std::fs::copy(&a, &aa).unwrap();
    std::fs::copy(&o, &oo).unwrap();
    let moved = cli(
        &[
            std::ffi::OsStr::new("--portable-png8"),
            aa.as_os_str(),
            new_output.as_os_str(),
            oo.as_os_str(),
        ],
        elsewhere.path(),
    );
    assert!(moved.status.success());
    assert_eq!(std::fs::read(new_output).unwrap(), first);
}
#[test]
fn default_preserves_precision_and_policy_changes_identity_even_for_png8() {
    for precise in [false, true] {
        let (d, a, o) = if precise {
            precise_fixture(false)
        } else {
            fixture(0x80000001, false, 2.)
        };
        let out = d.path().join("out.glb");
        let old = cli(&[a.as_os_str(), out.as_os_str(), o.as_os_str()], d.path());
        assert!(
            old.status.success(),
            "{}",
            String::from_utf8_lossy(&old.stderr)
        );
        let before: serde_json::Value = serde_json::from_slice(&old.stdout).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let doc = gltf::Gltf::from_slice(&bytes).unwrap();
        let blob = doc.blob.as_ref().unwrap();
        let image = doc.images().next().unwrap();
        let gltf::image::Source::View { view, .. } = image.source() else {
            panic!()
        };
        assert_eq!(blob[view.offset() + 24], if precise { 16 } else { 8 });
        let new = cli(
            &[
                std::ffi::OsStr::new("--portable-png8"),
                a.as_os_str(),
                out.as_os_str(),
                o.as_os_str(),
            ],
            d.path(),
        );
        assert!(new.status.success());
        let after: serde_json::Value = serde_json::from_slice(&new.stdout).unwrap();
        assert_ne!(before["glb_bake_revision"], after["glb_bake_revision"]);
    }
}
#[test]
fn invalid_flags_and_preparation_refusal_preserve_previous_output() {
    let (d, a, o) = precise_fixture(true);
    let output = d.path().join("previous.glb");
    std::fs::write(&output, b"previous").unwrap();
    let before_a = std::fs::read(&a).unwrap();
    let before_o = std::fs::read(&o).unwrap();
    for flags in [
        vec!["--portable-png8"],
        vec!["--portable-png8", "--portable-png8"],
        vec!["--unknown"],
        vec!["--inspect", "--inspect"],
        vec!["--preview-snapshot", "--preview-snapshot"],
    ] {
        let mut args = flags.iter().map(std::ffi::OsStr::new).collect::<Vec<_>>();
        args.extend([a.as_os_str(), output.as_os_str(), o.as_os_str()]);
        assert!(!cli(&args, d.path()).status.success());
        assert_eq!(std::fs::read(&output).unwrap(), b"previous");
        assert_eq!(std::fs::read(&a).unwrap(), before_a);
        assert_eq!(std::fs::read(&o).unwrap(), before_o);
    }
    assert!(!cli(&[], d.path()).status.success());
}

# Editable static visual exports

The standalone `editable_visual` example converts an accepted common static visual
GLB into an ordinary glTF 2.0 authoring directory. It works identically for baked
WLD, EQG and newly authored content. Materials use standard metallic-roughness PBR,
alpha modes and explicit `KHR_materials_unlit`; no source-specific shader is needed. The supported material subset is defined in
[static PBR materials](static-pbr-materials.md); broader authoring goals are in
[the shared material policy](shared-material-policy-design.md).

```sh
cargo run --locked --features editable-visual --example editable_visual -- INPUT.glb NEW_DIRECTORY
```

The destination must not exist. The command does not publish assets or modify the
input. It writes a JSON conversion report to stdout after completing the bundle.
Errors writing stdout after completion leave the completed bundle in place.

## Bundle contents

| File | Meaning |
| --- | --- |
| `scene.gltf` | Portable editable visual scene, with relative resource URIs |
| `scene.bin` | Original buffer's declared bytes, without GLB alignment padding |
| `images/image-000000.png`, etc. | Original eight-bit PNG payloads, named by image ordinal |
| `reference.glb` | Byte-identical original input; retain as reference, not edited output |
| `authoring.json` | Version 1 sidecar and completion marker |

The exporter reuses the pinned pure CPU common visual reader. Admission includes
the existing geometry, transform, reference, capability and complete PNG checks.
It then changes packaging without reconstructing geometry or parsing unchanged
numeric tokens through floating-point reserialization: positions, normals,
UVs, colors, primitive assignments, instance matrices, supported material fields
and standard extension declarations retain their meanings. Original binary ranges
and buffer views remain, including image ranges that become unused after extraction.
Empty optional root arrays are omitted to satisfy the standard glTF schema;
`omittedEmptyCollections` lists exactly those removed in this export.

The editable scene omits `extras.eqoxideAsset`. The original header is retained in
the reference and sidecar. An authoring scene is not an activated engine artifact.
The sidecar records the reference SHA256, header, identity mappings and hashes of
all other generated files; it does not hash itself or future edited content.
Its payload byte count excludes its own serialization, which is nevertheless
included in the exporter's total output limit.

## Identities and Blender

Materials, meshes and nodes receive an optional `extras.eqoxideAuthoring` object
with `referenceSha256`, `kind` and `id`. For example, `material-000003` identifies
original material ordinal 3 within the bundle identified by that reference hash.
It is not a universal native-asset identity and is not stable across arbitrary
rebakes. Names and reordered export indices are not substitutes for this mapping.

Import `scene.gltf` in Blender. Keep its external resources together. On glTF
export, enable custom properties (`export_extras`) to retain supported identity
aids. Blender can reorder resources, split geometry, adjust normals, rename
objects or discard metadata; byte-identical round trips are not promised. Use
supported glTF material node arrangements rather than arbitrary shader graphs.
The initial interoperability target is Blender 5.2.2 LTS. In a measured fixture,
its importer stored floating-point vertex colors as `BYTE_COLOR`: alpha 0.75
became 191/255. Its exported accessor used normalized unsigned shorts but retained
that earlier quantization. A separate independent fixture showed broader losses:
vertex RGBA became RGB, giving the omitted alpha its standard default of 1, and
some MASK/unlit vertex RGB values became white (maximum component change about
0.9). Inactive OPAQUE material-factor alpha changed from 0.625 to 1; inactive unlit
fallback metallic/roughness fields also changed. These authored-data changes are
not limited to small quantization errors.

The complete scene authoring round trip has not passed qualification. Material
and vertex-color preservation failed in that fixture, and GuildHall normal
directions exceeded the tested angular-error limit. Before Blender editing, the exporter preserves original visual field values and binary/image payloads; its JSON packaging changes are described above.
That does not qualify Blender's
edited output for engine reimport or activation.

This command does not reimport edits, repair engine headers, regenerate collision
or activate a package. Future reimport must validate the retained reference and
supported edits, explicitly handle missing or ambiguous identities, and rebuild
reports, hashes and affected component pairing. Adding the old header back to a
Blender export does not establish those checks.

## Refusals and resource limits

Portable export requires the common PBR-factors capability, including explicitly
declared unlit materials. Legacy inspection artifacts depend on a client unlit
override and must be rebaked with a modern material policy first. Eight-bit PNGs
are required; higher-precision originals refuse without silent quantization.
Some otherwise accepted static previews contain higher-precision derived effect
textures and therefore refuse this portable export. An explicit, separately tested
precision conversion is needed for those inputs; this command does not drop the
affected surface or infer a conversion. Unknown or unsupported required content is an error. No external URI is fetched.

Default logical limits are 128 MiB encoded input, 8 MiB input/output scene JSON,
100,000 entries per resource collection, 64 MiB per PNG, 128 MiB aggregate PNG
payload and 400 MiB total output including the reference, buffer, images, scene
and sidecar. The reused reader also bounds decoded image memory, geometry,
accessor-validation work and transformed vertices. Its per-PNG limit applies to
encoded bytes and decoded pixel bytes. These are logical bounds, not a guarantee
of peak process memory or protection against hostile concurrent filesystem changes.

The command stages complete files before reserving the new destination with
`create_dir`. Existing files, directories (including empty ones) and symlinks are
never replaced. The destination is then populated, with `authoring.json` moved
last. Directory publication is not atomic: observers must require that completion
marker. Ordinary pre-completion failures clean up the tool's own staging and
reserved destination; cleanup errors cannot be guaranteed away. This workflow
assumes a trusted local directory without concurrent mutation, and makes no
power-loss durability or hostile-path race claim.

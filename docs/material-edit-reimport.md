# Selected material edits onto original geometry

The opt-in `editable-visual` feature provides the offline `editable_materials` example
and pure API; the default server build does not include the client CPU reader.
The example applies selected standard glTF material
and base-color PNG edits to an immutable original static visual. Start with an
[editable visual bundle](editable-visual.md), import `scene.gltf` into Blender,
edit supported material nodes or image pixels, and export a GLB with custom
properties enabled (`export_extras`). The initial editor target is Blender
5.2.2 LTS.

```sh
cargo run --locked --features editable-visual --example editable_materials -- \
  --material material-000000 --material material-000004 \
  AUTHORING_BUNDLE EDITED.glb OUTPUT.glb
```

One or more explicit `--material` selectors are required. Find material IDs in
`authoring.json` under identities with kind `material`, or in Blender material
custom properties at `eqoxideAuthoring.id`. Keep the complete custom property
object, including its reference hash and kind. IDs belong to one reference
bundle; names and reordered editor indices do not identify the original surface.
Missing, duplicate, malformed or foreign selected identities refuse the import.
Unselected editor materials are counted and ignored, not validated for runtime use.

The original `reference.glb` and byte-identical `authoring.json` are required.
The sidecar must match the deterministic exporter report for that reference;
reformatting it or editing its contents is refused. Other original bundle files
are not inputs and are not claimed to have been freshly checked. Keep them for
further editing. Output must be outside the bundle and must not alias any input,
including through symlinks or Unix hardlinks. Inputs must be regular files on a
stable trusted local filesystem; concurrent hostile path changes are unsupported.

## Applied fields

Supported selected fields are baseColorFactor RGBA, scalar metallicFactor and
roughnessFactor, baseColorTexture on UV0, OPAQUE/MASK/BLEND, MASK alphaCutoff,
doubleSided, and an explicit empty KHR_materials_unlit extension. Omitted glTF
factors use their standard defaults (white RGBA, metallic 1, roughness 1).
Omitted alphaMode is OPAQUE, doubleSided is false, and MASK cutoff is 0.5.
Explicit cutoff on another alpha mode refuses. A selected texture can be added,
replaced or removed. Original material names are retained.

Factors must be finite and normalized, even when inactive under OPAQUE or unlit.
All supported selected values are applied and recorded; the importer does not
infer which changes were intentional. Blender has silently changed inactive
OPAQUE alpha and unlit metallic/roughness values in measured exports. Select only
materials whose resulting supported values you intend to apply. Normal,
occlusion, emissive and metallic/roughness maps, nonzero emission, other material
extensions, texture transforms, secondary UV and arbitrary shader graphs refuse.

Selected base-color images must be embedded eight-bit PNGs in the GLB's single
BIN buffer. No external or data URI is fetched. JPEG, higher-precision PNG,
malformed PNG streams, invalid image ranges and image views with stride refuse.
Validation checks the exact inflated scanline length; unused compressed padding
after the zlib stream is allowed. PNG payload bytes are retained exactly. Imported equal payloads share resources;
materials sharing an original image can still receive different selected images.

The current inspector uses one mip level and repeat bilinear sampling. Omitted
samplers use this profile's linear default. Explicit repeat wrapping, LINEAR
magnification and LINEAR, LINEAR_MIPMAP_NEAREST or LINEAR_MIPMAP_LINEAR
minification are accepted and normalized to that single-mip profile, with original
and resolved values recorded in the report. Nearest, clamp and mirror choices
refuse. This does not preserve authored mip filtering for a future multi-mip
runtime or promise equal pixels across viewers.

## Preserved original content and output

All editor meshes, normals, UVs, colors, transforms, scene structure and primitive
material assignments are ignored. They are not validated as geometry or imported.
Original geometry JSON numeric tokens remain unchanged; original declared BIN
bytes remain an exact prefix. Original buffer views keep their indices. Validated
PNG payloads and views are appended, and selected materials are replaced at their
original ordinals. This retains old image byte ranges even if no longer used,
which intentionally increases output size. Unselected original materials and
image payloads retain their meanings.

A deterministic new bake revision binds the reference, effective selected fields
and image identities, sampler policy, and emission implementation identity. Ignored editor
geometry, names and resource ordering do not change effective visual identity;
the audit report separately hashes the entire edited GLB. Required unlit and HDR
blend declarations are generated from the resulting supported material state.
The final candidate passes the common CPU reader before atomic publication.
Earlier refusal preserves an existing output. A stdout failure after successful
publication reports that output was committed and cannot roll it back.

The result is a standalone visual GLB. Collision and old hashed package/scene
pairings are not regenerated or validly reused for the new visual. Production
activation is not performed. Full Blender scene roundtrip qualification remains
unpassed: prior geometry, RGB/alpha and normal losses are not waived by this
material-only workflow.

## Limits

The pure API is `material_edit::reimport_material_edits(reference, sidecar,
edited_glb, selected_ids, limits)` and returns GLB bytes and a serializable audit
report. `MaterialEditLimits` defaults to edited GLB 128 MiB, JSON 8 MiB, 100,000
entries per edited root collection and selectors, single selected PNG 64 MiB
encoded and decoded RGBA, aggregate selected PNG 128 MiB encoded and decoded,
image axis 8192, output GLB 128 MiB, and report 16 MiB. Distinct selected image
ordinals charge once before content deduplication. Reference admission has its
independent nested editable-export limits. Final combined output must also pass
native common-reader ceilings, including 256 MiB decoded PNG aggregate and its
existing coupled encoded/decoded 64 MiB single-image limit. Lower selected-input
limits do not restrict retained original resource counts. These are logical
bounds, not hard process-memory guarantees. The API and standalone example do not
launch an editor, renderer or client.

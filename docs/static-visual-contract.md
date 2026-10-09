# Static visual inspection contract

The normalized static writer accepts resolved scene data through `static_scene::StaticScene` and `write_static_visual`. It is intended for source adapters and authored enhancements alike. It has no source-family discriminator and does not interpret native shader names or collision flags.

Positions, normals and instance matrices enter in server geometry axes. Triangle indices use counterclockwise winding in those axes. The writer applies `(sx, sy, sz) -> (sx, sz, -sy)` to geometry and conjugates instance matrices by the same transform. The mapping has positive determinant, so winding is preserved. Unit scale is one game unit per numeric glTF meter as an authoring convention; it is not a measurement of physical scale. Static geometry receives no actor-origin height correction.

The first profile accepts translation, proper rotation and positive uniform scale. Unsupported reflections, shear, nonuniform scale and projective transforms fail. Importers must resolve or bake unsupported transforms before calling the writer. Mesh references remain shared across instances.

Materials carry resolved standard glTF base-color RGBA factors, optional base-color textures, opaque/mask/blend modes, a normalized MASK cutoff, and an explicit double-sided choice. Texture and factor colors multiply according to glTF semantics. Full optional per-primitive RGBA vertex colors are emitted as `COLOR_0`; an uncolored primitive sharing the same vertex pool stays uncolored. Normals, UVs, references, finite world coordinates, normalized color/material values and PNG input are validated before output replacement. PNG streams must have valid complete chunk boundaries/CRCs and a terminal IEND with no trailing data; APNG requires a separate animation capability and is rejected. PNG chunk count is limited to 100,000. Each encoded PNG is limited to 64 MiB; decoded dimensions are limited to 8192 pixels per axis and decoder allocation to 64 MiB. Excessive input fails explicitly.

The document's `extras.eqoxideAsset` header contains `schemaVersion: 1`, `role: "visual"`, `coordinateProfile: "eqoxide-static-y-up-v1"`, `unitScale: 1`, `bakeRevision`, and `requirements` (the existing reader-requirements structure). The bake revision identifies input and conversion policy; it is not a hash of the GLB itself or the manifest revision. A publishing adapter must derive it from its source, enhancement and policy inputs. Reader 2 with `static-visual-v1` identifies this baseline. Artifacts with vertex colors additionally require `vertex-rgba-v1`.

These requirements are reserved producer declarations. Current clients advertise reader 1 only. This change does not enable package publication, change current legacy/staging output, or make clients capable of rendering the new artifacts. The future package publisher must bind the artifact and its requirements through the existing manifest; merely placing a new GLB in an old asset path is insufficient.

The profile covers static visual inspection. Additive blending, texture sequences, secondary UVs, skins, animation, collision, regions and dynamic actors need additional explicit contracts and implementations. No movement, navigation, server landmark alignment, or gameplay readiness is implied by a successful GLB decode.

For an artifact without native input, run:

```sh
cargo run --example static_visual_fixture -- output.glb
```

The example exercises asymmetric geometry, two instances of one mesh, a tinted textured MASK material with nondefault alpha/cutoff, and full vertex color. It reopens the written artifact and reports its header, requirements, instance count and world bounds. This validates the writer boundary; source-adapter equivalence and runtime landmark acceptance are separate follow-on gates.

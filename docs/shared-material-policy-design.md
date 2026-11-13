# Shared material policy: modern PBR, runtime lighting and editable assets

Status: owner-directed design revision, not implemented runtime support. Tracks [interoperability revision #122](https://github.com/djhenry/eqoxide_asset_server/issues/122), [material policy #65](https://github.com/djhenry/eqoxide_asset_server/issues/65), [asset-format project #51](https://github.com/djhenry/eqoxide_asset_server/issues/51) and [client project #1126](https://github.com/djhenry/eqoxide/issues/1126). No publication capability changes in this document.

## Decision and purpose

The owner prioritizes playable content with attractive modern rendering over exact reproduction of the original appearance. Preserve editable materials and use shared runtime lighting. Use standard glTF 2.0 metallic-roughness PBR and tested Khronos extensions for the common visual format, with Blender as the initial authoring tool. Both original source families and newly authored enhancements follow that format. Source interpretation and supported conversion belong in the bake; the client does not dispatch on archive family or native shader names.

This supersedes the earlier recommendation for a required `EQOXIDE_materials_rgb_combine` extension and its proposed `material-rgb-combine-v1` capability. Do not implement either as the default material path. The previous neutral-versus-native-lighting scope question is superseded: modern PBR with shared runtime illumination is the gameplay target. Unlit remains useful for inspection and deliberately authored exceptions, rather than the default zone lighting model. Freezing a complete lighting condition into delivery textures is not the chosen default.

Use direct standard mappings where they are valid and documented modern interpretations where the old material model has no equivalent. Exact native shader arithmetic and pixel matching are not acceptance prerequisites for appearance. Report chosen interpretations and defaults; unresolved safety, coverage or required-feature semantics remain explicit failures. Do not spend implementation effort recreating an obsolete effect merely to achieve native parity. A required custom visual extension, consequential gameplay visibility change, large resource increase or raised hardware floor needs a concrete owner discussion before adoption.

## Current implementation boundary

The existing [static visual contract](static-visual-contract.md) transports material RGBA, optional vertex RGBA, embedded textures, multiplication and opaque/mask/blend coverage. The inspector applies neutral unlit rendering as a viewer override; the writer does not yet declare `KHR_materials_unlit`. That override is not a portable material definition. The [geometry bridge](eqg-static-geometry.md) requires resolved inputs and does not resolve native shading or lighting.

The current inspection profile also retains 16-bit PNG samples. Core glTF base-color textures use 8-bit channels. Existing precision support must not be silently removed: portable delivery needs an explicit conversion and its own acceptance, while high-precision originals remain authoring inputs. This revision does not claim that today's writer, decoder or renderer already satisfies the proposed portable profile.

Production advertisements remain unchanged until the package loader, renderer and publication gates actually support the selected fields and extensions. A valid GLB or a manifest capability declaration alone is not proof of eqoxide support.

## Portable visual material profile

Start with a deliberately small supported subset, then extend it through measured interoperability tests:

| Input or intent | Standard representation | Boundary |
| --- | --- | --- |
| Surface geometry and tint | Mesh positions, normals, UVs, `COLOR_0`, base-color texture and factor | Preserve coordinate and color meanings; source packed lighting is not automatically vertex tint |
| Coverage and sidedness | `alphaMode`, `alphaCutoff`, `doubleSided`, independently composed alpha | Preserve OPAQUE, MASK and BLEND semantics; do not infer coverage from lighting weights |
| Converted and authored lit surfaces (default) | Core metallic-roughness PBR fields and standard normal, occlusion and emissive inputs | Admit each feature after producer/client/tool acceptance; documented defaults and authored overrides supply missing PBR parameters |
| Explicit unlit inspection or authored unlit surfaces | Ratified `KHR_materials_unlit` with ordinary base-color inputs | Does not reproduce native lighting or fog; retain the author's unlit intent |
| Additional static source effects | Validated texture/UV conversion into the supported standard profile | Record approximation, resolution, precision and instance costs; unsupported is explicit |

The [glTF material specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#materials) defines texture transfer, linear factors/vertex colors and alpha behavior. Do not repurpose these fields to transport encoded native RGB operations or light weights. Preserve straight alpha; an RGB bake must not accidentally bake opacity into color or apply tint twice.

For unlit materials, use the [Khronos extension definition](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_unlit). Declare it in `extensionsUsed`; also declare it in `extensionsRequired` when the artifact requires unlit interpretation. A core-PBR fallback may be optional only when its different lighting behavior is deliberately accepted and reported. Exact inspection fixtures require unlit support. An eqoxide inspection-only override for a PBR asset must remain visibly identified as an override, not exported as the asset's intended appearance.

Blender's [glTF importer/exporter](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/docs/blender_docs/scene_gltf2.rst) is the initial authoring interoperability target. Use its supported glTF material arrangements, not arbitrary Blender shader graphs. Pin the tested Blender version and export settings in acceptance evidence. A standard feature or extension is not automatically supported just because Blender or glTF defines it; the allowed set must match tested producer and client behavior.

## Modern runtime lighting target

Build on the existing client renderer where practical. One shared lighting path must serve both source families and enhanced content. The implementation order is a bounded engineering proposal, not a claim that these features exist today:

1. Establish standard base color, metallic/roughness, normals and tangent-space normal maps, occlusion and emissive inputs through the producer, actual client and Blender workflow. Keep color textures distinct from linear data textures and use consistent normal/tangent conventions. Start legacy conversions with documented rough, nonmetal defaults when no material-specific evidence or authored override exists; do not infer metalness from dark pixels or claim recovered physical measurements.
2. Add calibrated sky/environment illumination and a directional sun, together with HDR lighting and controlled exposure/tone mapping. An environment map supplies surrounding illumination and reflections; it is not a second albedo texture. Define day/night and indoor/outdoor behavior at the renderer/environment boundary. The existing game-unit coordinate convention must be accounted for when calibrating light range and intensity, rather than silently rescaling geometry or collision.
3. Add bounded local lights and filtered shadows, beginning with a useful directional shadow and selected local lights. Standard [KHR_lights_punctual](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_lights_punctual) can transport supported authored directional, point and spot lights. It does not define our sky, fog, shadow implementation or complete world-clock behavior. Source light definitions need verified placement/meaning and explicit intensity conversion or authored replacement; copying legacy numeric intensities is not calibration.
4. Improve environmental presentation and special surfaces after the baseline is playable: coherent fog, water and selected effects, followed by measured optional quality improvements such as ambient occlusion or reflections. Account for any static occlusion already supplied by materials. Graphics presets and bounded light/shadow/texture workloads must keep the common path usable on supported hardware. Ray tracing is not a prerequisite or an assumed hardware requirement.

Lighting and display choices belong to the common renderer and environment policy, not baked source-family shader switches. Standard glTF materials do not by themselves provide a sky, shadows, exposure policy or global illumination. Different tools can display the same material differently because their lighting and display settings differ; use a controlled reference scene for comparisons. Optional renderer techniques can improve eqoxide without making editable materials depend on proprietary shader graphs.

## Source conversion and preserved lighting inputs

Native static paths distinguish material tint, precomputed lighting and normals; some also amplify/clamp color in a different order or working space from standard glTF. These facts explain why exact appearance cannot generally be preserved by a texture-only conversion. They do not require emulating the old arithmetic under the owner's modern-PBR choice. WLD and EQG both have stored lighting inputs; neither source family should become a separate runtime lighting mode.

Do not insert native light arrays or their environment-light weights into standard material tint, alpha, occlusion or emissive slots merely because their storage resembles colors. Preserve available source lighting separately as rebake/reference data; do not apply it by default in the PBR profile. Omission of legacy prelighting is an explicit modernization choice, not a claim that native lighting was reproduced. Missing legacy prelighting alone need not block an otherwise supported modern material. Unsupported required geometry, coverage, transforms and package semantics still refuse.

Keep editable base color separate from newly generated illumination. Some source textures may already have painted highlights or shadows. We cannot reliably recover clean albedo, normals or roughness from a single image; retaining the original, reporting residual shading and allowing authored replacement is preferable to claiming automatic recovery. Validated texture-layer composition can still be baked into editable surface textures, subject to UV, precision, memory and output-size budgets. A fixed reference-lit/unlit export is an optional inspection artifact, not the default gameplay material.

Every converted material must report direct mappings, documented modern defaults/interpretations, authored overrides and unsupported inputs. Record texture depth/resolution, filtering/mips, UV changes and material/instance expansion. Evaluate the result for surface quality, coherent lighting, tool compatibility and gameplay readability, rather than native screenshot error. Artistic differences are expected; undisclosed missing surfaces, incorrect opacity or unusable dark areas are defects. Preserve entrances, silhouettes, foliage cutouts and water boundaries sufficiently for play; placement and collision correctness remain separate unchanged requirements.

Representative acceptance must include older and newer zones, indoor and outdoor scenes, day/night, moving lights, rough surfaces, authored smooth/metallic surfaces, cutouts and transparency. Capture controlled Blender/client material comparisons, in-game visibility examples and frame-time/GPU-memory measurements on identified hardware/settings. Set performance and resource budgets from those measurements before expanding quality features. Ask the owner about consequential quality/performance or compatibility tradeoffs, not every intentional departure from native color arithmetic.

## Editable authoring and delivery

Retain original/high-precision texture inputs and separate material/lighting facts for rebaking. Deliver portable base-color textures at the standard channel depth through an explicit, tested color/precision conversion; decide quantization and any dithering from acceptance results. Do not overwrite the original images or downgrade the existing reader in this documentation change.

Offer an editable export with `.gltf`, external images and buffers, alongside a packaged delivery `.glb`. These are two packagings of the same supported visual semantics, not two source-family runtime formats. A native `.blend` authoring convenience may be added later; it is not required to define the runtime contract. Newly authored PBR or unlit replacements should not need native shader emulation.

Give authors stable semantic identities and document the supported edit/reimport path. Blender export may reorder or split geometry and materials, alter names or indices, and discard unknown metadata. Do not promise byte-identical GLB round trips or preservation of arbitrary node graphs, `extras` or extensions. Reimport must validate supported content and rebuild affected conversion reports, hashes and component pairing; ambiguous identity changes require explicit remapping, not guessed matching.

Visual interoperability is distinct from collision and package semantics. Engine metadata can remain in the existing documented contract or companion records, without making material appearance depend on it. Editing visible geometry does not automatically update collision, regions or canonical query-unit frames. A package with stale visual/collision pairing must refuse activation until rebuilt and validated. This decision does not loosen the agreed collision transform restrictions or introduce a Blender-specific gameplay format.

## Compatibility and migration

Reuse the existing versioned manifest, artifact requirements and client capability checks. Do not add the abandoned RGB-combine requirement for this profile or change the meaning of an existing requirement to imply newly implemented PBR/unlit support. Introduce a new requirement/revision when actual supported semantics demand it; incompatible clients must fail explicitly. The project's pre-release policy permits requiring an updated client, with a coordinated rollout.

A future client may support more standard glTF features, but it must declare only what it implements. Inspection overrides must not conceal an unsupported required extension. Ordinary tools may render standard visual content without understanding engine-specific collision or package metadata; that does not make their output an immediately valid game package.

## Raw lighting data and association

The independently described classic lighting container is `EQGP`, followed by a little-endian `u32` sample count and that many packed 32-bit words. There is no version word in the traced classic path. A libeq reader should preserve the full words, count and any unexplained suffix; apply checked size arithmetic and bounded access before allocation. It should not perform gamma conversion, interpret alpha as coverage or decide provider precedence.

The importer owns associations: terrain lighting uses the terrain basename, binary descriptor v1 object lighting uses the placement name, and v2 placement extensions hold lighting words with record zero serving terrain lighting. Counts must be checked against the appropriate vertex pool. A native fallback on count mismatch is not proof that a complete bake succeeded.

Other `.lit` files have an apparently headerless shape. Their consumer and association remain unverified; extension alone must not select a raw word-array parser. Preserve/report them as unsupported rather than claiming universal LIT support.

Per-instance lighting can differ while geometry remains shared. The current global mesh/material binding is not sufficient for that input. Future normalized lighting records need explicit instance/mesh/vertex associations and independent budgets; forcing them into material tint would lose both meaning and correct operation order.

## Implementation and acceptance order

1. Inventory the common writer, client loaders and existing renderer against the modern PBR subset and shared runtime-lighting target. Identify current differences, including unlit override and image depth, before changing advertisements. Raw lighting readers/diagnostics remain useful for preservation without applying native lighting; keep source interpretation and provider selection outside libeq.
2. Create small source-independent PBR fixtures, plus explicitly unlit inspection fixtures. Check the Khronos glTF validator and pinned Blender import, edit, export and reimport, then the actual eqoxide decoder and renderer. Establish the standard material path before expanding zone shading coverage.
3. Verify geometry, UVs, normals, vertex tint, alpha modes/non-default cutoff, sidedness, instance transforms and material assignments across that round trip. Compare under controlled lighting and color management. Validator success alone does not establish matching appearance. Include metadata loss/reordering, unsupported-extension and stale-pairing negative controls.
4. Implement the missing shared producer/consumer PBR semantics and runtime illumination in bounded stages with capability checks. Author and independent reviewer each validate their own fixtures; include CPU assertions, rendered comparisons and mutation-discriminating regression checks where behavior changes. Do not advertise unfinished PBR, lights or unlit support.
5. Convert source materials with documented defaults and authored overrides, retaining editable inputs. Judge accepted mappings by modern appearance, functional coverage and resource budgets; unresolved required features stay unsupported. Use the same output profile for enhanced older content, without requiring native shader parity.
6. Validate representative actual assets through authoring, packaging, delivery and the lit client. Check playability, visual quality and measured performance, with human scene review. Material correctness, lighting, collision and gameplay remain distinct gates. Human review remains required before every merge.

This is a design revision only: no new bake, renderer, package publication or readiness claim is implemented here.

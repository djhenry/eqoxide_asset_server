# Shared material policy: standard glTF and editable assets

Status: owner-directed design revision, not implemented runtime support. Tracks [interoperability revision #122](https://github.com/djhenry/eqoxide_asset_server/issues/122), [material policy #65](https://github.com/djhenry/eqoxide_asset_server/issues/65), [asset-format project #51](https://github.com/djhenry/eqoxide_asset_server/issues/51) and [client project #1126](https://github.com/djhenry/eqoxide/issues/1126). No publication capability changes in this document.

## Decision and purpose

The owner prioritizes assets that people can modify, enhance and replace with open tools, including Blender. Use standard glTF 2.0 materials and tested Khronos extensions for the common visual format. Both original source families and newly authored content follow that format. Source interpretation and supported conversion belong in the bake; the client does not dispatch on archive family or native shader names.

This supersedes the earlier recommendation for a required `EQOXIDE_materials_rgb_combine` extension and its proposed `material-rgb-combine-v1` capability. Do not implement either as the default material path. The previous neutral-versus-native-lighting scope question is superseded by this interoperability priority. It does not authorize arbitrary visual loss or a complete lighting bake.

Use direct standard mappings where they are valid. For static effects that need conversion, evaluate baking to ordinary textures and UVs with explicit quality and resource limits. Refuse unsupported effects rather than silently substituting a plausible material. If acceptable fidelity requires an engine-specific operation, return with measured examples and the compatibility cost before adopting it.

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
| Authored lit surfaces | Core metallic-roughness PBR fields and standard normal, occlusion and emissive inputs | Admit each feature only after producer, client and tool acceptance; missing source parameters are not evidence of a physically accurate mapping |
| Explicit unlit inspection or authored unlit surfaces | Ratified `KHR_materials_unlit` with ordinary base-color inputs | Does not reproduce native lighting or fog; retain the author's unlit intent |
| Additional static source effects | Validated texture/UV conversion into the supported standard profile | Record approximation, resolution, precision and instance costs; unsupported is explicit |

The [glTF material specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#materials) defines texture transfer, linear factors/vertex colors and alpha behavior. Do not repurpose these fields to transport encoded native RGB operations or light weights. Preserve straight alpha; an RGB bake must not accidentally bake opacity into color or apply tint twice.

For unlit materials, use the [Khronos extension definition](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_unlit). Declare it in `extensionsUsed`; also declare it in `extensionsRequired` when the artifact requires unlit interpretation. A core-PBR fallback may be optional only when its different lighting behavior is deliberately accepted and reported. Exact inspection fixtures require unlit support. An eqoxide inspection-only override for a PBR asset must remain visibly identified as an override, not exported as the asset's intended appearance.

Blender's [glTF importer/exporter](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/docs/blender_docs/scene_gltf2.rst) is the initial authoring interoperability target. Use its supported glTF material arrangements, not arbitrary Blender shader graphs. Pin the tested Blender version and export settings in acceptance evidence. A standard feature or extension is not automatically supported just because Blender or glTF defines it; the allowed set must match tested producer and client behavior.

## What a bake can and cannot preserve

The inspected native static paths distinguish material tint, independent precomputed lighting and normals. Some passes amplify and clamp texture/tint RGB before applying lighting, potentially in normalized stored RGB code space. Static evidence is not a captured live-device-state proof; gamma, fog, sampler behavior and complete lighting equivalence remain unverified. Lighting RGB and its environment-light weight are separate from surface opacity.

These scalar examples demonstrate the conversion problem, rather than native screenshot agreement:

- Texture `0.8`, vertex multiplier `0.8`, gain `2`: clamping the complete product gives `1`; clamping the amplified texture first and then multiplying by the vertex gives `0.8`.
- Product `1.2`, lighting `0.5`: clamp before lighting gives `0.5`; including lighting inside the clamp gives `0.6`.

Nonlinear operations do not generally commute with interpolation, texture filtering or color transfer. Baking vertex-dependent effects may need new UVs and texture atlases. A finite texture samples an effect; it does not guarantee identical results at every pixel, distance, mip level or camera angle. A fixed-light bake cannot reproduce changing illumination. Ordinary metallic-roughness PBR is also not a general substitute for the original shading model.

Keep material conversion and lighting scope distinct. The first portable inspection fixtures can be unlit to isolate material conversion, explicitly without native lighting or fog. Keep any future reference-lit bake separately labeled and preserve its lighting inputs apart from editable albedo. Do not permanently merge native light streams into the only retained color source. Native-like runtime lighting remains a separate fidelity project; it must not require different client paths for EQG and older content.

Every converted material must have a report distinguishing direct mapping, measured approximation and unsupported input. Before enabling an approximation, define its reference context, color management, error metric/tolerance and comparison views; record resolution, image depth, filtering/mips, UV changes and material/instance expansion. Bound bake memory, time, output size and GPU texture cost. Unknown or unmeasured loss is not successful resolution. Consequential visual loss or a large asset-size increase goes back to the owner with concrete comparisons.

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

1. Inventory the common writer, client loaders and renderer against the portable subset. Identify current differences, including unlit override and image depth, before changing advertisements. Raw lighting readers/diagnostics remain useful without applying lighting; keep source interpretation and provider selection outside libeq.
2. Create small source-independent fixtures for directly mapped materials and explicit `KHR_materials_unlit`. Check the Khronos glTF validator and pinned Blender import, edit, export and reimport, then the actual eqoxide decoder and renderer. First establish the tooling path without claiming new source-family shading coverage.
3. Verify geometry, UVs, normals, vertex tint, alpha modes/non-default cutoff, sidedness, instance transforms and material assignments across that round trip. Compare under controlled lighting and color management. Validator success alone does not establish matching appearance. Include metadata loss/reordering, unsupported-extension and stale-pairing negative controls.
4. Implement the missing shared producer/consumer semantics with capability checks. Author and independent reviewer each validate their own fixtures; include CPU assertions, rendered comparisons and mutation-discriminating regression checks where behavior changes. Do not advertise unfinished PBR or unlit support.
5. Evaluate source-specific static conversions against declared references and budgets, retaining editable inputs and reporting loss. Enable only accepted mappings/approximations; unresolved effects stay unsupported. Use the same output profile for enhanced older content.
6. Validate representative actual assets through the supported authoring workflow, packaging, delivery and client. Material fidelity, lighting, collision and gameplay remain distinct acceptance gates. Human review remains required before every merge.

This is a design revision only: no new bake, renderer, package publication or readiness claim is implemented here.

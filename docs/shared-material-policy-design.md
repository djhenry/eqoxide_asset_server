# Shared material arithmetic and lighting policy

Status: proposal for owner scope review. Tracks [issue #65](https://github.com/djhenry/eqoxide_asset_server/issues/65), [asset-format project #51](https://github.com/djhenry/eqoxide_asset_server/issues/51) and [client project #1126](https://github.com/djhenry/eqoxide/issues/1126). This document changes no runtime behavior or publication capability.

## Purpose and current boundary

Both original source families and authored enhancements must use one material contract. Source shader selection, packed-color interpretation and lighting dependency discovery belong in the bake. Runtime behavior must depend on declared operations and capabilities, never archive family or native shader names.

The existing [static visual contract](static-visual-contract.md) supports normalized material RGBA, optional full vertex RGBA, embedded textures, ordinary multiplication and opaque/mask/blend coverage. The renderer uses neutral unlit inspection. The [geometry bridge](eqg-static-geometry.md) requires resolved inputs; it does not resolve native material or lighting behavior.

Current glTF base-color texture RGB is decoded from sRGB before computation. Factors and `COLOR_0` are linear multipliers; their alpha is linear coverage. That is defined by the [glTF material specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#materials). A native operation on normalized stored RGB codes is a different working space, not an alternative source format the client should detect.

## Findings that change the next step

The traced native static paths distinguish three inputs: material tint, independent precomputed lighting, and normals. Original mesh RGBA supplies material tint for some passes and is unused by others. Lighting RGB and its environment-light weight occupy a separate stream. That weight is not surface opacity.

Some passes amplify and clamp the texture/tint product before applying lighting. Static evidence and documented [D3D9 sampler defaults](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3dsamplerstatetype) and [render-state defaults](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3drenderstatetype) support normalized-code RGB arithmetic in the selected ordinary installed paths. This is not a captured live device-state proof. Gamma adjustment is separate; final gamma options, fog, mip/sampler behavior and complete lighting equivalence are not certified.

Consequently, adding only a gain operation to today's linear renderer is insufficient to claim native fidelity. Putting all packed source RGBA into ordinary vertex tint is also incorrect. No investigated complete shader family has been certified as equivalent to ordinary unlit linear glTF through a general static bake.

Two scalar examples show why operation order belongs in a shared renderer capability:

- Texture `0.8`, vertex multiplier `0.8`, factor `1`, gain `2`: clamping their complete product gives `1`. Amplifying/clamping the texture first then multiplying by the vertex gives `0.8`.
- A pre-lighting product of `1.2` and lighting `0.5`: clamping before lighting gives `0.5`; putting lighting inside that clamp gives `0.6`.

These examples are algebraic acceptance cases, not native screenshot measurements. Moving a nonlinear operation to vertices also changes interpolation. Changing texture decoding changes filtering; decoding after a filtered sample does not reproduce filtering already-decoded texels.

## Recommendation and scope choice

Proceed in two independently useful stages:

1. Add a raw lighting-data reader and dependency diagnostics. Preserve lighting facts without applying them or treating their alpha as opacity. This can proceed without selecting a visual approximation.
2. After owner approval, implement a small shared RGB-arithmetic capability for **neutral material inspection**. Keep native lighting, fog and full sampler parity as later capabilities. State the preview context explicitly; do not advertise it as native appearance or production/gameplay readiness.

The alternative is to require native-like lighting before using native materials through the unified preview. That needs per-instance lighting association, additional GPU streams or storage, environment/point-light evaluation, sampling and transfer calibration, and independent native reference captures. It preserves a stronger fidelity target but substantially enlarges the first material milestone. A fixed reference-light bake is another useful inspection profile, but cannot replace changing runtime illumination.

Approval of the unified architecture does not choose between those visual scopes. Neutral lighting is a deliberate preview approximation requiring the owner's decision; full lighting is a broader renderer project. Unknown effects still fail or produce an explicit unsupported result under either choice.

## Proposed neutral RGB capability

Use one typed RGB operation, not a shader graph. Preserve today's multiplication as the unchanged default. The first additional operation evaluates a complete product with gain `1` or `2`, then clamps RGB. Gain does not affect alpha, cutoffs or coverage.

The operation declares its working space:

| Mode | Texture upload/filtering | Vertex/factor processing | Output |
| --- | --- | --- | --- |
| Linear | Decode texture RGB before filtering, as today | Interpolate linear vertex RGB; keep factor linear | Linear RGB to the existing sRGB target |
| sRGB code | Filter normalized encoded texture RGB | Encode supplied linear vertex RGB in the vertex stage, before interpolation; encode the constant linear factor | Decode the resulting encoded RGB before the existing sRGB target |

For code mode, the bake converts a supplied normalized code-space tint into linear transport values before writing standard factor/vertex fields. The shader's inverse transform recovers those code values within the chosen floating-point tolerance before interpolation/product. This keeps the stored standard fields' linear meaning and avoids substituting `encode(interpolate(linear))` for `interpolate(encode(linear))`. It is an explicit operation definition, not a claim that native lighting weights are sRGB colors.

Code-mode vertex colors must use FLOAT accessors; normalized integer `COLOR_0` is refused for this first profile. Low decoded values can disappear when quantized to normalized bytes, so core glTF's otherwise legal compact color encodings are not interchangeable for this transport. Material factors use the existing float conversion. Reference cases include all 256 normalized byte codes, interpolation across the transfer breakpoint and the lowest nonzero code. Render comparisons use a documented per-channel tolerance; no bit-exact native rendering claim follows from a reversible real-number formula.

Alpha remains `textureAlpha * vertexAlpha * factorAlpha`, evaluated independently. MASK survives at or above its normalized cutoff. The first code-space implementation accepts only OPAQUE and MASK. Native-style encoded-space blending cannot be reproduced by simply sending decoded source RGB through today's linear blending stage; BLEND needs a separately defined operation. Linear-mode blending retains its current behavior.

The initial inspection profile uses lighting constant one, fog disabled, repeating bilinear filtering and mip level zero only. It applies no display gamma ramp. These are explicit preview choices matching the current inspector, not certifications of native sampler, fog or lighting equivalence. Unsupported source shading, extra layers, required UV1, animation, additive operations, lighting and render overrides remain explicit. Resolved inputs must not silently acquire white tint or opaque classification.

These choices also support enhanced legacy content. They are intentionally shared numeric/transfer operations; source-qualified provenance stays in the conversion report.

## Encoding and compatibility

Recommend a required, eqoxide-defined glTF extension for this operation, plus the existing manifest/artifact capability gate. The proposed name is `EQOXIDE_materials_rgb_combine`. Its version-1 object requires `version: 1`, `operation: "gain_saturate"`, `gain: 1` or `2`, and `workingSpace: "linear"` or `"srgb_code"`. Version and gain use unsigned JSON integer tokens; decimal/exponent spellings such as `1.0` are refused rather than rounded into a discrete selector. This is an explicit application encoding rule, not a claim about JSON Schema's mathematical integer type. Unknown fields fail. Version 1 fixes the neutral inspection profile just described. No registered Khronos extension is claimed.

Declare it in both `extensionsUsed` and `extensionsRequired` whenever needed. The [glTF extension rules](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#specifying-extensions) require that relationship. Standard fields remain unchanged; extension parameters specify the additional operation. Unknown versions, fields, modes, gains or inconsistent declarations fail before payload activation.

Retain reader contract 2/schema 1 for an additive capability and require `material-rgb-combine-v1`, alongside the existing baseline and vertex-RGBA requirement when used. Old reader-2 builds already reject unknown required capabilities. Do not redefine `static-visual-v1` or treat reader version as a minimum application release number. Capability metadata changes the existing compatibility revision.

This choice has an external-tool cost: tools without the extension cannot faithfully render it. Application `extras` alone would be simpler, but general viewers could ignore the operation and show a misleading approximation. Tools may still be used for geometry inspection when they explicitly support that mode; ordinary material viewing is not a fidelity check.

The current eqoxide inspection profile also retains 16-bit PNG sample precision; core glTF's base-color image rule is 8-bit. Treat this as an engine-specific GLB profile rather than assuming full general-viewer conformance. The new code-space upload must derive from retained original sample depth, including 16-bit input, without an RGBA8 intermediate; budget float preparation before allocating it. This proposal does not remove existing precision support.

Production advertisements remain reader 1/legacy until the package loader, renderer and complete publication gates actually support the selected scope. The isolated inspector may support the operation first. A manifest capability declaration alone is not runtime support.

## Raw lighting data and association

The independently described classic lighting container is `EQGP`, followed by a little-endian `u32` sample count and that many packed 32-bit words. There is no version word in the traced classic path. A libeq reader should preserve the full words, count and any unexplained suffix; apply checked size arithmetic and bounded access before allocation. It should not perform gamma conversion, interpret alpha as coverage or decide provider precedence.

The importer owns associations: terrain lighting uses the terrain basename, binary descriptor v1 object lighting uses the placement name, and v2 placement extensions hold lighting words with record zero serving terrain lighting. Counts must be checked against the appropriate vertex pool. A native fallback on count mismatch is not proof that a complete bake succeeded.

Other `.lit` files have an apparently headerless shape. Their consumer and association remain unverified; extension alone must not select a raw word-array parser. Preserve/report them as unsupported rather than claiming universal LIT support.

Per-instance lighting can differ while geometry remains shared. The current global mesh/material binding is not sufficient for that input. Future normalized lighting records need explicit instance/mesh/vertex associations and independent budgets; forcing them into material tint would lose both meaning and correct operation order.

## Implementation and acceptance order

- Independently implement and mutation-test the bounded raw container reader with synthetic fixtures. Keep provider resolution and source interpretation outside libeq; native-dependent corpus checks remain opt-in.
- Approve the visual scope and required-extension strategy, then finalize the numeric schema and sampling profile. No material resolver should claim unsupported native behavior is resolved.
- Implement typed producer/CPU/GPU operations and capability cross-checks with source-independent fixtures. Budget each GPU texture variant by texture identity and working space; sharing one image across modes does not make duplicate uploads free.
- Verify clipping boundaries, perspective interpolation, filtering gradients, code/linear round trips, independent alpha, unchanged baseline, and explicit rejection of code-mode blending and unknown features. Gain-before-clamp and lighting-after-clamp must discriminate intentional mutations.
- Author and independent reviewer each render their own fixture in CPU/GPU/live inspection, including negative controls and truthful capability/readiness failures.
- Resolve selected source families under the approved inspection profile, report omissions, then perform independent real-zone/reference acceptance. Native material, lighting, collision and gameplay readiness remain distinct gates.

No new bake/client behavior, material approximation, production advertisement or merge is authorized by this proposal. Human review remains required before every merge.

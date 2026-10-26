//! Conditional initial-load geometry preparation over a supplied q32 tile set.
//!
//! The caller must establish complete membership and source decoding separately.
//! This CPU model uses conditional 53-bit/toward-zero arithmetic and fixed load
//! controls. It enables no resource resolver, GLB publication or query route.

use super::{
    terrain_dependencies::Q32SplitStates,
    terrain_hierarchy::Q32Hierarchy,
    terrain_leaf_states::Q32QuadFlags,
    terrain_neighbors::Q32Neighbors,
    terrain_preparation::{prepare_self_53rz, PreparationError, Q32WorkingView},
    terrain_split_stage::Q32HeightGrid,
};

/// One decoded input in source-grid XY coordinates. Input order is immaterial;
/// duplicate coordinate pairs are refused, including byte-identical duplicates.
#[derive(Clone)]
pub struct Q32TileInput {
    pub coordinate: [i32; 2],
    pub heights: Q32HeightGrid,
    pub flags: Q32QuadFlags,
}

/// Logical output budgets, excluding caller inputs, allocator overhead and RSS.
/// Triangle usage is charged from actual emissions before compact reservation.
pub struct Q32LoadLimits {
    pub max_tiles: usize,
    pub max_triangles: usize,
}

impl Default for Q32LoadLimits {
    fn default() -> Self {
        Self {
            max_tiles: 4096,
            max_triangles: 8_388_608,
        }
    }
}

/// Owned diagnostic snapshot after all geometry updates, before any optional
/// cache cleanup. Triangles retain each tile's own update-time emission order.
/// These arrays do not certify source coverage, finite geometry, valid surfaces,
/// render/collision equivalence or native arithmetic-control persistence.
#[derive(Debug)]
pub struct Q32PreparedTile {
    coordinate: [i32; 2],
    heights: Q32HeightGrid,
    flags: Q32QuadFlags,
    states: Q32SplitStates,
    prepared: u8,
    triangles: Vec<[u16; 3]>,
}

impl Q32PreparedTile {
    pub fn coordinate(&self) -> [i32; 2] {
        self.coordinate
    }
    pub fn heights(&self) -> &Q32HeightGrid {
        &self.heights
    }
    pub fn flags(&self) -> &Q32QuadFlags {
        &self.flags
    }
    pub fn states(&self) -> &Q32SplitStates {
        &self.states
    }
    pub fn prepared(&self) -> u8 {
        self.prepared
    }
    pub fn triangles(&self) -> &[[u16; 3]] {
        &self.triangles
    }

    fn view(&mut self) -> Q32WorkingView<'_> {
        Q32WorkingView {
            heights: &mut self.heights,
            flags: &mut self.flags,
            states: &mut self.states,
            prepared: &mut self.prepared,
            local_tolerance: 10.0,
            override_tolerance: 4294967296.0,
        }
    }
}

/// An explicit whole-operation refusal; input arrays are always unchanged.
#[derive(Debug, thiserror::Error)]
pub enum InitialLoadError {
    #[error("tile budget {limit} exceeded by {requested}")]
    TileLimit { requested: usize, limit: usize },
    #[error("unsupported diagnostic coordinate {coordinate:?}")]
    Coordinate { coordinate: [i32; 2] },
    #[error("duplicate coordinate {coordinate:?}")]
    Duplicate { coordinate: [i32; 2] },
    #[error("cannot allocate initial-load output")]
    Allocation(#[from] std::collections::TryReserveError),
    #[error("triangle budget {limit} exceeded by {requested} at {coordinate:?}")]
    TriangleLimit {
        coordinate: [i32; 2],
        requested: usize,
        limit: usize,
    },
    #[error("preparation failed at {coordinate:?}: {source}")]
    Preparation {
        coordinate: [i32; 2],
        #[source]
        source: PreparationError,
    },
}

/// Prepare the entire supplied set in signed ascending X, then Y order.
/// First create all fresh working views, then finish every seam copy, then
/// perform every shared-cache self update. East's first column replaces self's
/// last column before north's first row replaces self's last row. In particular,
/// the corner comes from north before north's own seam turn, not directly NE.
///
/// Sparse coordinates are permitted; absence means only absence from this set.
/// Unique full coordinate pairs determine identity, without scalar hash aliasing.
/// Each coordinate must be within -200000..=200000: a conservative diagnostic
/// admission bound covering neighbor arithmetic and ordinary extent sentinels,
/// not a claimed native input-validation rule. Duplicates need shared-object
/// history outside this model and are refused before working-cache allocation.
///
/// Global tolerance is fixed at 1, local at 10 and override at 2^32. Fresh
/// markers are zero; split storage is deterministically zeroed, including the
/// unused reserved slot whose native initial contents are unspecified. All
/// contexts share state through the ordered pass; they are not reset per self.
/// Existing lazy finite-used-input refusals apply. No extra geometry validation
/// or host FP-control change occurs. Atomic owned scratch and fallible output
/// reservations prevent publishing a partial result, including late failure.
/// Empty input returns an empty diagnostic, without establishing known-empty
/// source coverage. Source binding, quantization, units, complete membership and
/// common-basis conversion remain caller responsibilities.
pub fn prepare_initial_load_53rz(
    input: &[Q32TileInput],
    limits: &Q32LoadLimits,
) -> Result<Vec<Q32PreparedTile>, InitialLoadError> {
    if input.len() > limits.max_tiles {
        return Err(InitialLoadError::TileLimit {
            requested: input.len(),
            limit: limits.max_tiles,
        });
    }
    for tile in input {
        if tile
            .coordinate
            .iter()
            .any(|c| !(-200000..=200000).contains(c))
        {
            return Err(InitialLoadError::Coordinate {
                coordinate: tile.coordinate,
            });
        }
    }
    let mut order = Vec::new();
    order.try_reserve_exact(input.len())?;
    order.extend(0..input.len());
    order.sort_unstable_by_key(|&i| input[i].coordinate);
    for pair in order.windows(2) {
        if input[pair[0]].coordinate == input[pair[1]].coordinate {
            return Err(InitialLoadError::Duplicate {
                coordinate: input[pair[0]].coordinate,
            });
        }
    }
    let mut tiles = Vec::new();
    tiles.try_reserve_exact(input.len())?;
    for i in order {
        tiles.push(Q32PreparedTile {
            coordinate: input[i].coordinate,
            heights: input[i].heights,
            flags: input[i].flags,
            states: [[0; 2048]; 2],
            prepared: 0,
            triangles: Vec::new(),
        });
    }
    if tiles.is_empty() {
        return Ok(tiles);
    }
    let find = |tiles: &[Q32PreparedTile], coordinate| {
        tiles
            .binary_search_by_key(&coordinate, |t| t.coordinate)
            .ok()
    };
    for i in 0..tiles.len() {
        let [x, y] = tiles[i].coordinate;
        if let Some(east) = find(&tiles, [x + 1, y]) {
            for row in 0..33 {
                tiles[i].heights[row][32] = tiles[east].heights[row][0];
            }
        }
        if let Some(north) = find(&tiles, [x, y + 1]) {
            tiles[i].heights[32] = tiles[north].heights[0];
        }
    }
    let hierarchy = Q32Hierarchy::try_new()?;
    let topology = Q32Neighbors::try_new()?;
    let mut total = 0usize;
    for i in 0..tiles.len() {
        let coordinate = tiles[i].coordinate;
        let [x, y] = coordinate;
        let indices = [
            Some(i),
            find(&tiles, [x + 1, y]),
            find(&tiles, [x, y + 1]),
            find(&tiles, [x + 1, y + 1]),
        ];
        let mut ordered = std::array::from_fn::<_, 4, _>(|context| {
            (indices[context].unwrap_or(usize::MAX), context)
        });
        ordered.sort_unstable();
        let emission = {
            let mut views: [Option<Q32WorkingView<'_>>; 4] = std::array::from_fn(|_| None);
            let mut remainder = tiles.as_mut_slice();
            let mut offset = 0;
            for (index, context) in ordered {
                if index == usize::MAX {
                    break;
                }
                let (_, tail) = remainder.split_at_mut(index - offset);
                let (tile, tail) = tail.split_first_mut().expect("present distinct tile");
                views[context] = Some(tile.view());
                remainder = tail;
                offset = index + 1;
            }
            let [mut self_view, east, north, northeast] = views;
            prepare_self_53rz(
                &hierarchy,
                &topology,
                1.0,
                self_view.as_mut().expect("present self"),
                &mut [east, north, northeast],
            )
            .map_err(|source| InitialLoadError::Preparation { coordinate, source })?
        };
        let count = emission.triangles().len();
        if count > limits.max_triangles.saturating_sub(total) {
            return Err(InitialLoadError::TriangleLimit {
                coordinate,
                requested: total.saturating_add(count),
                limit: limits.max_triangles,
            });
        }
        total += count;
        tiles[i].triangles.try_reserve_exact(count)?;
        tiles[i].triangles.extend_from_slice(emission.triangles());
    }
    Ok(tiles)
}

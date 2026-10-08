//! Binary mesh surface assessment; not a complete gameplay collision contract.
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderReference {
    Material,
    Sentinel,
    OutOfRange,
}
#[derive(Debug, Serialize)]
pub struct SurfaceAssessment {
    pub render_reference: RenderReference,
    pub default_collision_query_candidate: bool,
    pub excluded_by_flag2_query: bool,
    pub unclassified_low_flags: u16,
    pub upper_flags: u16,
}

/// Assess one source triangle against the verified default native collision
/// query. Material visibility does not control that query. Other query modes,
/// object-level exclusions, dynamic doors, and collision-hull selection still
/// require separate policy. The upper 16 bits do not enter this collision path;
/// unclassified lower bits are retained without assigning gameplay meanings.
pub fn assess(material_index: u32, material_count: usize, flags: u32) -> SurfaceAssessment {
    SurfaceAssessment {
        render_reference: if material_index == u32::MAX {
            RenderReference::Sentinel
        } else if (material_index as u64) < material_count as u64 {
            RenderReference::Material
        } else {
            RenderReference::OutOfRange
        },
        default_collision_query_candidate: flags & 1 == 0,
        excluded_by_flag2_query: flags & 2 != 0,
        unclassified_low_flags: (flags as u16) & !3,
        upper_flags: (flags >> 16) as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visibility_and_collision_are_independent() {
        let hidden = assess(u32::MAX, 1, 2);
        assert_eq!(hidden.render_reference, RenderReference::Sentinel);
        assert!(hidden.default_collision_query_candidate);
        assert!(hidden.excluded_by_flag2_query);
        let passable = assess(0, 1, 1);
        assert_eq!(passable.render_reference, RenderReference::Material);
        assert!(!passable.default_collision_query_candidate);
        assert!(!passable.excluded_by_flag2_query);
        assert_eq!(
            assess(1, 1, 0).render_reference,
            RenderReference::OutOfRange
        );
        assert!(assess(1, 1, 0).default_collision_query_candidate);
    }
    #[test]
    fn all_flag_combinations_keep_query_filters_and_raw_bits_separate() {
        for low in 0..=u16::MAX {
            let flags = 0xa5a5_0000 | u32::from(low);
            let a = assess(0, 1, flags);
            assert_eq!(a.default_collision_query_candidate, low & 1 == 0);
            assert_eq!(a.excluded_by_flag2_query, low & 2 != 0);
            assert_eq!(a.unclassified_low_flags, low & !3);
            assert_eq!(a.upper_flags, 0xa5a5);
        }
    }
}

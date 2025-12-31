//! High Throughput (HT) JPEG 2000
//!
//! See Rec. ITU-T T.814 | ISO/IEC 15444-15.

#[derive(Debug, PartialEq)]
pub enum CodeBlockMix {
    /// All code-blocks are HT code-blocks.
    AllHt,

    /// Single type per tile-component.
    ///
    /// Each tile-component either consists entirely of HT code-blocks, or
    /// consists entirely of code-blocks conforming to Rec. ITU-T T.800 |
    /// ISO/IEC 15444-1.
    OneOrOther,

    /// Potentially mixed types within tile-component.
    ///
    /// Code-blocks within a tile-component can either HT code-blocks, or
    /// conform to Rec. ITU-T T.800 | ISO/IEC 15444-1.
    Mix,

    /// Reserved.
    ///
    /// This value is reserved for future use by ITU-T | ISO/IEC.
    /// This probably results from an error in generating the file or in
    /// parsing the Ccap<sup>15</sup> bit field.
    Reserved,
}

/// High throughput capabilities (Ccap<sup>15</sup>).
///
/// HT requires the CAP marker segment. The Ccap<sup>15</sup> field
/// contains information that allows a decoder to fast-fail gracefully,
/// optimize its throughput, or generally simplify its operations,
/// without requiring the codestream to be processed in its entirety.
///
/// See ITU-T T.814 | ISO/IEC 15444-15 Section A.3.
struct HtCapabilities {
    bits: u16,
}

impl HtCapabilities {
    pub fn new(ccap15: u16) -> HtCapabilities {
        HtCapabilities { bits: ccap15 }
    }

    /// HT cleanup magnitude bound.
    pub fn magnitude_cleanup_bound(&self) -> u16 {
        let ht_magnitude_cleanup_bits = self.bits & 0b1_1111;
        if ht_magnitude_cleanup_bits == 0 {
            8
        } else if ht_magnitude_cleanup_bits < 20 {
            ht_magnitude_cleanup_bits + 8
        } else if (20..31).contains(&ht_magnitude_cleanup_bits) {
            4 * (ht_magnitude_cleanup_bits - 19) + 27
        } else if ht_magnitude_cleanup_bits == 31 {
            74
        } else {
            unreachable!(
                "{} should be in the range 0..=31",
                ht_magnitude_cleanup_bits
            );
        }
    }

    /// Homogeneous codestream.
    pub fn is_homogeneous_codestream(&self) -> bool {
        !self.is_heterogenous_codestream()
    }

    /// Heterogeneous codestream.
    pub fn is_heterogenous_codestream(&self) -> bool {
        // bit 11
        (self.bits & 0b1000_0000_0000) == 0b1000_0000_0000
    }

    /// HT code-blocks used only with reversible transforms.
    pub fn reversible_transforms(&self) -> bool {
        !self.irreversible_transforms()
    }

    /// HT code-blocks can be used with irreversible transforms.
    pub fn irreversible_transforms(&self) -> bool {
        // bit 5
        (self.bits & 0b10_0000) == 0b10_0000
    }

    /// Region-of-interest marker can be present.
    pub fn region_of_interest_marker_present(&self) -> bool {
        // bit 12
        (self.bits & 0b1_0000_0000_0000) == 0b1_0000_0000_0000
    }

    /// No region-of-interest marker present.
    pub fn no_region_of_interest_marker_present(&self) -> bool {
        !self.region_of_interest_marker_present()
    }

    /// Zero or one HT set is present for any HT code-block
    pub fn single_ht_set_per_codeblock(&self) -> bool {
        !self.multiple_ht_set_per_codeblock()
    }

    /// More than one HT set can be present for a HT code-block.
    ///
    /// This indicates that the codestream, when decoded, can
    /// result in different quality reconstructions.
    pub fn multiple_ht_set_per_codeblock(&self) -> bool {
        // bit 13
        (self.bits & 0b10_0000_0000_0000) == 0b10_0000_0000_0000
    }

    /// Code block mix of content.
    pub fn code_block_style(&self) -> CodeBlockMix {
        let bits14_15 = (self.bits >> 14) & 0b11;
        match bits14_15 {
            0b00 => CodeBlockMix::AllHt,
            0b01 => {
                log::error!("Reserved for future use by ITU-T | ISO/IEC");
                CodeBlockMix::Reserved
            }
            0b10 => CodeBlockMix::OneOrOther,
            0b11 => CodeBlockMix::Mix,
            _ => {
                unreachable!(
                    "Bits 14-15 of Ccap15 are {} but should be in the range 0-3",
                    bits14_15
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_logger() {
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Info)
            .try_init();
    }

    #[test]
    fn test_ccap_0() {
        init_logger();
        let caps = HtCapabilities::new(0b0000_0000_0000_0000);
        assert_eq!(caps.magnitude_cleanup_bound(), 8);
        assert!(caps.reversible_transforms());
        assert!(!caps.irreversible_transforms());
        assert!(caps.is_homogeneous_codestream());
        assert!(!caps.is_heterogenous_codestream());
        assert!(caps.no_region_of_interest_marker_present());
        assert!(!caps.region_of_interest_marker_present());
        assert!(caps.single_ht_set_per_codeblock());
        assert!(!caps.multiple_ht_set_per_codeblock());
        assert_eq!(caps.code_block_style(), CodeBlockMix::AllHt);
    }

    #[test]
    fn test_ccap_1() {
        init_logger();
        let caps = HtCapabilities::new(0b0000_0000_0010_0001);
        assert_eq!(caps.magnitude_cleanup_bound(), 9);
        assert!(!caps.reversible_transforms());
        assert!(caps.irreversible_transforms());
        assert!(caps.is_homogeneous_codestream());
        assert!(!caps.is_heterogenous_codestream());
        assert!(caps.no_region_of_interest_marker_present());
        assert!(!caps.region_of_interest_marker_present());
        assert!(caps.single_ht_set_per_codeblock());
        assert!(!caps.multiple_ht_set_per_codeblock());
        assert_eq!(caps.code_block_style(), CodeBlockMix::AllHt);
    }

    #[test]
    fn test_ccap_2() {
        init_logger();
        let caps = HtCapabilities::new(0b1100_0000_0011_1111);
        assert_eq!(caps.magnitude_cleanup_bound(), 74);
        assert!(!caps.reversible_transforms());
        assert!(caps.irreversible_transforms());
        assert!(caps.is_homogeneous_codestream());
        assert!(!caps.is_heterogenous_codestream());
        assert!(caps.no_region_of_interest_marker_present());
        assert!(!caps.region_of_interest_marker_present());
        assert!(caps.single_ht_set_per_codeblock());
        assert!(!caps.multiple_ht_set_per_codeblock());
        assert_eq!(caps.code_block_style(), CodeBlockMix::Mix);
    }

    #[test]
    fn test_ccap_3() {
        init_logger();
        let caps = HtCapabilities::new(0b1011_1000_0011_1011);
        assert_eq!(caps.magnitude_cleanup_bound(), 59);
        assert!(!caps.reversible_transforms());
        assert!(caps.irreversible_transforms());
        assert!(!caps.is_homogeneous_codestream());
        assert!(caps.is_heterogenous_codestream());
        assert!(!caps.no_region_of_interest_marker_present());
        assert!(caps.region_of_interest_marker_present());
        assert!(!caps.single_ht_set_per_codeblock());
        assert!(caps.multiple_ht_set_per_codeblock());
        assert_eq!(caps.code_block_style(), CodeBlockMix::OneOrOther);
    }

    #[test]
    fn test_ccap_4() {
        init_logger();
        let caps = HtCapabilities::new(0b1010_1000_0010_0011);
        assert_eq!(caps.magnitude_cleanup_bound(), 11);
        assert!(!caps.reversible_transforms());
        assert!(caps.irreversible_transforms());
        assert!(!caps.is_homogeneous_codestream());
        assert!(caps.is_heterogenous_codestream());
        assert!(caps.no_region_of_interest_marker_present());
        assert!(!caps.region_of_interest_marker_present());
        assert!(!caps.single_ht_set_per_codeblock());
        assert!(caps.multiple_ht_set_per_codeblock());
        assert_eq!(caps.code_block_style(), CodeBlockMix::OneOrOther);
    }
}

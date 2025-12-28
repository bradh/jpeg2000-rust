use std::{fs::File, io::BufReader, path::Path};

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

use jpc::{
    decode_jpc, CodingBlockStyle, CommentRegistrationValue, MultipleComponentTransformation,
    ProgressionOrder, QuantizationStyle, TransformationFilter,
};

#[test]
fn test_ds0_ht_01_b11_codestream() {
    init();
    let filename = "ds0_ht_01_b11.j2k";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../samples")
        .join(filename);
    let file = File::open(path).expect("file should exist");
    let mut reader = BufReader::new(file);
    let result = decode_jpc(&mut reader);
    assert!(result.is_ok());
    let codestream = result.unwrap();
    assert_eq!(codestream.length(), 0);
    assert_eq!(codestream.offset(), 0);

    let header = codestream.header();

    let siz = header.image_and_tile_size_marker_segment();
    assert_eq!(siz.reference_grid_width(), 128);
    assert_eq!(siz.reference_grid_height(), 128);
    assert_eq!(siz.image_horizontal_offset(), 0);
    assert_eq!(siz.image_vertical_offset(), 0);
    assert_eq!(siz.offset(), 4);
    assert_eq!(siz.length(), 41);
    assert_eq!(siz.decoder_capabilities(), 0b0100_0000_0000_0000);
    assert_eq!(siz.image_horizontal_offset(), 0);
    assert_eq!(siz.image_vertical_offset(), 0);
    assert_eq!(siz.reference_tile_width(), 128);
    assert_eq!(siz.reference_tile_height(), 128);
    assert_eq!(siz.no_components(), 1);
    assert_eq!(siz.precision(0).unwrap(), 8);
    assert_eq!(siz.values_are_signed(0).unwrap(), false);
    assert_eq!(siz.horizontal_separation(0).unwrap(), 1);
    assert_eq!(siz.vertical_separation(0).unwrap(), 1);

    // CAP
    let maybe_cap = header.extended_capabilities_marker_segment();
    assert!(maybe_cap.is_some());
    let cap = maybe_cap.as_ref().unwrap();
    assert_eq!(cap.length(), 8);
    assert_eq!(
        *cap.capabilities(),
        vec![
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(3),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None
        ]
    );
    assert_eq!(cap.capabilities().len(), 32);
    assert_eq!(cap.capability(2), None);
    assert_eq!(cap.capability(15), Some(3u16));
    assert_eq!(cap.capability_base_zero(0), None);
    assert_eq!(cap.capability_base_zero(14), Some(3u16));
    assert_eq!(cap.capability_base_zero(31), None);

    // TODO: PRF

    // CPF
    let maybe_cpf = header.corresponding_profile_marker_segment();
    assert!(maybe_cpf.is_some());
    let cpf = maybe_cpf.as_ref().unwrap();
    assert_eq!(cpf.length(), 4);
    assert_eq!(cpf.pcpf_raw(), [2]);
    assert_eq!(cpf.cpf_num(), 1); // From jpylyzer

    // COD
    let cod = header.coding_style_marker_segment();
    // Scod
    assert_eq!(cod.coding_style(), 1);
    // SGcod
    assert_eq!(cod.progression_order(), ProgressionOrder::RLLCPP);
    assert_eq!(cod.no_layers(), 1);
    assert_eq!(
        cod.multiple_component_transformation(),
        MultipleComponentTransformation::None
    );
    // SPcod
    assert_eq!(cod.coding_style_parameters().no_decomposition_levels(), 3);
    assert_eq!(cod.coding_style_parameters().code_block_width(), 64);
    assert_eq!(cod.coding_style_parameters().code_block_height(), 64);
    assert_eq!(cod.coding_style_parameters().code_block_style(), 64);
    assert_eq!(
        cod.coding_style_parameters().coding_block_styles(),
        vec![
            CodingBlockStyle::NoSelectiveArithmeticCodingBypass,
            CodingBlockStyle::NoResetOfContextProbabilities,
            CodingBlockStyle::NoTerminationOnEachCodingPass,
            CodingBlockStyle::NoVerticallyCausalContext,
            CodingBlockStyle::NoPredictableTermination,
            CodingBlockStyle::NoSegmentationSymbolsAreUsed
        ]
    );

    assert_eq!(
        cod.coding_style_parameters().transformation(),
        TransformationFilter::Reversible
    );

    // TODO: fix this
    // assert_eq!(cod.coding_style_parameters().has_precinct_size(), true);
    // assert!(cod.coding_style_parameters().precinct_sizes().is_some());

    // COC
    assert!(header.coding_style_component_segment().is_empty());

    // QCD
    let qcd = header.quantization_default_marker_segment();
    assert_eq!(qcd.length(), 13);
    assert_eq!(qcd.quantization_style(), QuantizationStyle::No { guard: 2 }); // style = No Quant
    assert_eq!(
        qcd.quantization_exponents(),
        vec![8, 9, 9, 10, 9, 9, 10, 9, 9, 10]
    );

    // QCC
    assert!(header.quantization_component_segments().is_empty());

    // RGN
    assert!(header.region_of_interest_segments().is_empty());

    // POC
    assert!(header.progression_order_change_segment().is_none());

    // PPM
    assert!(header.packed_packet_headers_segments().is_empty());

    // TLM
    assert!(header.tile_part_lengths_segment().is_none());

    // PLM
    assert!(header.packet_lengths_segments().is_empty());

    // CRG
    assert!(header.component_registration_segment().is_none());

    // COM
    assert_eq!(header.comment_marker_segments().len(), 1);
    let com = header.comment_marker_segments().first().unwrap();
    assert_eq!(com.registration_value(), CommentRegistrationValue::Latin);
    assert!(com.comment_utf8().is_ok());
    assert_eq!(com.comment_utf8().unwrap(), "Kakadu-vxt7.11-Beta");
}

#![allow(dead_code)]

//! JP2 file format.
//!
//! The JPEG 2000 file format (JP2 file format) provides a foundation for storing application specific data (metadata) in
//! association with a JPEG 2000 codestream, such as information which is required to display the image. As many
//! applications require a similar set of information to be associated with the compressed image data, it is useful to define the
//! format of that set of data along with the definition of the compression technology and codestream syntax.
//!
//! Conceptually, the JP2 file format encapsulates the JPEG 2000 codestream along with other core pieces of information
//! about that codestream. The building-block of the JP2 file format is called a box. All information contained within the JP2
//! file is encapsulated in boxes. ITU T.800 | ISO/IEC 15444-1 defines several types of boxes; the definition of each specific
//! box type defines the kinds of information that may be found within a box of that type. Some boxes will be defined to contain other boxes.
//!
//! In addition, some boxes are extended, and new boxes are defined in other standards, such as ITU T.801 | ISO/IEC 15444-2.
//!
//! The main entry point for this module is the `decode_jp2` function. That reads from the provided input, and returns a `JP2File` on success,
//! or an error on failure.

use log::{debug, info, warn};
use std::error;
use std::fmt;
use std::io;
use std::str;

/// Error values that may be returned from JP2 functions.
#[derive(Debug)]
pub enum JP2Error {
    /// Invalid signature.
    ///
    /// The signature box did not match the required value.
    /// This usually means that the file is not JPEG 2000
    /// file format. It could be a codestream without the Annex I
    /// wrapper.
    InvalidSignature { signature: [u8; 4], offset: u64 },

    /// Invalid brand.
    ///
    /// The major brand did not match a supported value.
    InvalidBrand { brand: [u8; 4], offset: u64 },

    /// Unsupported feature.
    ///
    /// At this time only JPEG 2000 part 1 (i.e. ISO/IEC 15444-1 | ITU T.800)
    /// is supported.
    Unsupported,

    /// Not compatible.
    ///
    /// The compatible brands did not contain a supported brand.
    /// At this time, at least `'jp2 '` is required.
    NotCompatible { compatibility_list: Vec<String> },

    /// Unexpected box type.
    ///
    /// An unsupported box was encountered during parsing.
    /// At this time only JPEG 2000 part 1 (i.e. ISO/IEC 15444-1 | ITU T.800)
    /// is supported.
    BoxUnexpected { box_type: BoxType, offset: u64 },

    /// Duplicate box.
    ///
    /// Some boxes are only permitted to be present once in the file.
    /// If the same kind of box is encountered later in the parsing, this
    /// error will be returned.
    BoxDuplicate { box_type: BoxType, offset: u64 },

    /// Malformed box.
    ///
    /// This indicates that the box was not in the expected form. Usually
    /// this indicates some form of truncation during generation or in transit.
    BoxMalformed { box_type: BoxType, offset: u64 },

    /// Missing box.
    ///
    /// Some boxes are required to be present. If a required
    /// box is not present, this error will be returned.
    BoxMissing { box_type: BoxType },
}

impl error::Error for JP2Error {}
impl fmt::Display for JP2Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidSignature { signature, offset } => {
                write!(
                    f,
                    "invalid signature {:?} at offset {}",
                    str::from_utf8(signature).unwrap(),
                    offset
                )
            }
            Self::InvalidBrand { brand, offset } => {
                write!(
                    f,
                    "invalid brand {:?} at offset {}",
                    str::from_utf8(brand).unwrap(),
                    offset
                )
            }
            Self::NotCompatible { compatibility_list } => {
                write!(
                    f,
                    "'jp2 ' not found in compatibility list '{}'",
                    compatibility_list.join(", ")
                )
            }
            Self::BoxDuplicate { box_type, offset } => {
                write!(
                    f,
                    "unexpected duplicate box type {:?} at offset {}",
                    box_type, offset
                )
            }
            Self::BoxUnexpected { box_type, offset } => {
                write!(f, "unexpected box type {:?} at offset {}", box_type, offset)
            }
            Self::BoxMalformed { box_type, offset } => {
                write!(f, "malformed box type {:?} at offset {}", box_type, offset)
            }
            Self::BoxMissing { box_type } => {
                write!(f, "box type {:?} missing", box_type)
            }
            Self::Unsupported => {
                write!(
                    f,
                    "only JPEG 2000 part-1 (ISO 15444-1 / T.800) is supported",
                )
            }
        }
    }
}

// jP\040\040 (0x6A50 2020)
const BOX_TYPE_SIGNATURE: BoxType = [106, 80, 32, 32];
const BOX_TYPE_FILE_TYPE: BoxType = [102, 116, 121, 112];
const BOX_TYPE_HEADER: BoxType = [106, 112, 50, 104];
const BOX_TYPE_IMAGE_HEADER: BoxType = [105, 104, 100, 114];
const BOX_TYPE_BITS_PER_COMPONENT: BoxType = [98, 112, 99, 99];
const BOX_TYPE_COLOUR_SPECIFICATION: BoxType = [99, 111, 108, 114];
const BOX_TYPE_PALETTE: BoxType = [112, 99, 108, 114];
const BOX_TYPE_COMPONENT_MAPPING: BoxType = [99, 109, 97, 112];
const BOX_TYPE_CHANNEL_DEFINITION: BoxType = [99, 100, 101, 102];
const BOX_TYPE_RESOLUTION: BoxType = [114, 101, 115, 32];
const BOX_TYPE_CAPTURE_RESOLUTION: BoxType = [114, 101, 115, 99];
const BOX_TYPE_DEFAULT_DISPLAY_RESOLUTION: BoxType = [114, 101, 115, 100];
const BOX_TYPE_CONTIGUOUS_CODESTREAM: BoxType = [106, 112, 50, 99];
const BOX_TYPE_INTELLECTUAL_PROPERTY: BoxType = [106, 112, 50, 105];
const BOX_TYPE_XML: BoxType = [120, 109, 108, 32];
const BOX_TYPE_UUID: BoxType = [117, 117, 105, 100];
const BOX_TYPE_UUID_INFO: BoxType = [117, 105, 110, 102];
const BOX_TYPE_UUID_LIST: BoxType = [117, 108, 115, 116];
const BOX_TYPE_DATA_ENTRY_URL: BoxType = [117, 114, 108, 32];

// jp2\040
const BRAND_JP2: [u8; 4] = [106, 112, 50, 32];

// jp2\040
const BRAND_JPX: [u8; 4] = [106, 112, 120, 32];

// <CR><LF><0x87><LF> (0x0D0A 870A).
const SIGNATURE_MAGIC: [u8; 4] = [13, 10, 135, 10];

#[derive(Debug)]
enum BoxTypes {
    Signature,
    FileType,
    Header,
    ImageHeader,
    BitsPerComponent,
    ColourSpecification,
    Palette,
    ComponentMapping,
    ChannelDefinition,
    Resolution,
    CaptureResolution,
    DefaultDisplayResolution,
    ContiguousCodestream,
    IntellectualProperty,
    Xml,
    Uuid,
    UUIDInfo,
    UUIDList,
    DataEntryURL,
    Unknown,
}

impl fmt::Display for BoxTypes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl BoxTypes {
    fn new(value: BoxType) -> BoxTypes {
        match value {
            BOX_TYPE_SIGNATURE => BoxTypes::Signature,
            BOX_TYPE_FILE_TYPE => BoxTypes::FileType,
            BOX_TYPE_HEADER => BoxTypes::Header,
            BOX_TYPE_IMAGE_HEADER => BoxTypes::ImageHeader,
            BOX_TYPE_BITS_PER_COMPONENT => BoxTypes::BitsPerComponent,
            BOX_TYPE_COLOUR_SPECIFICATION => BoxTypes::ColourSpecification,
            BOX_TYPE_PALETTE => BoxTypes::Palette,
            BOX_TYPE_COMPONENT_MAPPING => BoxTypes::ComponentMapping,
            BOX_TYPE_CHANNEL_DEFINITION => BoxTypes::ChannelDefinition,

            BOX_TYPE_RESOLUTION => BoxTypes::Resolution,
            BOX_TYPE_CAPTURE_RESOLUTION => BoxTypes::CaptureResolution,
            BOX_TYPE_DEFAULT_DISPLAY_RESOLUTION => BoxTypes::DefaultDisplayResolution,

            BOX_TYPE_CONTIGUOUS_CODESTREAM => BoxTypes::ContiguousCodestream,
            BOX_TYPE_INTELLECTUAL_PROPERTY => BoxTypes::IntellectualProperty,
            BOX_TYPE_XML => BoxTypes::Xml,

            BOX_TYPE_UUID => BoxTypes::Uuid,
            BOX_TYPE_UUID_INFO => BoxTypes::UUIDInfo,
            BOX_TYPE_UUID_LIST => BoxTypes::UUIDList,
            BOX_TYPE_DATA_ENTRY_URL => BoxTypes::DataEntryURL,
            _ => BoxTypes::Unknown,
        }
    }
}

type BoxType = [u8; 4];

/// JPEG 2000 box trait.
///
/// The building-block of the JP2 file format is called a box.
///
/// All information contained within the JP2 file is encapsulated in boxes.
///
/// ISO/IEC 15444-1 / ITU T-800 defines several types of boxes;
/// the definition of each specific box type defines the kinds of information
/// that may be found within a box of that type. Some boxes will be defined to
/// contain other boxes.
///
/// For more information, see ISO/IEC 15444-1 / ITU T-800 Appendix I.4.
pub trait JBox {
    fn identifier(&self) -> BoxType;
    fn length(&self) -> u64;
    fn offset(&self) -> u64;

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>>;
}

/// JPEG 2000 Signature box.
///
/// The Signature box identifies that the format of this file was defined by the
/// JPEG 2000 Recommendation | International Standard, as well as provides a
/// small amount of information which can help determine the validity of the rest
/// of the file.
///
/// The Signature box shall be the first box in the file, and all files shall
/// contain one and only one Signature box.
///
/// For file verification purposes, this box can be considered a fixed-length
/// 12-byte string which shall have the value: 0x0000 000C 6A50 2020 0D0A 870A.
///
/// The combination of the particular type and contents for this box enable an
/// application to detect a common set of file transmission errors.
///
/// - The CR-LF sequence in the contents catches bad file transfers that alter
///   newline sequences.
/// - The control-Z character in the type stops file display under MS-DOS.
/// - The final linefeed checks for the inverse of the CR-LF translation problem.
/// - The third character of the box contents has its high-bit set to catch bad
///   file transfers that clear bit 7.
///
/// For more information, see ISO/IEC 15444-1 / ITU T-800 Appendix I.5.1.
#[derive(Debug, Default)]
pub struct SignatureBox {
    length: u64,
    offset: u64,
}

impl SignatureBox {
    pub fn signature(&self) -> [u8; 4] {
        SIGNATURE_MAGIC
    }
}

impl JBox for SignatureBox {
    // The type of the JPEG 2000 Signature box shall be ‘jP\040\040’ (0x6A50 2020)
    fn identifier(&self) -> BoxType {
        BOX_TYPE_SIGNATURE
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    // The contents of this box shall be the 4-byte character string ‘<CR><LF><0x87><LF>’ (0x0D0A 870A).
    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        self.length = 12;

        let mut buffer: [u8; 4] = [0; 4];

        reader.read_exact(&mut buffer)?;

        if buffer != SIGNATURE_MAGIC {
            return Err(JP2Error::InvalidSignature {
                signature: buffer,
                offset: reader.stream_position()?,
            }
            .into());
        };

        Ok(())
    }
}

type CompatibilityList = Vec<[u8; 4]>;

/// File Type box.
///
/// The File Type box completely defines all of the contents of this file, as
/// well as a separate list of readers with which this file is compatible, and
/// thus the file can be properly interpreted within the scope of that other
/// standard.
///
/// This box shall immediately follow the Signature box.
///
/// All files shall contain one and only one File Type box
///
/// This differentiates between the standard which completely describes the file,
/// from other standards that interpret a subset of the file.
///
/// For more information, see ISO/IEC 15444-1 / ITU T-800 Appendix I.5.2.
#[derive(Debug, Default)]
pub struct FileTypeBox {
    length: u64,
    offset: u64,
    brand: [u8; 4],
    min_version: [u8; 4],
    compatibility_list: CompatibilityList,
}

impl FileTypeBox {
    /// Brand.
    ///
    /// This field specifies the Recommendation | International Standard which
    /// completely defines this file.
    //
    // This field is specified by a four byte string of ISO 646 characters.
    //
    // In addition, the Brand field shall be considered functionally equivalent
    // to a major version number. A major version change (if there ever is one),
    // representing an incompatible change in the JP2 file format, shall define
    // a different value for the Brand field.
    //
    // If the value of the Brand field is not ‘jp2\040’, then a value of
    // ‘jp2\040’ in the Compatibility list indicates that a JP2 reader can
    // interpret the file in some manner as intended by the creator of the
    // file.
    pub fn brand(&self) -> &str {
        str::from_utf8(&self.brand).unwrap()
    }

    /// Minor version.
    ///
    /// This parameter defines the minor version number of this JP2 specification
    /// for which the file complies.
    ///
    /// The parameter is defined as a 4-byte big endian unsigned integer.
    ///
    /// The value of this field shall be zero.
    ///
    /// However, readers shall continue to parse and interpret this file even if
    /// the value of this field is not zero.
    pub fn min_version(&self) -> u32 {
        u32::from_be_bytes(self.min_version)
    }

    /// Compatibility list
    ///
    /// This field specifies a code representing the standard, or a profile of a
    /// standard, to which the file conforms.
    ///
    /// This field is encoded as a four byte string of ISO 646 characters.
    pub fn compatibility_list(&self) -> Vec<String> {
        self.compatibility_list
            .iter()
            .map(|c| str::from_utf8(c).unwrap().to_owned())
            .collect()
    }
}

impl JBox for FileTypeBox {
    // The type of the File Type Box shall be ‘ftyp’ (0x6674 7970).
    fn identifier(&self) -> BoxType {
        BOX_TYPE_FILE_TYPE
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        reader.read_exact(&mut self.brand)?;
        if self.brand == BRAND_JPX {
            return Err(JP2Error::Unsupported {}.into());
        } else if self.brand != BRAND_JP2 {
            return Err(JP2Error::InvalidBrand {
                brand: self.brand,
                offset: reader.stream_position()?,
            }
            .into());
        }

        reader.read_exact(&mut self.min_version)?;

        let mut buffer: [u8; 4] = [0; 4];

        // The number of CL fields is determined by the length of this box
        let mut size = (self.length() - 8) / 4;
        while size > 0 {
            reader.read_exact(&mut buffer)?;
            self.compatibility_list.extend_from_slice(&[buffer]);
            size -= 1;
        }

        // A file shall have at least one CL field in the File Type box, and shall contain the value‘jp2\040’ in one of the CL fields in the File Type box, and all conforming readers shall properly interpret all files with ‘jp2\040’ in one of the CL fields.
        // Other values of the Compatibility list field are reserved for ISO use.
        if !self.compatibility_list.contains(&BRAND_JP2) {
            return Err(JP2Error::NotCompatible {
                compatibility_list: self.compatibility_list().clone(),
            }
            .into());
        }

        Ok(())
    }
}

/// JP2 Header Box.
///
/// The JP2 Header box contains generic information about the file, such as
/// number of components, colourspace, and grid resolution.
///
/// This box is a superbox. That is, it is a container for other boxes.
///
/// Within a JP2 file, there shall be one and only one JP2 Header box.
///
/// Other boxes may be defined in other standards and may be ignored by
/// conforming readers. Those boxes contained within the JP2 Header box that are
/// defined within ISO/IEC 15444-1 | ITU T-800 are as follows:
///
/// - Image Header box - This box specifies information about the image, such
///   as its height and width.
///
/// - Bits Per Component box - This box specifies the bit depth of each
///   component in the codestream after decompression. This box may be found
///   anywhere in the JP2 Header box provided that it comes after the Image Header
///   box.
///
/// - Colour Specification boxes - These boxes specify the colourspace of the
///   decompressed image. The use of multiple Colour Specification boxes
///   provides the ability for a decoder to be given multiple optimization or
///   compatibility options for colour processing. These boxes may be found
///   anywhere in the JP2 Header box provided that they come after the Image Header
///   box. All Colour Specification boxes shall be contiguous within the JP2 Header
///   box.
///
/// - Palette box - This box defines the palette to use to create multiple
///   components from a single component. This box may be found anywhere in the JP2
///   Header box provided that it comes after the Image Header box.
///
/// - Component Mapping box - This box defines how image channels are identified
///   from the actual components in the codestream. This box may be found anywhere
///   in the JP2 Header box provided that it comes after the Image Header box.
///
/// - Channel Definition box - This box defines the channels in the image. This
///   box may be found anywhere in the JP2 Header box provided that it comes after
///   the Image Header box.
///
/// - Resolution box - This box specifies the capture and default display grid
///   resolutions of the image. This box may be found anywhere in the JP2 Header
///   box provided that it comes after the Image Header box.
///
/// For more information, see ISO/IEC 15444-1 | ITU T-800 Appendix I.5.3.
#[derive(Debug, Default)]
pub struct HeaderSuperBox {
    length: u64,
    offset: u64,
    pub image_header_box: ImageHeaderBox,
    pub bits_per_component_box: Option<BitsPerComponentBox>,
    pub colour_specification_boxes: Vec<ColourSpecificationBox>,
    pub palette_box: Option<PaletteBox>,
    pub component_mapping_box: Option<ComponentMappingBox>,
    pub channel_definition_box: Option<ChannelDefinitionBox>,
    pub resolution_box: Option<ResolutionSuperBox>,
}

impl JBox for HeaderSuperBox {
    // The type of the JP2 Header box shall be ‘jp2h’ (0x6A70 3268)
    fn identifier(&self) -> BoxType {
        BOX_TYPE_HEADER
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        let BoxHeader {
            box_length,
            box_type,
            header_length: _,
        } = decode_box_header(reader)?;

        if box_type != self.image_header_box.identifier() {
            return Err(JP2Error::BoxUnexpected {
                box_type,
                offset: reader.stream_position()?,
            }
            .into());
        }
        self.image_header_box.length = box_length;
        self.image_header_box.offset = reader.stream_position()?;
        info!("ImageHeaderBox start at {:?}", self.image_header_box.offset);
        self.image_header_box.decode(reader)?;
        info!("ImageHeaderBox finish at {:?}", reader.stream_position()?);

        loop {
            let BoxHeader {
                box_length,
                box_type,
                header_length,
            } = decode_box_header(reader)?;

            match BoxTypes::new(box_type) {
                BoxTypes::ImageHeader => {
                    // Instances of Image Header box in other places in the file shall be ignored.
                    warn!("ImageHeaderBox found in other place, ignoring");
                }
                BoxTypes::ColourSpecification => {
                    let mut colour_specification_box = ColourSpecificationBox {
                        length: box_length,
                        offset: reader.stream_position()?,
                        method: ColourSpecificationMethods::EnumeratedColourSpace {
                            code: EnumeratedColourSpaces::Reserved,
                        },
                        precedence: [0; 1],
                        colourspace_approximation: [0; 1],
                    };
                    info!(
                        "ColourSpecificationBox start at {:?}",
                        colour_specification_box.offset,
                    );
                    colour_specification_box.decode(reader)?;
                    self.colour_specification_boxes
                        .push(colour_specification_box);
                    info!(
                        "ColourSpecificationBox finish at {:?}",
                        reader.stream_position()?
                    );
                }
                BoxTypes::BitsPerComponent => {
                    // There shall be one and only one Bits Per Component box inside a JP2 Header box.
                    if self.bits_per_component_box.is_some() {
                        return Err(JP2Error::BoxDuplicate {
                            box_type: BOX_TYPE_BITS_PER_COMPONENT,
                            offset: reader.stream_position()?,
                        }
                        .into());
                    }
                    let components_num = self.image_header_box.components_num();
                    let mut bits_per_component_box = BitsPerComponentBox {
                        components_num,
                        bits_per_component: vec![0; components_num as usize],
                        length: box_length,
                        offset: reader.stream_position()?,
                    };
                    info!(
                        "BitsPerComponentBox start at {:?}",
                        bits_per_component_box.offset
                    );
                    bits_per_component_box.decode(reader)?;
                    self.bits_per_component_box = Some(bits_per_component_box);
                    info!(
                        "BitsPerComponentBox finish at {:?}",
                        reader.stream_position()?
                    );
                }
                BoxTypes::Palette => {
                    // There shall be at most one Palette box inside a JP2 Header box.
                    if self.palette_box.is_some() {
                        return Err(JP2Error::BoxDuplicate {
                            box_type: BOX_TYPE_PALETTE,
                            offset: reader.stream_position()?,
                        }
                        .into());
                    }
                    let mut palette_box = PaletteBox {
                        length: box_length,
                        offset: reader.stream_position()?,
                        ..Default::default()
                    };
                    info!("PaletteBox start at {:?}", palette_box.offset);
                    palette_box.decode(reader)?;
                    self.palette_box = Some(palette_box);
                    info!("PaletteBox finish at {:?}", reader.stream_position()?);
                }
                BoxTypes::ComponentMapping => {
                    // There shall be at most one Component Mapping box inside a JP2 Header box.
                    if self.component_mapping_box.is_some() {
                        return Err(JP2Error::BoxDuplicate {
                            box_type: BOX_TYPE_COMPONENT_MAPPING,
                            offset: reader.stream_position()?,
                        }
                        .into());
                    }

                    let mut component_mapping_box = ComponentMappingBox {
                        length: box_length,
                        offset: reader.stream_position()?,
                        mapping: vec![],
                    };
                    info!(
                        "ComponentMappingBox start at {:?}",
                        component_mapping_box.offset
                    );
                    component_mapping_box.decode(reader)?;
                    info!(
                        "ComponentMappingBox finish at {:?}",
                        reader.stream_position()?
                    );
                    self.component_mapping_box = Some(component_mapping_box);
                }
                BoxTypes::ChannelDefinition => {
                    // There shall be at most one Channel Definition box inside a JP2 Header box.
                    if self.channel_definition_box.is_some() {
                        return Err(JP2Error::BoxDuplicate {
                            box_type: BOX_TYPE_CHANNEL_DEFINITION,
                            offset: reader.stream_position()?,
                        }
                        .into());
                    }

                    let mut channel_definition_box = ChannelDefinitionBox {
                        length: box_length,
                        offset: reader.stream_position()?,
                        ..Default::default()
                    };
                    info!(
                        "ChannelDefinitionBox start at {:?}",
                        channel_definition_box.offset
                    );
                    channel_definition_box.decode(reader)?;
                    info!(
                        "ChannelDefinitionBox finish at {:?}",
                        reader.stream_position()?
                    );
                    self.channel_definition_box = Some(channel_definition_box);
                }
                BoxTypes::Resolution => {
                    // There shall be at most one Resolution box inside a JP2 Header box.
                    if self.resolution_box.is_some() {
                        return Err(JP2Error::BoxDuplicate {
                            box_type: BOX_TYPE_RESOLUTION,
                            offset: reader.stream_position()?,
                        }
                        .into());
                    }

                    let mut resolution_box = ResolutionSuperBox {
                        length: box_length,
                        offset: reader.stream_position()?,
                        ..Default::default()
                    };
                    info!("ResolutionBox start at {:?}", resolution_box.offset);
                    resolution_box.decode(reader)?;
                    info!("ResolutionBox finish at {:?}", reader.stream_position()?);
                    self.resolution_box = Some(resolution_box);
                }

                BoxTypes::Unknown => {
                    warn!(
                        "Unknown box type 2 {:?} {:?}",
                        reader.stream_position(),
                        box_type
                    );
                    break;
                }

                // End of header but recognised new box type
                _ => {
                    reader.seek(io::SeekFrom::Current(-(header_length as i64)))?;
                    break;
                }
            }
        }

        // There shall be at least one Colour Specification box
        // within the JP2 Header box.
        if self.colour_specification_boxes.is_empty() {
            return Err(JP2Error::BoxMalformed {
                box_type: BOX_TYPE_IMAGE_HEADER,
                offset: reader.stream_position()?,
            }
            .into());
        }

        // TODO
        // Check that all u16/i16 are correct / big endian is correct

        Ok(())
    }
}

const COMPRESSION_TYPE_WAVELET: u8 = 7;

/// Image Header box.
///
/// This box contains fixed length generic information about the image, such as
/// the image size and number of components.
///
/// The contents of the JP2 Header box shall start with an Image Header box.
///
/// The length of the Image Header box shall be 22 bytes, including the box
/// length and type fields.
///
/// Much of the information within the Image Header box is redundant with
/// information stored in the codestream itself.
///
/// All references to “the codestream” in the descriptions of fields in this
/// Image Header box apply to the codestream found in the first Contiguous
/// Codestream box in the file.
///
/// Files that contain contradictory information between the Image Header box and
/// the first codestream are not conforming files. However, readers may choose
/// to attempt to read these files by using the values found within the
/// codestream.
///
/// For more information, see ISO/IEC 15444-1 | ITU T-800 Appendix I.5.3.1.
#[derive(Debug, Default)]
pub struct ImageHeaderBox {
    length: u64,
    offset: u64,
    height: [u8; 4],
    width: [u8; 4],
    components_num: [u8; 2],
    components_bits: [u8; 1],
    compression_type: [u8; 1],
    colourspace_unknown: [u8; 1],
    intellectual_property: [u8; 1],
}

impl ImageHeaderBox {
    /// Image area height (HEIGHT).
    ///
    /// The value of this parameter indicates the height of the image area.
    /// This field is stored as a 4-byte big endian unsigned integer.
    ///
    /// The value of this field shall be Ysiz – YOsiz, where Ysiz and YOsiz are
    /// the values of the respective fields in the SIZ marker in the codestream.
    ///
    /// However, reference grid points are not necessarily square; the aspect
    /// ratio of a reference grid point is specified by the Resolution box.
    ///
    /// If the Resolution box is not present, then a reader shall assume that
    /// reference grid points are square.
    pub fn height(&self) -> u32 {
        u32::from_be_bytes(self.height)
    }

    /// Image area width (WIDTH).
    ///
    /// The value of this parameter indicates the width of the image area.
    /// This field is stored as a 4-byte big endian unsigned integer.
    ///
    /// The value of this field shall be Xsiz – XOsiz, where Xsiz and XOsiz are
    /// the values of the respective fields in the SIZ marker in the codestream.
    ///
    /// However, reference grid points are not necessarily square; the aspect
    /// ratio of a reference grid point is specified by the Resolution box.
    ///
    /// If the Resolution box is not present, then a reader shall assume that
    /// reference grid points are square
    pub fn width(&self) -> u32 {
        u32::from_be_bytes(self.width)
    }

    /// Number of components (NC).
    ///
    /// This parameter specifies the number of components in the codestream and
    /// is stored as a 2-byte big endian unsigned integer.
    ///
    /// The value of this field shall be equal to the value of the Csiz field in
    /// the SIZ marker in the codestream.
    pub fn components_num(&self) -> u16 {
        u16::from_be_bytes(self.components_num)
    }

    /// Bits per component.
    ///
    /// This parameter specifies the bit depth of the components in the
    /// codestream, minus 1, and is stored as a 1-byte field.
    ///
    /// If the bit depth is the same for all components, then this parameter
    /// specifies that bit depth and shall be equivalent to the values of the
    /// Ssiz<sup>i</sup> fields in the SIZ marker in the codestream (which shall all be
    /// equal).
    ///
    /// If the components vary in bit depth, then the value of this field shall
    /// be 255 and the JP2 Header box shall also contain a Bits Per Component
    /// box defining the bit depth of each component.
    ///
    /// The low 7-bits of the value indicate the bit depth of the components.
    /// The high-bit indicates whether the components are signed or unsigned.
    /// If the high-bit is 1, then the components contain signed values.
    /// If the high-bit is 0, then the components contain unsigned values.
    pub fn components_bits(&self) -> u8 {
        // 1111 1111 (255) Components vary in bit depth
        // 1xxx xxxx (128 - 254) Components are signed values
        // 0xxx xxxx (37 - 127) Components are unsigned values
        if self.components_bits[0] == 255 {
            self.components_bits[0]
        } else {
            // x000 0000 — x010 0101 Component bit depth = value + 1. From 1 bit
            // deep through 38 bits deep respectively (counting the sign bit, if
            // appropriate)
            let low_bits = self.components_bits[0] & 0b0111_1111;
            if low_bits <= 37 {
                low_bits + 1
            } else {
                // All other values reserved for ISO use.
                todo!("reserved");
            }
        }
    }

    /// Signedness of the values.
    ///
    /// See [components_bits](fn@ImageHeaderBox::components_bits) for the BPC encoding.
    ///
    /// This returns true if the components are signed, false if they
    /// are unsigned or it varies (i.e. is given in the BitsPerComponent box).
    pub fn values_are_signed(&self) -> bool {
        if self.components_bits[0] == 255 {
            false
        } else {
            (self.components_bits[0] & 0x80) == 0x80
        }
    }

    /// Compression type (C).
    ///
    /// This parameter specifies the compression algorithm used to compress the
    /// image data.
    ///
    /// The value of this field shall be 7 for ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    /// Other values are reserved for ISO use, and there are other values that
    /// can be found in files conforming to other standards, including ITU-T T.801 | ISO/IEC 15444-2.
    ///
    /// It is encoded as a 1-byte unsigned integer.
    pub fn compression_type(&self) -> u8 {
        self.compression_type[0]
    }

    /// Colourspace Unknown (UnkC).
    ///
    /// This field specifies if the actual colourspace of the image data in the
    /// codestream is known.
    ///
    // /This field is encoded as a 1-byte unsigned integer.
    ///
    /// Legal values for this field are 0, if the colourspace of the image is
    /// known and correctly specified in the Colourspace Specification boxes
    /// within the file, or 1, if the colourspace of the image is not known.
    ///
    /// A value of 1 will be used in cases such as the transcoding of legacy
    /// images where the actual colourspace of the image data is not known.
    ///
    /// In those cases, while the colourspace interpretation methods specified
    /// in the file may not accurately reproduce the image with respect to some
    /// original, the image should be treated as if the methods do accurately
    /// reproduce the image.
    ///
    /// Values other than 0 and 1 are reserved for ISO use. There are no other
    /// values in ITU-T T.801 | ISO/IEC 15444-2 for this field.
    pub fn colourspace_unknown(&self) -> u8 {
        self.colourspace_unknown[0]
    }

    /// Intellectual Property.
    ///
    /// This parameter indicates whether this JP2 file contains intellectual
    /// property rights information.
    ///
    /// If the value of this field is 0, this file does not contain rights
    /// information, and thus the file does not contain an IPR box.
    ///
    /// If the value is 1, then the file does contain rights information and
    /// thus does contain an IPR box.
    ///
    /// Other values are reserved for ISO use. There are no other
    /// values in ITU-T T.801 | ISO/IEC 15444-2 for this field.
    pub fn intellectual_property(&self) -> u8 {
        self.intellectual_property[0]
    }
}

impl JBox for ImageHeaderBox {
    // The type of the Image Header box shall be ‘ihdr’ (0x6968 6472)
    fn identifier(&self) -> BoxType {
        BOX_TYPE_IMAGE_HEADER
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        reader.read_exact(&mut self.height)?;
        reader.read_exact(&mut self.width)?;
        reader.read_exact(&mut self.components_num)?;
        reader.read_exact(&mut self.components_bits)?;
        reader.read_exact(&mut self.compression_type)?;
        reader.read_exact(&mut self.colourspace_unknown)?;
        reader.read_exact(&mut self.intellectual_property)?;

        Ok(())
    }
}

/// Channel Definition Box.
///
/// The Channel Definition box specifies the meaning of the samples in each
/// channel in the image. The exact location of this box within the JP2 Header
/// box may vary provided that it follows the Image Header box.
///
/// The mapping between actual components from the codestream to channels is
/// specified in the Component Mapping box.
///
/// If the JP2 Header box does not contain a Component Mapping box, then a
/// reader shall map component _i_ to channel _i_, for all components in
/// the codestream.
///
/// This box contains an array of channel descriptions. For each description,
/// three values are specified:
/// - the index of the channel described by that association
/// - the type of that channel
/// - and the association of that channel with particular colours.
///
/// This box may specify multiple descriptions for a single channel; however,
/// the type value in each description for the same channel shall be the same in
/// all descriptions.
///
/// If a multiple component transform is specified within the codestream, the
/// image must be in an RGB colourspace and the red, green and blue colours as
/// channels 0, 1 and 2 in the codestream, respectively.
///
/// For more information, see ISO/IEC 15444-1 / ITU T-800 Appendix I.5.3.6
#[derive(Debug, Default)]
pub struct ChannelDefinitionBox {
    length: u64,
    offset: u64,
    channels: Vec<Channel>,
}

impl ChannelDefinitionBox {
    /// Channels in the Channel Definition box.
    ///
    /// The order of channels in the returned vector is the order of channels
    /// in the box. Note the Component Mapping box may map these to a different
    /// order to the components in the bitstream.
    pub fn channels(&self) -> &Vec<Channel> {
        &self.channels
    }
}

/// Channel information.
///
/// This represents one channel within the Channel Definition box.
#[derive(Debug, Default)]
pub struct Channel {
    // Channel index
    //
    // This field specifies the index of the channel for this description.
    //
    // The value of this field represents the index of the channel as defined
    // within the Component Mapping box (or the actual component from the
    // codestream if the file does not contain a Component Mapping box).
    //
    // This field is encoded as a 2-byte big endian unsigned integer.
    channel_index: [u8; 2],

    // Channel type
    //
    // This field specifies the type of the channel for this description.
    // The value of this field specifies the meaning of the decompressed
    // samples in this channel.
    //
    // This field is encoded as a 2-byte big endian unsigned integer.
    channel_type: [u8; 2],

    // Channel association
    //
    // This field specifies the index of the colour for which this channel is
    // directly associated (or a special value to indicate the whole image or
    // the lack of an association).
    //
    // For example, if this channel is an opacity channel for the red channel
    // in an RGB colourspace, this field would specify the index of the colour
    // red.
    channel_association: [u8; 2],
}

impl Channel {
    /// Channel index (Cn<sup>i</sup>).
    ///
    /// This field specifies the index of the channel for this description.
    ///
    /// The value of this field represents the index of the channel as defined
    /// within the Component Mapping box (or the actual component from the
    /// codestream if the file does not contain a Component Mapping box).
    pub fn channel_index(&self) -> u16 {
        u16::from_be_bytes(self.channel_index)
    }

    /// Channel type (Typ<sup>i</sup>).
    ///
    /// This field specifies the type of the channel for this description.
    /// The value of this field specifies the meaning of the decompressed
    /// samples in this channel.
    pub fn channel_type(&self) -> ChannelTypes {
        ChannelTypes::new(self.channel_type)
    }

    /// Channel type (Typ<sup>i</sup>) as unsigned value.
    ///
    /// This field specifies the type of the channel for this description.
    /// The value of this field specifies the meaning of the decompressed
    /// samples in this channel.
    pub fn channel_type_u16(&self) -> u16 {
        u16::from_be_bytes(self.channel_type)
    }

    /// Channel association (Asoc<sup>i</sup>).
    ///
    /// This field specifies the index of the colour for which this channel is
    /// directly associated (or a special value to indicate the whole image or
    /// the lack of an association).
    ///
    /// For example, if this channel is an opacity channel for the red channel
    /// in an RGB colourspace, this field would specify the index of the colour
    /// red.
    // TODO: Map channel association based on colourspace (Table I-18)
    pub fn channel_association(&self) -> u16 {
        u16::from_be_bytes(self.channel_association)
    }
}

// TODO: There shall not be more than one channel in a JP2 file with a the same
// Typ^i and Asoc^i value pair, with the exception of Typ^i and Asoc^i values of
// 2^16 – 1 (not specified)

const CHANNEL_TYPE_COLOUR_IMAGE_DATA: u16 = 0;
const CHANNEL_TYPE_OPACITY_DATA: u16 = 1;
const CHANNEL_TYPE_PREMULTIPLIED_OPACITY: u16 = 3;

/// Channel types.
///
/// For more information, see ISO/IEC 15444-1 / ITU T-800 Table I.16.
#[derive(Debug, PartialEq)]
pub enum ChannelTypes {
    /// Colour image data (0).
    ///
    /// This channel is the colour image data for the associated colour.
    ColourImageData,

    /// Opacity (1).
    ///
    /// A sample value of 0 indicates that the sample is 100% transparent and the maximum value of the
    /// channel (related to the bit depth of the codestream component or the related palette component
    /// mapped to this channel) indicates a 100% opaque sample. All opacity channels shall be mapped
    /// from unsigned components.
    Opacity,

    /// Premultiplied opacity (2).
    ///
    /// Premultiplied opacity. An opacity channel as specified above, except that the value of the
    /// opacity channel has been multiplied into the colour channels for which this channel is associated.
    PremultipliedOpacity,

    /// Reserved.
    ///
    /// A range of values reserved for ITU-T | ISO/IEC use.
    Reserved { value: u16 },

    /// Unspecified.
    ///
    /// Ths type of this channel is not specified.
    Unspecified { value: u16 },
}

impl ChannelTypes {
    fn new(value: [u8; 2]) -> ChannelTypes {
        let channel_type = u16::from_be_bytes(value);

        if channel_type == 0 {
            ChannelTypes::ColourImageData
        } else if channel_type == 1 {
            ChannelTypes::Opacity
        } else if channel_type == 2 {
            ChannelTypes::PremultipliedOpacity
        } else if channel_type <= 2u16.pow(16) - 2 {
            ChannelTypes::Reserved {
                value: channel_type,
            }
        } else {
            ChannelTypes::Unspecified {
                value: channel_type,
            }
        }
    }
}

impl JBox for ChannelDefinitionBox {
    fn identifier(&self) -> BoxType {
        BOX_TYPE_CHANNEL_DEFINITION
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        // Number of channel descriptions. This field specifies the number of
        // channel descriptions in this box. This field is encoded as a 2-byte
        // big endian unsigned integer.
        let mut no_channel_descriptions: [u8; 2] = [0; 2];

        reader.read_exact(&mut no_channel_descriptions)?;

        let mut size = u16::from_be_bytes(no_channel_descriptions);

        let mut channels: Vec<Channel> = Vec::with_capacity(size as usize);

        while size > 0 {
            let mut channel = Channel::default();
            reader.read_exact(&mut channel.channel_index)?;
            reader.read_exact(&mut channel.channel_type)?;
            reader.read_exact(&mut channel.channel_association)?;

            debug!(
                "Found channel at index {:?} of type {:?} and association {:?}",
                channel.channel_index(),
                channel.channel_type(),
                channel.channel_association(),
            );

            channels.push(channel);

            size -= 1;
        }

        self.channels = channels;

        Ok(())
    }
}

const COMPONENT_MAP_TYPE_DIRECT: [u8; 1] = [1];
const COMPONENT_MAP_TYPE_PALETTE: [u8; 1] = [2];

/// Type of component mapping.
///
/// The Component Mapping box supports both direct mapping and indirect
/// (palette) mapping. This enumeration represents which kind of
/// mapping is used.
#[derive(Debug)]
pub enum ComponentMapType {
    /// Direct use.
    ///
    /// This channel is created directly from an actual component in the
    /// codestream.
    /// The index of the component mapped to this channel is specified in the
    /// CMP<sup>i</sup> field for this channel.
    Direct,

    /// Palette mapping.
    ///
    /// This channel is created by applying the palette to an actual component
    /// in the codestream.
    ///
    /// The index of the component mapped into the palette is specified in the
    /// CMP<sup>i</sup> field for this channel.
    /// The column from the palette to use is specified in the PCOL<sup>i</sup>
    /// field for this channel.
    Palette,

    /// Reserved for ITU-T | ISO/IEC use.
    Reserved { value: [u8; 1] },
}

impl ComponentMapType {
    fn new(value: [u8; 1]) -> ComponentMapType {
        match value {
            COMPONENT_MAP_TYPE_DIRECT => ComponentMapType::Direct,
            COMPONENT_MAP_TYPE_PALETTE => ComponentMapType::Palette,
            value => ComponentMapType::Reserved { value },
        }
    }
}

#[derive(Debug)]
/// Component map entry.
///
/// The Component Mapping box contains a sequence of mapping entries. This
/// structure models one entry.
pub struct ComponentMap {
    // This field specifies the index of component from the codestream that is
    // mapped to this channel (either directly or through a palette).
    //
    // This field is encoded as a 2-byte big endian unsigned integer.
    component: [u8; 2],

    // This field specifies how this channel is generated from the actual
    // components in the file. This field is encoded as a 1-byte unsigned
    // integer.
    mapping_type: ComponentMapType,

    // This field specifies the index component from the palette that is used
    // to map the actual component from the codestream.
    // This field is encoded as a 1-byte unsigned integer.
    //
    // If the value of the MTYPi field for this channel is 0, then the value of
    // this field shall be 0.
    palette: [u8; 1],
}

impl ComponentMap {
    /// Component index (CMP<sup>i</sup>).
    ///
    /// This field specifies the index of component from the codestream that is
    /// mapped to this channel (either directly or through a palette).
    ///
    /// This field is encoded as a 2-byte big endian unsigned integer, and
    /// is represented here as an unsigned integer value.
    pub fn component(&self) -> u16 {
        u16::from_be_bytes(self.component)
    }

    /// Mapping type (MTYP<sup>i</sup>).
    ///
    /// This specifies how this channel is generated from the actual
    /// components in the file. This field is encoded as a 1-byte unsigned
    /// integer, and represented here as an enumerated value.
    pub fn mapping_type(&self) -> u8 {
        match self.mapping_type {
            ComponentMapType::Direct => COMPONENT_MAP_TYPE_DIRECT[0],
            ComponentMapType::Palette => COMPONENT_MAP_TYPE_PALETTE[0],
            ComponentMapType::Reserved { value } => value[0],
        }
    }

    /// Palette column index (PCOL<sup>i</sup>).
    ///
    /// This specifies the index component from the palette that is used
    /// to map the actual component from the codestream.
    /// This field is encoded as a 1-byte unsigned integer.
    ///
    /// If the value of the MTYP<sup>i</sup> field for this channel is 0, then the value of
    /// this field shall be 0.
    pub fn palette(&self) -> u8 {
        self.palette[0]
    }
}

/// Component Mapping Box.
///
/// The Component Mapping box defines how image channels are identified from the
/// actual components decoded from the codestream.
///
/// This abstraction allows a single structure (the Channel Definition box) to
/// specify the colour or type of both palettized images and non-palettized
/// images.
///
/// This box contains an array of CMP<sup>i</sup>, MTYP<sup>i</sup> and
/// PCOL<sup>i</sup> fields.
///
/// Each group of these fields represents the definition of one channel in the
/// image.
///
/// The channels are numbered in order starting with zero, and the number of
/// channels specified in the Component Mapping box is determined by the length
/// of the box.
///
/// If the JP2 Header box contains a Palette box, then the JP2 Header box shall
/// also contain a Component Mapping box.
/// If the JP2 Header box does not contain a Palette box, then the JP2 Header box
/// shall not contain a Component Mapping box.
/// In this case, the components shall be mapped directly to channels, such that
/// component _i_ is mapped to channel _i_.
///
/// See ITU T.800 (V4) | ISO/IEC 15444-1:2024 Section I.5.3.5.
#[derive(Debug, Default)]
pub struct ComponentMappingBox {
    length: u64,
    offset: u64,
    mapping: Vec<ComponentMap>,
}

impl ComponentMappingBox {
    pub fn component_map(&self) -> &Vec<ComponentMap> {
        &self.mapping
    }
}

impl JBox for ComponentMappingBox {
    fn identifier(&self) -> BoxType {
        BOX_TYPE_COMPONENT_MAPPING
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        let mut index = 0;
        while index < self.length {
            let mut component_map = ComponentMap {
                component: [0; 2],
                palette: [0; 1],
                mapping_type: ComponentMapType::new([255]),
            };
            reader.read_exact(&mut component_map.component)?;

            let mut mapping_type: [u8; 1] = [0; 1];
            reader.read_exact(&mut mapping_type)?;
            component_map.mapping_type = ComponentMapType::new(mapping_type);

            reader.read_exact(&mut component_map.palette)?;

            self.mapping.push(component_map);
            index += 4;
        }

        Ok(())
    }
}

#[derive(Debug, PartialEq)]
/// Bit depth variations.
pub enum BitDepth {
    /// Signed values.
    ///
    /// The value is the bit depth including the sign bit.
    Signed { value: u8 },

    /// Unsigned values.
    ///
    /// The value is the bit depth.
    Unsigned { value: u8 },

    /// Reserved.
    ///
    /// This value is reserved for ITU-T | ISO/IEC use.
    Reserved { value: u8 },
}

impl BitDepth {
    fn new(byte: u8) -> BitDepth {
        // The low 7-bits of the value indicate the bit depth of this component.
        let value = u8::from_be_bytes([byte << 1 >> 1]) + 1;

        // The high-bit indicates whether the component is signed or unsigned.
        let signedness = byte >> 7;
        match signedness {
            //  If the high-bit is 1, then the component contains signed values
            1 => BitDepth::Signed { value },
            //  If the high-bit is 0, then the component contains unsigned values.
            0 => BitDepth::Unsigned { value },
            _ => BitDepth::Reserved { value },
        }
    }

    /// The number of bits.
    pub fn value(&self) -> u8 {
        match &self {
            Self::Signed { value } => *value,
            Self::Unsigned { value } => *value,
            Self::Reserved { value } => *value,
        }
    }

    /// The number of whole bytes required to store the bit depth
    pub fn num_bytes(&self) -> u8 {
        match &self {
            Self::Signed { value } => value.div_ceil(8),
            Self::Unsigned { value } => value.div_ceil(8),
            Self::Reserved { value } => value.div_ceil(8),
        }
    }

    /// The encoded value.
    pub fn encoded(&self) -> u8 {
        match &self {
            BitDepth::Signed { value } => 0x80 | *value,
            BitDepth::Unsigned { value } => *value,
            BitDepth::Reserved { value } => *value,
        }
    }
}

/// Palette box.
///
/// This box specifies a palette that can be used to create channels from components. However, the Palette box does not
/// specify the creation of any particular channel; the creation of channels based on the application of the palette to a
/// component is specified by the Component Mapping box. The colourspace or meaning of the generated channel is specified
/// by the Channel Definition box (or specified through the defaults defined in the specification of the Channel Definition
/// box if the Channel Definition box does not exist).
///
/// There shall be at most one Palette box inside a JP2 Header box.
///
/// If the JP2 Header box contains a Palette box, then it shall also contain a
/// Component Mapping box. If the JP2 Header box does not contain a Palette box, then it shall not
/// contain a Component Mapping box.
///
/// See ITU-T T.800 (V4) | ISO/IEC 15444-1:2024 Section I.5.3.4 for more information.
#[derive(Debug, Default)]
pub struct PaletteBox {
    length: u64,
    offset: u64,

    bit_depths: Vec<BitDepth>,
    entries: Vec<Vec<u32>>,
}

impl PaletteBox {
    /// The number of entries in the palette.
    ///
    /// This number shall be in the range 1 to 1024.
    pub fn num_entries(&self) -> u16 {
        self.entries.len() as u16
    }

    /// The number of columns in the palette.
    ///
    /// This number shall be in the range 1 to 255.
    pub fn num_components(&self) -> u8 {
        self.bit_depths.len() as u8
    }

    /// The bit depth information for a given component column.
    ///
    /// Each column can have a different bit depth. The column_index
    /// parameter specifies the 0-base column index to query.
    pub fn bit_depth(&self, column_index: u8) -> Option<&BitDepth> {
        self.bit_depths.get(column_index as usize)
    }

    /// The entries in the palette lookup.
    ///
    /// The palette can be considered as a lookup table that has
    /// num_components() columns (inner vector) and num_entries() rows
    /// (outer vector).
    pub fn entries(&self) -> &Vec<Vec<u32>> {
        &self.entries
    }

    /// The entry for a single component column for a given entry.
    ///
    /// The entry_index specifies the row, and the column_index specifies the
    /// column.
    pub fn entry(&self, entry_index: u16, column_index: u8) -> Option<&u32> {
        match &self.entries.get(entry_index as usize) {
            Some(entries) => entries.get(column_index as usize),
            None => None,
        }
    }
}

impl JBox for PaletteBox {
    fn identifier(&self) -> BoxType {
        BOX_TYPE_PALETTE
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        let mut num_entries_bytes = [0u8; 2];
        reader.read_exact(&mut num_entries_bytes)?;
        let num_entries = u16::from_be_bytes(num_entries_bytes);

        let mut num_palette_columns_bytes = [0u8; 1];
        reader.read_exact(&mut num_palette_columns_bytes)?;
        let num_palette_columns = u8::from_be_bytes(num_palette_columns_bytes);

        let mut bit_depth_bytes = [0u8; 1];
        for _ in 0..num_palette_columns {
            reader.read_exact(&mut bit_depth_bytes)?;
            self.bit_depths.push(BitDepth::new(bit_depth_bytes[0]));
        }
        for _ in 0..num_entries {
            let mut entry_components = Vec::<u32>::with_capacity(num_palette_columns as usize);
            for i in 0..num_palette_columns as usize {
                let num_bytes = self.bit_depths[i].num_bytes() as usize;
                let value = match num_bytes {
                    1 => {
                        let mut value_bytes = [0u8; 1];
                        reader.read_exact(&mut value_bytes)?;
                        u8::from_be_bytes(value_bytes) as u32
                    }
                    2 => {
                        let mut value_bytes = [0u8; 2];
                        reader.read_exact(&mut value_bytes)?;
                        u16::from_be_bytes(value_bytes) as u32
                    }
                    _ => unimplemented!(
                        "more than 16 bit data is not yet supported for palette entries"
                    ),
                };
                entry_components.push(value);
            }
            self.entries.push(entry_components);
        }
        Ok(())
    }
}

/// Bits Per Component box.
///
/// The Bits Per Component box specifies the bit depth of each component.
///
/// If the bit depth of all components in the codestream is the same (in both
/// sign and precision), then this box shall not be found. Otherwise, this box
/// specifies the bit depth of each individual component.
///
/// The order of bit depth values in this box is the actual order in which those
/// components are enumerated within the codestream.
///
/// The exact location of this box within the JP2 Header box may vary provided
/// that it follows the Image Header box.
///
/// See ITU-T T.800 (V4) | ISO/IEC 15444-1:2024 Section I.5.3.2.
#[derive(Debug, Default)]
pub struct BitsPerComponentBox {
    length: u64,
    offset: u64,
    components_num: u16,
    bits_per_component: Vec<u8>,
}
impl BitsPerComponentBox {
    /// Bits per component.
    ///
    /// This parameter specifies the bit depth of the components.
    ///
    /// The ordering of the components within the Bits Per Component Box shall
    /// be the same as the ordering of the components within the codestream.
    ///
    /// The number of BPC<sup>i</sup> fields shall be the same as the value of the NC
    /// field from the Image Header box.
    ///
    /// The value of this field shall be equivalent to the respective Ssiz<sup>i</sup>
    /// field in the SIZ marker in the codestream.
    pub fn bits_per_component(&self) -> Vec<BitDepth> {
        self.bits_per_component
            .iter()
            .map(|byte| BitDepth::new(*byte))
            .collect()
    }
}

impl JBox for BitsPerComponentBox {
    fn identifier(&self) -> BoxType {
        BOX_TYPE_BITS_PER_COMPONENT
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        reader.read_exact(&mut self.bits_per_component)?;
        Ok(())
    }
}

type Method = [u8; 1];

const METHOD_ENUMERATED_COLOUR_SPACE: Method = [1];
const METHOD_ENUMERATED_RESTRICTED_ICC_PROFILE: Method = [2];
const METHOD_ENUMERATED_ANY_ICC_PROFILE: Method = [3];
const METHOD_ENUMERATED_VENDOR_METHOD: Method = [4];
const METHOD_ENUMERATED_PARAMETERIZED_COLOUR_SPACE: Method = [5];

#[derive(Debug, PartialEq)]
/// Colour specification methods (METH).
///
/// In ITU-T T.800 | ISO/IEC 15444-1, there are two supported colour specification
/// methods.
///
/// In ITU-T T.801 | ISO/IEC 15444-2, there area five supported colour specification
/// methods.
pub enum ColourSpecificationMethods {
    /// Enumerated colour space, using integer codes.
    ///
    /// This format is the same in both ITU-T T.800 | ISO/IEC 15444-1 and ITU-T T.801 | ISO/IEC 15444-2.
    /// However the JPX file format (ITU-T T.801 | ISO/IEC 15444-2) defines additional enumerated
    /// values and additional parameters for some enumerated colourspaces.
    EnumeratedColourSpace { code: EnumeratedColourSpaces },

    /// Restricted ICC method.
    ///
    /// The Colour Specification box contains an ICC profile in the PROFILE field. This profile shall
    /// specify the transformation needed to convert the decompressed image data into the PCS<sub>XYZ</sub>,
    /// and shall conform to either the Monochrome Input, the Three-Component Matrix-Based Input profile
    /// class, the Monochrome Display or the Three-Component Matrix-Based Display class and contain all
    /// the required tags specified therein, as defined in ISO 15076-1. As such, the value of the Profile
    /// Connection Space field in the profile header in the embedded profile shall be 'XYZ\040'
    /// (0x5859 5A20) indicating that the output colourspace of the profile is in the XYZ colourspace
    ///
    /// Any private tags in the ICC profile shall not change the visual appearance of an image processed
    /// using this ICC profile.
    ///
    /// The components from the codestream may have a range greater than the input range of the tone
    /// reproduction curve (TRC) of the ICC profile. Any decoded values should be clipped to the limits of
    /// the TRC before processing the image through the ICC profile. For example, negative sample values
    /// of signed components may be clipped to zero before processing the image data through the profile.
    ///
    /// See ITU-T T.800(V4) | ISO/IEC 15444-1:2024 J.8 for a more detailed description of the legal
    /// colourspace transforms, for how these transforms are stored in the file, and how to process an image
    /// using that transform without using an ICC colour management engine.
    ///
    /// If the value of METH is 2, then the PROFILE field shall immediately follow the APPROX field and the
    /// PROFILE field shall be the last field in the box.
    ///
    /// The definition of and format of this method is the same in both ITU-T T.800 | ISO/IEC 15444-1
    /// and ITU-T T.801 | ISO/IEC 15444-2.
    RestrictedICCProfile { profile_data: Vec<u8> },

    /// Any ICC method.
    ///
    /// This Colour Specification box indicates that the colourspace of the codestream is specified by an
    /// embedded input ICC profile. Contrary to the Restricted ICC method defined in the JP2 file format
    /// (ITU-T T.800 | ISO/IEC 15444-1), this method allows for any input ICC profile defined by ISO/IEC
    /// 15076-1.
    ///
    /// This method is from ITU-T T.801 | ISO/IEC 15444-2. It is also permitted in ITU-T T.814 | ISO/IEC 15444-15
    /// (High Throughput JPEG 2000) files. It is not permitted in ITU-T T.800 | ISO/IEC 15444-1 files.
    AnyICCProfile { profile_data: Vec<u8> },

    /// Vendor Colour method.
    ///
    /// The Colour Specification box indicates that the colourspace of the codestream is specified by a
    /// unique vendor defined code. The binary format of the METHDAT field is specified in
    /// ITU-T T.801(V4) | ISO/IEC 15444-2:2024 clause M.11.7.3.3.
    ///
    /// This method is from ITU-T T.801 | ISO/IEC 15444-2. It is not permitted in ITU-T T.800 | ISO/IEC 15444-1
    /// or ITU-T T.814 | ISO/IEC 15444-15 (High Throughput JPEG 2000) files.
    VendorColourMethod {
        vendor_defined_code: [u8; 16],
        vendor_parameters: Vec<u8>,
    },

    /// Parameterized colourspace
    ///
    /// The Colour Specification box indicates that the colourspace of the codestream is parameterized as
    /// specified in Rec. ITU-T H.273 | ISO/IEC 23091-2. The binary format of the METHDAT field is specified in
    /// ITU-T T.801(V4) | ISO/IEC 15444-2:2024 clause M.11.7.3.4.
    ///
    /// This method is from ITU-T T.801 | ISO/IEC 15444-2. It is also permitted in ITU-T T.814 | ISO/IEC 15444-15
    /// (High Throughput JPEG 2000) files. It is not permitted in ITU-T T.800 | ISO/IEC 15444-1 files.
    ParameterizedColourspace {
        colour_primaries: u16,
        transfer_characteristics: u16,
        matrix_coefficients: u16,
        video_full_range: bool,
    },

    /// Other value, reserved for use by ITU | ISO/IEC.
    ///
    /// For any value of the METH field, the length of the METHDAT field may not be 0, and applications shall
    /// not expect that the APPROX field be the last field in the box if the value of the METH field is not
    /// understood.
    ///
    /// In this case, a conforming reader shall ignore the entire Colour Specification box.
    Reserved { value: u8 },
}

impl ColourSpecificationMethods {
    pub fn encoded_meth(&self) -> [u8; 1] {
        match self {
            ColourSpecificationMethods::EnumeratedColourSpace { code: _ } => {
                METHOD_ENUMERATED_COLOUR_SPACE
            }
            ColourSpecificationMethods::RestrictedICCProfile { profile_data: _ } => {
                METHOD_ENUMERATED_RESTRICTED_ICC_PROFILE
            }
            ColourSpecificationMethods::AnyICCProfile { profile_data: _ } => {
                METHOD_ENUMERATED_ANY_ICC_PROFILE
            }
            ColourSpecificationMethods::VendorColourMethod {
                vendor_defined_code: _,
                vendor_parameters: _,
            } => METHOD_ENUMERATED_VENDOR_METHOD,
            ColourSpecificationMethods::ParameterizedColourspace {
                colour_primaries: _,
                transfer_characteristics: _,
                matrix_coefficients: _,
                video_full_range: _,
            } => METHOD_ENUMERATED_PARAMETERIZED_COLOUR_SPACE,
            ColourSpecificationMethods::Reserved { value } => [*value],
        }
    }

    fn encoded_methdat(&self) -> Vec<u8> {
        match self {
            ColourSpecificationMethods::EnumeratedColourSpace { code } => code.encoded_methdat(),
            ColourSpecificationMethods::RestrictedICCProfile { profile_data } => {
                profile_data.clone()
            }
            ColourSpecificationMethods::AnyICCProfile { profile_data } => profile_data.clone(),
            ColourSpecificationMethods::VendorColourMethod {
                vendor_defined_code,
                vendor_parameters,
            } => {
                let mut methdat = Vec::<u8>::with_capacity(16 + vendor_parameters.len());
                methdat.extend_from_slice(vendor_defined_code);
                methdat.extend_from_slice(vendor_parameters);
                methdat
            }
            ColourSpecificationMethods::ParameterizedColourspace {
                colour_primaries,
                transfer_characteristics,
                matrix_coefficients,
                video_full_range,
            } => {
                let mut methdat = Vec::<u8>::with_capacity(7); // 3 x u16, plus the flag byte
                methdat.extend_from_slice(&colour_primaries.to_be_bytes());
                methdat.extend_from_slice(&transfer_characteristics.to_be_bytes());
                methdat.extend_from_slice(&matrix_coefficients.to_be_bytes());
                let flags: u8 = if *video_full_range { 0x80 } else { 0x00 };
                methdat.push(flags);
                methdat
            }
            ColourSpecificationMethods::Reserved { value } => {
                vec![*value]
            }
        }
    }
}
impl Default for ColourSpecificationMethods {
    fn default() -> Self {
        ColourSpecificationMethods::Reserved { value: 0 }
    }
}
impl fmt::Display for ColourSpecificationMethods {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ColourSpecificationMethods::EnumeratedColourSpace { code } => {
                write!(f, "Enumerated colourspace: {code}")
            }
            ColourSpecificationMethods::RestrictedICCProfile { profile_data: _ } => {
                // TODO: could provide more info on the profile.
                write!(f, "Restricted ICC Profile")
            }
            ColourSpecificationMethods::AnyICCProfile { profile_data: _ } => {
                // TODO: could provide more info on the profile.
                write!(f, "\"Any\" ICC Profile")
            }
            ColourSpecificationMethods::VendorColourMethod {
                vendor_defined_code: _,
                vendor_parameters: _,
            } => {
                // TODO: could include the UUID.
                write!(f, "Vendor Colour")
            }
            ColourSpecificationMethods::ParameterizedColourspace {
                colour_primaries,
                transfer_characteristics,
                matrix_coefficients,
                video_full_range,
            } => {
                write!(f, "Parameterized colourspace, colour primaries: {colour_primaries}, transfer characteristics: {transfer_characteristics}, matrix coefficients: {matrix_coefficients}, video full range: {video_full_range}")
            }
            ColourSpecificationMethods::Reserved { value } => write!(f, "{}", value),
        }
    }
}

type EnumeratedColourSpace = [u8; 4];

const ENUMERATED_COLOUR_SPACE_BILEVEL: EnumeratedColourSpace = [0, 0, 0, 0];
const ENUMERATED_COLOUR_SPACE_YCBCR1: EnumeratedColourSpace = [0, 0, 0, 1];
// No entry for 2
const ENUMERATED_COLOUR_SPACE_YCBCR2: EnumeratedColourSpace = [0, 0, 0, 3];
const ENUMERATED_COLOUR_SPACE_YCBCR3: EnumeratedColourSpace = [0, 0, 0, 4];
// No entries for 5 to 8
const ENUMERATED_COLOUR_SPACE_PHOTO_YCC: EnumeratedColourSpace = [0, 0, 0, 9];
// No entry for 10
const ENUMERATED_COLOUR_SPACE_CMY: EnumeratedColourSpace = [0, 0, 0, 11];
const ENUMERATED_COLOUR_SPACE_CMYK: EnumeratedColourSpace = [0, 0, 0, 12];
const ENUMERATED_COLOUR_SPACE_YCCK: EnumeratedColourSpace = [0, 0, 0, 13];
const ENUMERATED_COLOUR_SPACE_CIELAB: EnumeratedColourSpace = [0, 0, 0, 14];
const ENUMERATED_COLOUR_SPACE_BILEVEL2: EnumeratedColourSpace = [0, 0, 0, 15];
const ENUMERATED_COLOUR_SPACE_SRGB: EnumeratedColourSpace = [0, 0, 0, 16];
const ENUMERATED_COLOUR_SPACE_GREYSCALE: EnumeratedColourSpace = [0, 0, 0, 17];
const ENUMERATED_COLOUR_SPACE_SYCC: EnumeratedColourSpace = [0, 0, 0, 18];
const ENUMERATED_COLOUR_SPACE_CIEJAB: EnumeratedColourSpace = [0, 0, 0, 19];
const ENUMERATED_COLOUR_SPACE_ESRGB: EnumeratedColourSpace = [0, 0, 0, 20];
const ENUMERATED_COLOUR_SPACE_ROMM_RGB: EnumeratedColourSpace = [0, 0, 0, 21];
const ENUMERATED_COLOUR_SPACE_YPBPR_1125_60: EnumeratedColourSpace = [0, 0, 0, 22];
const ENUMERATED_COLOUR_SPACE_YPBPR_1250_50: EnumeratedColourSpace = [0, 0, 0, 23];
const ENUMERATED_COLOUR_SPACE_ESYCC: EnumeratedColourSpace = [0, 0, 0, 24];
const ENUMERATED_COLOUR_SPACE_SCRGB: EnumeratedColourSpace = [0, 0, 0, 25];
const ENUMERATED_COLOUR_SPACE_SCRGB_GRAYSCALE: EnumeratedColourSpace = [0, 0, 0, 26];

#[derive(Clone, Copy, Debug, PartialEq)]
/// Enumerated colour space values (EnumCS)
///
/// See ITU-T T.800(V4) | ISO/IEC 15444-1:2024 Table I.10 for values allowed in core
/// coding system (JP2) files.
///
/// See ITU-T T.801(V3) | ISO/IEC 15444-2:2023 Table M.25 for values that may
/// occur in extended (JPX) files.
pub enum EnumeratedColourSpaces {
    /// Bi-level.
    ///
    /// This value shall be used to indicate bi-level images. Each image sample is
    /// one bit: 0 = white, 1 = black.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    BiLevel,

    /// YC<sub>b</sub>C<sub>r</sub>(1).
    ///
    /// This is a format often used for data that originated from a video signal.
    /// The colourspace is based on Rec. ITU-R BT.709-4. The valid ranges of the
    /// YC<sub>b</sub>C<sub>r</sub> components in this space is limited to less
    /// than the full range that could be represented given an 8-bit representation.
    /// Rec. ITU-R BT.601-5 specifies these ranges as well as defines a 3 x 3
    /// matrix transformation that can be used to convert these samples into RGB.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    YCbCr1,

    /// YC<sub>b</sub>C<sub>r</sub>(2).
    ///
    /// This is the most commonly used format for image data that was originally
    /// captured in RGB (uncalibrated format). The colourspace is based on Rec.
    /// ITU-R BT.601-5. The valid ranges of the YC<sub>b</sub>C<sub>r</sub>
    /// components in this space is [0, 255] for Y, and [–128, 127] for
    /// C<sub>b</sub> and C<sub>r</sub> (stored with an offset of 128 to convert
    /// the range to [0, 255]). These ranges are different from the ones defined
    /// in Rec. ITU-R BT.601-5. Rec. ITU-R BT.601-5 specifies a 3 x 3 matrix
    /// transformation that can be used to convert these samples into RGB.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    YCbCr2,

    /// YC<sub>b</sub>C<sub>r</sub>(3).
    ///
    /// This is a format often used for data that originated from a video signal.
    /// The colourspace is based on Rec. ITU-R BT.601-5. The valid ranges of the
    /// YC<sub>b</sub>C<sub>r</sub> components in this space is limited to less
    /// than the full range that could be represented given an 8-bit representation.
    /// Rec. ITU-R BT.601-5 specifies these ranges as well as defines a 3 x 3 matrix
    /// transformation that can be used to convert these samples into RGB.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    YCbCr3,

    /// PhotoYCC.
    ///
    /// This is the colour encoding method used in the Photo CD<sup>TM</sup>
    /// system. The colourspace is based on Rec. ITU-R BT.709 reference primaries.
    /// Rec. ITU-R BT.709 linear RGB image signals are transformed to non-linear R'G'B'
    /// values to YCC corresponding to Rec. ITU-R BT.601-5. Details of this encoding
    /// method can be found in Kodak Photo CD products, A Planning Guide for
    /// Developers, Eastman Kodak Company, Part No. DC1200R and also in Kodak Photo
    /// CD Information Bulletin PCD045.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    PhotoYCC,

    /// CMY.
    ///
    /// The encoded data consists of samples of Cyan, Magenta and Yellow samples,
    /// directly suitable for printing on typical CMY devices. A value of 0 shall
    /// indicate 0% ink coverages, whereas a value of 2<sup>BPS</sup>–1 shall
    /// indicate 100% in coverage for a given component sample.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    CMY,

    /// CMYK.
    ///
    /// As CMY above, except that there is also a black (K) ink component. Ink coverage
    /// is defined as above.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    CMYK,

    /// YCCK.
    ///
    /// This is the result of transforming original CMYK type data by computing
    /// R = (2<sup>BPS</sup>–1)–C, G = (2<sup>BPS</sup>–1)–M, and
    /// B = (2<sup>BPS</sup>–1)–Y, applying the RGB to YCC transformation specified
    /// for YC<sub>b</sub>C<sub>r</sub>(2) above, and then recombining the result
    /// with the unmodified K-sample. This transformation is intended to be the same
    /// as that specified in Adobe Postscript.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    YCCK,

    /// CIELab.
    ///
    /// CIELab: The CIE 1976 (L*a*b*) colourspace. A colourspace defined by the CIE
    /// (Commission Internationale de l'Eclairage), having approximately equal
    /// visually perceptible differences between equally spaced points throughout
    /// the space. The three components are L*, or Lightness, and a* and b* in
    /// chrominance. For this colourspace, additional Enumerated parameters are
    /// specified in the EP field as specified in ITU-T T.801 | ISO/IEC 15444-2
    /// clause M.11.7.4.1.
    CIELab {
        rl: u32,
        ol: u32,
        ra: u32,
        oa: u32,
        rb: u32,
        ob: u32,
        il: u32,
    },

    /// Bi-level(2).
    ///
    /// This value shall be used to indicate bi-level images. Each image sample is
    /// one bit: 1 = white, 0 = black.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    BiLevel2,

    /// sRGB.
    ///
    /// sRGB as defined by IEC 61966-2-1 with Lmin<sub>i</sub>=0 and Lmax<sub>i</sub>=255.
    /// This colourspace shall be used with channels carrying unsigned values only.
    #[allow(non_camel_case_types)]
    sRGB,

    /// Grey scale.
    ///
    /// A greyscale space where image luminance is related to code values using the sRGB non-linearity given
    /// in Equations (2) to (4) of IEC 61966-2-1 (sRGB) specification.
    /// This colourspace shall be used with channels carrying unsigned values only.
    Greyscale,

    /// sYCC.
    ///
    /// sYCC as defined by IEC 61966-2-1 / Amd.1 with Lmin<sub>i</sub>=0 and Lmax<sub>i</sub>=255.
    /// This colourspace shall be used with channels carrying unsigned values only.
    ///
    /// Note: it is not recommended to use the ICT or RCT specified in T.800 | ISO/IEC 15444-1 Annex G
    /// with sYCC image data. See T.800 | ISO/IEC 15444-1 J.14 for guidelines on handling YCC codestreams.
    #[allow(non_camel_case_types)]
    sYCC,

    /// CIEJab.
    ///
    /// As defined by CIE Colour Appearance Model 97s, CIE Publication 131. For this
    /// colourspace, additional Enumerated parameters are specified in the EP field as
    /// specified in ITU-T T.801 | ISO/IEC 15444-2 clause M.11.7.4.2.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    CIEJab {
        rj: u32,
        oj: u32,
        ra: u32,
        oa: u32,
        rb: u32,
        ob: u32,
    },

    /// e-sRGB.
    ///
    /// As defined by PIMA 7667.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    #[allow(non_camel_case_types)]
    esRGB,

    /// ROMM-RGB.
    ///
    /// As defined by ISO 22028-2.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    ROMMRGB,

    /// YPbPr(1125/60).
    ///
    /// This is the well-known colour space and value definition for the HDTV
    /// (1125/60/2:1) system for production and international program exchange
    /// specified by Rec. ITU-R BT.709-3. The Recommendation specifies the colour
    /// space conversion matrix from RGB to YPbPr(1125/60) and the range of values
    /// of each component. The matrix is different from the 1250/50 system. In the
    /// 8-bit/component case, the range of values of each component is [1, 254],
    /// the black level of Y is 16, the achromatic level of Pb/Pr is 128, the nominal
    /// peak of Y is 235, and the nominal extremes of Pb/Pr are 16 and 240. In the
    /// 10-bit case, these values are defined in a similar manner.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    YPbPr112560,

    /// YPbPr(1250/50).
    ///
    /// This is the well-known colour space and value definition for the HDTV
    /// (1250/50/2:1) system for production and international program exchange
    /// specified by Rec. ITU-R BT.709-3. The Recommendation specifies the
    /// colour space conversion matrix from RGB to YPbPr(1250/50) and the range
    /// of values of each component. The matrix is different from the 1125/60
    /// system. In the 8-bit/component case, the range of values of each component
    /// is [1, 254], the black level of Y is 16, the achromatic level of Pb/Pr
    /// is 128, the nominal peak of Y is 235, and the nominal extremes of Pb/Pr
    /// are 16 and 240. In the 10-bit case, these values are defined in a similar
    /// manner.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    YPbPr125050,

    /// e-sYCC.
    ///
    /// e-sRGB based YCC colourspace as defined by PIMA 7667:2001, Annex B.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    #[allow(non_camel_case_types)]
    esYCC,

    /// scRGB.
    ///
    /// scRGB as defined by IEC 61966-2-2.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    #[allow(non_camel_case_types)]
    scRGB,

    /// scRGB gray scale.
    ///
    /// scRGB gray scale, using only a luminance channel but the tone reproduction
    /// curves (non-linearities) defined by IEC 61966-2-2.
    ///
    /// This is an extension value from ITU-T T.801 | ISO/IEC 15444-2. This value
    /// is not permitted in ITU-T T.800 | ISO/IEC 15444-1 conformant files.
    #[allow(non_camel_case_types)]
    scRGBGrayScale,

    /// Value reserved for other ITU-T | ISO/IEC uses.
    Reserved,
}

impl EnumeratedColourSpaces {
    fn decode<R: io::Read + io::Seek>(reader: &mut R) -> Result<Self, Box<dyn error::Error>> {
        let mut enumcs: EnumeratedColourSpace = [0u8; 4];
        reader.read_exact(&mut enumcs)?;
        match enumcs {
            ENUMERATED_COLOUR_SPACE_BILEVEL => Ok(EnumeratedColourSpaces::BiLevel),
            ENUMERATED_COLOUR_SPACE_YCBCR1 => Ok(EnumeratedColourSpaces::YCbCr1),
            ENUMERATED_COLOUR_SPACE_YCBCR2 => Ok(EnumeratedColourSpaces::YCbCr2),
            ENUMERATED_COLOUR_SPACE_YCBCR3 => Ok(EnumeratedColourSpaces::YCbCr3),
            ENUMERATED_COLOUR_SPACE_PHOTO_YCC => Ok(EnumeratedColourSpaces::PhotoYCC),
            ENUMERATED_COLOUR_SPACE_CMY => Ok(EnumeratedColourSpaces::CMY),
            ENUMERATED_COLOUR_SPACE_CMYK => Ok(EnumeratedColourSpaces::CMYK),
            ENUMERATED_COLOUR_SPACE_YCCK => Ok(EnumeratedColourSpaces::YCCK),
            ENUMERATED_COLOUR_SPACE_CIELAB => {
                let mut rl_bytes = [0u8; 4];
                let mut ol_bytes = [0u8; 4];
                let mut ra_bytes = [0u8; 4];
                let mut oa_bytes = [0u8; 4];
                let mut rb_bytes = [0u8; 4];
                let mut ob_bytes = [0u8; 4];
                let mut il_bytes = [0u8; 4];
                reader.read_exact(&mut rl_bytes)?;
                reader.read_exact(&mut ol_bytes)?;
                reader.read_exact(&mut ra_bytes)?;
                reader.read_exact(&mut oa_bytes)?;
                reader.read_exact(&mut rb_bytes)?;
                reader.read_exact(&mut ob_bytes)?;
                reader.read_exact(&mut il_bytes)?;
                Ok(EnumeratedColourSpaces::CIELab {
                    rl: u32::from_be_bytes(rl_bytes),
                    ol: u32::from_be_bytes(ol_bytes),
                    ra: u32::from_be_bytes(ra_bytes),
                    oa: u32::from_be_bytes(oa_bytes),
                    rb: u32::from_be_bytes(rb_bytes),
                    ob: u32::from_be_bytes(ob_bytes),
                    il: u32::from_be_bytes(il_bytes),
                })
            }
            ENUMERATED_COLOUR_SPACE_BILEVEL2 => Ok(EnumeratedColourSpaces::BiLevel2),
            ENUMERATED_COLOUR_SPACE_SRGB => Ok(EnumeratedColourSpaces::sRGB),
            ENUMERATED_COLOUR_SPACE_GREYSCALE => Ok(EnumeratedColourSpaces::Greyscale),
            ENUMERATED_COLOUR_SPACE_SYCC => Ok(EnumeratedColourSpaces::sYCC),
            ENUMERATED_COLOUR_SPACE_CIEJAB => {
                let mut rj_bytes = [0u8; 4];
                let mut oj_bytes = [0u8; 4];
                let mut ra_bytes = [0u8; 4];
                let mut oa_bytes = [0u8; 4];
                let mut rb_bytes = [0u8; 4];
                let mut ob_bytes = [0u8; 4];
                reader.read_exact(&mut rj_bytes)?;
                reader.read_exact(&mut oj_bytes)?;
                reader.read_exact(&mut ra_bytes)?;
                reader.read_exact(&mut oa_bytes)?;
                reader.read_exact(&mut rb_bytes)?;
                reader.read_exact(&mut ob_bytes)?;
                Ok(EnumeratedColourSpaces::CIEJab {
                    rj: u32::from_be_bytes(rj_bytes),
                    oj: u32::from_be_bytes(oj_bytes),
                    ra: u32::from_be_bytes(ra_bytes),
                    oa: u32::from_be_bytes(oa_bytes),
                    rb: u32::from_be_bytes(rb_bytes),
                    ob: u32::from_be_bytes(ob_bytes),
                })
            }
            ENUMERATED_COLOUR_SPACE_ESRGB => Ok(EnumeratedColourSpaces::esRGB),
            ENUMERATED_COLOUR_SPACE_ROMM_RGB => Ok(EnumeratedColourSpaces::ROMMRGB),
            ENUMERATED_COLOUR_SPACE_YPBPR_1125_60 => Ok(EnumeratedColourSpaces::YPbPr112560),
            ENUMERATED_COLOUR_SPACE_YPBPR_1250_50 => Ok(EnumeratedColourSpaces::YPbPr125050),
            ENUMERATED_COLOUR_SPACE_ESYCC => Ok(EnumeratedColourSpaces::esYCC),
            ENUMERATED_COLOUR_SPACE_SCRGB => Ok(EnumeratedColourSpaces::scRGB),
            ENUMERATED_COLOUR_SPACE_SCRGB_GRAYSCALE => Ok(EnumeratedColourSpaces::scRGBGrayScale),
            _ => Ok(EnumeratedColourSpaces::Reserved),
        }
    }

    pub fn encoded_methdat(&self) -> Vec<u8> {
        match self {
            EnumeratedColourSpaces::BiLevel => ENUMERATED_COLOUR_SPACE_BILEVEL.to_vec(),
            EnumeratedColourSpaces::YCbCr1 => ENUMERATED_COLOUR_SPACE_YCBCR1.to_vec(),
            EnumeratedColourSpaces::YCbCr2 => ENUMERATED_COLOUR_SPACE_YCBCR2.to_vec(),
            EnumeratedColourSpaces::YCbCr3 => ENUMERATED_COLOUR_SPACE_YCBCR3.to_vec(),
            EnumeratedColourSpaces::PhotoYCC => ENUMERATED_COLOUR_SPACE_PHOTO_YCC.to_vec(),
            EnumeratedColourSpaces::CMY => ENUMERATED_COLOUR_SPACE_CMY.to_vec(),
            EnumeratedColourSpaces::CMYK => ENUMERATED_COLOUR_SPACE_CMYK.to_vec(),
            EnumeratedColourSpaces::YCCK => ENUMERATED_COLOUR_SPACE_YCCK.to_vec(),
            EnumeratedColourSpaces::CIELab {
                rl,
                ol,
                ra,
                oa,
                rb,
                ob,
                il,
            } => {
                let mut methdat = Vec::<u8>::with_capacity(32); // enum value + 7 x u32
                methdat.extend_from_slice(&ENUMERATED_COLOUR_SPACE_CIELAB);
                methdat.extend_from_slice(&rl.to_be_bytes());
                methdat.extend_from_slice(&ol.to_be_bytes());
                methdat.extend_from_slice(&ra.to_be_bytes());
                methdat.extend_from_slice(&oa.to_be_bytes());
                methdat.extend_from_slice(&rb.to_be_bytes());
                methdat.extend_from_slice(&ob.to_be_bytes());
                methdat.extend_from_slice(&il.to_be_bytes());
                methdat
            }
            EnumeratedColourSpaces::BiLevel2 => ENUMERATED_COLOUR_SPACE_BILEVEL2.to_vec(),
            EnumeratedColourSpaces::sRGB => ENUMERATED_COLOUR_SPACE_SRGB.to_vec(),
            EnumeratedColourSpaces::Greyscale => ENUMERATED_COLOUR_SPACE_GREYSCALE.to_vec(),
            EnumeratedColourSpaces::sYCC => ENUMERATED_COLOUR_SPACE_SYCC.to_vec(),
            EnumeratedColourSpaces::CIEJab {
                rj,
                oj,
                ra,
                oa,
                rb,
                ob,
            } => {
                let mut methdat = Vec::<u8>::with_capacity(32); // enum value + 7 x u32
                methdat.extend_from_slice(&ENUMERATED_COLOUR_SPACE_CIEJAB);
                methdat.extend_from_slice(&rj.to_be_bytes());
                methdat.extend_from_slice(&oj.to_be_bytes());
                methdat.extend_from_slice(&ra.to_be_bytes());
                methdat.extend_from_slice(&oa.to_be_bytes());
                methdat.extend_from_slice(&rb.to_be_bytes());
                methdat.extend_from_slice(&ob.to_be_bytes());
                methdat
            }
            EnumeratedColourSpaces::esRGB => ENUMERATED_COLOUR_SPACE_ESRGB.to_vec(),
            EnumeratedColourSpaces::ROMMRGB => ENUMERATED_COLOUR_SPACE_ROMM_RGB.to_vec(),
            EnumeratedColourSpaces::YPbPr112560 => ENUMERATED_COLOUR_SPACE_YPBPR_1125_60.to_vec(),
            EnumeratedColourSpaces::YPbPr125050 => ENUMERATED_COLOUR_SPACE_YPBPR_1250_50.to_vec(),
            EnumeratedColourSpaces::esYCC => ENUMERATED_COLOUR_SPACE_ESYCC.to_vec(),
            EnumeratedColourSpaces::scRGB => ENUMERATED_COLOUR_SPACE_SCRGB.to_vec(),
            EnumeratedColourSpaces::scRGBGrayScale => {
                ENUMERATED_COLOUR_SPACE_SCRGB_GRAYSCALE.to_vec()
            }
            EnumeratedColourSpaces::Reserved => vec![0xff, 0xff, 0xff, 0xff],
        }
    }
}

impl fmt::Display for EnumeratedColourSpaces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                EnumeratedColourSpaces::BiLevel => "Bi-level",
                EnumeratedColourSpaces::YCbCr1 => "YCbCr(1)",
                EnumeratedColourSpaces::YCbCr2 => "YCbCr(2)",
                EnumeratedColourSpaces::YCbCr3 => "YCbCr(3)",
                EnumeratedColourSpaces::PhotoYCC => "PhotoYCC",
                EnumeratedColourSpaces::CMY => "CMY",
                EnumeratedColourSpaces::CMYK => "CMYK",
                EnumeratedColourSpaces::YCCK => "YCCK",
                EnumeratedColourSpaces::CIELab {
                    rl: _,
                    ol: _,
                    ra: _,
                    oa: _,
                    rb: _,
                    ob: _,
                    il: _,
                } => "CIELab",
                EnumeratedColourSpaces::sRGB => "sRGB",
                EnumeratedColourSpaces::Greyscale => "greyscale",
                EnumeratedColourSpaces::sYCC => "sYCC",
                EnumeratedColourSpaces::BiLevel2 => "Bi-level(2)",
                EnumeratedColourSpaces::CIEJab {
                    rj: _,
                    oj: _,
                    ra: _,
                    oa: _,
                    rb: _,
                    ob: _,
                } => "CIEJab",
                EnumeratedColourSpaces::esRGB => "e-sRGB",
                EnumeratedColourSpaces::ROMMRGB => "ROMM-RGB",
                EnumeratedColourSpaces::YPbPr112560 => "YPbPr(1125/60)",
                EnumeratedColourSpaces::YPbPr125050 => "YPbPr(1250/50)",
                EnumeratedColourSpaces::esYCC => "e-sYCC",
                EnumeratedColourSpaces::scRGB => "scRGB",
                EnumeratedColourSpaces::scRGBGrayScale => "scRGB gray scale",
                EnumeratedColourSpaces::Reserved => "Reserved",
            }
        )
    }
}

pub enum ColourspaceMethod {}

/// Colour Specification box.
///
/// Each Colour Specification box defines one method by which an application can
/// interpret the colourspace of the decompressed image data. This colour
/// specification is to be applied to the image data after it has been
/// decompressed and after any reverse decorrelating component transform has been
/// applied to the image data.
///
/// A JP2 file may contain multiple Colour Specification boxes, but must contain
/// at least one, specifying different methods for achieving “equivalent” results.
/// A conforming JP2 reader shall ignore all Colour Specification boxes after the
/// first. However, readers conforming to other standards may use those boxes as
/// defined in those other standards.
///
/// See ITU-T T.800(V4) | ISO/IEC 15444-1:2024 I.5.3.3 for the core requirements.
/// See ITU-T T.801(V3) | ISO/IEC 15444-2:2023 Section M11.7.2 for the extension requirements.
/// See ITU-T T.814 | ISO/IEC 15444-15:2019 Section D.4 for the High Throughput requirements.
#[derive(Debug, Default)]
pub struct ColourSpecificationBox {
    length: u64,
    offset: u64,
    method: ColourSpecificationMethods,
    precedence: [u8; 1],
    colourspace_approximation: [u8; 1],
}

impl ColourSpecificationBox {
    /// Specification method (METH).
    ///
    /// This field specifies the method used by this Colour Specification box to
    /// define the colourspace of the decompressed image.
    ///
    /// This field is encoded as a 1-byte unsigned integer and represented here
    /// as an enumerated value.
    pub fn method(&self) -> &ColourSpecificationMethods {
        &self.method
    }

    /// Precedence (PREC).
    ///
    /// For ITU-T T.800 | ISO/IEC 15444-1, this field shall be 0; however, conforming
    /// readers shall ignore the value of this field. Only a single
    /// Colour Specification box is supported for this case.
    ///
    /// For ITU-T T.801 | ISO/IEC 15444-2, this field specifies the precedence of
    /// this Colour Specification box, with respect to the other Colour Specification
    /// boxes within the same Colour Group box, or the JP2 Header box if this Colour
    /// Specification box is in the JP2 Header box. It is suggested, but not
    /// required, that conforming readers use the colour specification method that
    /// is supported with the highest precedence.
    ///
    /// This field is specified as a signed 1 byte integer.
    pub fn precedence(&self) -> i8 {
        self.precedence[0] as i8
    }

    /// Colourspace approximation (APPROX).
    ///
    /// This field specifies the extent to which this colour specification method
    /// approximates the “correct” definition of the colourspace.
    ///
    /// For ITU-T T.800 | ISO/IEC 15444-1, the value of this field shall be set to
    /// zero; however, conforming readers shall ignore the value of this field.
    ///
    /// For ITU-T T.801 | ISO/IEC 15444-2, contrary to the APPROX field in a JP2
    /// file (a file with "jp2\040" in the BR field in the File Type box), a value
    /// of 0 in the APPROX field is illegal in a JPX file (a file with "jpx\040"
    /// in the BR field in the File Type box). JPX writers are required to properly
    /// indicate the degree of approximation of the colour specification to the
    /// correct definition of the colourspace. This does not specify if the writer
    /// of the file knew the actual colourspace of the image data. If the actual
    /// colourspace is unknown, then the value of the UnkC field in the Image Header
    /// box shall be set to 1 and the APPROX field shall specify the degree to
    /// which this Colour Specification box matches the correct definition of the
    /// assumed or target colourspace. In addition, high values of the APPROX field
    /// (indicating poor approximation) shall not be used to hide that the multiple
    /// Colour Specification boxes in either a Colour Group box or the JP2 Header
    /// box actually represent different colourspaces; the specification of multiple
    /// different colourspaces within a single Colour Group box is illegal. The
    /// legal values are:
    /// - 1: This colour specification method accurately represents the correct
    ///   definition of the colourspace.
    /// - 2: This colour specification method approximates the correct definition
    ///   of the colourspace with exceptional quality.
    /// - 3: This colour specification method approximates the correct definition
    ///   of the colourspace with reasonable quality.
    /// - 4: This colour specification method approximates the correct definition
    ///   of the colourspace with poor quality.
    ///
    /// Other values are reserved.
    ///
    /// This field is specified as 1 byte unsigned integer.
    pub fn colourspace_approximation(&self) -> u8 {
        self.colourspace_approximation[0]
    }
}

impl JBox for ColourSpecificationBox {
    // The type of a Colour Specification box shall be ‘colr’ (0x636F 6C72).
    fn identifier(&self) -> BoxType {
        BOX_TYPE_COLOUR_SPECIFICATION
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        let mut method: Method = [0u8; 1];
        reader.read_exact(&mut method)?;
        reader.read_exact(&mut self.precedence)?;
        reader.read_exact(&mut self.colourspace_approximation)?;

        if self.precedence() != 0 {
            warn!("Precedence {:?} Unexpected", self.precedence());
        }
        if self.colourspace_approximation() != 0 {
            warn!(
                "Colourspace Approximation {:?} unexpected",
                self.colourspace_approximation()
            );
        }

        debug!("Method {:?}", method);
        debug!("Precedence {:?}", self.precedence());
        debug!(
            "ColourSpace Approximation {:?}",
            self.colourspace_approximation()
        );

        self.method = match method {
            // 1 - Enumerated Colourspace.
            //
            // This colourspace specification box contains the enumerated value
            // of the colourspace of this image.
            //
            // The enumerated value is found in the EnumCS field in this box.
            // If the value of the METH field is 1, then the EnumCS shall exist
            // in this box immediately following the APPROX field, and the
            // EnumCS field shall be the last field in this box
            METHOD_ENUMERATED_COLOUR_SPACE => ColourSpecificationMethods::EnumeratedColourSpace {
                code: EnumeratedColourSpaces::decode(reader)?,
            },

            // 2 - Restricted ICC profile.
            // This Colour Specification box contains an ICC profile in the PROFILE field.
            //
            // This profile shall specify the transformation needed to convert the decompressed image data into the PCS_XYZ, and shall conform to either the Monochrome Input or Three-Component Matrix-Based Input profile class, and contain all the required tags specified therein, as defined in ICC.1:1998-09.
            //
            // As such, the value of the Profile Connection Space field in the profile header in the embedded profile shall be ‘XYZ\040’ (0x5859 5A20) indicating that the
            // output colourspace of the profile is in the XYZ colourspace.
            //
            // Any private tags in the ICC profile shall not change the visual appearance of an image processed using this ICC profile.
            //
            // The components from the codestream may have a range greater than the input range of the tone reproduction curve (TRC) of the ICC profile.
            //
            // Any decoded values should be clipped to the limits of the TRC before processing the image through the ICC profile.
            //
            // For example,
            // negative sample values of signed components may be clipped to zero before processing the image data through the profile.
            //
            // If the value of METH is 2, then the PROFILE field shall immediately follow the APPROX field and the PROFILE field shall be the last field in the box.
            METHOD_ENUMERATED_RESTRICTED_ICC_PROFILE => {
                let mut restricted_icc_profile = vec![0; self.length as usize - 3];

                reader.read_exact(&mut restricted_icc_profile)?;
                debug!("Restricted ICC Profile");
                ColourSpecificationMethods::RestrictedICCProfile {
                    profile_data: restricted_icc_profile,
                }
            }
            METHOD_ENUMERATED_ANY_ICC_PROFILE => {
                let mut any_icc_profile = vec![0; self.length as usize - 3];

                reader.read_exact(&mut any_icc_profile)?;
                debug!("Any ICC Profile");
                ColourSpecificationMethods::AnyICCProfile {
                    profile_data: any_icc_profile,
                }
            }
            METHOD_ENUMERATED_VENDOR_METHOD => {
                let mut vendor_defined_code = [0u8; 16];
                let mut vendor_parameters = vec![0; self.length as usize - 16];
                reader.read_exact(&mut vendor_defined_code)?;
                reader.read_exact(&mut vendor_parameters)?;
                debug!("Vendor method");
                ColourSpecificationMethods::VendorColourMethod {
                    vendor_defined_code,
                    vendor_parameters,
                }
            }
            METHOD_ENUMERATED_PARAMETERIZED_COLOUR_SPACE => {
                let mut colprims = [0u8; 2];
                let mut transfc = [0u8; 2];
                let mut matcoeffs = [0u8; 2];
                let mut flags = [0u8; 1];
                reader.read_exact(&mut colprims)?;
                reader.read_exact(&mut transfc)?;
                reader.read_exact(&mut matcoeffs)?;
                reader.read_exact(&mut flags)?;
                ColourSpecificationMethods::ParameterizedColourspace {
                    colour_primaries: u16::from_be_bytes(colprims),
                    transfer_characteristics: u16::from_be_bytes(transfc),
                    matrix_coefficients: u16::from_be_bytes(matcoeffs),
                    video_full_range: flags[0] & 0x80 == 0x80,
                }
            }
            _ => {
                debug!("Reserved method {}", method[0]);
                ColourSpecificationMethods::Reserved { value: method[0] }
            }
        };

        Ok(())
    }
}

/// Resolution box (superbox)
///
/// This box specifies the capture and default display grid resolutions of this
/// image.
///
/// See Part 1 Section I.5.3.7 for more information.
#[derive(Debug, Default)]
pub struct ResolutionSuperBox {
    length: u64,
    offset: u64,

    capture_resolution_box: Option<CaptureResolutionBox>,

    default_display_resolution_box: Option<DefaultDisplayResolutionBox>,
}

impl ResolutionSuperBox {
    /// Capture Resolution box.
    ///
    /// This box specifies the grid resolution at which this image was captured.
    pub fn capture_resolution_box(&self) -> &Option<CaptureResolutionBox> {
        &self.capture_resolution_box
    }

    /// Default Display Resolution box.
    ///
    /// This box specifies the default grid resolution at which this image
    /// should be displayed.
    pub fn default_display_resolution_box(&self) -> &Option<DefaultDisplayResolutionBox> {
        &self.default_display_resolution_box
    }
}

impl JBox for ResolutionSuperBox {
    fn identifier(&self) -> BoxType {
        BOX_TYPE_RESOLUTION
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    // The type of a Resolution box shall be ‘res\040’ (0x7265 7320).
    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        loop {
            let BoxHeader {
                box_length,
                box_type,
                header_length,
            } = decode_box_header(reader)?;

            match BoxTypes::new(box_type) {
                BoxTypes::CaptureResolution => {
                    if self.capture_resolution_box.is_some() {
                        return Err(JP2Error::BoxUnexpected {
                            box_type: BOX_TYPE_CAPTURE_RESOLUTION,
                            offset: reader.stream_position()?,
                        }
                        .into());
                    }
                    let mut capture_resolution_box = CaptureResolutionBox {
                        length: box_length,
                        offset: reader.stream_position()?,
                        ..Default::default()
                    };
                    info!(
                        "CaptureResolutionBox start at {:?}",
                        capture_resolution_box.offset
                    );
                    capture_resolution_box.decode(reader)?;
                    info!(
                        "CaptureResolutionBox finish at {:?}",
                        reader.stream_position()?
                    );
                    self.capture_resolution_box = Some(capture_resolution_box);
                }
                BoxTypes::DefaultDisplayResolution => {
                    if self.default_display_resolution_box.is_some() {
                        return Err(JP2Error::BoxUnexpected {
                            box_type: BOX_TYPE_DEFAULT_DISPLAY_RESOLUTION,
                            offset: reader.stream_position()?,
                        }
                        .into());
                    }

                    let mut default_display_resolution_box = DefaultDisplayResolutionBox {
                        length: box_length,
                        offset: reader.stream_position()?,
                        ..Default::default()
                    };
                    info!(
                        "DisplayResolutionBox start at {:?}",
                        default_display_resolution_box.offset
                    );
                    default_display_resolution_box.decode(reader)?;
                    info!(
                        "DisplayResolutionBox finish at {:?}",
                        reader.stream_position()?
                    );
                    self.default_display_resolution_box = Some(default_display_resolution_box);
                }

                // End of capture resolution but recognised new box type
                _ => {
                    reader.seek(io::SeekFrom::Current(-(header_length as i64)))?;
                    break;
                }
            }
        }

        // If this box exists, it shall contain either a Capture Resolution box,
        // or a Default Display Resolution box, or both.
        if self.capture_resolution_box.is_none() && self.default_display_resolution_box.is_none() {
            return Err(JP2Error::BoxMalformed {
                box_type: BOX_TYPE_RESOLUTION,
                offset: self.offset,
            }
            .into());
        }

        Ok(())
    }
}

/// Intellectual Property box.
///
/// A box type for a box which is devoted to carrying intellectual property
/// rights information within a JP2 file.
///
/// Inclusion of this information in a JP2 file is optional for conforming files.
///
/// In ISO/IEC 15444-1 / T.800, the definition of the format of the contents of
/// this box is reserved for ISO.
///
/// However, the type of this box is defined as a means to allow applications to
/// recognize the existence of IPR information.
///
/// In ISO/IEC 15444-2 / T.801, the definition of the format of the contents of
/// this box is given as XML. See ISO/IEC 15444-2 / T.801 Annex N.
#[derive(Debug, Default)]
pub struct IntellectualPropertyBox {
    length: u64,
    offset: u64,
    data: Vec<u8>,
}

impl IntellectualPropertyBox {
    /// Get the XML body as a UTF-8 string.
    pub fn format(&self) -> String {
        str::from_utf8(&self.data).unwrap().to_string()
    }
}

impl JBox for IntellectualPropertyBox {
    // The type of the Intellectual Property Box shall be ‘jp2i’ (0x6A70 3269).
    fn identifier(&self) -> BoxType {
        BOX_TYPE_INTELLECTUAL_PROPERTY
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        self.data = vec![0; self.length as usize];
        reader.read_exact(&mut self.data)?;
        Ok(())
    }
}

/// XML box
///
/// An XML box contains vendor specific information (in XML format) other than
/// the information contained within boxes defined.
///
/// There may be multiple XML boxes within the file, and those boxes may be found
/// anywhere in the file except before the File Type box.
///
/// A potential use for this is embedding vendor or domain-specific metadata.
///
/// See ISO/IEC 15444-1:2024 Section I.7.1 for more details on this box.
#[derive(Debug, Default)]
pub struct XMLBox {
    length: u64,
    offset: u64,
    xml: Vec<u8>,
}

impl XMLBox {
    /// Get the XML body as a UTF-8 string.
    pub fn format(&self) -> String {
        str::from_utf8(&self.xml).unwrap().to_string()
    }
}

impl JBox for XMLBox {
    // The type of an XML box is ‘xml\040’ (0x786D 6C20).
    fn identifier(&self) -> BoxType {
        BOX_TYPE_XML
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        self.xml = vec![0; self.length as usize];
        reader.read_exact(&mut self.xml)?;
        Ok(())
    }
}

/// UUID box.
///
/// A UUID box contains vendor specific information other than the information
/// contained within boxes defined.
///
/// There may be multiple UUID boxes within the file, and those boxes may be
/// found anywhere in the file except before the File Type box.
///
/// See ISO/IEC 15444-1:2024 Section I.7.2 for more details on this box.
#[derive(Debug, Default)]
pub struct UUIDBox {
    length: u64,
    offset: u64,
    uuid: [u8; 16],
    data: Vec<u8>,
}

impl UUIDBox {
    /// Get the UUID for the box.
    ///
    /// This field contains a 16-byte UUID as specified by ISO/IEC 11578. The
    /// value of this UUID specifies the format of the vendor-specific information
    /// stored in the DATA field and the interpretation of that information.
    pub fn uuid(&self) -> &[u8; 16] {
        &self.uuid
    }

    /// Get the vendor-specific information.
    ///
    /// This field contains vendor-specific information. The format of this information
    /// is defined outside of the scope of ISO/IEC 15444-1, but is indicated by the
    /// value of the UUID field.
    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

impl JBox for UUIDBox {
    // The type of a UUID box shall be ‘uuid’ (0x7575 6964).
    fn identifier(&self) -> BoxType {
        BOX_TYPE_UUID
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        reader.read_exact(&mut self.uuid)?;
        self.data = vec![0; self.length as usize - self.uuid.len()];
        reader.read_exact(&mut self.data)?;

        Ok(())
    }
}

/// UUID Info box (superbox)
///
/// While it is useful to allow vendors to extend JP2 files by adding information
/// using UUID boxes, it is also useful to provide information in a standard form
/// which can be used by non-extended applications to get more information about
/// the extensions in the file. This information is contained in UUID Info boxes.
///
/// A JP2 file may contain zero or more UUID Info boxes.
///
/// These boxes may be found anywhere in the top level of the file (the superbox
/// of a UUID Info box shall be the JP2 file itself) except before the File Type
/// box.
///
/// These boxes, if present, may not provide a complete index for the UUIDs in
/// the file, may reference UUIDs not used in the file, and possibly may provide
/// multiple references for the same UUID.
///
/// See ITU-T T.800 (V4) | ISO/IEC 15444-1:2024 Section I.7.3.
#[derive(Debug, Default)]
pub struct UUIDInfoSuperBox {
    length: u64,
    offset: u64,
    uuid_list: Option<UUIDListBox>,
    data_entry_url_box: Option<DataEntryURLBox>,
}

impl UUIDInfoSuperBox {
    /// UUID List box (UList).
    ///
    /// This box contains a list of UUIDs for which this UUID Info box specifies
    /// a link to more information.
    pub fn uuid_list_box(&self) -> &Option<UUIDListBox> {
        &self.uuid_list
    }

    /// Data Entry IRL box (DE).
    ///
    /// This box contains a URL. An application can acquire more information
    /// about the UUIDs contained in the UUID List box.
    pub fn data_entry_url_box(&self) -> &Option<DataEntryURLBox> {
        &self.data_entry_url_box
    }
}

impl JBox for UUIDInfoSuperBox {
    // The type of a UUID Info box shall be 'uinf' (0x7569 6E66)
    fn identifier(&self) -> BoxType {
        BOX_TYPE_UUID_INFO
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        _reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        Ok(())
    }
}

/// UUID List box.
///
/// This box contains a list of UUIDs.
///
/// See ITU-T T.800 (V4) | ISO/IEC 15444-1:2024 Section I.7.3.1.
#[derive(Debug, Default)]
pub struct UUIDListBox {
    length: u64,
    offset: u64,

    // IDs.
    //
    // Each instance of this field specifies one UUID, as specified in ISO/IEC 11578, which
    // shall be associated with the URL contained in the URL box within the
    // same UUID Info box.
    //
    // The number of UUIDi fields shall be the same as the value of the NU
    // field.
    //
    // The value of this field shall be a 16-byte UUID
    ids: Vec<[u8; 16]>,
}

impl UUIDListBox {
    pub fn ids(&self) -> &Vec<[u8; 16]> {
        &self.ids
    }
    pub fn number_of_uuids(&self) -> u16 {
        self.ids().len() as u16
    }
}

impl JBox for UUIDListBox {
    // The type of a UUID List box shall be ‘ulst’ (0x756C 7374)
    fn identifier(&self) -> BoxType {
        BOX_TYPE_UUID_LIST
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        let mut number_of_uuids = [0u8; 2];
        reader.read_exact(&mut number_of_uuids)?;

        let mut size = u16::from_be_bytes(number_of_uuids) as usize;

        self.ids = Vec::with_capacity(size);

        let mut buffer: [u8; 16] = [0; 16];
        while size > 0 {
            reader.read_exact(&mut buffer)?;
            self.ids.extend_from_slice(&[buffer]);
            size -= 1;
        }

        Ok(())
    }
}

/// Data Entry URL box.
///
/// This box contains a URL which can be used by an application to acquire more
/// information about the associated vendor-specific extensions.
///
/// The format of the information acquired through the use of this URL is not
/// defined in ITU-T T.800 | ISO/IEC 15444-1.
///
/// The URL type should be of a service which delivers a file (e.g., URLs of
/// type file, http, ftp, etc.), which ideally also permits random access.
///
/// Relative URLs are permissible and are relative to the file containing this
/// Data Entry URL box.
///
/// See ITU-T T.800 (V4) | ISO/IEC 15444-1:2024 Section I.7.3.2.
#[derive(Debug, Default)]
pub struct DataEntryURLBox {
    length: u64,
    offset: u64,

    // VERS: Version number.
    //
    // This field specifies the version number of the format of this box and is
    // encoded as a 1-byte unsigned integer.
    //
    // The value of this field shall be 0.
    version: [u8; 1],

    // FLAG: Flags.
    //
    // This field is reserved for other uses to flag particular attributes of
    // this box and is encoded as a 3-byte unsigned integer.
    //
    // The value of this field shall be 0.
    flags: [u8; 3],

    // LOC: Location.
    //
    // This field specifies the URL of the additional information associated
    // with the UUIDs contained in the UUID List box within the same UUID Info
    // superbox.
    //
    // The URL is encoded as a null terminated string of UTF-8 characters.
    location: Vec<u8>,
}

impl DataEntryURLBox {
    /// Version (VERS).
    ///
    /// This field specifies the version number of the format of this box and is
    /// encoded as a 1-byte unsigned integer.
    ///
    /// The value of this field shall be 0.
    pub fn version(&self) -> u8 {
        self.version[0]
    }

    /// Flags (FLAG).
    ///
    /// This field is reserved for other uses to flag particular attributes of
    /// this box and is encoded as a 3-byte unsigned integer.
    ///
    /// The value of this field shall be 0 (`0x00 0x00 0x00`).
    pub fn flags(&self) -> &[u8; 3] {
        &self.flags
    }

    /// Location (LOC).
    ///
    /// This field specifies the URL of the additional information associated
    /// with the UUIDs contained in the UUID List box within the same UUID Info
    /// superbox.
    ///
    /// The URL is encoded as a null terminated string of ISO/IEC 646 (effectively ASCII)
    /// characters, but this accessor function converts it a standard Rust string without
    /// the null terminator.
    pub fn location(&self) -> Result<&str, str::Utf8Error> {
        let ascii = str::from_utf8(&self.location)?;
        Ok(ascii.trim_matches(char::from(0)))
    }
}

impl JBox for DataEntryURLBox {
    // The type of a Data Entry URL box shall be 'url\040' (0x7572 6C20).
    fn identifier(&self) -> BoxType {
        BOX_TYPE_DATA_ENTRY_URL
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        reader.read_exact(&mut self.version)?;
        reader.read_exact(&mut self.flags)?;

        // location
        let mut size = self.length() - 4;

        let mut buffer: [u8; 1] = [0; 1];
        while size > 0 {
            reader.read_exact(&mut buffer)?;
            self.location.extend_from_slice(&buffer);
            size -= 1;
        }

        Ok(())
    }
}

/// Contiguous Codestream box
///
/// The Contiguous Codestream box contains a valid and complete JPEG 2000
/// codestream. When displaying the image, a conforming T.800 | ISO/IEC 15444-1
/// reader shall ignore all codestreams after the first codestream found in the file.
///
/// Note: there can be other codestream boxes, and this is valid in some extensions
/// in T.801 | ISO/IEC 15444-2.
///
/// Contiguous Codestream boxes may be found anywhere in the file
/// except before the JP2 Header box.
///
/// The intention is that this box provides the information required to get the
/// codestream data, rather than holding the entire codestream. If the codestream
/// is required, seek to the codestream offset, and read up the codestream length
/// number of bytes.
///
/// See T.800 | ISO/IEC 15444-1 Section I.5.4.
#[derive(Debug, Default)]
pub struct ContiguousCodestreamBox {
    length: u64,
    pub offset: u64,
}

impl JBox for ContiguousCodestreamBox {
    // The type of a Contiguous Codestream box shall be ‘jp2c’
    fn identifier(&self) -> BoxType {
        BOX_TYPE_CONTIGUOUS_CODESTREAM
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        if self.length == 0 {
            reader.seek(io::SeekFrom::End(0))?;
            self.length = reader.stream_position()? - self.offset;
        } else {
            reader.seek(io::SeekFrom::Current(self.length as i64))?;
        }

        Ok(())
    }
}

/// Default Display Resolution box.
///
/// This box specifies a desired display grid resolution.
///
/// For example, this may be used to determine the size of the image on a page
/// when the image is placed in a page-layout program.
///
/// However, this value is only a default. Each application must determine an
/// appropriate display size for that application.
///
/// See Part 1 Section I.5.3.7.2 for more information.
#[derive(Debug, Default)]
pub struct DefaultDisplayResolutionBox {
    length: u64,
    offset: u64,

    // Vertical Display grid resolution numerator.
    vertical_display_grid_resolution_numerator: [u8; 2],

    // Vertical Display grid resolution denominator.
    vertical_display_grid_resolution_denominator: [u8; 2],

    // Horizontal Display grid resolution numerator.
    horizontal_display_grid_resolution_numerator: [u8; 2],

    // Horizontal Display grid resolution denominator.
    horizontal_display_grid_resolution_denominator: [u8; 2],

    // Vertical Display grid resolution exponent.
    vertical_display_grid_resolution_exponent: [u8; 1],

    // Horizontal Display grid resolution exponent.
    horizontal_display_grid_resolution_exponent: [u8; 1],
}

impl DefaultDisplayResolutionBox {
    pub fn vertical_display_grid_resolution_numerator(&self) -> u16 {
        u16::from_be_bytes(self.vertical_display_grid_resolution_numerator)
    }
    pub fn vertical_display_grid_resolution_denominator(&self) -> u16 {
        u16::from_be_bytes(self.vertical_display_grid_resolution_denominator)
    }
    pub fn horizontal_display_grid_resolution_numerator(&self) -> u16 {
        u16::from_be_bytes(self.horizontal_display_grid_resolution_numerator)
    }
    pub fn horizontal_display_grid_resolution_denominator(&self) -> u16 {
        u16::from_be_bytes(self.horizontal_display_grid_resolution_denominator)
    }
    pub fn vertical_display_grid_resolution_exponent(&self) -> i8 {
        self.vertical_display_grid_resolution_exponent[0] as i8
    }
    pub fn horizontal_display_grid_resolution_exponent(&self) -> i8 {
        self.horizontal_display_grid_resolution_exponent[0] as i8
    }

    // VRd = VRdN/VRdD * 10^VRdE
    pub fn vertical_display_grid_resolution(&self) -> f64 {
        self.vertical_display_grid_resolution_numerator() as f64
            / self.vertical_display_grid_resolution_denominator() as f64
            * (10_f64).powi(self.vertical_display_grid_resolution_exponent() as i32)
    }

    // HRd = HRdN/HRdD * 10^HRdE
    pub fn horizontal_display_grid_resolution(&self) -> f64 {
        self.horizontal_display_grid_resolution_numerator() as f64
            / self.horizontal_display_grid_resolution_denominator() as f64
            * (10_f64).powi(self.horizontal_display_grid_resolution_exponent() as i32)
    }
}

impl JBox for DefaultDisplayResolutionBox {
    fn identifier(&self) -> BoxType {
        BOX_TYPE_DEFAULT_DISPLAY_RESOLUTION
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        reader.read_exact(&mut self.vertical_display_grid_resolution_numerator)?;
        reader.read_exact(&mut self.vertical_display_grid_resolution_denominator)?;

        reader.read_exact(&mut self.horizontal_display_grid_resolution_numerator)?;
        reader.read_exact(&mut self.horizontal_display_grid_resolution_denominator)?;

        reader.read_exact(&mut self.vertical_display_grid_resolution_exponent)?;
        reader.read_exact(&mut self.horizontal_display_grid_resolution_exponent)?;

        Ok(())
    }
}

/// Capture Resolution box
///
/// This box specifies the grid resolution at which the source was digitized to
/// create the image samples specified by the codestream.
///
/// For example, this may specify the resolution of the flatbed scanner that
/// captured a page from a book. The capture grid resolution could also specify
/// the resolution of an aerial digital camera or satellite camera.
///
/// See Part 1 Section I.5.3.7.1 for more information.
#[derive(Debug, Default)]
pub struct CaptureResolutionBox {
    length: u64,
    offset: u64,

    // VRcN: Vertical Capture grid resolution numerator.
    //
    // This parameter specifies the VRcN value in which is used to calculate
    // the vertical capture grid resolution.
    //
    // This parameter is encoded as a 2-byte big endian unsigned integer.
    vertical_capture_grid_resolution_numerator: [u8; 2],

    // VRcD: Vertical Capture grid resolution denominator.
    //
    // This parameter specifies the VRcD value which is used to calculate the
    // vertical capture grid resolution.
    //
    // This parameter is encoded as a 2-byte big endian unsigned integer.
    vertical_capture_grid_resolution_denominator: [u8; 2],

    // HRcN: Horizontal Capture grid resolution numerator.
    //
    // This parameter specifies the HRcN value  which is used to calculate the
    // horizontal capture grid resolution.
    //
    // This parameter is encoded as a 2-byte big endian unsigned integer.
    horizontal_capture_grid_resolution_numerator: [u8; 2],

    // HRcD: Horizontal Capture grid resolution denominator.
    //
    // This parameter specifies the HRcD value in which is used to calculate
    // the horizontal capture grid resolution.
    //
    // This parameter is encoded as a 2-byte big endian unsigned integer.
    horizontal_capture_grid_resolution_denominator: [u8; 2],

    // VRcE: Vertical Capture grid resolution exponent.
    //
    // This parameter specifies the VRcE value which is used to calculate the
    // vertical capture grid resolution.
    //
    // This parameter is encoded as a twos-complement 1-byte signed integer.
    vertical_capture_grid_resolution_exponent: [u8; 1],

    // HRcE: Horizontal Capture grid resolution exponent.
    //
    // This parameter specifies the HRcE value in which is used to calculate
    // the horizontal capture grid resolution.
    //
    // This parameter is encoded as a twos-complement 1-byte signed integer.
    horizontal_capture_grid_resolution_exponent: [u8; 1],
}

impl CaptureResolutionBox {
    pub fn vertical_capture_grid_resolution_numerator(&self) -> u16 {
        u16::from_be_bytes(self.vertical_capture_grid_resolution_numerator)
    }
    pub fn vertical_capture_grid_resolution_denominator(&self) -> u16 {
        u16::from_be_bytes(self.vertical_capture_grid_resolution_denominator)
    }
    pub fn horizontal_capture_grid_resolution_numerator(&self) -> u16 {
        u16::from_be_bytes(self.horizontal_capture_grid_resolution_numerator)
    }
    pub fn horizontal_capture_grid_resolution_denominator(&self) -> u16 {
        u16::from_be_bytes(self.horizontal_capture_grid_resolution_denominator)
    }
    pub fn vertical_capture_grid_resolution_exponent(&self) -> i8 {
        self.vertical_capture_grid_resolution_exponent[0] as i8
    }
    pub fn horizontal_capture_grid_resolution_exponent(&self) -> i8 {
        self.horizontal_capture_grid_resolution_exponent[0] as i8
    }

    // VRc = (VRcN / VRcD) * 10^VRcE
    // The values VRc and HRc are always in reference grid points per meter.
    pub fn vertical_resolution_capture(&self) -> f64 {
        let mut vertical_resolution_capture: f64 = self.vertical_capture_grid_resolution_numerator()
            as f64
            / self.vertical_capture_grid_resolution_denominator() as f64;

        vertical_resolution_capture *=
            10_f64.powi(self.vertical_capture_grid_resolution_exponent() as i32);

        vertical_resolution_capture
    }

    // HRc = (HRcN / HRcD) * 10^HRcE
    // The values VRc and HRc are always in reference grid points per meter.
    pub fn horizontal_resolution_capture(&self) -> f64 {
        let mut horizontal_resolution_capture: f64 =
            self.horizontal_capture_grid_resolution_numerator() as f64
                / self.horizontal_capture_grid_resolution_denominator() as f64;

        horizontal_resolution_capture *=
            10_f64.powi(self.horizontal_capture_grid_resolution_exponent() as i32);

        horizontal_resolution_capture
    }
}

impl JBox for CaptureResolutionBox {
    // The type of a Capture Resolution box shall be ‘resc’ (0x7265 7363).
    fn identifier(&self) -> BoxType {
        BOX_TYPE_CAPTURE_RESOLUTION
    }

    fn length(&self) -> u64 {
        self.length
    }

    fn offset(&self) -> u64 {
        self.offset
    }

    fn decode<R: io::Read + io::Seek>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), Box<dyn error::Error>> {
        reader.read_exact(&mut self.vertical_capture_grid_resolution_numerator)?;
        reader.read_exact(&mut self.vertical_capture_grid_resolution_denominator)?;
        reader.read_exact(&mut self.horizontal_capture_grid_resolution_numerator)?;
        reader.read_exact(&mut self.horizontal_capture_grid_resolution_denominator)?;
        reader.read_exact(&mut self.vertical_capture_grid_resolution_exponent)?;
        reader.read_exact(&mut self.horizontal_capture_grid_resolution_exponent)?;

        Ok(())
    }
}

/// JP2 file format instance.
///
/// This structure models the JP2 file format defined in ITU-T T.800 | ISO/IEC 15444-1
/// Annex I. Each instance of this structure is conceptually equal to a file.
///
/// From ITU-T T.800 (V4) | ISO/IEC 15444-1:2024, Section I.2:
///
/// > The JPEG 2000 file format (JP2 file format) provides a foundation for storing application
/// > specific data (metadata) in association with a JPEG 2000 codestream, such as information
/// > which is required to display the image. As many applications require a similar set of
/// > information to be associated with the compressed image data, it is useful to define the
/// > format of that set of data along with the definition of the compression technology and
/// > codestream syntax.
///
/// > Conceptually, the JP2 file format encapsulates the JPEG 2000 codestream along with
/// > other core pieces of information about that codestream. The building-block of the JP2
/// > file format is called a box. All information contained within the JP2 file is encapsulated
/// > in boxes. This Recommendation | International Standard defines several types of boxes;
/// > the definition of each specific box type defines the kinds of information that may
/// > be found within a box of that type. Some boxes will be defined to contain other boxes.
///
/// The box structure used in the JP2 file format is (intentionally) very similar to the
/// ISO Base Media File Format (ISO/IEC 14496-12), which is used to encapsulate video in
/// MPEG 4 (ISO/IEC 14496-14) and HEIF (ISO/IEC 23008-12) amongst other uses.
#[derive(Debug)]
pub struct JP2File {
    length: u64,
    signature: Option<SignatureBox>,
    file_type: Option<FileTypeBox>,
    header: Option<HeaderSuperBox>,
    contiguous_codestreams: Vec<ContiguousCodestreamBox>,
    intellectual_property: Option<IntellectualPropertyBox>,
    xml: Vec<XMLBox>,
    uuid: Vec<UUIDBox>,
    uuid_info: Vec<UUIDInfoSuperBox>,
}

impl JP2File {
    pub fn length(&self) -> u64 {
        self.length
    }

    /// JPEG 2000 Signature box.
    ///
    /// This box uniquely identifies the file as being part of the JPEG 2000 family of files.
    ///
    /// This box is required.
    pub fn signature_box(&self) -> &Option<SignatureBox> {
        &self.signature
    }

    /// File Type box.
    ///
    /// This box specifies file type, version and compatibility information, including
    /// specifying if this file is a conforming JP2 file or if it can be read by a
    /// conforming JP2 reader.
    ///
    /// This box is required.
    pub fn file_type_box(&self) -> &Option<FileTypeBox> {
        &self.file_type
    }

    /// JP2 Header box.
    ///
    /// This box contains a series of boxes that contain header-type information
    /// about the file.
    ///
    /// This box is required.
    pub fn header_box(&self) -> &Option<HeaderSuperBox> {
        &self.header
    }

    /// Contiguous codestream boxes.
    ///
    /// This box contains the codestream as defined by ITU-T T.800 | ISO/IEC 15444-1 Annex A.
    ///
    /// This box is required. It can be present multiple times. ITU-T T.800 | ISO/IEC 15444-1
    /// readers shall ignore the codestream boxes after the first box. However there is
    /// use of additional boxes in ITU-T T.801 | ISO/IEC 15444-2 and potentially other
    /// standards and profiles.
    pub fn contiguous_codestreams_boxes(&self) -> &Vec<ContiguousCodestreamBox> {
        &self.contiguous_codestreams
    }

    /// Intellectual Property Box associated with this file.
    ///
    /// This box contains Intellectual property rights (IPR) related information
    /// associated with the image such as moral rights, copyrights as well as
    /// exploitation information.
    ///
    /// In ISO/IEC 15444-1 / T.800 the content of this box is reserved to ISO.
    ///
    /// In ISO/IEC 15444-2 / T.801 Section N.5.4, the content of this box is
    /// required to be well formed XML. See Annex N for more detail on the JPX
    /// file format extended metadata definition and syntax.
    pub fn intellectual_property_box(&self) -> &Option<IntellectualPropertyBox> {
        &self.intellectual_property
    }

    /// XML boxes.
    ///
    /// An XML box provides a tool by which vendors can add XML formatted information to
    /// a JP2 file.
    ///
    /// This box is not required, and can be present multiple times.
    pub fn xml_boxes(&self) -> &Vec<XMLBox> {
        &self.xml
    }

    /// UUID boxes.
    ///
    /// This box provides a tool by which vendors can add additional information to a file
    /// without risking conflict with other vendors.
    ///
    /// This box is not required, and can be present multiple times.
    pub fn uuid_boxes(&self) -> &Vec<UUIDBox> {
        &self.uuid
    }

    /// UUID Info boxes associated with this file.
    ///
    /// These boxes provide a tool by which a vendor may provide access to
    /// additional information associated with a UUID.
    pub fn uuid_info_boxes(&self) -> &Vec<UUIDInfoSuperBox> {
        &self.uuid_info
    }
}

struct BoxHeader {
    // Box Length
    //
    // This field specifies the length of the box, stored as a 4-byte big
    // endian unsigned integer.
    //
    // This value includes all of the fields of the box, including the length
    // and type.
    box_length: u64,

    // Box Type
    //
    // This field specifies the type of information found in the DBox field.
    //
    // The value of this field is encoded as a 4-byte big endian unsigned
    // integer. However, boxes are generally referred to by an ISO 646
    // character string translation of the integer value.
    //
    // For all box types defined box types will be indicated as both character
    // string (normative) and as 4-byte hexadecimal integers (informative).
    //
    // Also, a space character is shown in the character string translation of
    // the box type as “\040”.
    //
    // All values of TBox not defined are reserved for ISO use.
    box_type: [u8; 4],

    header_length: u8,
}

fn decode_box_header<R: io::Read + io::Seek>(
    reader: &mut R,
) -> Result<BoxHeader, Box<dyn error::Error>> {
    let mut header_length = 8;
    let mut box_length: [u8; 4] = [0; 4];
    let mut box_type: [u8; 4] = [0; 4];

    reader.read_exact(&mut box_length)?;

    let mut box_length_value = u32::from_be_bytes(box_length) as u64;
    if box_length_value == 0 {
        // If the value of this field is 0, then the length of the box was not known when the LBox field was written. In this case, this box contains all bytes up to the end of the file. If a box of length 0 is contained with in another box (its superbox), then the length of that superbox shall also be 0. This means that this box is the last box in the file.
        reader.read_exact(&mut box_type)?;
    } else if box_length_value == 1 {
        // If the value of this field is 1, then the XLBox field shall exist and the value of that field shall be the actual length of the box.
        reader.read_exact(&mut box_type)?;

        let mut xl_length: [u8; 8] = [0; 8];
        // This field specifies the actual length of the box if the value of the LBox field is 1.
        // This field is stored as an 8-byte big endian unsigned integer. The value includes all of the fields of the box, including the LBox, TBox and XLBox fields
        reader.read_exact(&mut xl_length)?;

        box_length_value = u64::from_be_bytes(xl_length) - 16;
        header_length = 16;
    } else if box_length_value <= 7 {
        // The values 2–7 are reserved for ISO use.
        panic!("unsupported reserved box length {:?}", box_length_value);
    } else {
        reader.read_exact(&mut box_type)?;

        // Subtract LBox and TBox from length
        box_length_value -= 8;
    }

    Ok(BoxHeader {
        box_length: box_length_value,
        box_type,
        header_length,
    })
}

// TODO: Consider lazy parsing where possible
pub fn decode_jp2<R: io::Read + io::Seek>(
    reader: &mut R,
) -> Result<JP2File, Box<dyn error::Error>> {
    let BoxHeader {
        box_length,
        box_type,
        header_length: _,
    } = decode_box_header(reader)?;

    // TODO: Enforce the following
    // Check Image Headerbox (header, width) with codestream and allow user to read it otherwise
    // If resolution box is not present, then a header shall assume that reference grid points are square.

    let mut signature_box = SignatureBox::default();
    // The Signature box shall be the first box
    if box_type != signature_box.identifier() {
        return Err(JP2Error::BoxUnexpected {
            box_type,
            offset: reader.stream_position()?,
        }
        .into());
    }
    signature_box.length = box_length;
    signature_box.offset = reader.stream_position().unwrap();
    info!("SignatureBox start at {:?}", signature_box.length);
    signature_box.decode(reader)?;
    info!("SignatureBox finish at {:?}", reader.stream_position()?);

    let BoxHeader {
        box_length,
        box_type,
        header_length: _,
    } = decode_box_header(reader)?;
    // The File Type box shall immediately follow the Signature box
    let mut file_type_box = FileTypeBox {
        length: box_length,
        offset: reader.stream_position().unwrap(),
        brand: [0; 4],
        min_version: [0; 4],
        compatibility_list: vec![],
    };
    if box_type != file_type_box.identifier() {
        return Err(JP2Error::BoxUnexpected {
            box_type,
            offset: reader.stream_position()?,
        }
        .into());
    }
    info!("FileTypeBox start at {:?}", file_type_box.offset);
    file_type_box.decode(reader)?;
    info!("FileTypeBox finish at {:?}", reader.stream_position()?);

    let mut header_box_option: Option<HeaderSuperBox> = None;
    let mut contiguous_codestream_boxes: Vec<ContiguousCodestreamBox> = vec![];
    let mut intellectual_property_option: Option<IntellectualPropertyBox> = None;

    let mut xml_boxes: Vec<XMLBox> = vec![];
    let mut uuid_boxes: Vec<UUIDBox> = vec![];
    let mut uuid_info_boxes: Vec<UUIDInfoSuperBox> = vec![];
    let mut current_uuid_info_box: Option<UUIDInfoSuperBox> = None;

    loop {
        let BoxHeader {
            box_length,
            box_type,
            header_length: _,
        } = match decode_box_header(reader) {
            Ok(value) => value,
            Err(derr) => {
                // TODO: Improve check for EOF
                if let Some(e) = derr.downcast_ref::<io::Error>() {
                    if e.kind() == io::ErrorKind::UnexpectedEof {
                        break;
                    }
                }
                return Err(derr);
            }
        };

        match BoxTypes::new(box_type) {
            BoxTypes::Header => {
                // The header box must be at the same level as the Signature
                // and File Type boxes it shall not be inside any other
                // superbox within the file)
                info!("HeaderSuperBox start at {:?}", reader.stream_position()?);
                let mut header_box = HeaderSuperBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                    ..Default::default()
                };
                header_box.decode(reader)?;
                header_box_option = Some(header_box);
                info!("HeaderSuperBox finish at {:?}", reader.stream_position()?);
            }
            BoxTypes::IntellectualProperty => {
                let mut intellectual_property_box = IntellectualPropertyBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                    data: vec![0; box_length as usize],
                };
                info!(
                    "IntellectualPropertyBox start at {:?}",
                    intellectual_property_box.offset
                );
                intellectual_property_box.decode(reader)?;
                info!(
                    "IntellectualPropertyBox finish at {:?}",
                    reader.stream_position()
                );
                intellectual_property_option = Some(intellectual_property_box);
            }
            BoxTypes::Xml => {
                let mut xml_box = XMLBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                    xml: Vec::with_capacity(box_length as usize).to_owned(),
                };
                info!("XMLBox start at {:?}", xml_box.offset);
                xml_box.decode(reader)?;
                xml_boxes.push(xml_box);
                info!("XMLBox finish at {:?}", reader.stream_position()?);
            }
            BoxTypes::Uuid => {
                let mut uuid_box = UUIDBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                    ..Default::default()
                };
                info!("UUIDBox start at {:?}", uuid_box.offset);
                uuid_box.decode(reader)?;
                uuid_boxes.push(uuid_box);
                info!("UUIDBox finish at {:?}", reader.stream_position()?);
            }
            BoxTypes::UUIDInfo => {
                let mut uuid_info_box = UUIDInfoSuperBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                    ..Default::default()
                };
                info!("UUIDInfoBox start at {:?}", uuid_info_box.offset);
                uuid_info_box.decode(reader)?;

                if let Some(info_box) = current_uuid_info_box {
                    uuid_info_boxes.push(info_box);
                }
                current_uuid_info_box = Some(uuid_info_box);
                info!("UUIDInfoBox finish at {:?}", reader.stream_position()?);
            }
            BoxTypes::UUIDList => {
                let mut uuid_list_box = UUIDListBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                    ..Default::default()
                };
                info!("UUIDListBox start at {:?}", uuid_list_box.offset);
                uuid_list_box.decode(reader)?;
                match &mut current_uuid_info_box {
                    Some(uuid_info_box) => {
                        uuid_info_box.uuid_list = Some(uuid_list_box);
                    }
                    None => {
                        return Err(JP2Error::BoxMissing {
                            box_type: BOX_TYPE_UUID_INFO,
                        }
                        .into());
                    }
                }
                info!("UUIDListBox finish at {:?}", reader.stream_position()?);
            }
            BoxTypes::DataEntryURL => {
                let mut data_entry_url_box = DataEntryURLBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                    version: [0; 1],
                    flags: [0; 3],
                    location: Vec::with_capacity(box_length as usize - 4).to_owned(),
                };

                data_entry_url_box.length = box_length;
                data_entry_url_box.offset = reader.stream_position()?;
                info!("DataEntryURLBox start at {:?}", data_entry_url_box.offset);
                data_entry_url_box.decode(reader)?;
                match &mut current_uuid_info_box {
                    Some(uuid_info_box) => {
                        uuid_info_box.data_entry_url_box = Some(data_entry_url_box);
                    }
                    None => {
                        return Err(JP2Error::BoxMissing {
                            box_type: BOX_TYPE_UUID_INFO,
                        }
                        .into());
                    }
                }
                info!("DataEntryURLBox finish at {:?}", reader.stream_position()?);
            }
            BoxTypes::ContiguousCodestream => {
                // The Header box shall fall before the Contiguous Codestream box
                if header_box_option.is_none() {
                    return Err(JP2Error::BoxUnexpected {
                        box_type,
                        offset: reader.stream_position()?,
                    }
                    .into());
                }

                let mut continuous_codestream_box = ContiguousCodestreamBox {
                    length: box_length,
                    offset: reader.stream_position()?,
                };
                info!(
                    "ContiguousCodestreamBox start at {:?}",
                    continuous_codestream_box.offset
                );
                continuous_codestream_box.decode(reader)?;
                info!(
                    "ContiguousCodestreamBox finish at {:?}",
                    reader.stream_position()?
                );
                contiguous_codestream_boxes.push(continuous_codestream_box);
            }

            _ => {
                panic!(
                    "Unexpected box type {:?} {:?}",
                    reader.stream_position(),
                    box_type
                );
            }
        }
    }

    if let Some(uuid_box) = current_uuid_info_box {
        uuid_info_boxes.push(uuid_box);
    }

    let result = JP2File {
        length: reader.stream_position()?,
        signature: Some(signature_box),
        file_type: Some(file_type_box),
        header: header_box_option,
        contiguous_codestreams: contiguous_codestream_boxes,
        intellectual_property: intellectual_property_option,
        xml: xml_boxes,
        uuid: uuid_boxes,
        uuid_info: uuid_info_boxes,
    };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn parse_enumerated_colourspace() {
        let input: Vec<u8> = vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10];
        let colour_specification_box = do_colour_specification_box_parse(input);
        assert_eq!(
            *colour_specification_box.method(),
            ColourSpecificationMethods::EnumeratedColourSpace {
                code: EnumeratedColourSpaces::sRGB
            }
        );
        assert_eq!(colour_specification_box.colourspace_approximation(), 0);
        assert_eq!(colour_specification_box.precedence(), 0);
    }

    #[test]
    fn parse_enumerated_colourspace_approx() {
        let input: Vec<u8> = vec![0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x12];
        let colour_specification_box = do_colour_specification_box_parse(input);
        assert_eq!(
            *colour_specification_box.method(),
            ColourSpecificationMethods::EnumeratedColourSpace {
                code: EnumeratedColourSpaces::sYCC
            }
        );
        assert_eq!(colour_specification_box.colourspace_approximation(), 1);
        assert_eq!(colour_specification_box.precedence(), 0);
    }

    #[test]
    fn parse_restricted_icc_colourspace() {
        let input: Vec<u8> = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0x04, 0xFF];
        let colour_specification_box = do_colour_specification_box_parse(input);
        assert_eq!(
            *colour_specification_box.method(),
            ColourSpecificationMethods::RestrictedICCProfile {
                profile_data: vec![0x01, 0x02, 0x04, 0xFF]
            }
        );
        assert_eq!(colour_specification_box.colourspace_approximation(), 4);
        assert_eq!(colour_specification_box.precedence(), 3);
    }

    #[test]
    fn parse_any_icc_colourspace() {
        let input: Vec<u8> = vec![0x03, 0x00, 0x02, 0x01, 0x02, 0x04, 0xFF];
        let colour_specification_box = do_colour_specification_box_parse(input);
        assert_eq!(
            *colour_specification_box.method(),
            ColourSpecificationMethods::AnyICCProfile {
                profile_data: vec![0x01, 0x02, 0x04, 0xFF]
            }
        );
        assert_eq!(colour_specification_box.colourspace_approximation(), 2);
        assert_eq!(colour_specification_box.precedence(), 0);
    }

    #[test]
    fn parse_parameterized_colourspace() {
        let input: Vec<u8> = vec![0x05, 0x01, 0x02, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x80];
        let colour_specification_box = do_colour_specification_box_parse(input);
        assert_eq!(
            *colour_specification_box.method(),
            ColourSpecificationMethods::ParameterizedColourspace {
                colour_primaries: 1,
                transfer_characteristics: 2,
                matrix_coefficients: 3,
                video_full_range: true
            }
        );
        assert_eq!(colour_specification_box.colourspace_approximation(), 2);
        assert_eq!(colour_specification_box.precedence(), 1);
    }

    fn do_colour_specification_box_parse(input: Vec<u8>) -> ColourSpecificationBox {
        let mut colour_specification_box = ColourSpecificationBox::default();
        colour_specification_box.length = input.len() as u64;
        let mut cursor = Cursor::new(input);
        let decode_result = colour_specification_box.decode(&mut cursor);
        assert!(decode_result.is_ok());
        colour_specification_box
    }

    #[test]
    fn test_colourspace_method_format_bilevel() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::BiLevel,
                }
            ),
            "Enumerated colourspace: Bi-level"
        );
    }

    #[test]
    fn test_colourspace_method_format_ycbcr1() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::YCbCr1,
                }
            ),
            "Enumerated colourspace: YCbCr(1)"
        );
    }

    #[test]
    fn test_colourspace_method_format_ycbcr2() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::YCbCr2,
                }
            ),
            "Enumerated colourspace: YCbCr(2)"
        );
    }

    #[test]
    fn test_colourspace_method_format_ycbcr3() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::YCbCr3,
                }
            ),
            "Enumerated colourspace: YCbCr(3)"
        );
    }

    #[test]
    fn test_colourspace_method_format_photo_ycc() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::PhotoYCC,
                }
            ),
            "Enumerated colourspace: PhotoYCC"
        );
    }

    #[test]
    fn test_colourspace_method_format_cmy() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::CMY,
                }
            ),
            "Enumerated colourspace: CMY"
        );
    }

    #[test]
    fn test_colourspace_method_format_cmyk() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::CMYK,
                }
            ),
            "Enumerated colourspace: CMYK"
        );
    }

    #[test]
    fn test_colourspace_method_format_ycck() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::YCCK,
                }
            ),
            "Enumerated colourspace: YCCK"
        );
    }

    #[test]
    fn test_colourspace_method_format_cielab() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::CIELab {
                        rl: 100,
                        ol: 0,
                        ra: 170,
                        oa: 256,
                        rb: 200,
                        ob: 192,
                        il: 0x00443635
                    },
                }
            ),
            "Enumerated colourspace: CIELab"
        );
    }

    #[test]
    fn test_colourspace_method_format_bilevel2() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::BiLevel2,
                }
            ),
            "Enumerated colourspace: Bi-level(2)"
        );
    }

    #[test]
    fn test_colourspace_method_format_srgb() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::sRGB,
                }
            ),
            "Enumerated colourspace: sRGB"
        );
    }

    #[test]
    fn test_colourspace_method_format_greyscale() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::Greyscale,
                }
            ),
            "Enumerated colourspace: greyscale"
        );
    }

    #[test]
    fn test_colourspace_method_format_sycc() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::sYCC,
                }
            ),
            "Enumerated colourspace: sYCC"
        );
    }

    #[test]
    fn test_colourspace_method_format_ciejab() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::CIEJab {
                        rj: 100,
                        oj: 0,
                        ra: 255,
                        oa: 192,
                        rb: 255,
                        ob: 128
                    },
                }
            ),
            "Enumerated colourspace: CIEJab"
        );
    }

    #[test]
    fn test_colourspace_method_format_esrgb() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::esRGB,
                }
            ),
            "Enumerated colourspace: e-sRGB"
        );
    }

    #[test]
    fn test_colourspace_method_format_romm_rgb() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::ROMMRGB,
                }
            ),
            "Enumerated colourspace: ROMM-RGB"
        );
    }

    #[test]
    fn test_colourspace_method_format_ybpbr_1125_60() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::YPbPr112560,
                }
            ),
            "Enumerated colourspace: YPbPr(1125/60)"
        );
    }

    #[test]
    fn test_colourspace_method_format_ybpbr_1250_50() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::YPbPr125050,
                }
            ),
            "Enumerated colourspace: YPbPr(1250/50)"
        );
    }

    #[test]
    fn test_colourspace_method_format_e_sycc() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::esYCC,
                }
            ),
            "Enumerated colourspace: e-sYCC"
        );
    }

    #[test]
    fn test_colourspace_method_format_scrgb() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::scRGB,
                }
            ),
            "Enumerated colourspace: scRGB"
        );
    }

    #[test]
    fn test_colourspace_method_format_scrgb_gray_scale() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::EnumeratedColourSpace {
                    code: EnumeratedColourSpaces::scRGBGrayScale,
                }
            ),
            "Enumerated colourspace: scRGB gray scale"
        );
    }

    #[test]
    fn test_colourspace_method_format_restricted_icc() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::RestrictedICCProfile {
                    // Not actually valid ICC data
                    profile_data: vec![0, 0, 1, 3, 3]
                }
            ),
            "Restricted ICC Profile"
        );
    }

    #[test]
    fn test_colourspace_method_format_any_icc() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::AnyICCProfile {
                    // Not actually valid ICC data
                    profile_data: vec![2, 3]
                }
            ),
            "\"Any\" ICC Profile"
        );
    }

    #[test]
    fn test_colourspace_method_format_parameterized() {
        assert_eq!(
            format!(
                "{}",
                ColourSpecificationMethods::ParameterizedColourspace {
                    colour_primaries: 1,
                    transfer_characteristics: 17,
                    matrix_coefficients: 10,
                    video_full_range: true
                }
            ),
            "Parameterized colourspace, colour primaries: 1, transfer characteristics: 17, matrix coefficients: 10, video full range: true"
        );
    }
}

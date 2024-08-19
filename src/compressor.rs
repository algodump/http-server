use anyhow::anyhow;
use flate2::read::{DeflateEncoder, GzEncoder};
use flate2::Compression;
use std::io::Read;
use std::str::FromStr;

// https://www.iana.org/assignments/http-parameters/http-parameters.xhtml#content-coding
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentEncoding {
    Aes128gcm,
    Br,
    Compress,
    Deflate,
    Exi,
    Gzip,
    Identity,
    Pack200gzip,
    Zstd,
}

static SUPPORTED_ENCODINGS: [ContentEncoding; 2] =
    [ContentEncoding::Gzip, ContentEncoding::Identity];
pub const DEFAULT_ENCODING: ContentEncoding = ContentEncoding::Identity;

impl ContentEncoding {
    pub fn is_supported(&self) -> bool {
        SUPPORTED_ENCODINGS.contains(self)
    }
}

impl ToString for ContentEncoding {
    fn to_string(&self) -> String {
        match self {
            ContentEncoding::Pack200gzip => return "pack200-gzip".to_string(),
            _ => return format!("{:?}", self).to_lowercase(),
        }
    }
}

impl FromStr for ContentEncoding {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("*") {
            return Ok(DEFAULT_ENCODING);
        } else {
            match s {
                "aes128gcm" => Ok(ContentEncoding::Aes128gcm),
                "br" => Ok(ContentEncoding::Br),
                "compress" => Ok(ContentEncoding::Compress),
                "deflate" => Ok(ContentEncoding::Deflate),
                "exi" => Ok(ContentEncoding::Exi),
                "gzip" => Ok(ContentEncoding::Gzip),
                "identity" => Ok(ContentEncoding::Identity),
                "pack200-gzip" => Ok(ContentEncoding::Pack200gzip),
                "zstd" => Ok(ContentEncoding::Zstd),
                _ => Err(anyhow!("")),
            }
        }
    }
}

pub struct Compressor {}
impl Compressor {
    pub fn compress(data: &[u8], content_encoding: ContentEncoding) -> Vec<u8> {
        fn compress_internal<T: Read>(mut compressor: T) -> Vec<u8> {
            let mut ret_vec = Vec::new();
            compressor
                .read_to_end(&mut ret_vec)
                .expect("Failed to compress");
            ret_vec
        }
        match content_encoding {
            ContentEncoding::Gzip => {
                return compress_internal(GzEncoder::new(data, Compression::fast()));
            }
            ContentEncoding::Deflate => {
                return compress_internal(DeflateEncoder::new(data, Compression::fast()));
            }
            ContentEncoding::Identity => {
                return Vec::from(data);
            }
            _ => panic!("Unsupported content encoding {:?}", content_encoding),
        }
    }
}

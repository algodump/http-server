use anyhow::anyhow;
use flate2::{
    read::{DeflateEncoder, GzEncoder},
    Compression,
};
use std::{io::Read, str::FromStr};

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
            ContentEncoding::Pack200gzip => "pack200-gzip".to_string(),
            _ => format!("{:?}", self).to_lowercase(),
        }
    }
}

impl FromStr for ContentEncoding {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("*") {
            Ok(DEFAULT_ENCODING)
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
            ContentEncoding::Gzip => compress_internal(GzEncoder::new(data, Compression::fast())),
            ContentEncoding::Deflate => {
                compress_internal(DeflateEncoder::new(data, Compression::fast()))
            }
            ContentEncoding::Identity => Vec::from(data),
            _ => panic!("Unsupported content encoding {:?}", content_encoding),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CompressionMethod {
    gzip
}

static SUPPORTED_METHODS : [&str; 1] = ["gzip"] ;

pub struct Compressor {}

impl Compressor {
    pub fn compress(data: &Vec<u8>, compression_type: CompressionMethod) -> Vec<u8> {
        match compression_type {
            CompressionMethod::gzip => {
                return Vec::from(data.clone());
            }
            _ => panic!("Compression type {:?} is not implemented", compression_type)
        }
    }
}
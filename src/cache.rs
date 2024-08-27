use std::{
    collections::HashMap,
    fs::{self, File},
    hash::{DefaultHasher, Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Error, Result};
use log::trace;

use crate::response::HttpResponse;

pub struct CacheControl {
    cache_directives: HashMap<String, String>,
}

impl FromStr for CacheControl {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cache_directives = s
            .split(',')
            .map(|directive| {
                let directive = directive.trim();
                if let Some((key, value)) = directive.split_once('=') {
                    (key.to_string(), value.to_string())
                } else {
                    (directive.to_string(), "".to_string())
                }
            })
            .collect();
        Ok(Self { cache_directives })
    }
}

impl CacheControl {
    pub fn store_allowed(&self) -> bool {
        !self.cache_directives.contains_key("no-store")
    }
}

const PATH_TO_CACHE: &str = ".cache";
pub struct Cache {}

// TODO: use serde rather than writing the raw data to cache
impl Cache {
    fn get_resource_path(resource: &str) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        resource.hash(&mut hasher);
        let resource_name = hasher.finish();
        Path::new(PATH_TO_CACHE).join(resource_name.to_string())
    }

    pub fn add(resource: &str, http_response: &HttpResponse) -> Result<()> {
        fs::create_dir_all(PATH_TO_CACHE)?;

        let resource_path = Cache::get_resource_path(resource);
        let mut file = File::create(resource_path)?;

        trace!("Adding response for {:?} to cache", resource);
        file.write_all(&http_response.as_bytes())?;
        Ok(())
    }

    pub fn retrieve(resource: &str) -> Result<Vec<u8>> {
        trace!("Reading response for {:?} from cache", resource);

        let resource_path = Cache::get_resource_path(resource);
        let file_content = fs::read(resource_path)?;
        Ok(file_content)
    }
}

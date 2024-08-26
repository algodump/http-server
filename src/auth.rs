use std::str::FromStr;

use anyhow::{anyhow, Error};
use base64::prelude::*;

#[derive(Debug)]
pub enum AuthMethod {
    Basic,
    Bearer,
}

impl ToString for AuthMethod {
    fn to_string(&self) -> String {
        return format!("{:?}", self);
    }
}

impl FromStr for AuthMethod {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Basic" => Ok(AuthMethod::Basic),
            "Bearer" => Ok(AuthMethod::Bearer),
            _ => Err(anyhow!("Unsupported authentication method")),
        }
    }
}

pub struct Authenticator {}
impl Authenticator {
    pub fn default_credentials() -> String {
        BASE64_STANDARD.encode("admin:password")
    }

    pub fn authenticate(data: &[u8], auth_type: &AuthMethod) -> bool {
        fn auth_basic(data: &[u8]) -> bool {
            Authenticator::default_credentials().as_bytes().eq(data)
        }

        match auth_type {
            AuthMethod::Basic => return auth_basic(data),
            _ => panic!("{:?} is not supported", auth_type),
        }
    }
}

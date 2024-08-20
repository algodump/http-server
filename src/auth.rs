use base64::prelude::*;

#[derive(Debug)]
pub enum AuthType {
    Basic,
}

impl ToString for AuthType {
    fn to_string(&self) -> String {
        return format!("{:?}", self);
    }
}

pub struct Authenticator {}
impl Authenticator {
    pub fn authenticate(data: &[u8], auth_type: AuthType) -> bool {
        fn auth_basic(data: &[u8]) -> bool {
            BASE64_STANDARD.encode("admin:password").as_bytes().eq(data)
        }

        match auth_type {
            AuthType::Basic => return auth_basic(data),
            _ => panic!("{:?} is not supported", auth_type),
        }
    }
}

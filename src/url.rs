#[derive(Debug, Clone)]
pub struct Url {
    resource: String,
    query: String,
}

impl Url {
    pub fn new(data: &str) -> Self {
        if let Some((resource, query)) = data.split_once('?') {
            Self {
                resource: resource.to_string(),
                query: query.to_string(),
            }
        } else {
            Self {
                resource: data.to_string(),
                query: String::from(""),
            }
        }
    }

    pub fn resource(&self) -> String {
        self.resource.clone()
    }

    pub fn query(&self) -> String {
        self.query.clone()
    }
}

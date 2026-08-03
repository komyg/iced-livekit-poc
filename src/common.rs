#[derive(Debug, Clone)]
pub struct ApiKey {
    pub api_key: String,
    pub api_secret: String,
    pub identity: String,
    pub room: String,
}

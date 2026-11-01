use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_env")]
    pub app_env: String,

    #[serde(default = "default_bind")]
    pub app_bind: String,

    #[serde(default = "default_base_url")]
    pub app_base_url: String,

    #[serde(default = "default_db_url")]
    pub database_url: String,

    #[serde(default)]
    pub master_api_key: String,

    #[serde(default)]
    pub master_api_key_previous: Option<String>,

    #[serde(default)]
    pub data_encryption_key: String,

    #[serde(default = "default_false")]
    pub require_signed_requests: bool,

    #[serde(default = "default_false")]
    pub allow_public_stateless: bool,

    /// Hanya untuk dev/test: izinkan callback `http://` dan host privat.
    /// Default produksi WAJIB `false` (ERP §9.x, RULES §9 RL-14).
    #[serde(default = "default_false")]
    pub allow_insecure_callbacks: bool,
}

fn default_env() -> String {
    "development".to_string()
}
fn default_bind() -> String {
    "127.0.0.1:8080".to_string()
}
fn default_base_url() -> String {
    "http://127.0.0.1:8080".to_string()
}
fn default_db_url() -> String {
    "sqlite://./data/qris.db".to_string()
}
fn default_false() -> bool {
    false
}

impl AppConfig {
    pub fn load_from_env() -> Result<Self, String> {
        let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| default_env());
        let app_bind = std::env::var("APP_BIND").unwrap_or_else(|_| default_bind());
        let app_base_url = std::env::var("APP_BASE_URL").unwrap_or_else(|_| default_base_url());
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| default_db_url());

        let master_api_key = std::env::var("MASTER_API_KEY").unwrap_or_default();
        let master_api_key_previous = std::env::var("MASTER_API_KEY_PREVIOUS").ok();
        let data_encryption_key = std::env::var("DATA_ENCRYPTION_KEY").unwrap_or_default();

        let require_signed_requests = std::env::var("REQUIRE_SIGNED_REQUESTS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let allow_public_stateless = std::env::var("ALLOW_PUBLIC_STATELESS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let allow_insecure_callbacks = std::env::var("ALLOW_INSECURE_CALLBACKS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        Ok(Self {
            app_env,
            app_bind,
            app_base_url,
            database_url,
            master_api_key,
            master_api_key_previous,
            data_encryption_key,
            require_signed_requests,
            allow_public_stateless,
            allow_insecure_callbacks,
        })
    }
}

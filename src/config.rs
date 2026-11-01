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

    /// Rate limit per kredensial/IP: request per menit (RL-01/RL-02).
    #[serde(default = "default_rate_per_minute")]
    pub rate_limit_per_minute: u32,

    /// Burst capacity token bucket (RL-01).
    #[serde(default = "default_rate_burst")]
    pub rate_limit_burst: u32,

    /// Rate limit khusus decode gambar (RL-03).
    #[serde(default = "default_rate_image")]
    pub rate_limit_image_per_minute: u32,

    /// Origin CORS yang diizinkan; kosong = deny all (RL-12).
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,

    /// Percaya header `X-Forwarded-For` dari reverse proxy (RL-01).
    /// Default `false`: IP diambil dari koneksi TCP (`ConnectInfo`) agar
    /// klien tidak bisa memalsukan header untuk melewati rate limit.
    #[serde(default = "default_false")]
    pub trust_proxy_headers: bool,
}

fn default_rate_per_minute() -> u32 {
    120
}
fn default_rate_burst() -> u32 {
    60
}
fn default_rate_image() -> u32 {
    20
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

        let rate_limit_per_minute = std::env::var("RATE_LIMIT_PER_MINUTE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(default_rate_per_minute);

        let rate_limit_burst = std::env::var("RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(default_rate_burst);

        let rate_limit_image_per_minute = std::env::var("RATE_LIMIT_IMAGE_PER_MINUTE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(default_rate_image);

        let cors_allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();

        let trust_proxy_headers = std::env::var("TRUST_PROXY")
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
            rate_limit_per_minute,
            rate_limit_burst,
            rate_limit_image_per_minute,
            cors_allowed_origins,
            trust_proxy_headers,
        })
    }
}

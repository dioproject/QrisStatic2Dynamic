use crate::config::AppConfig;
use crate::db::DbPool;
use crate::services::rate_limit::RateLimiter;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: DbPool,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub fn new(config: AppConfig, db: DbPool) -> Self {
        let rate_limiter = Arc::new(RateLimiter::new(
            config.rate_limit_per_minute,
            config.rate_limit_burst,
            config.rate_limit_image_per_minute,
        ));

        Self {
            config: Arc::new(config),
            db,
            rate_limiter,
        }
    }
}

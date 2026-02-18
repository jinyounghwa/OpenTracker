pub mod chrome;
pub mod window;

use crate::analyzer::categorizer::CategoryRules;
use crate::config::Config;
use crate::db::Database;
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{Duration, MissedTickBehavior, interval};
use tracing::{error, info};

pub async fn run_activity_collector(config: Arc<Config>, rules: Arc<CategoryRules>) -> Result<()> {
    let mut ticker = interval(Duration::from_secs(config.polling_seconds));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    info!(
        polling_seconds = config.polling_seconds,
        "activity collector started"
    );

    loop {
        ticker.tick().await;

        let sample = window::collect_active_window();
        let category = rules.categorize_app(&sample.app_name);

        let inserted = Database::open(&config.db_path)
            .and_then(|database| {
                database.insert_activity(
                    sample.recorded_at,
                    &sample.app_name,
                    sample.window_title.as_deref(),
                    &category,
                    config.polling_seconds as i64,
                )?;
                database.cleanup_old_activities(config.retention_days)?;
                Ok(())
            })
            .map_err(|error| {
                error!(error = %error, "failed to store activity sample");
                error
            });

        if inserted.is_ok() {
            info!(app = sample.app_name, category, "activity sample captured");
        }
    }
}

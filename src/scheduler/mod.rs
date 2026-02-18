use crate::config::parse_hhmm;
use anyhow::{Context, Result, bail};
use chrono::{
    Duration as ChronoDuration, Local, LocalResult, NaiveDate, NaiveTime, TimeZone, Timelike,
};
use std::future::Future;
use tokio::time::{Duration, sleep};
use tracing::{error, info};

const RESCHEDULE_POLL_SECONDS: u64 = 30;

pub fn cron_from_report_time(report_time: &str) -> Result<String> {
    let time = parse_hhmm(report_time)?;
    Ok(format!("{} {} * * *", time.minute(), time.hour()))
}

pub async fn run_cron_scheduler<S, F, Fut>(mut schedule_provider: S, mut task: F) -> Result<()>
where
    S: FnMut() -> Result<String>,
    F: FnMut(NaiveDate) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut last_logged_cron = String::new();

    loop {
        let cron_expr = match schedule_provider() {
            Ok(value) => value,
            Err(error) => {
                error!(error = %error, "failed to load report schedule");
                sleep(Duration::from_secs(RESCHEDULE_POLL_SECONDS)).await;
                continue;
            }
        };

        let delay = match seconds_until_next_run(&cron_expr) {
            Ok(value) => value,
            Err(error) => {
                error!(error = %error, cron = %cron_expr, "invalid report cron expression");
                sleep(Duration::from_secs(RESCHEDULE_POLL_SECONDS)).await;
                continue;
            }
        };

        if cron_expr != last_logged_cron {
            info!(seconds = delay.as_secs(), cron = %cron_expr, "next report schedule set");
            last_logged_cron = cron_expr.clone();
        }

        if delay > Duration::from_secs(RESCHEDULE_POLL_SECONDS) {
            sleep(Duration::from_secs(RESCHEDULE_POLL_SECONDS)).await;
            continue;
        }

        sleep(delay).await;

        let date = Local::now().date_naive();
        let result = task(date).await;

        if let Err(error) = result {
            error!(error = %error, date = %date, "scheduled report generation failed");
        }

        sleep(Duration::from_secs(1)).await;
    }
}

fn seconds_until_next_run(cron_expr: &str) -> Result<Duration> {
    let target_time = parse_daily_cron_time(cron_expr)?;
    let now = Local::now();
    let today = now.date_naive();

    let candidate_today = match Local.from_local_datetime(&today.and_time(target_time)) {
        LocalResult::Single(datetime) => datetime,
        _ => {
            let fallback_day = today + ChronoDuration::days(1);
            Local
                .from_local_datetime(&fallback_day.and_time(target_time))
                .single()
                .context("Failed to convert schedule time")?
        }
    };

    let next_run = if candidate_today > now {
        candidate_today
    } else {
        let tomorrow = today + ChronoDuration::days(1);
        Local
            .from_local_datetime(&tomorrow.and_time(target_time))
            .single()
            .context("Failed to convert next execution time")?
    };

    (next_run - now)
        .to_std()
        .context("Failed to compute next execution delay")
}

fn parse_daily_cron_time(cron_expr: &str) -> Result<NaiveTime> {
    let fields = cron_expr.split_whitespace().collect::<Vec<_>>();

    if fields.len() != 5 {
        bail!("Invalid cron expression: {cron_expr}. Expected format: '<minute> <hour> * * *'");
    }

    if fields[2] != "*" || fields[3] != "*" || fields[4] != "*" {
        bail!(
            "Unsupported cron expression: {cron_expr}. Only daily format '<minute> <hour> * * *' is supported"
        );
    }

    let minute = fields[0]
        .parse::<u32>()
        .with_context(|| format!("Invalid cron minute: {}", fields[0]))?;
    let hour = fields[1]
        .parse::<u32>()
        .with_context(|| format!("Invalid cron hour: {}", fields[1]))?;

    NaiveTime::from_hms_opt(hour, minute, 0)
        .with_context(|| format!("Invalid cron time values: hour={hour}, minute={minute}"))
}

#[cfg(test)]
mod tests {
    use super::{cron_from_report_time, seconds_until_next_run};

    #[test]
    fn cron_conversion_from_report_time() {
        let expr = cron_from_report_time("23:30").expect("cron expression");
        assert_eq!(expr, "30 23 * * *");
    }

    #[test]
    fn schedule_delay_is_positive() {
        let delay = seconds_until_next_run("30 23 * * *").expect("delay computed");
        assert!(delay.as_secs() > 0);
    }

    #[test]
    fn rejects_non_daily_cron_expression() {
        assert!(seconds_until_next_run("*/5 * * * *").is_err());
    }
}

use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::commands::prices::refresh_all;
use crate::settings;
use crate::state::AppState;

/// Wait this long after launch before the first check, so starting the app
/// never blocks on the network or hammers an API during rapid restarts.
const STARTUP_GRACE: Duration = Duration::from_secs(90);

/// Periodically re-check wishlist prices and announce anything that drops
/// below its target.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_GRACE).await;

        loop {
            let state = app.state::<AppState>();

            let (interval_hours, enabled) = state
                .db
                .with(|conn| {
                    let s = settings::read(conn)?;
                    Ok((s.refresh_interval_hours, s.notifications_enabled))
                })
                .unwrap_or((6, true));

            // Cheap and self-limiting: one request covers 200 games and each
            // is stamped either way, so this does nothing after the first pass.
            match crate::services::metadata::backfill_time_to_beat(&state).await {
                Ok(0) => {}
                Ok(found) => log::info!("playtimes filled in for {found} games"),
                Err(e) => log::warn!("could not fetch playtimes: {e}"),
            }

            match refresh_all(&state).await {
                Ok(report) => {
                    if report.checked > 0 {
                        log::info!(
                            "price refresh: {} checked, {} failed, {} alerts",
                            report.checked,
                            report.failed,
                            report.alerts
                        );
                    }
                }
                Err(e) => log::warn!("scheduled price refresh failed: {e}"),
            }

            let alerts: Vec<_> = std::mem::take(
                &mut *state
                    .pending_alerts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
            );

            if enabled {
                for alert in alerts {
                    let body = format!(
                        "{} at {} — you wanted it under {:.2}",
                        format_money(alert.price, &alert.currency),
                        alert.shop,
                        alert.target
                    );
                    if let Err(e) = app
                        .notification()
                        .builder()
                        .title(format!("{} dropped", alert.name))
                        .body(body)
                        .show()
                    {
                        log::warn!("could not show a price notification: {e}");
                    }
                }
            }

            // Re-read the interval each pass so a settings change takes effect
            // on the next cycle rather than needing a restart.
            tokio::time::sleep(Duration::from_secs(u64::from(interval_hours.max(1)) * 3600)).await;
        }
    });
}

fn format_money(amount: f64, currency: &str) -> String {
    match currency {
        "EUR" => format!("{amount:.2} €"),
        "USD" => format!("${amount:.2}"),
        "GBP" => format!("£{amount:.2}"),
        other => format!("{amount:.2} {other}"),
    }
}

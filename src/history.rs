use super::*;
use axum::extract::Query;
use rusqlite::{Connection, params};
use std::collections::HashSet;

pub fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("data")?;
    let db = Connection::open("data/monitor.sqlite3")?;
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS samples(time INTEGER PRIMARY KEY, rx REAL, tx REAL, download REAL, upload REAL, cpu REAL, memory REAL, optical_rx REAL, optical_tx REAL, online REAL);")?;
    db.execute("DELETE FROM samples WHERE time < unixepoch()-7776000", [])?;
    Ok(())
}
fn number(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.trim().trim_end_matches('%').parse().ok())
        .filter(|n| n.is_finite())
}
pub fn save(value: &serde_json::Value) -> Result<(), rusqlite::Error> {
    let mut db = Connection::open("data/monitor.sqlite3")?;
    db.busy_timeout(Duration::from_secs(3))?;
    let transaction = db.transaction()?;
    let time = value["sampled_at"].as_f64().unwrap_or_default() as i64;
    let rx = number(&value["optical"]["counters"][0]["bytes_received"]);
    let tx = number(&value["optical"]["counters"][0]["bytes_sent"]);
    let previous = transaction
        .query_row(
            "SELECT time,rx,tx FROM samples ORDER BY time DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<f64>>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                ))
            },
        )
        .ok();
    let rate = |current: Option<f64>, previous_value: Option<f64>, previous_time: i64| {
        let delta = time - previous_time;
        current
            .zip(previous_value)
            .filter(|(a, b)| delta > 0 && delta <= 60 && a >= b)
            .map(|(a, b)| (a - b) * 8.0 / delta as f64 / 1e6)
    };
    let download = previous.and_then(|(t, r, _)| rate(rx, r, t));
    let upload = previous.and_then(|(t, _, s)| rate(tx, s, t));
    let online = value["clients"].as_array().map(|clients| {
        clients
            .iter()
            .filter(|c| c["status"] == "Online")
            .filter_map(|c| c["mac"].as_str())
            .map(str::to_lowercase)
            .collect::<HashSet<_>>()
            .len() as f64
    });
    transaction.execute(
        "INSERT OR IGNORE INTO samples VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            time,
            rx,
            tx,
            download,
            upload,
            number(&value["device"]["cpu"]),
            number(&value["device"]["memory"]),
            number(&value["optical"]["info"][0]["rx_dbm"]),
            number(&value["optical"]["info"][0]["tx_dbm"]),
            online
        ],
    )?;
    transaction.execute("DELETE FROM samples WHERE time < ?1", [time - 7776000])?;
    transaction.commit()
}
#[derive(Deserialize)]
pub struct Range {
    days: Option<u32>,
}
pub async fn read(
    headers: HeaderMap,
    Query(range): Query<Range>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !local_request(&headers) {
        return Err(error("Host not allowed."));
    }
    let days = range.days.unwrap_or(1);
    if ![1, 7, 30, 90].contains(&days) {
        return Err(error("Unsupported history range."));
    }
    tokio::task::spawn_blocking(move||{
        let db=Connection::open("data/monitor.sqlite3").map_err(|_|error("History unavailable."))?;
        // Maximum ~720 buckets. NULL metrics remain missing rather than zero.
        let bucket=(days as i64*86400/720).max(15);
        let mut query=db.prepare("SELECT MAX(time),AVG(download),AVG(upload),AVG(cpu),AVG(memory),AVG(optical_rx),AVG(optical_tx),AVG(online) FROM samples WHERE time>=unixepoch()-?1 GROUP BY time/?2 ORDER BY MAX(time)").map_err(|_|error("History query failed."))?;
        let rows=query.query_map(params![days as i64*86400,bucket],|r|Ok(serde_json::json!({"time":r.get::<_,i64>(0)?,"download":r.get::<_,Option<f64>>(1)?,"upload":r.get::<_,Option<f64>>(2)?,"cpu":r.get::<_,Option<f64>>(3)?,"memory":r.get::<_,Option<f64>>(4)?,"opticalRx":r.get::<_,Option<f64>>(5)?,"opticalTx":r.get::<_,Option<f64>>(6)?,"online":r.get::<_,Option<f64>>(7)?}))).map_err(|_|error("History query failed."))?;
        let samples=rows.collect::<Result<Vec<_>,_>>().map_err(|_|error("History decoding failed."))?;
        Ok(Json(serde_json::json!({"samples":samples,"bucket_seconds":bucket,"retention_days":90})))
    }).await.map_err(|_|error("History worker failed."))?
}

use super::*;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

pub fn initialize() -> rusqlite::Result<()> {
    Connection::open("data/monitor.sqlite3")?.execute_batch(
        "CREATE TABLE IF NOT EXISTS device_labels(mac TEXT PRIMARY KEY, label TEXT NOT NULL);",
    )
}
#[derive(Deserialize)]
pub struct Update {
    labels: BTreeMap<String, String>,
    #[serde(default)]
    import: bool,
}
fn normalize(mac: &str) -> Result<String, ApiError> {
    let mac = mac.to_ascii_lowercase();
    if mac.len() != 17
        || !mac
            .split(':')
            .all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(error("Invalid device MAC address."));
    }
    Ok(mac)
}
fn all(db: &Connection) -> rusqlite::Result<BTreeMap<String, String>> {
    db.prepare("SELECT mac,label FROM device_labels")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}
pub async fn read(headers: HeaderMap) -> Result<Json<BTreeMap<String, String>>, ApiError> {
    if !local_request(&headers) {
        return Err(error("Origin not allowed."));
    }
    tokio::task::spawn_blocking(|| {
        let db = Connection::open("data/monitor.sqlite3")?;
        db.busy_timeout(Duration::from_secs(3))?;
        all(&db)
    })
    .await
    .map_err(|_| error("Label worker failed."))?
    .map(Json)
    .map_err(|_| error("Unable to load device labels."))
}
pub async fn save(
    headers: HeaderMap,
    Json(input): Json<Update>,
) -> Result<Json<BTreeMap<String, String>>, ApiError> {
    if !local_request(&headers) {
        return Err(error("Origin not allowed."));
    }
    if input.labels.len() > 1000 {
        return Err(error("Too many labels."));
    }
    let mut labels = BTreeMap::new();
    for (mac, label) in input.labels {
        let label = label.trim().to_owned();
        if label.chars().count() > 64 {
            return Err(error("Labels must be at most 64 characters."));
        }
        labels.insert(normalize(&mac)?, label);
    }
    tokio::task::spawn_blocking(move || -> rusqlite::Result<_> {
        let mut db = Connection::open("data/monitor.sqlite3")?;
        db.busy_timeout(Duration::from_secs(3))?;
        let tx = db.transaction()?;
        for (mac, label) in labels {
            if input.import {
                if !label.is_empty() { tx.execute("INSERT OR IGNORE INTO device_labels VALUES (?1,?2)", params![mac,label])?; }
            } else if label.is_empty() {
                tx.execute("DELETE FROM device_labels WHERE mac=?1", [mac])?;
            } else {
                tx.execute("INSERT INTO device_labels VALUES (?1,?2) ON CONFLICT(mac) DO UPDATE SET label=excluded.label", params![mac,label])?;
            }
        }
        tx.commit()?;
        all(&db)
    }).await.map_err(|_| error("Label worker failed."))?
        .map(Json).map_err(|_| error("Unable to save device labels."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_mac() {
        assert_eq!(normalize("AA:BB:CC:DD:EE:FF").unwrap(), "aa:bb:cc:dd:ee:ff");
        assert!(normalize("bad").is_err());
    }
}

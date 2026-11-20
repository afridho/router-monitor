use regex::Regex;
use serde_json::{Map, Value, json};

pub fn records(body: &str, constructor: &str, fields: &[&str]) -> Vec<Value> {
    let pattern = Regex::new(&format!(
        r"new\s+{}\s*\(([^\r\n]*?)\)",
        regex::escape(constructor)
    ))
    .unwrap();
    let strings = Regex::new(r#""((?:\\.|[^"\\])*)"|'((?:\\.|[^'\\])*)'"#).unwrap();
    pattern
        .captures_iter(body)
        .filter_map(|c| {
            let values: Vec<String> = strings
                .captures_iter(&c[1])
                .map(|v| {
                    super::decode(v.get(1).or_else(|| v.get(2)).unwrap().as_str())
                        .trim()
                        .to_owned()
                })
                .collect();
            if values.len() != fields.len() {
                return None;
            }
            Some(Value::Object(
                fields
                    .iter()
                    .zip(values)
                    .map(|(k, v)| (k.to_string(), Value::String(v)))
                    .collect::<Map<_, _>>(),
            ))
        })
        .collect()
}
fn scalar(body: &str, name: &str) -> Value {
    Regex::new(&format!(
        r#"var\s+{}\s*=\s*['"]([^'"]*)['"]"#,
        regex::escape(name)
    ))
    .unwrap()
    .captures(body)
    .map(|v| json!(super::decode(&v[1])))
    .unwrap_or(Value::Null)
}
pub async fn collect(client: &reqwest::Client) -> Value {
    let sources = [
        ("device", "/html/ssmp/deviceinfo/deviceinfo.asp"),
        ("optical", "/html/amp/opticinfo/opticinfo.asp"),
        ("clients", "/html/bbsp/userdevinfo/getuserdevinfo.asp"),
        ("wifi", "/html/amp/wlaninfo/wlaninfo.asp"),
        ("ethernet", "/html/amp/ethinfo/ethinfo.asp"),
    ];
    let mut output = Map::new();
    let mut errors = Vec::new();
    for (name, path) in sources {
        let body = match client
            .get(format!("{}{path}", super::router_url()))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
            _ => {
                errors.push(format!(
                    "Unable to read {name}. Refresh or reconnect to the router."
                ));
                continue;
            }
        };
        let value = match name {
            "device" => {
                json!({"info":records(&body,"stDeviceInfo", &["path","serial","hardware","firmware","model","vendor","release","mac","description","manufacture","alias"]),"ont":records(&body,"ONTInfo", &["path","id","status"]),"cpu":scalar(&body,"cpuUsed"),"memory":scalar(&body,"memUsed"),"nat_sessions":scalar(&body,"NatSessionInfo")})
            }
            "optical" => {
                json!({"info":records(&body,"stOpticInfo", &["path","link","tx_dbm","rx_dbm","voltage_mv","temperature_c","bias_ma","rf_rx","rf_output","vendor","serial","date","tx_nm","rx_nm","distance_km","los"]),"counters":records(&body,"stStats", &["path","errors_sent","errors_received","discards_received","bytes_sent","bytes_received","packets_sent","packets_received"])})
            }
            "clients" => json!(records(
                &body,
                "stUserDevInfoPTVDF",
                &[
                    "hostname",
                    "type",
                    "ip",
                    "mac",
                    "status",
                    "port",
                    "time",
                    "lease",
                    "path",
                    "real_mac",
                    "receive_rate",
                    "send_rate"
                ]
            )),
            "wifi" => json!(records(
                &body,
                "stWlan",
                &[
                    "path",
                    "enabled",
                    "interface",
                    "ssid",
                    "beacon",
                    "basic_encrypt",
                    "basic_auth",
                    "wpa_encrypt",
                    "wpa_auth",
                    "wpa2_encrypt",
                    "wpa2_auth",
                    "mixed_encrypt",
                    "mixed_auth",
                    "channel",
                    "radio",
                    "standard",
                    "country",
                    "width_code",
                    "tx_power",
                    "service",
                    "hidden",
                    "wmm",
                    "client_limit",
                    "auto_channel"
                ]
            )),
            "ethernet" => {
                json!({"ports":records(&body,"GEInfo", &["path","duplex_code","speed_code","status"]),"counters":records(&body,"LANStats", &["path","tx_packets","tx_packets_high","tx_bytes","tx_bytes_high","tx_bad_bytes","tx_bad_bytes_high","rx_packets","rx_packets_high","rx_bytes","rx_bytes_high","rx_bad_bytes","rx_bad_bytes_high"])})
            }
            _ => Value::Null,
        };
        output.insert(name.into(), value);
    }
    // The inventory's port field can say LAN1 for a wireless client. Use the
    // actual association table and match MAC addresses instead.
    match client
        .post(format!(
            "{}/html/amp/wlaninfo/getassociateddeviceinfo.asp",
            super::router_url()
        ))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            let body = response.text().await.unwrap_or_default();
            let stations = records(
                &body,
                "stAssociatedDevice",
                &[
                    "path",
                    "mac",
                    "uptime",
                    "rx_rate",
                    "tx_rate",
                    "rssi",
                    "noise",
                    "snr",
                    "quality",
                    "mode",
                    "wmm",
                    "power_save",
                    "high_band",
                    "ip",
                    "description",
                    "antennas",
                    "supports_11k",
                    "supports_11v",
                    "dual_band",
                    "beamforming",
                    "supports_11r",
                ],
            );
            let wifi = output
                .get("wifi")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(clients) = output.get_mut("clients").and_then(Value::as_array_mut) {
                enrich_associations(clients, &stations, &wifi);
            }
        }
        _ => errors.push(
            "Wi-Fi association data unavailable; some client bands cannot be identified.".into(),
        ),
    }
    output.insert("errors".into(), json!(errors));
    output.insert(
        "sampled_at".into(),
        json!(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        ),
    );
    Value::Object(output)
}

fn enrich_associations(clients: &mut [Value], stations: &[Value], wifi: &[Value]) {
    for client in clients {
        let mac = client["mac"].as_str().unwrap_or_default();
        if mac.is_empty() {
            continue;
        }
        let Some(station) = stations.iter().find(|s| {
            s["mac"]
                .as_str()
                .is_some_and(|m| m.eq_ignore_ascii_case(mac))
        }) else {
            continue;
        };
        let path = station["path"].as_str().unwrap_or_default();
        let Some((network_path, _)) = path.split_once(".AssociatedDevice.") else {
            continue;
        };
        let Some(network) = wifi
            .iter()
            .find(|w| w["path"].as_str() == Some(network_path))
        else {
            continue;
        };
        let band = match network["radio"].as_str().and_then(|r| r.rsplit('.').next()) {
            Some("1") => "2.4 GHz",
            Some("2") => "5 GHz",
            _ => continue,
        };
        if let Some(object) = client.as_object_mut() {
            object.insert("wifi_band".into(), json!(band));
            object.insert("wifi_ssid".into(), network["ssid"].clone());
            object.insert("wifi_rssi".into(), station["rssi"].clone());
            object.insert("wifi_rx_rate".into(), station["rx_rate"].clone());
            object.insert("wifi_tx_rate".into(), station["tx_rate"].clone());
            object.insert("wifi_source".into(), json!("association_table"));
        }
    }
}

#[cfg(test)]
mod association_tests {
    use super::*;
    #[test]
    fn association_overrides_misleading_lan_inventory() {
        let mut clients = vec![
            json!({"mac":"aa:bb:cc:dd:ee:ff","port":"LAN1"}),
            json!({"mac":"00:11:22:33:44:55","port":"LAN1"}),
        ];
        let stations = vec![
            json!({"mac":"AA:BB:CC:DD:EE:FF","path":"InternetGatewayDevice.LANDevice.1.WLANConfiguration.5.AssociatedDevice.1","rssi":"-45"}),
        ];
        let wifi = vec![
            json!({"path":"InternetGatewayDevice.LANDevice.1.WLANConfiguration.5","radio":"InternetGatewayDevice.LANDevice.1.WiFi.Radio.2","ssid":"Test"}),
        ];
        enrich_associations(&mut clients, &stations, &wifi);
        assert_eq!(clients[0]["wifi_band"], "5 GHz");
        assert_eq!(clients[0]["wifi_rssi"], "-45");
        assert!(clients[1].get("wifi_band").is_none());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructor_is_bounded() {
        let r = records(
            r#"new X("a", "192\x2e168"),new X("b","c")"#,
            "X",
            &["name", "ip"],
        );
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["ip"], "192.168");
    }
    #[test]
    fn unknown_schema_is_not_guessed() {
        assert!(records("new X(\"a\")", "X", &["a", "b"]).is_empty());
    }
}

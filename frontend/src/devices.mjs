// One inventory record per address is not one physical device.
export function groupDevices(records) {
  const groups = new Map();
  for (const [index, record] of records.entries()) {
    const mac = String(record.mac || '').trim().toLowerCase();
    const key = /^(?:[0-9a-f]{2}:){5}[0-9a-f]{2}$/.test(mac) ? mac : `record:${index}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(record);
  }
  return [...groups.values()].map(group => {
    // Prefer a live IPv4 record for device-level counters and connected time.
    const ranked = [...group].sort((a, b) => score(b) - score(a));
    const best = ranked[0];
    const addresses = [...new Set(ranked.map(r => r.ip).filter(Boolean))];
    const station = ranked.find(r => r.status === 'Online' && r.wifi_source === 'association_table');
    const device = {...best, addresses, ip: best.ip || addresses[0] || '', searchAddresses: addresses.join(' ')};
    if (station) {
      for (const field of ['wifi_source','wifi_band','wifi_ssid','wifi_rssi','wifi_rx_rate','wifi_tx_rate']) {
        device[field] = station[field];
      }
    }
    return device;
  });
}

function score(record) {
  return (record.status === 'Online' ? 4 : 0) + (String(record.ip || '').includes(':') ? 0 : 2);
}

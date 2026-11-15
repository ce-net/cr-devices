//! Major 1 (frozen contract `cr_devices_api::versions::v1`): the first edition's envelope, from the same rows.
use crate::CELL;
use cr_devices_api::versions::v1 as old;
use cr_devices_api::DeviceRow;
use ui_chain as cr_view;

pub fn kind(k: &cr_devices_api::DeviceKind) -> old::DeviceKind {
    use cr_devices_api::DeviceKind as K;
    match k {
        K::Server => old::DeviceKind::Server,
        K::Desktop => old::DeviceKind::Desktop,
        K::Laptop => old::DeviceKind::Laptop,
        K::Phone | K::Tablet => old::DeviceKind::Phone,
        K::Unknown => old::DeviceKind::Unknown,
    }
}

/// A v2 row as v1 had it (tailnet address back in `addrs`, no Part fields).
pub fn to_v1(r: &DeviceRow) -> old::DeviceRow {
    let mut addrs = r.addrs.clone();
    addrs.extend(r.tailscale.iter().map(|t| t.ip.clone()));
    old::DeviceRow {
        id: r.id.clone(), name: r.name.clone(), kind: kind(&r.kind), addrs, online: r.online, last_seen_ms: r.last_seen_ms,
        checked_ms: r.checked_ms, rtt_ms: r.rtt_ms, cells: r.cells, why: r.why.clone(),
        inventory: r.inventory.as_ref().map(|i| old::Inventory { live: i.live, age_s: i.age_s, summary: i.summary.clone() }),
        refs: r.refs.clone(),
    }
}

/// The rows this caller may see, offline first, then by name; and whether the chain removed any.
pub fn view(c: &ce_wire::Caller, ask: &old::DevicesAsk, all: &[old::DeviceRow]) -> (Vec<old::DeviceRow>, bool, Vec<String>) {
    let nodes = cr_view::scopes(c, CELL, "node");
    let q = ask.q.to_lowercase();
    let mut narrowed = false;
    let mut rows: Vec<old::DeviceRow> = Vec::new();
    for r in all {
        if !cr_view::in_scope(&nodes, &r.name) {
            narrowed = true;
            continue;
        }
        if (q.is_empty() || r.name.to_lowercase().contains(&q) || r.id.starts_with(&q)) && (!ask.only_offline || !r.online) {
            rows.push(r.clone());
        }
    }
    rows.sort_by(|a, b| a.online.cmp(&b.online).then(a.name.cmp(&b.name)));
    let can_ask = if nodes.is_empty() { vec![] } else { vec!["cr.devices:read (every node)".to_string()] };
    (rows, narrowed, can_ask)
}

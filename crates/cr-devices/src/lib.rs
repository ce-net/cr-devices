//! The one function of `cr.devices`: every device as a row, online or not, filtered by the caller's chain.
//!
//! Which devices exist: this node, the mesh peers file (`~/.config/ce-wire/peers`, which ce-door rewrites from the
//! mesh every 60 s) and `~/.config/control-room/devices` (same line format, for devices that are not mesh nodes),
//! grouped by node id. Whether one is online: its door answered a typed Hello for `cell.install` within
//! [`PROBE_TIMEOUT_MS`] (an answer that refuses still proves the door is up). Every device is asked every [`EVERY_S`]
//! seconds, so one that goes down shows as offline within EVERY_S + PROBE_TIMEOUT, and `Watch` pushes that row.
#![forbid(unsafe_code)]

pub mod v1;

use cr_devices_api::*;
use std::collections::BTreeMap;
use ui_chain::{layer_view, Caveats};
use ui_part::Parts;

pub const CELL: &str = "cr.devices";
pub const EVERY_S: u64 = 15;
pub const PROBE_TIMEOUT_MS: u64 = 4000;
pub const STREAM_MAX_S: u64 = 30;

pub use cr_sources::nodes::{parse_peers, wallet_names, Node};

pub fn kind_of(name: &str) -> DeviceKind {
    let n = name.to_lowercase();
    if n.contains("phone") || n.contains("iphone") {
        DeviceKind::Phone
    } else if n.contains("ipad") || n.contains("tablet") {
        DeviceKind::Tablet
    } else if n.contains("mac") || n.contains("book") || n.contains("laptop") {
        DeviceKind::Laptop
    } else if n.contains("desk") {
        DeviceKind::Desktop
    } else if n.contains("ops") || n.contains("debian") || n.contains("server") || n.contains("hetzner") {
        DeviceKind::Server
    } else {
        DeviceKind::Unknown
    }
}

/// A tailnet address (100.64.0.0/10).
pub fn is_tailnet(ip: &str) -> bool {
    let o: Vec<u8> = ip.split('.').filter_map(|x| x.parse().ok()).collect();
    o.len() == 4 && o[0] == 100 && (64..128).contains(&o[1])
}

/// What one probe of one device found.
#[derive(Debug, Clone, PartialEq)]
pub enum Probe {
    /// the door answered; `cells` when its cell.install list could be read
    Up { rtt_ms: u32, cells: Option<u32> },
    Down { why: String },
}

/// Fold a probe into the device's row (keeping its last-seen time when it is down now). The row is whole: every
/// Part filled; [`for_caller`] withholds per viewer.
pub fn apply(prev: Option<&DeviceRow>, n: &Node, p: &Probe, now_ms: u64) -> DeviceRow {
    let mut r = prev.cloned().unwrap_or_else(|| DeviceRow {
        id: n.id.clone(),
        name: n.name.clone(),
        kind: kind_of(&n.name),
        addrs: vec![],
        online: false,
        last_seen_ms: 0,
        checked_ms: 0,
        rtt_ms: None,
        cells: None,
        why: String::new(),
        inventory: None,
        tailscale: None,
        passkeys: Some(vec![]),
        refs: vec![format!("node:{}", n.name)],
    });
    r.name = n.name.clone();
    // tailnet addresses are the `tailscale` Part, never in the plain address list
    r.addrs = n.addrs.iter().filter(|a| !is_tailnet(a)).cloned().collect();
    r.checked_ms = now_ms;
    match p {
        Probe::Up { rtt_ms, cells } => {
            r.online = true;
            r.last_seen_ms = now_ms;
            r.rtt_ms = Some(*rtt_ms);
            if cells.is_some() {
                r.cells = *cells;
            }
            r.why.clear();
        }
        Probe::Down { why } => {
            r.online = false;
            r.rtt_ms = None;
            r.why = why.clone();
        }
    }
    let online = r.online;
    r.tailscale = n.addrs.iter().find(|a| is_tailnet(a)).map(|ip| Tailnet { ip: ip.clone(), online, key_expires_at: None });
    r
}

fn caveat(r: &DeviceRow, k: &str) -> Option<String> {
    match k {
        "node" => Some(r.name.clone()),
        "id" => Some(r.id.clone()),
        _ => None,
    }
}

/// What one caller gets: the rows their caveats and query let through, Part fields withheld per their chain;
/// offline first, then by name.
pub fn for_caller(c: &ce_wire::Caller, q: &DevicesQuery, rows: &BTreeMap<String, DeviceRow>) -> Vec<DeviceRow> {
    let cv = Caveats::of(c, CELL);
    let text = q.q.as_deref().unwrap_or("").to_lowercase();
    let mut out: Vec<DeviceRow> = rows
        .values()
        .filter(|r| layer_view::visible_by(&cv, |k| caveat(r, k)))
        .filter(|r| (text.is_empty() || r.name.to_lowercase().contains(&text) || r.id.starts_with(&text)) && (q.only_offline != Some(true) || !r.online))
        .map(|r| {
            let mut r = r.clone();
            r.withhold(&|right| ui_chain::holds(c, right));
            r
        })
        .collect();
    out.sort_by(|a, b| a.online.cmp(&b.online).then(a.name.cmp(&b.name)));
    out
}

pub fn delta(before: &[DeviceRow], now: &[DeviceRow], hlc: u64) -> Option<DeviceDelta> {
    let old: BTreeMap<&str, &DeviceRow> = before.iter().map(|r| (r.id.as_str(), r)).collect();
    let new: BTreeMap<&str, &DeviceRow> = now.iter().map(|r| (r.id.as_str(), r)).collect();
    // checked_ms, rtt and an online row's last_seen change every round; a row is sent again only when what it shows
    // changed (online, why, cells, inventory, ..)
    let same = |a: &DeviceRow, b: &DeviceRow| {
        let mut a = a.clone();
        a.checked_ms = b.checked_ms;
        a.rtt_ms = b.rtt_ms;
        if a.online && b.online {
            a.last_seen_ms = b.last_seen_ms;
        }
        &a == b
    };
    let put: Vec<DeviceRow> = now.iter().filter(|r| !old.get(r.id.as_str()).is_some_and(|o| same(o, r))).cloned().collect();
    let del: Vec<String> = before.iter().filter(|r| !new.contains_key(r.id.as_str())).map(|r| r.id.clone()).collect();
    (!put.is_empty() || !del.is_empty()).then_some(DeviceDelta { reset: false, put, del, hlc })
}

/// Every device this node knows: itself, the mesh peers, and `~/.config/control-room/devices`.
pub fn devices() -> Vec<Node> {
    let mut v = cr_sources::nodes::mesh_nodes();
    let extra = std::fs::read_to_string(format!("{}/.config/control-room/devices", cr_sources::nodes::home())).unwrap_or_default();
    for n in parse_peers(&extra) {
        if !v.iter().any(|x| x.id == n.id) {
            let mut n = n;
            if n.name.is_empty() {
                n.name = n.id.chars().take(8).collect();
            }
            v.push(n);
        }
    }
    v
}

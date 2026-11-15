//! cr.devices@2 as `#[cell_test(sim, as = [operator, root])]` would run it: one body per viewer chain.
//! A device that stops answering stays listed, offline, with its last-seen time, and its row is the delta `Watch`
//! sends; the operator sees ops-1 only and no `tailscale`/`passkeys` field on the wire.
use ce_wire::Caller;
use cr_devices::*;
use cr_devices_api::*;
use std::collections::BTreeMap;

fn caller(abilities: &[&str]) -> Caller {
    let mut rights = vec!["cr.devices:read".to_string()];
    rights.extend(abilities.iter().map(|a| format!("ability:{a}")));
    Caller { name: "cd".repeat(32), rights }
}

const ROOT: &[&str] = &["cr.devices:read", "cr.devices:net", "cr.devices:passkeys"];
const OPERATOR: &[&str] = &["cr.devices:read", "cr.devices:where:node=foreman-ops-1"];
const ALL: DevicesQuery = DevicesQuery { q: None, only_offline: None };

fn nodes() -> Vec<Node> {
    vec![
        Node { id: "aa".into(), name: "foreman-ops-1".into(), addrs: vec!["127.0.0.1".into()], local: true },
        Node { id: "bb".into(), name: "leif-desk".into(), addrs: vec!["10.77.76.155".into(), "100.120.94.24".into()], local: false },
        Node { id: "cc".into(), name: "debian".into(), addrs: vec!["10.77.37.223".into()], local: false },
    ]
}

fn table(probes: &[Probe], prev: &BTreeMap<String, DeviceRow>, now: u64) -> BTreeMap<String, DeviceRow> {
    nodes().iter().zip(probes).map(|(n, p)| (n.id.clone(), apply(prev.get(&n.id), n, p, now))).collect()
}

#[test]
fn peers_group_by_id_and_take_tailscale_names() {
    let n = parse_peers("10.0.0.2 aaaa000000000000000000000000000000000000000000000000000000000000 # mesh\n100.64.0.2 aaaa000000000000000000000000000000000000000000000000000000000000 # tailscale desk-1\n");
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].name, "desk-1");
    assert!(is_tailnet("100.64.0.2") && is_tailnet("100.120.94.24") && !is_tailnet("100.12.0.1") && !is_tailnet("10.77.76.155"));
}

#[test]
fn a_device_going_offline_is_one_delta_and_keeps_its_last_seen() {
    let up = Probe::Up { rtt_ms: 3, cells: Some(30) };
    let t0 = table(&[up.clone(), up.clone(), Probe::Down { why: "no route".into() }], &BTreeMap::new(), 1_000);
    let root = caller(ROOT);
    let before = for_caller(&root, &ALL, &t0);
    assert_eq!(before[0].name, "debian", "offline first");
    // one round (EVERY_S = 15 s) later leif-desk does not answer
    let t1 = table(&[up.clone(), Probe::Down { why: "10.77.76.155: no answer in 4000 ms".into() }, Probe::Down { why: "no route".into() }], &t0, 16_000);
    let after = for_caller(&root, &ALL, &t1);
    let d = delta(&before, &after, 16_000).expect("a delta");
    assert_eq!(d.put.len(), 1);
    let desk = &d.put[0];
    assert!(!desk.online && desk.last_seen_ms == 1_000 && desk.name == "leif-desk");
    assert_eq!(desk.tailscale.as_ref().map(|t| t.online), Some(false));
    assert!(EVERY_S * 1000 + PROBE_TIMEOUT_MS < 60_000, "shows within a minute");
    // nothing changed: no delta (checked_ms and rtt alone are not a change)
    let t2 = table(&[Probe::Up { rtt_ms: 9, cells: Some(30) }, Probe::Down { why: "10.77.76.155: no answer in 4000 ms".into() }, Probe::Down { why: "no route".into() }], &t1, 31_000);
    assert!(delta(&after, &for_caller(&root, &ALL, &t2), 31_000).is_none());
}

#[test]
fn the_operator_sees_ops_1_only_and_no_part_fields() {
    let up = Probe::Up { rtt_ms: 3, cells: Some(30) };
    let t = table(&[up.clone(), up.clone(), up], &BTreeMap::new(), 1_000);
    let rows = for_caller(&caller(OPERATOR), &ALL, &t);
    assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["foreman-ops-1"]);
    assert!(rows.iter().all(|r| r.tailscale.is_none() && r.passkeys.is_none()));
    let wire = ce_wire::postcard::to_stdvec(&rows).unwrap();
    for leak in ["leif-desk", "debian", "100.120.94.24"] {
        assert!(!wire.windows(leak.len()).any(|w| w == leak.as_bytes()), "{leak} on the operator's wire");
    }
    let root = for_caller(&caller(ROOT), &ALL, &t);
    assert_eq!(root.len(), 3);
    assert_eq!(root.iter().find(|r| r.name == "leif-desk").and_then(|r| r.tailscale.as_ref()).map(|t| t.ip.as_str()), Some("100.120.94.24"));
    assert!(root.iter().all(|r| !r.addrs.iter().any(|a| is_tailnet(a))), "tailnet addresses only in the Part");
}

//! Major 1, frozen: same schema as the first edition, the same scope rule, from v2 rows.
use ce_wire::{Api, Caller};
use cr_devices::*;
use cr_devices_api::versions::v1 as old;

#[test]
fn major_1_is_frozen_byte_for_byte() {
    assert_eq!(<old::CrDevices as Api>::SCHEMA, 0x034aabe59e03e812, "the schema the first edition's clients speak");
    assert_eq!(<cr_devices_api::CrDevices as Api>::VERSION, 2);
}

#[test]
fn v1_rows_keep_the_tailnet_address_and_the_scope_rule() {
    let n = Node { id: "aa".into(), name: "node-1".into(), addrs: vec!["10.0.0.2".into(), "100.64.0.2".into()], local: false };
    let r = apply(None, &n, &Probe::Up { rtt_ms: 3, cells: Some(4) }, 1);
    assert_eq!(r.addrs, ["10.0.0.2"]);
    let o = v1::to_v1(&r);
    assert_eq!(o.addrs, ["10.0.0.2", "100.64.0.2"]);
    let c = Caller { name: "cd".repeat(32), rights: vec!["cr.devices:read".into(), "ability:cr.devices:scope:node=other".into()] };
    let (rows, narrowed, _) = v1::view(&c, &old::DevicesAsk { q: String::new(), only_offline: false }, &[o]);
    assert!(rows.is_empty() && narrowed);
}

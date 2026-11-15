//! `cr.devices@2`: the Devices panel's full-stack cell. Every node of the mesh (and every device listed in
//! `~/.config/control-room/devices`), whether its door answers now, when it last did, how long the answer took, how
//! many cells it runs and its inventory line. A device that stops answering stays listed as offline with its
//! last-seen time; the cell asks every device every 15 s and `Watch` pushes the change at once.
//!
//! Rights: `cr.devices:read` (ops and the `DeviceList` view). Field level: `tailscale` needs `cr.devices:net`,
//! `passkeys` needs `cr.devices:passkeys`. Row level: `cr.devices:where:node=..` caveats.
#![forbid(unsafe_code)]

pub mod versions {
    pub mod v1;
}

ce_wire::api! {
    api CrDevices = "cr.devices", version 2;
    types {
        /// Health contract: these three fields, in this order, in every cell.
        pub struct Health { pub ok: bool, pub version: String, pub uptime_s: u64 }
        pub enum SourceState { Live, Stale { age_s: u64 }, Down { why: String }, Refused { needs: String } }
        pub struct Source { pub cell: String, pub node: String, pub state: SourceState, pub ms: u32 }
        pub enum DeviceKind { Server, Desktop, Laptop, Phone, Tablet, Unknown }
        /// One line of fleet.inventory for the node: live, age of its last report, and its summary.
        pub struct Inventory { pub live: bool, pub age_s: u64, pub summary: String }
        /// The node on the tailnet: its 100.64/10 address and whether that answered.
        pub struct Tailnet { pub ip: String, pub online: bool, pub key_expires_at: Option<i64> }
        /// A passkey or approval key the device holds (label, kind), never the key.
        pub struct PasskeyRef { pub label: String, pub kind: String }
        /// online: its door answered the last probe. last_seen_ms: the last answer (0 = not since this cell started).
        /// checked_ms: when it was last asked. rtt_ms: how long the last answer took. cells: cells placed there, when
        /// its cell.install answered. why: what the last failed probe said. `tailscale` (cr.devices:net) and
        /// `passkeys` (cr.devices:passkeys) are Part fields: None on the wire for a viewer without that right.
        pub struct DeviceRow {
            pub id: String, pub name: String, pub kind: DeviceKind, pub addrs: Vec<String>,
            pub online: bool, pub last_seen_ms: u64, pub checked_ms: u64, pub rtt_ms: Option<u32>, pub cells: Option<u32>,
            pub why: String, pub inventory: Option<Inventory>,
            pub tailscale: Option<Tailnet>, pub passkeys: Option<Vec<PasskeyRef>>,
            pub refs: Vec<String>
        }
        /// q: substring of name or id. only_offline: just the devices that do not answer.
        pub struct DevicesQuery { pub q: Option<String>, pub only_offline: Option<bool> }
        /// One item of `Watch`: reset = replace all rows; else put upserts (by id) and del removes.
        pub struct DeviceDelta { pub reset: bool, pub put: Vec<DeviceRow>, pub del: Vec<String>, pub hlc: u64 }
    }
    ops {
        1 => HealthOp(()) -> Health, needs "cr.devices:health";
        2 => List(DevicesQuery) -> Vec<DeviceRow>, needs "cr.devices:read", deadline_ms 15000;
        6 => Sources(()) -> Vec<Source>, needs "cr.devices:read", deadline_ms 15000;
    }
    streams {
        3 => Watch(DevicesQuery) -> stream DeviceDelta, needs "cr.devices:read";
    }
}

ui_part::views! {
    api CrDevices;
    view DeviceList {
        props { q: Option<String>, only_offline: Option<bool> }
        needs "cr.devices:read";
        events { opened(String) }
        data { sources: Sources }
        streams { rows: Watch by id }
        slots {}
        speaks [2];
    }
    parts { DeviceRow { tailscale: "cr.devices:net", passkeys: "cr.devices:passkeys" } }
}

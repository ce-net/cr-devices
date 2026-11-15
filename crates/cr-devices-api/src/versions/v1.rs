//! FROZEN: cr.devices major 1, the first edition's contract, byte for byte. Served beside major 2. Never edited.
//!
//! `cr.devices`: the Devices panel's view cell. Every node of the mesh with whether its door answers now, when it
//! last did, how long the answer took, how many cells it runs and its inventory line. A node that stops answering
//! stays listed as offline with its last-seen time; the cell asks every node every 15 s.
//!
//! Rights: `cr.devices:read`. Rows may be scoped with `cr.devices:scope:node=<glob>` abilities in the same chain.

ce_wire::api! {
    api CrDevices = "cr.devices", version 1;
    types {
        /// Health contract: these three fields, in this order, in every cell.
        pub struct Health { pub ok: bool, pub version: String, pub uptime_s: u64 }
        pub enum SourceState { Live, Stale { age_s: u64 }, Down { why: String }, Refused { needs: String } }
        pub struct Source { pub cell: String, pub node: String, pub state: SourceState, pub ms: u32 }
        pub enum DeviceKind { Server, Desktop, Laptop, Phone, Unknown }
        /// One line of fleet.inventory for the node: live, age of its last report, and its summary.
        pub struct Inventory { pub live: bool, pub age_s: u64, pub summary: String }
        /// online: its door answered the last probe. last_seen_ms: the last answer (0 = not since this cell started).
        /// checked_ms: when it was last asked. rtt_ms: how long the last answer took. cells: cells placed there, when
        /// its cell.install answered. why: what the last failed probe said.
        pub struct DeviceRow {
            pub id: String, pub name: String, pub kind: DeviceKind, pub addrs: Vec<String>,
            pub online: bool, pub last_seen_ms: u64, pub checked_ms: u64, pub rtt_ms: Option<u32>, pub cells: Option<u32>,
            pub why: String, pub inventory: Option<Inventory>, pub refs: Vec<String>
        }
        /// q: substring of name or id. only_offline: just the nodes that do not answer.
        pub struct DevicesAsk { pub q: String, pub only_offline: bool }
        pub struct DevicesView {
            pub rows: Vec<DeviceRow>, pub as_of_ms: u64, pub sources: Vec<Source>, pub narrowed: bool,
            pub can_ask: Vec<String>, pub call_id: String
        }
        pub enum CrError { Refused { needs: String }, Source { cell: String, why: String }, Invalid { why: String }, NotFound { what: String } }
    }
    ops {
        1 => HealthOp(()) -> Health, needs "cr.devices:health";
        2 => List(DevicesAsk) -> Result<DevicesView, CrError>, needs "cr.devices:read", deadline_ms 15000;
        3 => One(String) -> Result<DeviceRow, CrError>, needs "cr.devices:read", deadline_ms 15000;
    }
}

//! `cr-devices serve`: the cr.devices cell behind this machine's door (ce-cap, default deny). A background loop asks
//! every node's door every 15 s (typed Hello for cell.install, then its List for the cell count) and reads
//! fleet.inventory State when a replica answers; `List` and `One` answer from that state at once.
use ce_wire::{Server, WireError};
use cr_devices::*;
use cr_devices::v1;
use cr_devices_api::*;
use cr_sources::cell_install::{self as ci, CellInstall};
use cr_sources::fleet_inventory::{self as fi, FleetInventory, StateReq};
use cr_sources::trace::Trace;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, RwLock};

#[derive(Default)]
struct State {
    rows: BTreeMap<String, DeviceRow>,
    sources: Vec<Source>,
    as_of_ms: u64,
    hlc: u64,
}

async fn probe(n: &Node) -> Probe {
    let mut why = String::from("no address");
    for a in &n.addrs {
        let t0 = Instant::now();
        let tok = match ui_chain::source_token::<CellInstall>(&["cell.install:list"], 60, None) {
            Ok(t) => t,
            Err(e) => return Probe::Down { why: e },
        };
        let c = tokio::time::timeout(Duration::from_millis(PROBE_TIMEOUT_MS), ce_wire::Client::<CellInstall>::connect_opts(format!("{a}:7700"), tok, false, Duration::from_millis(PROBE_TIMEOUT_MS))).await;
        match c {
            Ok(Ok(mut c)) => {
                let rtt_ms = t0.elapsed().as_millis() as u32;
                c.deadline = Duration::from_secs(4);
                let cells = c.call::<ci::List>(&()).await.ok().map(|l| l.iter().filter(|p| p.processes > 0).count() as u32);
                return Probe::Up { rtt_ms, cells };
            }
            // the door answered and said no (other schema, no right): it is up
            Ok(Err(WireError::Refused(_))) | Ok(Err(WireError::Decode(_))) => return Probe::Up { rtt_ms: t0.elapsed().as_millis() as u32, cells: None },
            Ok(Err(e)) => why = format!("{a}: {e}"),
            Err(_) => why = format!("{a}: no answer in {PROBE_TIMEOUT_MS} ms"),
        }
    }
    Probe::Down { why }
}

async fn inventory() -> (Option<fi::StateResp>, Source) {
    let t0 = Instant::now();
    let got = tokio::time::timeout(Duration::from_secs(6), async {
        let tok = ui_chain::source_token::<FleetInventory>(&["fleet.inventory:read"], 60, None)?;
        let mut c = ce_wire::Client::<FleetInventory>::by_name(tok).await.map_err(|e| e.to_string())?;
        c.deadline = Duration::from_secs(4);
        c.call::<fi::State>(&StateReq { stale_s: 900 }).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:?}"))
    })
    .await
    .unwrap_or_else(|_| Err("no replica answered in 6 s".into()));
    let ms = t0.elapsed().as_millis() as u32;
    match got {
        Ok(s) => { let node = s.served_by.clone(); (Some(s), Source { cell: "fleet.inventory".into(), node, state: SourceState::Live, ms }) }
        Err(why) => (None, Source { cell: "fleet.inventory".into(), node: String::new(), state: SourceState::Down { why: why.chars().take(300).collect() }, ms }),
    }
}

async fn round(st: &Arc<RwLock<State>>, tx: Option<&watch::Sender<u64>>) {
    let start = ui_chain::now_ms();
    let t = Trace::root(&ui_chain::hostname());
    let ns = devices();
    let hs: Vec<_> = ns.iter().cloned().map(|n| tokio::spawn(async move { probe(&n).await })).collect();
    let mut probes = Vec::new();
    for h in hs {
        probes.push(h.await.unwrap_or(Probe::Down { why: "probe failed".into() }));
    }
    let (inv, inv_src) = inventory().await;
    let now = ui_chain::now_ms();
    let mut s = st.write().await;
    let mut rows = BTreeMap::new();
    for (n, p) in ns.iter().zip(probes.iter()) {
        let mut r = apply(s.rows.get(&n.id), n, p, now);
        if let Some(l) = inv.as_ref().and_then(|i| i.nodes.iter().find(|l| l.node == n.name)) {
            r.inventory = Some(Inventory { live: l.live, age_s: l.age_s, summary: l.summary.clone() });
        }
        rows.insert(n.id.clone(), r);
    }
    let up = probes.iter().filter(|p| matches!(p, Probe::Up { .. })).count();
    let changed = rows.iter().any(|(k, r)| s.rows.get(k).is_none_or(|o| o.online != r.online || o.name != r.name)) || rows.len() != s.rows.len();
    s.rows = rows;
    if changed {
        s.hlc = now;
        if let Some(tx) = tx {
            let _ = tx.send(now);
        }
    }
    t.finish(CELL, &format!("Probe {up}/{}", ns.len()), start, "ok");
    s.sources = vec![
        Source { cell: "net.mesh doors".into(), node: format!("{up}/{} answered", ns.len()), state: SourceState::Live, ms: 0 },
        inv_src,
    ];
    s.as_of_ms = now;
}

fn sources(s: &State) -> Vec<Source> {
    let mut sources = s.sources.clone();
    let age = ui_chain::now_ms().saturating_sub(s.as_of_ms) / 1000;
    for src in sources.iter_mut().filter(|x| matches!(x.state, SourceState::Live)) {
        if age > EVERY_S * 2 {
            src.state = SourceState::Stale { age_s: age };
        }
    }
    sources
}

async fn serve() -> Result<(), String> {
    // this cell's own key and narrow grant (never the node's key); the gate roots callers' chains at the grant's root
    let me = ui_chain::init(CELL)?.root();
    let started = Instant::now();
    let st: Arc<RwLock<State>> = Arc::default();
    let (tx, rx) = watch::channel(0u64);
    let bg = st.clone();
    tokio::spawn(async move {
        loop {
            round(&bg, Some(&tx)).await;
            tokio::time::sleep(Duration::from_secs(EVERY_S)).await;
        }
    });
    let (s1, s2, s3, s4, s5) = (st.clone(), st.clone(), st.clone(), st.clone(), st.clone());
    let gate = ce_gate::Gate::<CrDevices>::new(me, ce_gate::mesh_roots()).into_fn();
    let v2 = Server::<CrDevices>::new(gate)
        .on::<HealthOp, _, _>(move |_: (), _| async move { Ok(Health { ok: true, version: env!("CARGO_PKG_VERSION").into(), uptime_s: started.elapsed().as_secs() }) })
        .on::<List, _, _>(move |q: DevicesQuery, who| {
            let st = s1.clone();
            async move {
                let t = Trace::root(&ui_chain::hostname());
                let start = ui_chain::now_ms();
                let rows = for_caller(&who, &q, &st.read().await.rows);
                t.finish(CELL, &format!("List chain={}", ui_chain::chain_id(&who)), start, "ok");
                Ok(rows)
            }
        })
        .on::<Sources, _, _>(move |_: (), who| {
            let st = s2.clone();
            async move { Ok(sources_for(&who, sources(&*st.read().await))) }
        })
        .stream::<Watch, _, _>(move |q: DevicesQuery, who, sink| {
            let (st, mut rx) = (s3.clone(), rx.clone());
            async move {
                let end = Instant::now() + Duration::from_secs(STREAM_MAX_S);
                let (mut sent, hlc) = {
                    let g = st.read().await;
                    (for_caller(&who, &q, &g.rows), g.hlc)
                };
                sink.send(&DeviceDelta { reset: true, put: sent.clone(), del: vec![], hlc }).map_err(|e| e.to_string())?;
                loop {
                    let left = end.saturating_duration_since(Instant::now());
                    match tokio::time::timeout(left, rx.changed()).await {
                        Err(_) | Ok(Err(_)) => return Ok(()),
                        Ok(Ok(())) => {}
                    }
                    let g = st.read().await;
                    let now = for_caller(&who, &q, &g.rows);
                    if let Some(d) = delta(&sent, &now, g.hlc) {
                        sink.send(&d).map_err(|e| e.to_string())?;
                        sent = now;
                    }
                }
            }
        })
        .audit(ce_gate::audit::file())
        .serve_door();

    // major 1, frozen: the first edition's envelope from the same rows
    use cr_devices_api::versions::v1 as old;
    let list1 = |st: Arc<RwLock<State>>, ask: old::DevicesAsk, who: ce_wire::Caller| async move {
        let s = st.read().await;
        let all: Vec<old::DeviceRow> = s.rows.values().map(v1::to_v1).collect();
        let (rows, narrowed, can_ask) = v1::view(&who, &ask, &all);
        let srcs = sources(&s).into_iter().map(|x| old::Source { cell: x.cell, node: x.node, ms: x.ms, state: match x.state {
            SourceState::Live => old::SourceState::Live,
            SourceState::Stale { age_s } => old::SourceState::Stale { age_s },
            SourceState::Down { why } => old::SourceState::Down { why },
            SourceState::Refused { needs } => old::SourceState::Refused { needs },
        } }).collect();
        old::DevicesView { rows, as_of_ms: s.as_of_ms, sources: srcs, narrowed, can_ask, call_id: String::new() }
    };
    let gate1 = ce_gate::Gate::<old::CrDevices>::new(me, ce_gate::mesh_roots()).into_fn();
    let v1s = Server::<old::CrDevices>::new(gate1)
        .on::<old::HealthOp, _, _>(move |_: (), _| async move { Ok(old::Health { ok: true, version: env!("CARGO_PKG_VERSION").into(), uptime_s: started.elapsed().as_secs() }) })
        .on::<old::List, _, _>(move |ask: old::DevicesAsk, who| { let st = s4.clone(); async move { Ok(Ok(list1(st, ask, who).await)) } })
        .on::<old::One, _, _>(move |id: String, who| {
            let st = s5.clone();
            async move {
                let v = list1(st, old::DevicesAsk { q: String::new(), only_offline: false }, who).await;
                Ok(v.rows.into_iter().find(|r| r.id == id || r.name == id).ok_or(old::CrError::NotFound { what: id }))
            }
        })
        .audit(ce_gate::audit::file())
        .serve_door();
    let (a, b) = tokio::join!(v2, v1s);
    a.map_err(|e| e.to_string())?;
    b.map_err(|e| e.to_string())
}

fn main() {
    let r = match std::env::args().nth(1).as_deref() {
        Some("serve") => tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string()).and_then(|rt| rt.block_on(serve())),
        Some("probe") => tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string()).map(|rt| rt.block_on(async {
            let st: Arc<RwLock<State>> = Arc::default();
            round(&st, None).await;
            for r in st.read().await.rows.values() {
                println!("{:<18} {:<8} {:>6} cells={:<4} {}", r.name, if r.online { "online" } else { "OFFLINE" }, r.rtt_ms.map(|m| format!("{m}ms")).unwrap_or_default(), r.cells.map(|c| c.to_string()).unwrap_or("-".into()), r.why);
            }
            for s in &st.read().await.sources { println!("source {} {} {:?}", s.cell, s.node, s.state) }
        })),
        _ => Err("usage: cr-devices serve | probe".into()),
    };
    if let Err(e) = r {
        eprintln!("cr-devices: {e}");
        std::process::exit(1);
    }
}

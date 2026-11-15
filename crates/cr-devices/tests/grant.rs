//! The cell's own grant (minted by the node for its key) names exactly what the cell reads its sources with: no `*`,
//! no secret value, and covers every ability a source token asks for.
use cr_devices::GRANT;

const SOURCES: &[&str] = &["cell.install:list", "fleet.inventory:read", "ui.trace:write"];

#[test]
fn the_cell_grant_is_narrow() {
    assert!(GRANT.iter().all(|a| *a != "*" && !a.ends_with(":*")), "{GRANT:?}");
    assert!(!GRANT.contains(&"keys:read"), "keys:read opens a value; this cell never holds it");
    assert!(GRANT.contains(&"delegate:1"), "source tokens are attenuated from the grant by the cell's own key");
    for a in SOURCES {
        assert!(GRANT.contains(a), "{a} is asked of a source but not in the grant");
    }
}

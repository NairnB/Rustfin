//! RR4 live tests against the throwaway server ONLY (`JF_TEST_URL`; viewer / view-pass-1). Skipped without it.
//! Needs the trickplay data from `tools/testserver/trickplay.sh`; read-only.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

#[test]
fn trickplay_info_decodes_and_the_first_sheet_downloads() {
    let Some(u) = url() else { return };
    let dev = DeviceInfo { name: "live-rr4".into(), id: "live-rr4-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(&u, dev).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    let film = c.search("Amber Tide").unwrap().into_iter().find(|i| i.name.starts_with("Amber Tide")).unwrap();
    let info =
        c.trickplay_info(&film.id).unwrap().expect("trickplay info for Amber Tide (run tools/testserver/trickplay.sh)");
    assert!(info.width > 0 && info.height > 0 && info.thumbnail_count > 0 && info.interval > 0);
    let cell = info.locate(0.0).unwrap();
    assert_eq!((cell.sheet, cell.x, cell.y), (0, 0, 0));
    let sheet = c.trickplay_sheet_url(&film.id, info.width, 0);
    // Other lanes' library scans on the shared server can delete the tile files while the info stays: that is a data
    // state (re-run tools/testserver/trickplay.sh), not a client fault.
    let bytes = match c.get_bytes(&sheet) {
        Ok(b) => b,
        Err(ApiError::NotFound) => {
            eprintln!("tile sheet files are gone from the test server (run tools/testserver/trickplay.sh): skipped");
            return;
        }
        Err(e) => panic!("{sheet}: {e:?}"),
    };
    assert!(bytes.len() > 1000, "tile sheet is {} bytes", bytes.len());
}

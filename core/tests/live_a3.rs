//! A3 live tests against the throwaway server ONLY (`JF_TEST_URL`; admin tester / test-pass-1, non-admin
//! viewer / view-pass-1). Skipped when JF_TEST_URL is unset. Run with --test-threads=1. Writes go to a temporary
//! library `tmp-a3` that is removed at the end; config pages change a value and put it back.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-a3".into(), id: "live-a3-device".into(), version: "0.1.0".into() }
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

/// A fresh name per run, so a leftover from a crashed run can't be mistaken for this one's library.
fn temp_name() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("tmp-a3-{:x}", n % 0xffff_ffff)
}

fn find(c: &Client, name: &str) -> Option<jf_core::api::gen_admin::VirtualFolderInfo> {
    c.virtual_folders().unwrap().into_iter().find(|f| f.name.as_deref() == Some(name))
}

/// A library only gets its item (id, options) once the scan the add call started has created it; wait up to 10 s.
fn find_with_id(c: &Client, name: &str) -> jf_core::api::gen_admin::VirtualFolderInfo {
    for _ in 0..20 {
        if let Some(f) = find(c, name).filter(|f| f.item_id.is_some()) {
            return f;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    panic!("{name} never got an item id");
}

#[test]
fn available_options_and_configs_decode_and_viewer_is_forbidden() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let a = c.library_available_options().unwrap();
    assert!(a.metadata_savers.iter().any(|s| s.name == "Nfo"), "{a:?}");
    let movie = a.type_options.iter().find(|t| t.type_ == "Movie").expect("Movie type");
    assert!(!movie.metadata_fetchers.is_empty() && !movie.image_fetchers.is_empty());
    assert!(!movie.default_image_options.is_empty());
    let cfg = c.system_configuration().unwrap();
    assert!(cfg.get("PreferredMetadataLanguage").is_some() && cfg.get("EnableFolderView").is_some());
    let nfo = c.named_configuration("xbmcmetadata").unwrap();
    assert!(nfo.get("ReleaseDateFormat").is_some());

    let v = signed(&u, "viewer", "view-pass-1");
    assert!(matches!(v.virtual_folders(), Err(ApiError::Forbidden)));
    assert!(matches!(v.remove_virtual_folder("tmp-a3-nothing"), Err(ApiError::Forbidden)));
    // The NFO section reads fine for a viewer (200) on 10.10.7; only its write is admin-gated, so it is not poked here.
}

#[test]
fn temp_library_add_rename_options_paths_scan_remove() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let (name, renamed) = (temp_name(), format!("{}-renamed", temp_name()));
    let mut opts = jf_core::api::admin::new_library_options(&["/media/Movies".to_string()]);
    opts.preferred_metadata_language = Some("en".into());
    opts.metadata_country_code = Some("GB".into());
    c.add_virtual_folder(&name, Some("movies"), &["/media/Movies".to_string()], &opts, true).unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let f = find_with_id(&c, &name);
        assert_eq!(f.collection_type.as_deref(), Some("movies"));
        assert_eq!(f.locations.clone().unwrap_or_default(), vec!["/media/Movies".to_string()]);
        let id = f.item_id.clone().expect("item id");

        let mut o = f.library_options.clone().expect("options");
        assert!(!o.enable_realtime_monitor);
        o.enable_realtime_monitor = true;
        o.automatic_refresh_interval_days = 30;
        o.preferred_metadata_language = Some("fr".into());
        c.set_library_options(&id, &o).unwrap();
        let o = find(&c, &name).unwrap().library_options.unwrap();
        assert!(o.enable_realtime_monitor);
        assert_eq!(o.automatic_refresh_interval_days, 30);
        assert_eq!(o.preferred_metadata_language.as_deref(), Some("fr"));

        c.add_media_path(&name, "/media/Mixed").unwrap();
        let locs = find(&c, &name).unwrap().locations.unwrap_or_default();
        assert!(locs.contains(&"/media/Mixed".to_string()), "{locs:?}");
        c.remove_media_path(&name, "/media/Mixed").unwrap();
        let locs = find(&c, &name).unwrap().locations.unwrap_or_default();
        assert_eq!(locs, vec!["/media/Movies".to_string()]);

        c.scan_library(&id).unwrap();

        // A rename moves the library's folder: the name changes at once, but the item (id, options) is only
        // rebuilt by the next scan, so nothing after this uses the old id.
        c.rename_virtual_folder(&name, &renamed).unwrap();
        assert!(find(&c, &name).is_none());
        let f = find(&c, &renamed).expect("renamed");
        assert_eq!(f.locations.unwrap_or_default(), vec!["/media/Movies".to_string()]);
    }));
    c.remove_virtual_folder(&renamed).ok();
    c.remove_virtual_folder(&name).ok();
    assert!(find(&c, &name).is_none() && find(&c, &renamed).is_none(), "temp library left behind");
    // The removal's refresh drops the library from the views a moment later; other tests list the views.
    let mut gone = false;
    for _ in 0..40 {
        if !c.views().unwrap().iter().any(|v| v.name.starts_with("tmp-a3")) {
            gone = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert!(gone, "a tmp-a3 library is still in the views");
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn config_pages_change_a_value_and_put_it_back() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let before = c.system_configuration().unwrap();
    let mut edited = before.clone();
    let old = before["DisplaySpecialsWithinSeasons"].as_bool().unwrap();
    edited["DisplaySpecialsWithinSeasons"] = serde_json::json!(!old);
    c.set_system_configuration(&edited).unwrap();
    let now = c.system_configuration().unwrap();
    assert_eq!(now["DisplaySpecialsWithinSeasons"].as_bool(), Some(!old));
    // Everything else came back untouched.
    assert_eq!(now["PreferredMetadataLanguage"], before["PreferredMetadataLanguage"]);
    assert_eq!(now["MetadataOptions"], before["MetadataOptions"]);
    c.set_system_configuration(&before).unwrap();
    assert_eq!(c.system_configuration().unwrap()["DisplaySpecialsWithinSeasons"].as_bool(), Some(old));

    let nfo = c.named_configuration("xbmcmetadata").unwrap();
    let mut e = nfo.clone();
    e["ReleaseDateFormat"] = serde_json::json!("dd/MM/yyyy");
    c.set_named_configuration("xbmcmetadata", &e).unwrap();
    assert_eq!(c.named_configuration("xbmcmetadata").unwrap()["ReleaseDateFormat"], "dd/MM/yyyy");
    c.set_named_configuration("xbmcmetadata", &nfo).unwrap();
    assert_eq!(c.named_configuration("xbmcmetadata").unwrap()["ReleaseDateFormat"], nfo["ReleaseDateFormat"]);
}

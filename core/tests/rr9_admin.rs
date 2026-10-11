//! RR9 pure tests: plugin settings page lookup and URL (P-ADM-24).
use jf_core::api::admin::{page_for_plugin, plugin_settings_url, ConfigPage};

fn page(name: &str, id: Option<&str>) -> ConfigPage {
    ConfigPage { name: name.into(), plugin_id: id.map(str::to_string) }
}

#[test]
fn url_encodes_the_page_name() {
    assert_eq!(
        plugin_settings_url("http://x:8096/", "My Page&1"),
        "http://x:8096/web/#/configurationpage?name=My%20Page%261"
    );
    assert_eq!(plugin_settings_url("http://x:8096", "tmdb"), "http://x:8096/web/#/configurationpage?name=tmdb");
}

#[test]
fn page_found_by_plugin_id_ignoring_dashes_and_case() {
    let pages = vec![page("a", Some("11111111-aaaa-bbbb-cccc-000000000001")), page("b", None)];
    let hit = page_for_plugin(&pages, "11111111AAAABBBBCCCC000000000001").unwrap();
    assert_eq!(hit.name, "a");
}

#[test]
fn no_page_means_no_settings_button() {
    let pages = vec![page("a", Some("1")), page("", Some("2")), page("c", None)];
    assert!(page_for_plugin(&pages, "9").is_none());
    assert!(page_for_plugin(&pages, "2").is_none());
    assert!(page_for_plugin(&[], "1").is_none());
}

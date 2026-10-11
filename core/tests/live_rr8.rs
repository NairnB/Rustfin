//! RR8 live tests (P-USER-1, P-USER-4). Throwaway server only; skipped without `JF_TEST_URL`. The picture test puts
//! the profile back as it was (no picture), even when it fails.
use jf_core::api::*;

fn viewer() -> Option<Client> {
    let u = std::env::var("JF_TEST_URL").ok()?;
    let dev = DeviceInfo { name: "live-rr8".into(), id: "live-rr8-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(&u, dev).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    Some(c)
}

/// A 64x64 PNG made on the spot.
fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, (y * 4) as u8, 160, 255]));
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

struct RemovePicture(Client, String);
impl Drop for RemovePicture {
    fn drop(&mut self) {
        let _ = self.0.delete_user_image(&self.1);
    }
}

#[test]
fn picture_uploads_shows_and_goes() {
    let Some(c) = viewer() else {
        eprintln!("JF_TEST_URL unset: live test skipped");
        return;
    };
    let uid = c.session.as_ref().unwrap().user_id.clone();
    let _ = c.delete_user_image(&uid);
    let _guard = RemovePicture(c.clone(), uid.clone());
    let url = c.user_image_url(&uid, 64);
    assert!(ureq::get(&url).call().is_err(), "no picture to begin with");
    c.set_user_image(&uid, &png()).unwrap();
    let mut resp = ureq::get(&url).call().unwrap();
    let bytes = resp.body_mut().read_to_vec().unwrap();
    assert!(image_content_type(&bytes).is_some(), "the server returns a picture");
    c.delete_user_image(&uid).unwrap();
    assert!(ureq::get(&url).call().is_err(), "picture removed");
}

#[test]
fn a_non_picture_is_refused_before_it_is_sent() {
    let Some(c) = viewer() else { return };
    let uid = c.session.as_ref().unwrap().user_id.clone();
    assert!(c.set_user_image(&uid, b"hello").is_err());
}

#[test]
fn fmp4_plan_asks_for_mp4_segments_and_next_up_options_are_accepted() {
    let Some(c) = viewer() else { return };
    let arch = c.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
    let film = c.library_page(&arch.id, 0, 1).unwrap().items.remove(0);
    let direct = c.playback_plan(&film.id, 0, false).unwrap();
    let cap = (direct.source_bitrate.expect("source bitrate") / 2000).max(1) as u32;
    let plan = c.playback_plan_with(&film.id, cap, false, true).unwrap();
    let url = plan.transcoding_url.expect("a cap below the source bitrate returns a TranscodingUrl");
    assert!(url.contains("SegmentContainer=mp4"), "{url}");
    let plain = c.playback_plan_with(&film.id, cap, false, false).unwrap().transcoding_url.unwrap();
    assert!(!plain.contains("SegmentContainer=mp4"), "{plain}");
    c.next_up_with(20, true, 30).unwrap();
    c.next_up_with(20, false, 0).unwrap();
}

#[test]
fn backdrop_items_come_back_with_backdrops() {
    let Some(c) = viewer() else { return };
    let items = c.backdrop_items(10).unwrap();
    assert!(items.iter().all(|i| c.image_url(i, ImageKind::Backdrop, 640).is_some()));
}

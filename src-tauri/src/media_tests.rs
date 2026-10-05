// HUP-S10.1 (US-10.1) — media generation tiers, routes, cost lines, image checks, the granted
// output folder, and the gallery.
//
// BDD map:
// - AC1 tiered local vs registry/endpoint: `tier_caps_follow_the_planset_table`,
//   `routes_are_honest_about_what_is_available`, `video_has_no_backend_yet_and_says_so`.
// - AC2 outputs open in the Media pop-out: the gallery (`saved_images_land_in_the_gallery`) is
//   what the pop-out shows; the pop-out side is covered by the TS tests.
// - AC3 cost is shown: `every_route_carries_a_cost_line`, `usage_from_the_provider_is_kept`.
// - Generated files go to a granted folder only: `only_live_write_folder_grants_are_targets`,
//   `a_write_never_follows_a_swapped_root_or_overwrites`.

use super::*;
use crate::agent_grants::{Access, Grant, GrantKind, GrantState};

const NOW: u64 = 1_790_812_800;

fn tmp() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "citrate-media-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn png() -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    v.extend_from_slice(&[0u8; 64]);
    v
}

fn grant(id: &str, root: &std::path::Path, access: Access, kind: GrantKind) -> Grant {
    Grant {
        id: id.into(),
        kind,
        root: root.display().to_string(),
        access,
        scope: "subtree".into(),
        granted_at: NOW - 10,
        expires_at: None,
        granted_by: "member".into(),
        reason: "media".into(),
        revoked_at: None,
    }
}

fn state(grants: Vec<Grant>) -> GrantState {
    GrantState {
        version: 1,
        next_id: 10,
        grants,
    }
}

#[test]
fn tier_caps_follow_the_planset_table() {
    assert_eq!(
        tier_caps(Some("T0")),
        TierCaps {
            local_image: false,
            local_video: false
        }
    );
    assert_eq!(
        tier_caps(Some("T1")),
        TierCaps {
            local_image: true,
            local_video: false
        }
    );
    assert_eq!(
        tier_caps(Some("T2")),
        TierCaps {
            local_image: true,
            local_video: true
        }
    );
    // Unknown hardware never claims a local capability.
    assert_eq!(
        tier_caps(None),
        TierCaps {
            local_image: false,
            local_video: false
        }
    );
    assert_eq!(
        tier_caps(Some("T9")),
        TierCaps {
            local_image: false,
            local_video: false
        }
    );
}

fn settings(local: Option<&str>, remote: Option<&str>) -> MediaSettings {
    MediaSettings {
        local_url: local.map(str::to_string),
        local_model: Some("sd-turbo".into()),
        remote_provider: remote.map(str::to_string),
        remote_model: Some("gpt-image-1".into()),
    }
}

fn remote(id: &str, base: &str) -> RemoteProvider {
    RemoteProvider {
        id: id.into(),
        base_url: base.into(),
    }
}

#[test]
fn routes_are_honest_about_what_is_available() {
    // T0 with nothing configured: nothing is available, and each says why.
    let r = image_routes(Some("T0"), &MediaSettings::default(), None);
    assert_eq!(r.len(), 2);
    assert!(r.iter().all(|x| !x.available));
    assert!(r[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("not available on this device"));
    assert!(r[1]
        .reason
        .as_deref()
        .unwrap()
        .contains("no image provider"));

    // T1 with a local backend configured: local is available.
    let r = image_routes(
        Some("T1"),
        &settings(Some("http://127.0.0.1:7860/v1"), None),
        None,
    );
    let local = r.iter().find(|x| x.id == "local").unwrap();
    assert!(local.available, "{local:?}");
    assert_eq!(local.destination, "this device (127.0.0.1:7860)");

    // T1 but no backend running/configured: honest.
    let r = image_routes(Some("T1"), &MediaSettings::default(), None);
    let local = r.iter().find(|x| x.id == "local").unwrap();
    assert!(!local.available);
    assert!(local
        .reason
        .as_deref()
        .unwrap()
        .contains("no local image backend"));

    // A configured remote provider is available on any tier.
    let p = remote("openai", "https://api.openai.com/v1");
    let r = image_routes(Some("T0"), &settings(None, Some("openai")), Some(&p));
    let rem = r.iter().find(|x| x.id == "remote").unwrap();
    assert!(rem.available, "{rem:?}");
    assert_eq!(rem.destination, "api.openai.com");

    // A remote provider chosen in settings but no longer configured is not available.
    let r = image_routes(Some("T0"), &settings(None, Some("openai")), None);
    assert!(!r.iter().find(|x| x.id == "remote").unwrap().available);
}

#[test]
fn every_route_carries_a_cost_line() {
    let p = remote("gateway", "https://infer.citrate.ai/v1");
    let s = settings(Some("http://127.0.0.1:7860/v1"), Some("gateway"));
    for r in image_routes(Some("T2"), &s, Some(&p)) {
        assert!(!r.cost.is_empty(), "{r:?}");
    }
    let r = image_routes(Some("T2"), &s, Some(&p));
    assert!(r[0].cost.contains("No charge"));
    assert!(r[1].cost.contains("infer.citrate.ai"));
}

#[test]
fn video_has_no_backend_yet_and_says_so() {
    for t in [Some("T0"), Some("T1"), Some("T2"), None] {
        let v = video_routes(t);
        assert!(v.iter().all(|r| !r.available));
        assert!(!v[0].reason.as_deref().unwrap().is_empty());
    }
    // On T2 the reason is the missing backend, not the hardware.
    let v = video_routes(Some("T2"));
    assert!(v[0].reason.as_deref().unwrap().contains("no video backend"));
    let v = video_routes(Some("T0"));
    assert!(v[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("not available on this device"));
}

#[test]
fn the_local_backend_must_be_loopback_http() {
    for ok in [
        "http://127.0.0.1:7860/v1",
        "http://localhost:8188/v1",
        "http://[::1]:9000/v1",
    ] {
        assert!(validate_local_url(ok).is_ok(), "{ok}");
    }
    for bad in [
        "https://example.com/v1",
        "http://127.0.0.1.evil.com/v1",
        "http://127.0.0.1:80@evil.com/v1",
        "http://10.0.0.2:7860/v1",
        "file:///etc/passwd",
        "",
    ] {
        assert!(validate_local_url(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_request_body_is_bounded_and_model_aware() {
    let b = image_request("a lemon tree at dusk", "1024x1024", "gpt-image-1").unwrap();
    assert_eq!(b["n"], 1);
    assert_eq!(b["size"], "1024x1024");
    // gpt-image models always answer with base64 and refuse the response_format field.
    assert!(b.get("response_format").is_none());
    let b = image_request("x", "512x512", "sd-turbo").unwrap();
    assert_eq!(b["response_format"], "b64_json");
    assert!(image_request("", "1024x1024", "m").is_err());
    assert!(image_request(&"p".repeat(MAX_PROMPT_CHARS + 1), "1024x1024", "m").is_err());
    assert!(image_request("x", "4096x4096", "m").is_err());
    assert!(image_request("x", "1024x1024", "").is_err());
}

#[test]
fn only_base64_images_of_a_known_type_are_accepted() {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png());
    let body = serde_json::json!({"data": [{"b64_json": b64}], "usage": {"total_tokens": 4200}});
    let img = parse_image_response(&body.to_string()).unwrap();
    assert_eq!(img.mime, "image/png");
    assert_eq!(img.ext, "png");
    assert_eq!(img.usage.unwrap()["total_tokens"], 4200);

    // A link instead of bytes is refused: the app never fetches a URL a provider hands back.
    let link = serde_json::json!({"data": [{"url": "https://cdn.example/x.png"}]});
    let e = parse_image_response(&link.to_string()).unwrap_err();
    assert!(e.contains("link"), "{e}");

    // Not an image.
    let html = base64::engine::general_purpose::STANDARD.encode(b"<html>hi</html>");
    let body = serde_json::json!({"data": [{"b64_json": html}]});
    assert!(parse_image_response(&body.to_string()).is_err());

    for (bytes, mime) in [
        (b"\xff\xd8\xff\xe0rest".to_vec(), "image/jpeg"),
        (b"RIFF\x10\x00\x00\x00WEBPVP8 ".to_vec(), "image/webp"),
    ] {
        assert_eq!(sniff(&bytes).map(|s| s.0), Some(mime));
    }
    assert!(parse_image_response("{}").is_err());
}

#[test]
fn usage_from_the_provider_is_kept() {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png());
    let body = serde_json::json!({"data": [{"b64_json": b64}]});
    assert!(parse_image_response(&body.to_string())
        .unwrap()
        .usage
        .is_none());
}

#[test]
fn only_live_write_folder_grants_are_targets() {
    let a = tmp();
    let b = tmp();
    let c = tmp();
    let mut revoked = grant("3", &c, Access::Write, GrantKind::Folder);
    revoked.revoked_at = Some(NOW - 1);
    let mut expired = grant("4", &c, Access::Write, GrantKind::Folder);
    expired.expires_at = Some(NOW);
    let st = state(vec![
        grant("1", &a, Access::Write, GrantKind::Folder),
        grant("2", &b, Access::Read, GrantKind::Folder),
        revoked,
        expired,
        grant("5", &c, Access::Read, GrantKind::FullAccess),
        // Full access never writes, even if a document claimed a write full-access grant.
        grant("6", &c, Access::Write, GrantKind::FullAccess),
    ]);
    let t = write_targets(&st, NOW);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].grant_id, "1");
    assert!(target_root(&st, "2", NOW).is_err());
    assert!(target_root(&st, "3", NOW).is_err());
    assert!(target_root(&st, "4", NOW).is_err());
    assert!(target_root(&st, "6", NOW).is_err());
    assert!(target_root(&st, "nope", NOW).is_err());
    assert_eq!(target_root(&st, "1", NOW).unwrap(), a);
}

#[cfg(unix)]
#[test]
fn a_write_never_follows_a_swapped_root_or_overwrites() {
    let base = tmp();
    let real = base.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let st = state(vec![grant("1", &real, Access::Write, GrantKind::Folder)]);
    let root = target_root(&st, "1", NOW).unwrap();
    let p = write_new_file(&root, "citrate-image-x.png", &png()).unwrap();
    assert!(p.starts_with(&real));
    assert_eq!(std::fs::read(&p).unwrap(), png());
    // Never overwrites.
    assert!(write_new_file(&root, "citrate-image-x.png", &png()).is_err());
    // A name with a separator is refused.
    assert!(write_new_file(&root, "../escape.png", &png()).is_err());
    // The granted folder swapped for a symlink to elsewhere grants nothing.
    let elsewhere = base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::rename(&real, base.join("moved")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &real).unwrap();
    assert!(target_root(&st, "1", NOW).is_err());
}

#[test]
fn file_names_are_safe_and_distinct() {
    let a = output_name(MediaKind::Image, NOW, "png");
    let b = output_name(MediaKind::Image, NOW, "png");
    assert_ne!(a, b);
    assert!(a.starts_with("citrate-image-20261001-000000-"), "{a}");
    assert!(a.ends_with(".png"));
    assert!(!a.contains('/') && !a.contains('\\'));
}

#[test]
fn saved_images_land_in_the_gallery_newest_first_and_capped() {
    let dir = tmp();
    let store = GalleryStore::new(&dir);
    for i in 0..(MAX_GALLERY + 3) {
        store
            .record_item(GalleryItem {
                id: format!("m{i}"),
                kind: MediaKind::Image,
                path: dir.join(format!("{i}.png")).display().to_string(),
                grant_id: "1".into(),
                prompt: "p".into(),
                route: "local".into(),
                destination: "this device".into(),
                model: "m".into(),
                created_at: NOW + i as u64,
                bytes: 10,
                mime: "image/png".into(),
                cost: "No charge".into(),
                usage: None,
            })
            .unwrap();
    }
    let items = store.items().unwrap();
    assert_eq!(items.len(), MAX_GALLERY);
    assert_eq!(items[0].id, format!("m{}", MAX_GALLERY + 2));
    assert!(store.find_item("m0").unwrap().is_none());
    // A corrupted gallery reads as empty and is not overwritten by a read.
    std::fs::write(dir.join(GALLERY_FILE), "nope").unwrap();
    assert!(store.items().is_err());
}

#[test]
fn a_gallery_file_is_shown_only_if_it_is_still_a_real_image() {
    let dir = tmp();
    let p = dir.join("a.png");
    std::fs::write(&p, png()).unwrap();
    let url = data_url_for(&p).unwrap();
    assert!(url.starts_with("data:image/png;base64,"));
    std::fs::write(&p, b"<svg onload=alert(1)>").unwrap();
    assert!(data_url_for(&p).is_err());
    assert!(data_url_for(&dir.join("missing.png")).is_err());
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_a_gallery_file_is_never_read() {
    let dir = tmp();
    let secret = dir.join("secret.png");
    std::fs::write(&secret, png()).unwrap();
    // A symlink where the gallery file was.
    let sym = dir.join("sym.png");
    std::os::unix::fs::symlink(&secret, &sym).unwrap();
    assert!(data_url_for(&sym).is_err());
    assert!(read_gallery_file(&sym).is_err());
    // A hard link: a second name for a file somewhere else.
    let hard = dir.join("hard.png");
    std::fs::hard_link(&secret, &hard).unwrap();
    assert!(data_url_for(&hard).is_err());
    std::fs::remove_file(&hard).unwrap();
    // Back to one name: readable.
    assert!(data_url_for(&secret).is_ok());
}

fn item_at(path: &std::path::Path, grant_id: &str) -> GalleryItem {
    GalleryItem {
        id: "m1".into(),
        kind: MediaKind::Image,
        path: path.display().to_string(),
        grant_id: grant_id.into(),
        prompt: "p".into(),
        route: "local".into(),
        destination: "this device".into(),
        model: "m".into(),
        created_at: NOW,
        bytes: 10,
        mime: "image/png".into(),
        cost: "No charge".into(),
        usage: None,
    }
}

#[test]
fn a_gallery_image_is_read_only_while_its_folder_is_still_granted() {
    let dir = tmp();
    let p = dir.join("a.png");
    std::fs::write(&p, png()).unwrap();
    let live = state(vec![grant("1", &dir, Access::Write, GrantKind::Folder)]);
    assert!(gallery_read_allowed(&live, &item_at(&p, "1"), NOW).is_ok());
    let mut revoked_g = grant("1", &dir, Access::Write, GrantKind::Folder);
    revoked_g.revoked_at = Some(NOW - 1);
    let revoked = state(vec![revoked_g]);
    assert!(gallery_read_allowed(&revoked, &item_at(&p, "1"), NOW).is_err());
    // The item's path must be inside the grant it names.
    let other = tmp();
    let q = other.join("b.png");
    std::fs::write(&q, png()).unwrap();
    assert!(gallery_read_allowed(&live, &item_at(&q, "1"), NOW).is_err());
}

#[test]
fn a_write_folder_in_a_protected_location_is_never_a_target() {
    let base = tmp();
    let protected = base.join(".ssh");
    std::fs::create_dir_all(&protected).unwrap();
    let st = state(vec![grant("1", &protected, Access::Write, GrantKind::Folder)]);
    assert!(target_root(&st, "1", NOW).is_err());
    assert!(write_targets(&st, NOW).is_empty());
}

// ---- the generate pipeline (media_generate_image's body) ----

fn b64_png_reply() -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png());
    serde_json::json!({"data": [{"b64_json": b64}], "usage": {"total_tokens": 7}}).to_string()
}

#[test]
fn generate_writes_into_the_granted_folder_and_records_the_cost_line() {
    let out = tmp();
    let media = tmp();
    let st = state(vec![grant("1", &out, Access::Write, GrantKind::Folder)]);
    let store = GalleryStore::new(&media);
    let p = remote("gateway", "https://infer.citrate.ai/v1");
    let s = settings(None, Some("gateway"));
    let ctx = GenerateContext {
        settings: &s,
        tier: Some("T0"),
        remote: Some(&p),
        grants: &st,
        gallery: &store,
    };
    let req = GenerateRequest {
        route: "remote",
        prompt: "  a lemon on a table  ",
        size: "512x512",
        grant_id: "1",
    };
    let mut asked = None;
    let item = generate_image(
        &ctx,
        &req,
        || NOW,
        |id, body| {
            asked = Some((id.to_string(), body.clone()));
            Ok(b64_png_reply())
        },
    )
    .unwrap();
    let (id, body) = asked.unwrap();
    assert_eq!(id, "gateway");
    assert_eq!(body["prompt"], "a lemon on a table");
    assert_eq!(item.prompt, "a lemon on a table");
    assert_eq!(item.route, "remote");
    assert!(item.cost.contains("infer.citrate.ai"), "{}", item.cost);
    assert_eq!(item.mime, "image/png");
    let path = std::path::PathBuf::from(&item.path);
    assert_eq!(path.parent().unwrap(), out.as_path());
    assert_eq!(std::fs::read(&path).unwrap(), png());
    assert_eq!(store.items().unwrap(), vec![item.clone()]);
    assert!(data_url_for(&path).unwrap().starts_with("data:image/png;base64,"));
}

#[test]
fn generate_checks_the_route_and_the_destination_before_asking_any_backend() {
    let out = tmp();
    let media = tmp();
    let st = state(vec![grant("2", &out, Access::Read, GrantKind::Folder)]);
    let store = GalleryStore::new(&media);
    let p = remote("openai", "https://api.openai.com/v1");
    let s = settings(None, Some("openai"));
    let ctx = GenerateContext {
        settings: &s,
        tier: Some("T0"),
        remote: Some(&p),
        grants: &st,
        gallery: &store,
    };
    let mut called = 0;
    // A read-only grant is no destination: the provider is never asked.
    let req = GenerateRequest {
        route: "remote",
        prompt: "x",
        size: "512x512",
        grant_id: "2",
    };
    assert!(generate_image(&ctx, &req, || NOW, |_, _| {
        called += 1;
        Ok(b64_png_reply())
    })
    .is_err());
    // T0 has no local route, and an unknown route is refused.
    for route in ["local", "video"] {
        let req = GenerateRequest {
            route,
            prompt: "x",
            size: "512x512",
            grant_id: "2",
        };
        assert!(generate_image(&ctx, &req, || NOW, |_, _| {
            called += 1;
            Ok(b64_png_reply())
        })
        .is_err());
    }
    assert_eq!(called, 0);
    assert!(store.items().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
}

#[test]
fn a_provider_link_instead_of_image_bytes_saves_nothing() {
    let out = tmp();
    let media = tmp();
    let st = state(vec![grant("1", &out, Access::Write, GrantKind::Folder)]);
    let store = GalleryStore::new(&media);
    let p = remote("custom", "https://images.example.test/v1");
    let s = settings(None, Some("custom"));
    let ctx = GenerateContext {
        settings: &s,
        tier: Some("T1"),
        remote: Some(&p),
        grants: &st,
        gallery: &store,
    };
    let req = GenerateRequest {
        route: "remote",
        prompt: "x",
        size: "512x512",
        grant_id: "1",
    };
    let link = serde_json::json!({"data": [{"url": "https://images.example.test/a.png"}]}).to_string();
    assert!(generate_image(&ctx, &req, || NOW, |_, _| Ok(link)).is_err());
    assert!(store.items().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
}

/// LIVE proof (US-10.1 AC1..AC3, image tier): one real image from a loopback OpenAI-images server
/// (for example stable-diffusion.cpp's `sd-server`), through the same pipeline the Media view
/// calls, into a granted folder, recorded in the gallery with its cost line, and read back as the
/// data URL the Media pop-out shows. Run:
///
/// `CITRATE_MEDIA_LIVE_URL=http://127.0.0.1:18731/v1 cargo test -p citrate-core --lib media::tests::live_ -- --ignored --nocapture`
///
/// Optional `CITRATE_MEDIA_LIVE_OUT=<dir>` keeps the image there (the folder is granted for this
/// run only); otherwise a temporary folder is used.
#[test]
#[ignore = "live: needs a loopback OpenAI-images server in CITRATE_MEDIA_LIVE_URL"]
fn live_local_image_generation_into_a_granted_folder() {
    use sha2::Digest as _;
    let Ok(url) = std::env::var("CITRATE_MEDIA_LIVE_URL") else {
        panic!("set CITRATE_MEDIA_LIVE_URL to the loopback image server, e.g. http://127.0.0.1:18731/v1");
    };
    let out = match std::env::var("CITRATE_MEDIA_LIVE_OUT") {
        Ok(d) => {
            std::fs::create_dir_all(&d).unwrap();
            std::path::PathBuf::from(d).canonicalize().unwrap()
        }
        Err(_) => tmp(),
    };
    let media = tmp();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut g = grant("1", &out, Access::Write, GrantKind::Folder);
    g.granted_at = now - 1;
    let st = state(vec![g]);
    let store = GalleryStore::new(&media);
    let s = MediaSettings {
        local_url: Some(url.clone()),
        local_model: std::env::var("CITRATE_MEDIA_LIVE_MODEL").ok(),
        remote_provider: None,
        remote_model: None,
    };
    let ctx = GenerateContext {
        settings: &s,
        tier: Some("T1"),
        remote: None,
        grants: &st,
        gallery: &store,
    };
    let local = image_routes(Some("T1"), &s, None)
        .into_iter()
        .find(|r| r.id == "local")
        .unwrap();
    assert!(local.available, "{local:?}");
    let req = GenerateRequest {
        route: "local",
        prompt: "a ripe lemon on a wooden table, soft daylight, photo",
        size: "512x512",
        grant_id: "1",
    };
    let started = std::time::Instant::now();
    let item = generate_image(&ctx, &req, move || now, |_, _| {
        Err("the remote route is not used in this proof".into())
    })
    .unwrap();
    let elapsed = started.elapsed();
    let path = std::path::PathBuf::from(&item.path);
    assert_eq!(path.parent().unwrap(), out.as_path());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(sniff(&bytes).map(|x| x.0), Some(item.mime.as_str()));
    assert_eq!(store.items().unwrap()[0], item);
    let data_url = data_url_for(&path).unwrap();
    assert!(data_url.starts_with(&format!("data:{};base64,", item.mime)));
    assert_eq!(item.cost, local.cost);
    println!(
        "{}",
        serde_json::json!({
            "route": item.route,
            "destination": item.destination,
            "model": item.model,
            "cost": item.cost,
            "usage": item.usage,
            "mime": item.mime,
            "bytes": item.bytes,
            "sha256": hex::encode(sha2::Sha256::digest(&bytes)),
            "file_name": path.file_name().map(|n| n.to_string_lossy().to_string()),
            "in_granted_folder": path.parent() == Some(out.as_path()),
            "gallery_items": store.items().unwrap().len(),
            "data_url_prefix": &data_url[..data_url.find(',').unwrap_or(0) + 1],
            "elapsed_ms": elapsed.as_millis() as u64,
        })
    );
}

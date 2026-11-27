// L5-WP01 L5-MEDIA (characterization, citrate-core a826e441). Included into `media::tests` from
// media_tests.rs; registry ci/characterization.d/l5.json.
//
// Pins the media routes as they are before L5 adds `image_generate` and BYOK images (E18):
//
// * the tier rows: T0, unknown and any other string allow nothing local; T1 allows a local image;
//   T2 allows a local image and video; video still has no backend on any tier;
// * the remote image route is the legacy `ai.rs` provider the member picked in Settings > AI
//   providers (its stored base URL names the host and the cost line), posted through
//   `AiManager::post_to_provider`, not the D-11 BYOK provider keys. L5-WP10 (provider images through
//   the member's BYOK keys) supersedes that half with a stricter test, in a separate commit.

mod lq_l5_media {
    use crate::media::{image_routes, tier_caps, video_routes, MediaSettings, RemoteProvider};

    #[test]
    fn tier_caps_rows() {
        let rows: [(Option<&str>, bool, bool); 7] = [
            (None, false, false),
            (Some("T0"), false, false),
            (Some("T1"), true, false),
            (Some("T2"), true, true),
            (Some("T3"), false, false),
            (Some("t1"), false, false),
            (Some(""), false, false),
        ];
        for (tier, image, video) in rows {
            let caps = tier_caps(tier);
            assert_eq!(caps.local_image, image, "{tier:?} image");
            assert_eq!(caps.local_video, video, "{tier:?} video");
            // Video: one local route, never available, on every tier.
            let v = video_routes(tier);
            assert_eq!(v.len(), 1, "{tier:?}");
            assert_eq!(v[0].id, "local");
            assert!(!v[0].available, "{tier:?} video is unavailable");
            // The local image route is offered only where the row allows it.
            let routes = image_routes(tier, &MediaSettings::default(), None);
            let ids: Vec<&str> = routes.iter().map(|r| r.id).collect();
            assert_eq!(ids, vec!["local", "remote"], "{tier:?}");
            let why = routes[0].reason.clone().unwrap_or_default();
            assert_eq!(
                why.starts_with("Local image generation is not available on this device"),
                !image,
                "{tier:?}: {why}"
            );
        }
    }

    #[test]
    fn remote_route_is_legacy_ai_provider() {
        let chosen = MediaSettings {
            remote_provider: Some("openai".into()),
            remote_model: Some("gpt-image-1".into()),
            ..MediaSettings::default()
        };
        let configured = RemoteProvider {
            id: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
        };

        // The configured provider: its stored base URL names the host that bills the key.
        let routes = image_routes(Some("T1"), &chosen, Some(&configured));
        let remote = &routes[1];
        assert_eq!(remote.id, "remote");
        assert!(remote.available);
        assert_eq!(remote.reason, None);
        assert_eq!(remote.destination, "api.openai.com");
        assert_eq!(remote.model, "gpt-image-1");
        assert_eq!(
            remote.cost,
            "Billed by api.openai.com to your key; the price is set by the provider, not by Citrate"
        );

        // The chosen provider with no key: sent to Settings > AI providers (the ai.rs keys).
        let routes = image_routes(Some("T1"), &chosen, None);
        assert!(!routes[1].available);
        assert_eq!(
            routes[1].reason.as_deref(),
            Some(
                "The provider \"openai\" has no key configured. Add one in Settings > AI providers."
            )
        );

        // Nothing chosen: the member picks one of their AI providers in the Media settings.
        let routes = image_routes(Some("T1"), &MediaSettings::default(), Some(&configured));
        assert!(!routes[1].available);
        assert_eq!(
            routes[1].reason.as_deref(),
            Some("There is no image provider chosen. Pick one of your AI providers in the Media settings.")
        );

        // Where the provider comes from and how the request is sent: the ai.rs manager only.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/media.rs");
        let src = std::fs::read_to_string(path).expect("media.rs");
        assert!(src.contains("try_state::<crate::ai::AiState>(app)?"));
        assert!(src.contains("ai.0.provider_base_url(id)"));
        assert!(src.contains(
            "ai.post_to_provider(proxy, &permit, provider_id, \"/images/generations\", body)"
        ));
        for byok in ["crate::model_provider", "crate::provider_"] {
            assert!(!src.contains(byok), "media.rs does not use {byok}");
        }
    }
}

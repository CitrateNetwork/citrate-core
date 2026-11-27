// L1-WP01 CH-6 (characterization, citrate-core a826e441). Included into `model_proxy_http::tests`.
//
// Pins today's loopback model proxy surface (`serve_one`, model_proxy_http.rs): the origin and
// Host guards run first, then the path; `/v1/chat/completions` is the only path served and only
// for POST; every other path is 404 and reaches no vendor. WP10 (`/v1/responses`) and WP12
// (`/v1/messages`) supersede the 404 rows for their own path and name CH-6.

mod lq_ch6_proxy_paths {
    use super::*;
    use crate::model_proxy::tests::Rig;

    /// Paths a client might try. None of them is the chat path at a826e441.
    const OTHER_PATHS: &[&str] = &[
        "/v1/responses",
        "/v1/responses/compact",
        "/v1/messages",
        "/v1/models",
        "/v1/embeddings",
        "/v1/completions",
        "/v1/chat",
        "/v1/chat/completions/",
        "/V1/chat/completions",
        "/v1/chat/completions?stream=true",
        "/v1//chat/completions",
        "/health",
        "/",
        "",
    ];

    fn granted(rig: &Rig) -> String {
        let g = rig
            .proxy
            .mint_route(rig.route("gpt-known"), rig.gate())
            .unwrap();
        g.bearer.expose().to_string()
    }

    fn at(bearer: &str, method: &str, path: &str, extra: &[(&str, &str)]) -> HttpReq {
        let mut req = post(bearer, chat_body("gpt-known", false), extra);
        req.method = method.to_string();
        req.path = path.to_string();
        req
    }

    #[test]
    fn lq_ch6_the_chat_path_is_the_only_one_served() {
        assert_eq!(CHAT_PATH, "/v1/chat/completions");
        let rig = Rig::new("lq-ch6-paths");
        let b = granted(&rig);
        let before = rig.vendor.calls();
        for path in OTHER_PATHS {
            for method in ["POST", "GET", "PUT", "DELETE"] {
                let resp = serve(&rig, &at(&b, method, path, &[]));
                assert_eq!(status_of(&resp), 404, "{method} {path:?}: {resp}");
                assert!(resp.contains("not_found"), "{method} {path:?}: {resp}");
            }
        }
        assert_eq!(rig.vendor.calls(), before, "no other path reached the vendor");
        // The chat path itself still answers.
        let ok = serve(&rig, &at(&b, "POST", CHAT_PATH, &[]));
        assert_eq!(status_of(&ok), 200, "{ok}");
    }

    #[test]
    fn lq_ch6_the_chat_path_is_post_only() {
        let rig = Rig::new("lq-ch6-methods");
        let b = granted(&rig);
        let before = rig.vendor.calls();
        for method in ["GET", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD", "post"] {
            let resp = serve(&rig, &at(&b, method, CHAT_PATH, &[]));
            assert_eq!(status_of(&resp), 405, "{method}: {resp}");
        }
        assert_eq!(rig.vendor.calls(), before);
    }

    #[test]
    fn lq_ch6_the_origin_and_host_guards_run_before_the_path() {
        let rig = Rig::new("lq-ch6-guards");
        let b = granted(&rig);
        for path in ["/v1/responses", "/v1/messages", CHAT_PATH, "/"] {
            for extra in [
                [("origin", "https://evil.example")],
                [("host", "evil.example:1")],
                [("host", "127.0.0.1:2")],
            ] {
                let resp = serve(&rig, &at(&b, "POST", path, &extra));
                assert_eq!(status_of(&resp), 403, "{path} {extra:?}: {resp}");
            }
        }
    }

    #[test]
    fn lq_ch6_an_unknown_path_is_404_before_any_bearer_check() {
        let rig = Rig::new("lq-ch6-bearer");
        for path in ["/v1/responses", "/v1/messages"] {
            let mut none = at("x", "POST", path, &[]);
            none.headers.retain(|(k, _)| k != "authorization");
            assert_eq!(status_of(&serve(&rig, &none)), 404, "{path}");
            let unknown = at("cmp_nope", "POST", path, &[]);
            assert_eq!(status_of(&serve(&rig, &unknown)), 404, "{path}");
        }
        // On the chat path the same requests are 401.
        let unknown = at("cmp_nope", "POST", CHAT_PATH, &[]);
        assert_eq!(status_of(&serve(&rig, &unknown)), 401);
    }
}

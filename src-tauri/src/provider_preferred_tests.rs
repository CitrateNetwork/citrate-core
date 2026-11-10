// D-11 (P-5) — the preferred cloud model: stored only by the member's set, checked against the
// proxy (connected, able to take calls; never the delegate), approved for the anomaly halt, and
// read as "no preference" when missing, unreadable or edited into something invalid.

use super::*;
use crate::model_proxy::tests::Rig;

fn choice(p: &str, m: &str) -> Option<PreferredModel> {
    Some(PreferredModel { provider_id: p.into(), model: m.into() })
}

#[test]
fn missing_is_no_preference_and_a_set_choice_reads_back_and_routes() {
    let rig = Rig::new("pref-set");
    let dir = rig.dir.0.join("providers-pref");
    assert_eq!(load(&dir), PreferredView { preferred: None, load_error: None });
    assert!(route(&dir, "hs-x").unwrap_err().contains("no cloud model is chosen"));
    let v = set(&dir, &rig.proxy, choice("openai", "gpt-chosen")).unwrap();
    assert_eq!(v.preferred, choice("openai", "gpt-chosen"));
    let r = route(&dir, "hs-x").unwrap();
    assert_eq!((r.provider.as_str(), r.model.as_str(), r.session_id.as_str()), ("openai", "gpt-chosen", "hs-x"));
    // The member's model is approved for the anomaly halt.
    let approved = rig.proxy.with_state(|g, _| g.get("openai").approved_models.clone());
    assert!(approved.contains("gpt-chosen"));
    // Clearing it.
    assert_eq!(set(&dir, &rig.proxy, None).unwrap().preferred, None);
    assert!(route(&dir, "hs-x").is_err());
}

#[test]
fn only_a_connected_chat_provider_that_can_take_calls_is_accepted() {
    let rig = Rig::new("pref-refuse");
    let dir = rig.dir.0.join("providers-pref");
    for (p, m, why) in [
        ("xai", "grok", "not connected"),
        ("cognition", "devin", "Cognition"),
        ("nope", "m", "unknown provider"),
        ("openai", "", "choose the model"),
        ("openai", " padded", "choose the model"),
    ] {
        let e = set(&dir, &rig.proxy, choice(p, m)).unwrap_err();
        assert!(e.contains(why), "{p}/{m}: {e}");
    }
    let c = rig.proxy.add_custom("Box", "https://llm.example.org/v1", vec!["m".into()], 1).unwrap();
    let e = set(&dir, &rig.proxy, choice(&c.id, "m")).unwrap_err();
    assert!(e.contains("cannot take calls now"), "{e}");
    assert_eq!(load(&dir).preferred, None, "nothing was stored");
}

#[test]
fn an_unreadable_or_edited_file_is_no_preference_and_says_so() {
    let rig = Rig::new("pref-bad");
    let dir = rig.dir.0.join("providers-pref");
    std::fs::create_dir_all(&dir).unwrap();
    for bad in ["{", r#"{"providerId":"cognition","model":"devin"}"#, r#"{"providerId":"openai","model":"m","extra":1}"#, "7"] {
        std::fs::write(path(&dir), bad).unwrap();
        let v = load(&dir);
        assert_eq!(v.preferred, None, "{bad}");
        assert!(v.load_error.is_some(), "{bad}");
        assert!(route(&dir, "hs-x").is_err(), "{bad}");
    }
    std::fs::write(path(&dir), "null").unwrap();
    assert_eq!(load(&dir), PreferredView { preferred: None, load_error: None });
}

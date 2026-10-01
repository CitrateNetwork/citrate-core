//! HUP-S10.1 (US-10.1) — image and video generation, tiered, with a gallery and the Media pop-out.
//!
//! ## Routes (AC1)
//!
//! An image can be made two ways, and the app says which are available and why not:
//!
//! * **local**: a member-configured image server on this machine (a loopback `http` URL that
//!   speaks the OpenAI images API, `POST {url}/images/generations`). Offered only on tiers whose
//!   row in the planset table allows local images (T1, T2; 02_ARCHITECTURE section 3). On T0 it
//!   reads "not available on this device".
//! * **remote**: one of the member's configured AI providers (Settings > AI providers: OpenAI,
//!   the Citrate gateway, or any OpenAI-compatible endpoint). The key stays sealed in the OS
//!   keyring with its base URL (`ai.rs` invariant 3): core posts to `{stored baseURL}` plus the
//!   fixed `/images/generations` path, never to a URL from the webview. Whether that provider
//!   actually offers image generation is the provider's answer; a refusal is shown as such.
//!
//! **Video** has no supported backend in this build on any tier. The routes say so (on T2 the
//! reason is the missing backend; below T2, the hardware), and nothing pretends otherwise. Which
//! video backend to support is pending owner sign-off.
//!
//! ## Cost (AC3)
//!
//! Every route carries a cost line before anything runs: local is "no charge, runs on this
//! device"; remote names the host that bills the member's key and says the price is set there.
//! When the provider reports usage (tokens), it is kept with the gallery item and shown.
//!
//! ## Output (granted folders only)
//!
//! A generated file is written only into a live **write folder grant** the member picked
//! (`agent_grants.rs`): never full access, never anywhere else. The grant's stored canonical root
//! must still resolve to itself (a folder swapped for a symlink grants nothing), the name is
//! generated here, the file is created new (never overwriting) without following a symlink. The
//! provider's reply must be base64 image bytes of a known type (PNG, JPEG, WebP); a link is
//! refused, so the app never fetches a URL a provider hands back.
//!
//! ## Gallery and pop-out (AC2)
//!
//! Each saved image is recorded in `<app data>/media/gallery.json` (owner-only, newest first,
//! capped). The Media pop-out (no app commands of its own) asks the main window over its typed
//! bridge; the main window reads a gallery image back as a data URL only after re-checking it is
//! still a regular file of a known image type.
//!
//! Keyless: nothing here signs or holds a wallet key (Rule 3).

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_grants::{Access, GrantKind, GrantState};

// Conservative defaults, pending owner sign-off.
/// Longest prompt, in characters.
pub const MAX_PROMPT_CHARS: usize = 4_000;
/// Largest image accepted from a backend, in bytes.
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;
/// Most items the gallery keeps.
pub const MAX_GALLERY: usize = 300;
/// The sizes the generator offers.
pub const SIZES: [&str; 4] = ["512x512", "1024x1024", "1024x1536", "1536x1024"];
pub const GALLERY_FILE: &str = "gallery.json";
const SETTINGS_FILE: &str = "media.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
}

impl MediaKind {
    fn as_str(self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Video => "video",
        }
    }
}

/// What a tier allows locally (02_ARCHITECTURE section 3: T0 registry/endpoint only, T1 small
/// image locally, T2 image and short video).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierCaps {
    pub local_image: bool,
    pub local_video: bool,
}

pub fn tier_caps(tier: Option<&str>) -> TierCaps {
    match tier {
        Some("T1") => TierCaps {
            local_image: true,
            local_video: false,
        },
        Some("T2") => TierCaps {
            local_image: true,
            local_video: true,
        },
        _ => TierCaps {
            local_image: false,
            local_video: false,
        },
    }
}

/// The member's media settings (non-secret; `<app config>/media.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaSettings {
    /// A loopback `http` base URL of a local OpenAI-images-compatible server.
    #[serde(default)]
    pub local_url: Option<String>,
    #[serde(default)]
    pub local_model: Option<String>,
    /// The AI provider id (`openai`, `gateway`, `custom`) used for remote images.
    #[serde(default)]
    pub remote_provider: Option<String>,
    /// The image model name at that provider.
    #[serde(default)]
    pub remote_model: Option<String>,
}

/// A configured remote provider (non-secret facts from `ai.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteProvider {
    pub id: String,
    pub base_url: String,
}

/// One way to make media, with its availability and cost line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    /// `local` or `remote`.
    pub id: &'static str,
    pub kind: MediaKind,
    pub available: bool,
    /// Why it is not available (member-facing). `None` when available.
    pub reason: Option<String>,
    /// Where the prompt goes.
    pub destination: String,
    /// What it costs, as far as the app can know.
    pub cost: String,
    pub model: String,
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_string(),
            })
        })
        .unwrap_or_else(|| "unknown host".into())
}

/// A local backend must be a loopback `http` URL (the AI-provider loopback guard).
pub fn validate_local_url(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    if !crate::ai::loopback_url_is_safe(url) {
        return Err(
            "the local image server must be an http address on this machine (127.0.0.1, localhost or ::1)"
                .into(),
        );
    }
    Ok(url.to_string())
}

/// The image routes for `tier`, given the settings and the configured remote provider (if the
/// chosen one is configured).
pub fn image_routes(
    tier: Option<&str>,
    s: &MediaSettings,
    remote: Option<&RemoteProvider>,
) -> Vec<Route> {
    let caps = tier_caps(tier);
    let local_url = s
        .local_url
        .as_deref()
        .and_then(|u| validate_local_url(u).ok());
    let local_model = s.local_model.clone().unwrap_or_default();
    let local = match (&local_url, caps.local_image) {
        (_, false) => Route {
            id: "local",
            kind: MediaKind::Image,
            available: false,
            reason: Some(format!(
                "Local image generation is not available on this device ({} tier). Use a provider instead.",
                tier.unwrap_or("unknown")
            )),
            destination: "this device".into(),
            cost: "No charge: runs on this device".into(),
            model: local_model,
        },
        (None, true) => Route {
            id: "local",
            kind: MediaKind::Image,
            available: false,
            reason: Some(
                "There is no local image backend set up. Add the address of an image server running on this machine in the Media settings."
                    .into(),
            ),
            destination: "this device".into(),
            cost: "No charge: runs on this device".into(),
            model: local_model,
        },
        (Some(u), true) => Route {
            id: "local",
            kind: MediaKind::Image,
            available: !local_model.is_empty(),
            reason: local_model
                .is_empty()
                .then(|| "Name the model the local image server should use.".to_string()),
            destination: format!("this device ({})", host_of(u)),
            cost: "No charge: runs on this device".into(),
            model: local_model,
        },
    };
    let remote_model = s.remote_model.clone().unwrap_or_default();
    let chosen = s.remote_provider.as_deref();
    let remote_route = match (chosen, remote) {
        (Some(id), Some(p)) if p.id == id => {
            let host = host_of(&p.base_url);
            Route {
                id: "remote",
                kind: MediaKind::Image,
                available: !remote_model.is_empty(),
                reason: remote_model
                    .is_empty()
                    .then(|| "Name the image model to ask the provider for.".to_string()),
                cost: format!(
                    "Billed by {host} to your key; the price is set by the provider, not by Citrate"
                ),
                destination: host,
                model: remote_model,
            }
        }
        (Some(id), _) => Route {
            id: "remote",
            kind: MediaKind::Image,
            available: false,
            reason: Some(format!(
                "The provider \"{id}\" has no key configured. Add one in Settings > AI providers."
            )),
            destination: "a provider".into(),
            cost: "Billed by the provider to your key".into(),
            model: remote_model,
        },
        (None, _) => Route {
            id: "remote",
            kind: MediaKind::Image,
            available: false,
            reason: Some(
                "There is no image provider chosen. Pick one of your AI providers in the Media settings."
                    .into(),
            ),
            destination: "a provider".into(),
            cost: "Billed by the provider to your key".into(),
            model: remote_model,
        },
    };
    vec![local, remote_route]
}

/// Video routes: no backend is supported in this build (pending owner sign-off on which one).
pub fn video_routes(tier: Option<&str>) -> Vec<Route> {
    let caps = tier_caps(tier);
    let reason = if caps.local_video {
        "There is no video backend in this build yet, so video cannot be generated."
    } else {
        "Video generation is not available on this device, and there is no video backend in this build yet."
    };
    vec![Route {
        id: "local",
        kind: MediaKind::Video,
        available: false,
        reason: Some(reason.to_string()),
        destination: "this device".into(),
        cost: "No charge: would run on this device".into(),
        model: String::new(),
    }]
}

/// The OpenAI images request body.
pub fn image_request(prompt: &str, size: &str, model: &str) -> Result<Value, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("describe the image first".into());
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(format!(
            "the description is longer than {MAX_PROMPT_CHARS} characters"
        ));
    }
    if !SIZES.contains(&size) {
        return Err(format!("the size must be one of {}", SIZES.join(", ")));
    }
    let model = model.trim();
    if model.is_empty() || model.len() > 128 || model.chars().any(char::is_control) {
        return Err("name the image model".into());
    }
    let mut b = json!({ "model": model, "prompt": prompt, "n": 1, "size": size });
    // gpt-image models always answer with base64 and reject this field; others need it.
    if !model.starts_with("gpt-image") {
        b["response_format"] = json!("b64_json");
    }
    Ok(b)
}

/// Sniff the image type from its first bytes: `(mime, ext)`.
pub fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("image/jpeg", "jpg"))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub ext: &'static str,
    pub usage: Option<Value>,
}

/// Parse an images reply: `data[0].b64_json` must decode to a known image type.
pub fn parse_image_response(body: &str) -> Result<GeneratedImage, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|_| "the image server's reply could not be read")?;
    let first = v
        .get("data")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or("the image server returned no image")?;
    let b64 = match first.get("b64_json").and_then(Value::as_str) {
        Some(s) => s,
        None if first.get("url").is_some() => return Err(
            "the image server returned a link instead of the image; this app does not fetch links"
                .into(),
        ),
        None => return Err("the image server returned no image".into()),
    };
    if b64.len() > MAX_IMAGE_BYTES / 3 * 4 + 4 {
        return Err("the image is larger than this app accepts".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| "the image data could not be decoded")?;
    let (mime, ext) = sniff(&bytes).ok_or("the reply is not a PNG, JPEG or WebP image")?;
    let usage = v.get("usage").filter(|u| u.is_object()).cloned();
    Ok(GeneratedImage {
        bytes,
        mime,
        ext,
        usage,
    })
}

// ---------------------------------------------------------------------------
// Granted output folders
// ---------------------------------------------------------------------------

/// A folder the member granted Hermes write access to: a place generated files may go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub grant_id: String,
    pub root: String,
}

fn live_write_folder(g: &crate::agent_grants::Grant, now: u64) -> bool {
    g.kind == GrantKind::Folder
        && g.access == Access::Write
        && g.revoked_at.is_none()
        && g.granted_at <= now
        && g.expires_at.is_none_or(|t| now < t)
}

/// The live write folder grants.
pub fn write_targets(st: &GrantState, now: u64) -> Vec<Target> {
    st.grants
        .iter()
        .filter(|g| live_write_folder(g, now))
        .map(|g| Target {
            grant_id: g.id.clone(),
            root: g.root.clone(),
        })
        .collect()
}

/// The folder of a live write folder grant, re-checked on disk: its stored canonical root must
/// still canonicalize to itself.
pub fn target_root(st: &GrantState, grant_id: &str, now: u64) -> Result<PathBuf, String> {
    let g = st
        .grants
        .iter()
        .find(|g| g.id == grant_id && live_write_folder(g, now))
        .ok_or("that folder is not granted for writing (grant one in Settings > App > Hermes folder access)")?;
    let stored = PathBuf::from(&g.root);
    let canon = std::fs::canonicalize(&stored)
        .map_err(|_| format!("the granted folder {} is missing", g.root))?;
    if canon != stored || !canon.is_dir() {
        return Err(format!(
            "the granted folder {} has moved or was replaced; grant it again",
            g.root
        ));
    }
    Ok(canon)
}

/// A fresh, safe file name: `citrate-<kind>-YYYYMMDD-HHMMSS-<random>.<ext>`.
pub fn output_name(kind: MediaKind, now: u64, ext: &str) -> String {
    let stamp = crate::google_workspace::rfc3339(now);
    let compact: String = stamp
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .collect::<String>()
        .replace('T', "-");
    format!(
        "citrate-{}-{compact}-{}.{ext}",
        kind.as_str(),
        random_hex(4)
    )
}

fn random_hex(n: usize) -> String {
    use rand::RngCore;
    let mut r = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut r);
    r.iter().map(|b| format!("{b:02x}")).collect()
}

/// Create `root/name` new (never overwriting, never following a symlink) and write `bytes`.
pub fn write_new_file(root: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err("bad file name".into());
    }
    let path = root.join(name);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o644).custom_flags(libc::O_NOFOLLOW);
    }
    let mut f = o
        .open(&path)
        .map_err(|e| format!("could not create {}: {}", path.display(), e.kind()))?;
    use std::io::Write;
    f.write_all(bytes)
        .and_then(|_| f.flush())
        .map_err(|e| format!("could not write {}: {}", path.display(), e.kind()))?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Gallery
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GalleryItem {
    pub id: String,
    pub kind: MediaKind,
    pub path: String,
    pub grant_id: String,
    pub prompt: String,
    pub route: String,
    pub destination: String,
    pub model: String,
    pub created_at: u64,
    pub bytes: u64,
    pub mime: String,
    pub cost: String,
    #[serde(default)]
    pub usage: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GalleryDoc {
    version: u32,
    items: Vec<GalleryItem>,
}

pub struct GalleryStore {
    dir: PathBuf,
}

impl GalleryStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        GalleryStore { dir: dir.into() }
    }

    fn file(&self) -> PathBuf {
        self.dir.join(GALLERY_FILE)
    }

    fn load_doc(&self) -> Result<GalleryDoc, String> {
        match std::fs::read_to_string(self.file()) {
            Ok(t) => serde_json::from_str::<GalleryDoc>(&t)
                .ok()
                .filter(|d| d.version == 1)
                .ok_or_else(|| "the media gallery list could not be read".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GalleryDoc {
                version: 1,
                items: Vec::new(),
            }),
            Err(e) => Err(format!(
                "the media gallery list could not be read: {}",
                e.kind()
            )),
        }
    }

    /// Newest first.
    pub fn items(&self) -> Result<Vec<GalleryItem>, String> {
        self.load_doc().map(|d| d.items)
    }

    pub fn find_item(&self, id: &str) -> Result<Option<GalleryItem>, String> {
        Ok(self.items()?.into_iter().find(|i| i.id == id))
    }

    /// Add an item at the front; the oldest drop off past [`MAX_GALLERY`] (their files stay).
    pub fn record_item(&self, item: GalleryItem) -> Result<(), String> {
        let mut doc = match self.load_doc() {
            Ok(d) => d,
            // A damaged list is replaced only when a new item is saved (the files themselves are
            // in the member's folders and are never touched).
            Err(_) => GalleryDoc {
                version: 1,
                items: Vec::new(),
            },
        };
        doc.items.insert(0, item);
        doc.items.truncate(MAX_GALLERY);
        let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(format!("{GALLERY_FILE}.tmp"));
        citrate_core_kit::fsutil::write_secret_file(&tmp, text.as_bytes())
            .map_err(|e| e.kind().to_string())?;
        std::fs::rename(&tmp, self.file()).map_err(|e| e.kind().to_string())
    }
}

/// Read a gallery file back for display: a regular file (not a symlink), within the size cap,
/// whose bytes are still a known image type.
pub fn data_url_for(path: &Path) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "the file is no longer there")?;
    if !meta.is_file() {
        return Err("the file is no longer a regular file".into());
    }
    if meta.len() > MAX_IMAGE_BYTES as u64 {
        return Err("the file is larger than this app shows".into());
    }
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read the file: {}", e.kind()))?;
    let (mime, _) = sniff(&bytes).ok_or("the file is no longer a PNG, JPEG or WebP image")?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// POST to the member's local image server (loopback only, no key). Image generation on a small
/// machine is slow, so the deadline is generous but bounded.
fn post_local(url: &str, body: &Value) -> Result<String, String> {
    if !crate::ai::loopback_url_is_safe(url) {
        return Err("the local image server must be on this machine".into());
    }
    let mut resp = ureq::post(url)
        .config()
        .timeout_connect(Some(std::time::Duration::from_secs(5)))
        .timeout_global(Some(std::time::Duration::from_secs(600)))
        .build()
        .header("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| match e {
            ureq::Error::StatusCode(c) => format!("the local image server answered HTTP {c}"),
            _ => "could not reach the local image server; is it running?".to_string(),
        })?;
    resp.body_mut()
        .with_config()
        .limit(MAX_IMAGE_BYTES as u64 * 2)
        .read_to_string()
        .map_err(|_| "the local image server's reply could not be read".to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join(SETTINGS_FILE))
}

fn load_settings(app: &tauri::AppHandle) -> MediaSettings {
    settings_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn gallery(app: &tauri::AppHandle) -> Result<GalleryStore, String> {
    use tauri::Manager;
    Ok(GalleryStore::new(
        app.path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("media"),
    ))
}

fn grant_state(app: &tauri::AppHandle) -> Result<GrantState, String> {
    match crate::agent_grants::GrantStore::for_app(app)?.load() {
        crate::agent_grants::Loaded::Ok(s) => Ok(s),
        crate::agent_grants::Loaded::Corrupted(_) => Err(
            "the saved folder grants could not be read, so nothing can be saved; reset them in Settings"
                .into(),
        ),
    }
}

fn effective_tier(app: &tauri::AppHandle) -> Option<String> {
    crate::tier::tier_recommend_sync(app.clone())
        .ok()
        .map(|r| r.effective.id().to_string())
}

fn remote_provider(app: &tauri::AppHandle, s: &MediaSettings) -> Option<RemoteProvider> {
    let id = s.remote_provider.as_deref()?;
    let ai = tauri::Manager::try_state::<crate::ai::AiState>(app)?;
    let base_url = ai.0.provider_base_url(id).ok()?;
    Some(RemoteProvider {
        id: id.to_string(),
        base_url,
    })
}

/// What the Media view shows before anything runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaOptions {
    pub tier: Option<String>,
    pub caps: TierCaps,
    pub settings: MediaSettings,
    pub image: Vec<Route>,
    pub video: Vec<Route>,
    pub sizes: Vec<&'static str>,
    pub targets: Vec<Target>,
    /// Set when the grants could not be read.
    pub targets_error: Option<String>,
}

/// **media_options** — routes with availability and cost, sizes, and where files may be saved.
#[tauri::command]
pub async fn media_options(app: tauri::AppHandle) -> Result<MediaOptions, String> {
    crate::blocking::off_main(move || {
        let s = load_settings(&app);
        let tier = effective_tier(&app);
        let remote = remote_provider(&app, &s);
        let (targets, targets_error) = match grant_state(&app) {
            Ok(st) => (write_targets(&st, now_secs()), None),
            Err(e) => (Vec::new(), Some(e)),
        };
        Ok(MediaOptions {
            caps: tier_caps(tier.as_deref()),
            image: image_routes(tier.as_deref(), &s, remote.as_ref()),
            video: video_routes(tier.as_deref()),
            sizes: SIZES.to_vec(),
            tier,
            settings: s,
            targets,
            targets_error,
        })
    })
    .await
}

/// **media_set_settings** — save the local backend address/model and the remote provider/model.
#[tauri::command]
pub async fn media_set_settings(
    app: tauri::AppHandle,
    settings: MediaSettings,
) -> Result<MediaSettings, String> {
    crate::blocking::off_main(move || {
        let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let local_url = clean(settings.local_url)
            .map(|u| validate_local_url(&u))
            .transpose()?;
        let remote_provider = clean(settings.remote_provider);
        if let Some(p) = &remote_provider {
            if !["openai", "gateway", "custom"].contains(&p.as_str()) {
                return Err("pick one of your AI providers".into());
            }
        }
        let s = MediaSettings {
            local_url,
            local_model: clean(settings.local_model),
            remote_provider,
            remote_model: clean(settings.remote_model),
        };
        let path = settings_path(&app)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.kind().to_string())?;
        }
        let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.kind().to_string())?;
        Ok(s)
    })
    .await
}

/// **media_generate_image** — make one image on `route` and save it into the write folder grant
/// `grant_id`. Returns the gallery item.
#[tauri::command]
pub async fn media_generate_image(
    app: tauri::AppHandle,
    route: String,
    prompt: String,
    size: String,
    grant_id: String,
) -> Result<GalleryItem, String> {
    crate::blocking::off_main(move || {
        let s = load_settings(&app);
        let tier = effective_tier(&app);
        let remote = remote_provider(&app, &s);
        let routes = image_routes(tier.as_deref(), &s, remote.as_ref());
        let r = routes
            .into_iter()
            .find(|r| r.id == route)
            .ok_or("unknown route")?;
        if !r.available {
            return Err(r
                .reason
                .unwrap_or_else(|| "that route is not available".into()));
        }
        // Check the destination before spending anything.
        let st = grant_state(&app)?;
        let root = target_root(&st, &grant_id, now_secs())?;
        let body = image_request(&prompt, &size, &r.model)?;
        let reply = match r.id {
            "local" => {
                let base = s
                    .local_url
                    .as_deref()
                    .map(validate_local_url)
                    .transpose()?
                    .ok_or("no local image backend is set up")?;
                post_local(&format!("{base}/images/generations"), &body)?
            }
            _ => {
                let id = s
                    .remote_provider
                    .as_deref()
                    .ok_or("no image provider chosen")?;
                let ai = tauri::Manager::try_state::<crate::ai::AiState>(&app)
                    .ok_or("internal: managed state unavailable")?;
                ai.0.post_to_provider(id, "/images/generations", &body)
                    .map_err(|e| e.to_string())?
            }
        };
        let img = parse_image_response(&reply)?;
        let now = now_secs();
        let path = write_new_file(
            &root,
            &output_name(MediaKind::Image, now, img.ext),
            &img.bytes,
        )?;
        let item = GalleryItem {
            id: format!("m{now}-{}", random_hex(4)),
            kind: MediaKind::Image,
            path: path.display().to_string(),
            grant_id,
            prompt: prompt.trim().to_string(),
            route: r.id.to_string(),
            destination: r.destination,
            model: r.model,
            created_at: now,
            bytes: img.bytes.len() as u64,
            mime: img.mime.to_string(),
            cost: r.cost,
            usage: img.usage,
        };
        gallery(&app)?.record_item(item.clone())?;
        Ok(item)
    })
    .await
}

/// **media_gallery** — saved items, newest first, each marked whether its file is still there.
#[tauri::command]
pub async fn media_gallery(app: tauri::AppHandle) -> Result<Vec<GalleryEntry>, String> {
    crate::blocking::off_main(move || {
        Ok(gallery(&app)?
            .items()?
            .into_iter()
            .map(|item| GalleryEntry {
                present: Path::new(&item.path).is_file(),
                item,
            })
            .collect())
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryEntry {
    #[serde(flatten)]
    pub item: GalleryItem,
    pub present: bool,
}

/// **media_read** — a gallery image as a data URL, for the main window and the Media pop-out.
#[tauri::command]
pub async fn media_read(app: tauri::AppHandle, id: String) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let item = gallery(&app)?
            .find_item(&id)?
            .ok_or("that item is not in the gallery")?;
        data_url_for(Path::new(&item.path))
    })
    .await
}

/// **media_save_copy** — save a copy of a gallery image into another write folder grant.
#[tauri::command]
pub async fn media_save_copy(
    app: tauri::AppHandle,
    id: String,
    grant_id: String,
) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let item = gallery(&app)?
            .find_item(&id)?
            .ok_or("that item is not in the gallery")?;
        let st = grant_state(&app)?;
        let root = target_root(&st, &grant_id, now_secs())?;
        let src = Path::new(&item.path);
        let meta = std::fs::symlink_metadata(src).map_err(|_| "the file is no longer there")?;
        if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES as u64 {
            return Err("the file is no longer a regular image file".into());
        }
        let bytes = std::fs::read(src).map_err(|e| e.kind().to_string())?;
        let (_, ext) = sniff(&bytes).ok_or("the file is no longer a PNG, JPEG or WebP image")?;
        let path = write_new_file(&root, &output_name(item.kind, now_secs(), ext), &bytes)?;
        Ok(path.display().to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("media_tests.rs");
}

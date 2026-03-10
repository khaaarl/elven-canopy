// Elfcyclopedia HTTP server — embedded localhost web server for game information.
//
// Serves HTML pages in the player's default browser, providing a species
// bestiary, procedurally generated fruit species catalog, and civilization
// diplomacy overview. The server runs on a background thread with read-only
// access to a shared data snapshot, updated periodically from the main thread.
//
// Architecture:
// - `ElfcyclopediaServer` manages the lifecycle: start, stop, URL reporting.
// - The server thread holds an `Arc<RwLock<ElfcyclopediaData>>` snapshot.
// - The main thread calls `update_data()` during `frame_update()` to push
//   fresh snapshots (species data is static; civ and fruit data refresh
//   each frame).
// - `tiny_http` handles HTTP on `127.0.0.1:PORT` (localhost only).
// - Pages are server-rendered HTML with no JavaScript dependencies.
//
// The server is strictly read-only — it never mutates sim state, so it has
// no impact on determinism.
//
// Species data is embedded at compile time via `include_str!` (same pattern
// as `elven_canopy_lang`'s lexicon), so there are no runtime file path issues.
//
// See also: `sim_bridge.rs` which manages the global `ElfcyclopediaServer`
// static and calls `update_data()`. The design doc is at
// `docs/drafts/elfcyclopedia_civs.md` §Elfcyclopedia (Web-Based).

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Static species entry loaded from `data/species_elfcyclopedia.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct SpeciesEntry {
    pub name: String,
    pub sapient: bool,
    pub description: String,
    pub traits: Vec<String>,
}

/// A known civilization entry for the elfcyclopedia.
#[derive(Clone, Debug)]
pub struct KnownCivEntry {
    pub civ_id: u16,
    pub name: String,
    pub primary_species: String,
    pub culture_tag: String,
    pub our_opinion: String,
    pub their_opinion: Option<String>,
}

/// A fruit species entry for the elfcyclopedia.
#[derive(Clone, Debug)]
pub struct FruitEntry {
    pub id: u16,
    pub vaelith_name: String,
    pub english_gloss: String,
    pub habitat: String,
    pub rarity: String,
    pub shape: String,
    pub color_hex: String,
    pub glows: bool,
    pub size_percent: u16,
    pub greenhouse_cultivable: bool,
    /// Each part: (type_name, properties, pigment, yield_percent)
    pub parts: Vec<FruitPartEntry>,
}

/// A single part within a fruit species entry.
#[derive(Clone, Debug)]
pub struct FruitPartEntry {
    pub part_type: String,
    pub properties: Vec<String>,
    pub pigment: Option<String>,
    pub yield_percent: u8,
}

/// Shared data snapshot read by the HTTP server thread. Updated by the main
/// thread via `update_data()`.
#[derive(Default)]
pub struct ElfcyclopediaData {
    pub species: Vec<SpeciesEntry>,
    pub game_name: String,
    pub current_tick: u64,
    /// Known civilizations (from the player civ's perspective).
    pub civilizations: Vec<KnownCivEntry>,
    /// Name of the player's own civilization.
    pub player_civ_name: String,
    /// All fruit species in the world (not knowledge-gated).
    pub fruits: Vec<FruitEntry>,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// Manages the elfcyclopedia HTTP server lifecycle.
pub struct ElfcyclopediaServer {
    /// The port the server is actually listening on (after fallback).
    port: u16,
    /// Shared data snapshot, readable by the HTTP thread.
    data: Arc<RwLock<ElfcyclopediaData>>,
    /// Signal to tell the server thread to shut down.
    shutdown: Arc<AtomicBool>,
    /// Handle to the server thread (for join on stop).
    thread_handle: Option<thread::JoinHandle<()>>,
}

const DEFAULT_PORT: u16 = 7777;
const MAX_PORT_ATTEMPTS: u16 = 20;

impl ElfcyclopediaServer {
    /// Create and start the elfcyclopedia server. Tries `DEFAULT_PORT` first,
    /// then increments up to `MAX_PORT_ATTEMPTS` times if the port is taken.
    /// Returns `None` if no port could be bound.
    pub fn start(species: Vec<SpeciesEntry>) -> Option<Self> {
        let data = Arc::new(RwLock::new(ElfcyclopediaData {
            species,
            ..Default::default()
        }));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Try binding to successive ports.
        let mut server = None;
        let mut bound_port = DEFAULT_PORT;
        for offset in 0..MAX_PORT_ATTEMPTS {
            let port = DEFAULT_PORT + offset;
            let addr = format!("127.0.0.1:{port}");
            match tiny_http::Server::http(&addr) {
                Ok(s) => {
                    bound_port = port;
                    server = Some(s);
                    break;
                }
                Err(_) => continue,
            }
        }

        let server = server?;

        let data_clone = Arc::clone(&data);
        let shutdown_clone = Arc::clone(&shutdown);

        let thread_handle = thread::spawn(move || {
            run_server(server, data_clone, shutdown_clone);
        });

        Some(Self {
            port: bound_port,
            data,
            shutdown,
            thread_handle: Some(thread_handle),
        })
    }

    /// The URL the elfcyclopedia is accessible at.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Update the shared data snapshot. Called from the main thread during
    /// `frame_update()`.
    pub fn update_data(
        &self,
        tick: u64,
        game_name: &str,
        civilizations: Vec<KnownCivEntry>,
        player_civ_name: String,
        fruits: Vec<FruitEntry>,
    ) {
        if let Ok(mut data) = self.data.write() {
            data.current_tick = tick;
            data.game_name = game_name.to_owned();
            data.civilizations = civilizations;
            data.player_civ_name = player_civ_name;
            data.fruits = fruits;
        }
    }

    /// Shut down the server. Signals the thread and waits for it to exit.
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            // The server thread checks `shutdown` on each request timeout,
            // so it will exit within a few hundred milliseconds.
            let _ = handle.join();
        }
    }
}

impl Drop for ElfcyclopediaServer {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// Server thread
// ---------------------------------------------------------------------------

/// Main loop for the HTTP server thread. Processes requests until shutdown.
fn run_server(
    server: tiny_http::Server,
    data: Arc<RwLock<ElfcyclopediaData>>,
    shutdown: Arc<AtomicBool>,
) {
    // Use a short recv timeout so we can check the shutdown flag periodically.
    let timeout = std::time::Duration::from_millis(200);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // recv() with timeout so we don't block forever on shutdown.
        let request = match server.recv_timeout(timeout) {
            Ok(Some(req)) => req,
            Ok(None) => continue, // Timeout, loop to check shutdown.
            Err(_) => break,      // Server error, exit.
        };

        let response = handle_request(&request, &data);
        let _ = request.respond(response);
    }
}

/// Route a request to the appropriate handler and return an HTTP response.
fn handle_request(
    request: &tiny_http::Request,
    data: &Arc<RwLock<ElfcyclopediaData>>,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let path = request.url().to_owned();
    let data = match data.read() {
        Ok(d) => d,
        Err(_) => return error_response(500, "Internal server error"),
    };

    let (status, content_type, body) = match path.as_str() {
        "/" => (200, "text/html", render_index(&data)),
        "/species" => (200, "text/html", render_species_list(&data)),
        p if p.starts_with("/species/") => {
            let name = &p["/species/".len()..];
            let decoded = percent_decode(name);
            match data.species.iter().find(|s| s.name == decoded) {
                Some(entry) => (200, "text/html", render_species_detail(entry, &data)),
                None => (404, "text/html", render_not_found(&data)),
            }
        }
        "/fruits" => (200, "text/html", render_fruits_list(&data)),
        p if p.starts_with("/fruits/") => {
            let id_str = &p["/fruits/".len()..];
            match id_str.parse::<u16>() {
                Ok(id) => match data.fruits.iter().find(|f| f.id == id) {
                    Some(entry) => (200, "text/html", render_fruit_detail(entry, &data)),
                    None => (404, "text/html", render_not_found(&data)),
                },
                Err(_) => (404, "text/html", render_not_found(&data)),
            }
        }
        "/civilizations" => (200, "text/html", render_civilizations_list(&data)),
        p if p.starts_with("/civilizations/") => {
            let id_str = &p["/civilizations/".len()..];
            match id_str.parse::<u16>() {
                Ok(id) => match data.civilizations.iter().find(|c| c.civ_id == id) {
                    Some(entry) => (200, "text/html", render_civilizations_detail(entry, &data)),
                    None => (404, "text/html", render_not_found(&data)),
                },
                Err(_) => (404, "text/html", render_not_found(&data)),
            }
        }
        "/style.css" => (200, "text/css", render_css()),
        _ => (404, "text/html", render_not_found(&data)),
    };

    let header = tiny_http::Header::from_bytes("Content-Type", content_type).expect("valid header");
    tiny_http::Response::from_data(body.into_bytes())
        .with_status_code(status)
        .with_header(header)
}

fn error_response(code: u16, msg: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let header = tiny_http::Header::from_bytes("Content-Type", "text/plain").expect("valid header");
    tiny_http::Response::from_data(msg.as_bytes().to_vec())
        .with_status_code(code)
        .with_header(header)
}

/// Decode percent-encoded URL segments (e.g., %20 → space).
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(s) = std::str::from_utf8(&hex)
                && let Ok(val) = u8::from_str_radix(s, 16)
            {
                result.push(val as char);
                continue;
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else {
            result.push(b as char);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// HTML rendering
// ---------------------------------------------------------------------------

fn html_page(title: &str, body: &str, data: &ElfcyclopediaData) -> String {
    let tick_info = if data.current_tick > 0 {
        let secs = data.current_tick / 1000;
        let mins = secs / 60;
        format!(
            "<footer>Tick {} ({:02}:{:02}) &middot; {}</footer>",
            data.current_tick,
            mins,
            secs % 60,
            html_escape(&data.game_name),
        )
    } else {
        "<footer>No game loaded</footer>".to_owned()
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — Elven Canopy Elfcyclopedia</title>
<link rel="stylesheet" href="/style.css">
</head>
<body>
<nav><a href="/">Home</a> · <a href="/species">Species</a> · <a href="/fruits">Fruits</a> · <a href="/civilizations">Civilizations</a></nav>
<main>
<h1>{title}</h1>
{body}
</main>
{tick_info}
</body>
</html>"#,
        title = html_escape(title),
        body = body,
        tick_info = tick_info,
    )
}

fn render_index(data: &ElfcyclopediaData) -> String {
    let civ_info = if !data.player_civ_name.is_empty() {
        format!(
            "<p>Knowledge held by the <strong>{}</strong> civilization.</p>",
            html_escape(&data.player_civ_name),
        )
    } else {
        "<p>Welcome to the Elven Canopy Elfcyclopedia.</p>".to_owned()
    };

    let body = format!(
        r#"{civ_info}
<h2>Sections</h2>
<ul>
<li><a href="/species">Species Bestiary</a> — all known creature types</li>
<li><a href="/fruits">Fruits</a> — procedurally generated fruit species</li>
<li><a href="/civilizations">Civilizations</a> — known civilizations and diplomacy</li>
</ul>"#,
    );
    html_page("Elfcyclopedia", &body, data)
}

fn render_species_list(data: &ElfcyclopediaData) -> String {
    let mut body = String::from("<p>All known species in the world.</p>");

    // Sapient species first, then animals.
    body.push_str("<h2>Sapient Species</h2><ul>");
    for s in &data.species {
        if s.sapient {
            body.push_str(&format!(
                r#"<li><a href="/species/{}">{}</a></li>"#,
                html_escape(&s.name),
                html_escape(&s.name),
            ));
        }
    }
    body.push_str("</ul>");

    body.push_str("<h2>Wildlife</h2><ul>");
    for s in &data.species {
        if !s.sapient {
            body.push_str(&format!(
                r#"<li><a href="/species/{}">{}</a></li>"#,
                html_escape(&s.name),
                html_escape(&s.name),
            ));
        }
    }
    body.push_str("</ul>");

    html_page("Species Bestiary", &body, data)
}

fn render_species_detail(entry: &SpeciesEntry, data: &ElfcyclopediaData) -> String {
    let kind = if entry.sapient { "Sapient" } else { "Wildlife" };
    let traits_html: Vec<String> = entry
        .traits
        .iter()
        .map(|t| format!("<span class=\"trait\">{}</span>", html_escape(t)))
        .collect();

    let body = format!(
        r#"<p class="species-kind">{kind}</p>
<p>{description}</p>
<h2>Traits</h2>
<div class="traits">{traits}</div>
<p><a href="/species">&larr; Back to species list</a></p>"#,
        kind = kind,
        description = html_escape(&entry.description),
        traits = traits_html.join(" "),
    );

    html_page(&entry.name, &body, data)
}

fn render_civilizations_list(data: &ElfcyclopediaData) -> String {
    let mut body = String::new();

    if data.civilizations.is_empty() {
        body.push_str("<p>No other civilizations are known.</p>");
    } else {
        body.push_str("<p>Civilizations your people know of.</p>");
        body.push_str(
            "<table class=\"civ-table\"><thead><tr>\
             <th>Name</th><th>Species</th><th>Culture</th>\
             <th>Our Opinion</th><th>Their Opinion</th>\
             </tr></thead><tbody>",
        );
        for civ in &data.civilizations {
            let their = civ.their_opinion.as_deref().unwrap_or("Unknown");
            body.push_str(&format!(
                "<tr><td><a href=\"/civilizations/{}\">{}</a></td>\
                 <td>{}</td><td>{}</td>\
                 <td class=\"opinion-{}\">{}</td>\
                 <td class=\"opinion-{}\">{}</td></tr>",
                civ.civ_id,
                html_escape(&civ.name),
                html_escape(&civ.primary_species),
                html_escape(&civ.culture_tag),
                opinion_css_class(&civ.our_opinion),
                html_escape(&civ.our_opinion),
                opinion_css_class(their),
                html_escape(their),
            ));
        }
        body.push_str("</tbody></table>");
    }

    html_page("Known Civilizations", &body, data)
}

fn render_civilizations_detail(entry: &KnownCivEntry, data: &ElfcyclopediaData) -> String {
    let their = entry.their_opinion.as_deref().unwrap_or("Unknown");

    let body = format!(
        r#"<dl>
<dt>Species</dt><dd>{species}</dd>
<dt>Culture</dt><dd>{culture}</dd>
<dt>Our Opinion</dt><dd class="opinion-{our_class}">{our_opinion}</dd>
<dt>Their Opinion</dt><dd class="opinion-{their_class}">{their_opinion}</dd>
</dl>
<p><a href="/civilizations">&larr; Back to civilizations</a></p>"#,
        species = html_escape(&entry.primary_species),
        culture = html_escape(&entry.culture_tag),
        our_class = opinion_css_class(&entry.our_opinion),
        our_opinion = html_escape(&entry.our_opinion),
        their_class = opinion_css_class(their),
        their_opinion = html_escape(their),
    );

    html_page(&entry.name, &body, data)
}

fn render_fruits_list(data: &ElfcyclopediaData) -> String {
    let mut body = String::new();

    if data.fruits.is_empty() {
        body.push_str("<p>No fruit species have been generated in this world yet.</p>");
    } else {
        body.push_str(&format!(
            "<p>{} fruit species grow in this world.</p>",
            data.fruits.len(),
        ));

        // Group by rarity.
        for rarity in &["Common", "Uncommon", "Rare"] {
            let fruits: Vec<_> = data.fruits.iter().filter(|f| f.rarity == *rarity).collect();
            if fruits.is_empty() {
                continue;
            }
            body.push_str(&format!("<h2>{rarity}</h2>"));
            body.push_str("<table class=\"fruit-table\"><thead><tr>");
            body.push_str(
                "<th>Name</th><th>Gloss</th><th>Habitat</th>\
                 <th>Shape</th><th>Color</th></tr></thead><tbody>",
            );
            for f in &fruits {
                let color_swatch = format!(
                    "<span class=\"color-swatch\" style=\"background:{};\"></span>",
                    html_escape(&f.color_hex),
                );
                let glow = if f.glows { " &#x2728;" } else { "" };
                body.push_str(&format!(
                    "<tr><td><a href=\"/fruits/{}\">{}</a></td>\
                     <td>{}</td><td>{}</td><td>{}</td>\
                     <td>{}{}</td></tr>",
                    f.id,
                    html_escape(&f.vaelith_name),
                    html_escape(&f.english_gloss),
                    html_escape(&f.habitat),
                    html_escape(&f.shape),
                    color_swatch,
                    glow,
                ));
            }
            body.push_str("</tbody></table>");
        }
    }

    html_page("Fruit Species", &body, data)
}

fn render_fruit_detail(entry: &FruitEntry, data: &ElfcyclopediaData) -> String {
    let color_swatch = format!(
        "<span class=\"color-swatch-lg\" style=\"background:{};\"></span>",
        html_escape(&entry.color_hex),
    );
    let glow_text = if entry.glows { "Yes" } else { "No" };
    let greenhouse_text = if entry.greenhouse_cultivable {
        "Yes"
    } else {
        "No"
    };

    let mut body = format!(
        r#"<p class="species-kind">{rarity} · {habitat}</p>
<p class="fruit-gloss">{gloss}</p>
<dl>
<dt>Shape</dt><dd>{shape}</dd>
<dt>Color</dt><dd>{swatch}</dd>
<dt>Size</dt><dd>{size}%</dd>
<dt>Glows</dt><dd>{glow}</dd>
<dt>Greenhouse Cultivable</dt><dd>{greenhouse}</dd>
</dl>"#,
        rarity = html_escape(&entry.rarity),
        habitat = html_escape(&entry.habitat),
        gloss = html_escape(&entry.english_gloss),
        shape = html_escape(&entry.shape),
        swatch = color_swatch,
        size = entry.size_percent,
        glow = glow_text,
        greenhouse = greenhouse_text,
    );

    // Parts table.
    body.push_str("<h2>Parts</h2>");
    body.push_str(
        "<table class=\"fruit-table\"><thead><tr>\
         <th>Part</th><th>Yield</th><th>Properties</th><th>Pigment</th>\
         </tr></thead><tbody>",
    );
    for part in &entry.parts {
        let props = if part.properties.is_empty() {
            "—".to_owned()
        } else {
            part.properties
                .iter()
                .map(|p| format!("<span class=\"trait\">{}</span>", html_escape(p)))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let pigment = part
            .pigment
            .as_deref()
            .map(html_escape)
            .unwrap_or_else(|| "—".to_owned());
        body.push_str(&format!(
            "<tr><td>{}</td><td>{}%</td><td>{}</td><td>{}</td></tr>",
            html_escape(&part.part_type),
            part.yield_percent,
            props,
            pigment,
        ));
    }
    body.push_str("</tbody></table>");

    body.push_str("<p><a href=\"/fruits\">&larr; Back to fruit species</a></p>");

    html_page(&entry.vaelith_name, &body, data)
}

/// Map opinion text to a CSS class name for color coding.
fn opinion_css_class(opinion: &str) -> &str {
    match opinion {
        "Friendly" => "friendly",
        "Neutral" => "neutral",
        "Suspicious" => "suspicious",
        "Hostile" => "hostile",
        _ => "unknown",
    }
}

fn render_not_found(data: &ElfcyclopediaData) -> String {
    html_page(
        "Not Found",
        "<p>The page you requested does not exist.</p><p><a href=\"/\">&larr; Home</a></p>",
        data,
    )
}

fn render_css() -> String {
    r#"
:root {
    --bg: #1a1a2e;
    --surface: #16213e;
    --text: #e0e0e0;
    --accent: #7ec8a0;
    --link: #8ecae6;
    --link-hover: #b8e0d2;
    --muted: #888;
    --border: #2a2a4a;
}

* { margin: 0; padding: 0; box-sizing: border-box; }

body {
    font-family: Georgia, 'Times New Roman', serif;
    background: var(--bg);
    color: var(--text);
    line-height: 1.6;
    max-width: 48rem;
    margin: 0 auto;
    padding: 1rem 1.5rem;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
}

nav {
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: 1.5rem;
    font-size: 0.9rem;
}

main { flex: 1; }

h1 {
    color: var(--accent);
    font-size: 1.8rem;
    margin-bottom: 1rem;
    font-weight: normal;
}

h2 {
    color: var(--accent);
    font-size: 1.2rem;
    margin: 1.5rem 0 0.5rem 0;
    font-weight: normal;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.25rem;
}

p { margin-bottom: 0.75rem; }

a { color: var(--link); text-decoration: none; }
a:hover { color: var(--link-hover); text-decoration: underline; }

ul { padding-left: 1.5rem; margin-bottom: 1rem; }
li { margin-bottom: 0.25rem; }

.species-kind {
    font-style: italic;
    color: var(--muted);
    margin-bottom: 1rem;
}

.traits { display: flex; flex-wrap: wrap; gap: 0.5rem; }

.trait {
    background: var(--surface);
    border: 1px solid var(--border);
    padding: 0.2rem 0.6rem;
    border-radius: 0.25rem;
    font-size: 0.85rem;
    color: var(--accent);
}

table.civ-table {
    width: 100%;
    border-collapse: collapse;
    margin: 1rem 0;
}

table.civ-table th, table.civ-table td {
    padding: 0.4rem 0.75rem;
    border-bottom: 1px solid var(--border);
    text-align: left;
}

table.civ-table th {
    color: var(--accent);
    font-weight: normal;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
}

dl { margin: 1rem 0; }
dt { color: var(--muted); font-size: 0.85rem; margin-top: 0.75rem; }
dd { margin-left: 0; font-size: 1rem; }

table.fruit-table {
    width: 100%;
    border-collapse: collapse;
    margin: 1rem 0;
}

table.fruit-table th, table.fruit-table td {
    padding: 0.4rem 0.75rem;
    border-bottom: 1px solid var(--border);
    text-align: left;
}

table.fruit-table th {
    color: var(--accent);
    font-weight: normal;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
}

.color-swatch {
    display: inline-block;
    width: 1em;
    height: 1em;
    border-radius: 0.15rem;
    vertical-align: middle;
    border: 1px solid var(--border);
}

.color-swatch-lg {
    display: inline-block;
    width: 1.5em;
    height: 1.5em;
    border-radius: 0.2rem;
    vertical-align: middle;
    border: 1px solid var(--border);
}

.fruit-gloss {
    font-style: italic;
    font-size: 1.1rem;
    color: var(--accent);
    margin-bottom: 1rem;
}

.opinion-friendly { color: #7ec8a0; }
.opinion-neutral { color: var(--text); }
.opinion-suspicious { color: #e6b84e; }
.opinion-hostile { color: #e05252; }
.opinion-unknown { color: var(--muted); font-style: italic; }

footer {
    margin-top: 2rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--border);
    font-size: 0.8rem;
    color: var(--muted);
    text-align: center;
}
"#
    .to_owned()
}

/// Escape HTML special characters in user-facing text.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load species data embedded at compile time from `data/species_elfcyclopedia.json`.
pub fn load_species_data() -> Vec<SpeciesEntry> {
    let json = include_str!("../../data/species_elfcyclopedia.json");
    serde_json::from_str(json).expect("embedded species_elfcyclopedia.json is malformed")
}

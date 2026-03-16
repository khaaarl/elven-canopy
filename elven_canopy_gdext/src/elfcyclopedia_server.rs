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
// - Fruit sprites are generated via `elven_canopy_sprites::create_fruit`,
//   then encoded as inline PNG data URIs using a minimal hand-rolled PNG
//   encoder (no image library dependency).
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
    /// Each part: (type_name, properties, pigment, component_units)
    pub parts: Vec<FruitPartEntry>,
}

/// A single part within a fruit species entry.
#[derive(Clone, Debug)]
pub struct FruitPartEntry {
    pub part_type: String,
    pub properties: Vec<String>,
    pub pigment: Option<String>,
    pub component_units: u16,
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
// Fruit sprite generation (delegates to elven_canopy_sprites)
// ---------------------------------------------------------------------------

/// Generate a 16x16 RGBA fruit sprite. Returns base64-encoded PNG as a data
/// URI string. Delegates all drawing to `elven_canopy_sprites::create_fruit`.
fn generate_fruit_sprite_data_uri(entry: &FruitEntry) -> String {
    use elven_canopy_sim::fruit::{FruitAppearance, FruitColor, FruitShape};

    let (cr, cg, cb) = parse_hex_color(&entry.color_hex);
    let shape = match entry.shape.as_str() {
        "Round" => FruitShape::Round,
        "Oblong" => FruitShape::Oblong,
        "Clustered" => FruitShape::Clustered,
        "Pod" => FruitShape::Pod,
        "Nut" => FruitShape::Nut,
        "Gourd" => FruitShape::Gourd,
        _ => FruitShape::Round,
    };
    let appearance = FruitAppearance {
        exterior_color: FruitColor {
            r: cr,
            g: cg,
            b: cb,
        },
        shape,
        size_percent: entry.size_percent,
        glows: entry.glows,
    };
    let buf = elven_canopy_sprites::create_fruit(&appearance);
    let png = encode_png_16x16(buf.data());
    let b64 = base64_encode(&png);
    format!("data:image/png;base64,{b64}")
}

fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(128);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(128);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(128);
        (r, g, b)
    } else {
        (128, 128, 128)
    }
}

// ---------------------------------------------------------------------------
// Minimal PNG encoder (no dependencies, uncompressed)
// ---------------------------------------------------------------------------

/// Encode a 16x16 RGBA pixel buffer as a PNG file. Uses uncompressed deflate
/// (store blocks) to avoid needing a zlib/deflate library.
fn encode_png_16x16(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2048);

    // PNG signature.
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    // IHDR chunk: 16x16, 8-bit RGBA.
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&16u32.to_be_bytes()); // width
    ihdr.extend_from_slice(&16u32.to_be_bytes()); // height
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type: RGBA
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    write_png_chunk(&mut out, b"IHDR", &ihdr);

    // IDAT chunk: filtered row data wrapped in uncompressed zlib.
    // Each row: filter byte (0 = None) + 16 * 4 bytes = 65 bytes.
    // Total raw: 16 * 65 = 1040 bytes.
    let mut raw = Vec::with_capacity(16 * 65);
    for y in 0..16 {
        raw.push(0); // filter: None
        let row_start = y * 16 * 4;
        raw.extend_from_slice(&data[row_start..row_start + 16 * 4]);
    }

    let zlib = zlib_store(&raw);
    write_png_chunk(&mut out, b"IDAT", &zlib);

    // IEND chunk.
    write_png_chunk(&mut out, b"IEND", &[]);

    out
}

fn write_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    // CRC32 over type + data.
    let crc = crc32(chunk_type, data);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// CRC32 as specified by PNG (ISO 3309 / ITU-T V.42).
fn crc32(chunk_type: &[u8], data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in chunk_type.iter().chain(data.iter()) {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC_TABLE[idx] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Pre-computed CRC32 lookup table.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0u32;
    while n < 256 {
        let mut c = n;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[n as usize] = c;
        n += 1;
    }
    table
};

/// Wrap raw data in a valid zlib stream using uncompressed (store) deflate
/// blocks. No compression, but no external dependencies needed.
fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 64);
    // Zlib header: CM=8 (deflate), CINFO=7 (32K window), FCHECK so header % 31 == 0.
    out.push(0x78);
    out.push(0x01);

    // Split into 65535-byte store blocks (max for uncompressed deflate).
    let chunks: Vec<&[u8]> = data.chunks(65535).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i == chunks.len() - 1;
        out.push(if is_last { 0x01 } else { 0x00 }); // BFINAL + BTYPE=00
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes()); // NLEN
        out.extend_from_slice(chunk);
    }

    // Adler-32 checksum.
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());

    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Base64 encode bytes to a string.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
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
                "<th></th><th>Name</th><th>Gloss</th><th>Habitat</th>\
                 <th>Shape</th><th>Color</th></tr></thead><tbody>",
            );
            for f in &fruits {
                let sprite_uri = generate_fruit_sprite_data_uri(f);
                let color_swatch = format!(
                    "<span class=\"color-swatch\" style=\"background:{};\"></span>",
                    html_escape(&f.color_hex),
                );
                let glow = if f.glows { " &#x2728;" } else { "" };
                body.push_str(&format!(
                    "<tr><td><img src=\"{sprite}\" class=\"fruit-sprite\" alt=\"{name}\"></td>\
                     <td><a href=\"/fruits/{id}\">{name}</a></td>\
                     <td>{gloss}</td><td>{habitat}</td><td>{shape}</td>\
                     <td>{swatch}{glow}</td></tr>",
                    sprite = sprite_uri,
                    id = f.id,
                    name = html_escape(&f.vaelith_name),
                    gloss = html_escape(&f.english_gloss),
                    habitat = html_escape(&f.habitat),
                    shape = html_escape(&f.shape),
                    swatch = color_swatch,
                    glow = glow,
                ));
            }
            body.push_str("</tbody></table>");
        }
    }

    html_page("Fruit Species", &body, data)
}

fn render_fruit_detail(entry: &FruitEntry, data: &ElfcyclopediaData) -> String {
    let sprite_uri = generate_fruit_sprite_data_uri(entry);
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
<img src="{sprite}" class="fruit-sprite-lg" alt="{name}">
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
        sprite = sprite_uri,
        name = html_escape(&entry.vaelith_name),
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
         <th>Part</th><th>Units</th><th>Properties</th><th>Pigment</th>\
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
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            html_escape(&part.part_type),
            part.component_units,
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

.fruit-sprite {
    image-rendering: pixelated;
    width: 32px;
    height: 32px;
    vertical-align: middle;
}

.fruit-sprite-lg {
    image-rendering: pixelated;
    width: 96px;
    height: 96px;
    margin-bottom: 0.5rem;
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the PNG encoder produces a valid PNG that starts with the correct
    /// signature, contains IHDR/IDAT/IEND chunks, and has internally consistent
    /// CRC checksums.
    #[test]
    fn png_encoder_produces_valid_png() {
        // Use elven_canopy_sprites to draw a small red circle.
        let mut buf = elven_canopy_sprites::PixelBuffer::new(16, 16);
        buf.draw_circle(
            8,
            8,
            4,
            elven_canopy_sprites::Color::from_u8(255, 0, 0, 255),
        );

        let png = encode_png_16x16(buf.data());

        // PNG signature (8 bytes).
        assert_eq!(&png[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

        // Walk chunks and verify structure + CRCs.
        let mut pos = 8;
        let mut found_ihdr = false;
        let mut found_idat = false;
        let mut found_iend = false;

        while pos + 8 <= png.len() {
            let length =
                u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
            let chunk_type = &png[pos + 4..pos + 8];
            let data_start = pos + 8;
            let data_end = data_start + length;
            assert!(
                data_end + 4 <= png.len(),
                "chunk data extends past end of file"
            );

            // Verify CRC over type + data.
            let expected_crc = u32::from_be_bytes([
                png[data_end],
                png[data_end + 1],
                png[data_end + 2],
                png[data_end + 3],
            ]);
            let mut type_arr = [0u8; 4];
            type_arr.copy_from_slice(chunk_type);
            let actual_crc = crc32(&type_arr, &png[data_start..data_end]);
            assert_eq!(
                actual_crc,
                expected_crc,
                "CRC mismatch for chunk {:?}",
                std::str::from_utf8(chunk_type)
            );

            match chunk_type {
                b"IHDR" => {
                    assert_eq!(length, 13, "IHDR must be 13 bytes");
                    let width = u32::from_be_bytes([
                        png[data_start],
                        png[data_start + 1],
                        png[data_start + 2],
                        png[data_start + 3],
                    ]);
                    let height = u32::from_be_bytes([
                        png[data_start + 4],
                        png[data_start + 5],
                        png[data_start + 6],
                        png[data_start + 7],
                    ]);
                    assert_eq!(width, 16);
                    assert_eq!(height, 16);
                    assert_eq!(png[data_start + 8], 8, "bit depth should be 8");
                    assert_eq!(png[data_start + 9], 6, "color type should be 6 (RGBA)");
                    found_ihdr = true;
                }
                b"IDAT" => found_idat = true,
                b"IEND" => {
                    assert_eq!(length, 0, "IEND must be empty");
                    found_iend = true;
                }
                _ => {}
            }

            pos = data_end + 4;
        }

        assert!(found_ihdr, "missing IHDR chunk");
        assert!(found_idat, "missing IDAT chunk");
        assert!(found_iend, "missing IEND chunk");
        assert_eq!(pos, png.len(), "trailing bytes after IEND");
    }

    /// Verify the data URI wrapper produces the expected format.
    #[test]
    fn fruit_sprite_data_uri_format() {
        let entry = FruitEntry {
            id: 1,
            vaelith_name: "Test".into(),
            english_gloss: "test fruit".into(),
            habitat: "Branch".into(),
            rarity: "Common".into(),
            shape: "Round".into(),
            color_hex: "#FF6633".into(),
            glows: false,
            size_percent: 100,
            greenhouse_cultivable: false,
            parts: vec![],
        };
        let uri = generate_fruit_sprite_data_uri(&entry);
        assert!(uri.starts_with("data:image/png;base64,"));
        // Base64 should only contain valid characters.
        let b64 = &uri["data:image/png;base64,".len()..];
        assert!(
            b64.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        );
    }
}

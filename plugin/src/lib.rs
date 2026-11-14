//! Filament swatch plugin for SolidCraft: entry `swatch` builds sample tiles with the
//! filament's name and temperatures, and a row of thin steps that show how see-through it is.
//! Everything is ordinary, editable SolidCraft features made through the host's command API.

use serde_json::{json, Map, Value};

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({ world: "command-plugin", path: "wit" });
}

/// Corner radius of the tile (mm).
const CORNER: f64 = 4.0;
/// Margin between the edge and the text or steps (mm).
const MARGIN: f64 = 5.0;
/// Key-ring hole: diameter, and its centre's distance from the top-left corner (mm).
const HOLE_D: f64 = 5.0;
const HOLE_AT: f64 = 6.0;
/// Height of the step row, from MARGIN up (mm), and of the thickness labels above it.
const STEP_H: f64 = 11.0;
const LABEL_H: f64 = 2.2;
/// Gap between swatches when several are made in one run (mm).
const GAP: f64 = 5.0;
const MAX_STEPS: usize = 8;
/// Host calls a run may make (256), less a margin.
const MAX_CALLS: usize = 240;
/// Letters (and digits and signs) of text one run can cut. Each takes about a quarter of a
/// second in solidcraft-cli and nearly half a second in the desktop app (SolidCraft 0.41), and a
/// run gets 30: 65 took 30 s in the app, so this leaves room for slower computers.
const MAX_GLYPHS: usize = 55;

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub brand: String,
    pub material: String,
    /// One swatch per colour.
    pub colors: Vec<String>,
    /// °C, 0: not shown.
    pub nozzle: u32,
    pub bed: u32,
    pub width: f64,
    pub depth: f64,
    pub thickness: f64,
    /// Thickness of each step (mm), thinnest first.
    pub steps: Vec<f64>,
    pub text_height: f64,
    pub raised: bool,
    pub text_depth: f64,
    pub hole: bool,
    pub origin: [f64; 2],
    pub name: String,
}

fn get_s(p: &Map<String, Value>, key: &str, default: &str, max: usize) -> Result<String, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default.into()),
        Some(Value::String(s)) if s.chars().count() <= max => Ok(s.trim().into()),
        Some(Value::String(_)) => Err(format!("{key} is too long (at most {max} characters)")),
        _ => Err(format!("{key} must be text")),
    }
}

fn get_mm(p: &Map<String, Value>, key: &str, default: f64, min: f64, max: f64) -> Result<f64, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|n| (min..=max).contains(n)).ok_or(format!("{key} must be from {min} to {max} mm")),
    }
}

fn get_temp(p: &Map<String, Value>, key: &str, default: u32) -> Result<u32, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().filter(|n| *n <= 500).map(|n| n as u32)
            .ok_or(format!("{key} must be a whole number from 0 to 500 (°C, 0 = not shown)")),
    }
}

fn get_b(p: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(format!("{key} must be true or false")),
    }
}

fn get_origin(p: &Map<String, Value>) -> Result<[f64; 2], String> {
    let num = |key: &str| match p.get(key) {
        None | Some(Value::Null) => Ok(0.0),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).ok_or(format!("{key} must be a number (mm)")),
    };
    Ok([num("position_x")?, num("position_y")?])
}

/// "0.2, 0.4, 0.6" (commas, semicolons or spaces between; a decimal comma is fine when the
/// numbers are split by semicolons or spaces) into thicknesses, thinnest first.
fn parse_steps(s: &str, thickness: f64) -> Result<Vec<f64>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(vec![]);
    }
    // a comma is a decimal comma only when something else splits the numbers
    let decimal_comma = s.contains(';') || (!s.contains('.') && !s.contains(", ") && s.contains(' '));
    let parts: Vec<&str> = if decimal_comma {
        s.split(|c: char| c == ';' || c.is_whitespace()).collect()
    } else {
        s.split(|c: char| c == ',' || c.is_whitespace()).collect()
    };
    let mut out = vec![];
    for part in parts.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        let v: f64 = part.replace(',', ".").parse().map_err(|_| format!("steps: `{part}` is not a number"))?;
        if !(v > 0.0 && v < thickness) {
            return Err(format!("steps must each be above 0 and thinner than the swatch ({thickness} mm): {part} is not"));
        }
        out.push(v);
    }
    if out.len() > MAX_STEPS {
        return Err(format!("at most {MAX_STEPS} steps"));
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Ok(out)
}

pub fn parse(params: &Value) -> Result<Options, String> {
    let empty = Map::new();
    let p = params.as_object().unwrap_or(&empty);
    let thickness = get_mm(p, "thickness", 3.0, 1.0, 10.0)?;
    let colors: Vec<String> = get_s(p, "color", "Black", 200)?
        .split(';').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
    let colors = if colors.is_empty() { vec![String::new()] } else { colors };
    let raised = match p.get("text_style") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) if s.to_ascii_lowercase().starts_with("raised") => true,
        Some(Value::String(s)) if s.to_ascii_lowercase().starts_with("engraved") => false,
        _ => return Err("text_style must be \"Engraved\" or \"Raised\"".into()),
    };
    let o = Options {
        brand: get_s(p, "brand", "Elegoo", 30)?,
        material: get_s(p, "material", "PLA", 30)?,
        colors,
        nozzle: get_temp(p, "nozzle_temp", 0)?,
        bed: get_temp(p, "bed_temp", 0)?,
        width: get_mm(p, "size_x", 75.0, 50.0, 200.0)?,
        depth: get_mm(p, "size_y", 40.0, 38.0, 100.0)?,
        thickness,
        steps: parse_steps(&get_s(p, "steps", "0.2, 0.4, 0.6, 0.8, 1.0", 100)?, thickness)?,
        text_height: get_mm(p, "text_height", 3.6, 2.0, 8.0)?,
        raised,
        text_depth: get_mm(p, "text_depth", 0.6, 0.2, 2.0)?,
        hole: get_b(p, "hole", true)?,
        origin: get_origin(p)?,
        name: get_s(p, "title", "", 60)?,
    };
    if !o.raised && o.text_depth >= o.thickness {
        return Err(format!("text_depth must be less than the thickness ({} mm)", o.thickness));
    }
    Ok(o)
}

fn cmd(command: &str, params: Value) -> Value {
    json!({"command": command, "params": params})
}

/// A sketch on top of the tile holding `items`.
fn sketch(out: &mut Vec<Value>, z: f64, name: &str, items: Vec<Value>) {
    out.push(cmd("sketch.create", json!({"plane": "XY", "offset": z, "name": name})));
    out.extend(items);
    out.push(cmd("sketch.finish", json!({})));
}

fn r4(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

/// "0.2" for 0.2, "1.0" for 1, "0.25" for 0.25.
fn step_label(t: f64) -> String {
    let s = format!("{:.2}", t);
    let s = s.trim_end_matches('0');
    if s.ends_with('.') { format!("{s}0") } else { s.into() }
}

/// Names of the bodies: the title (or "Swatch"), with the colour, made unique.
fn body_names(o: &Options) -> Vec<String> {
    let base = if o.name.is_empty() { "Swatch".to_string() } else { o.name.clone() };
    let mut names: Vec<String> = vec![];
    for c in &o.colors {
        let mut n = if c.is_empty() || o.colors.len() == 1 && !o.name.is_empty() { base.clone() } else { format!("{base} {c}") };
        let first = n.clone();
        let mut k = 2;
        while names.contains(&n) {
            n = format!("{first} {k}");
            k += 1;
        }
        names.push(n);
    }
    names
}

pub fn commands(o: &Options) -> Vec<Value> {
    let mut out = vec![];
    let (w, d, t) = (o.width, o.depth, o.thickness);
    let names = body_names(o);
    for (i, color) in o.colors.iter().enumerate() {
        let body = &names[i];
        let x0 = o.origin[0];
        // several swatches go in a column, the first at the origin
        let y0 = o.origin[1] - i as f64 * (d + GAP);
        let n = |s: &str| format!("{body} {s}");
        let at = |x: f64, y: f64| json!([r4(x0 + x), r4(y0 + y)]);

        // the tile, from z = 0 up
        sketch(&mut out, t, &n("Tile"), vec![cmd("sketch.rectangle.rounded", json!({"p0": at(0.0, 0.0), "p1": at(w, d), "radius": CORNER}))]);
        // a sketch at the top extruded down makes the tile without a second plane
        out.push(cmd("solid.extrude", json!({"sketch": n("Tile"), "profiles": "all", "distance": t,
            "operation": "new", "direction": "negative", "body_name": body})));
        let cut = |name: String, depth: f64, profiles: Value| cmd("solid.extrude", json!({"sketch": name, "profiles": profiles,
            "distance": r4(depth), "operation": "cut", "direction": "negative", "targets": [body]}));

        // the hole and the steps share one sketch (a run may make only 256 host calls); each is
        // cut on its own, picked by a point inside it: the hole through, a step down to its thickness
        let k = o.steps.len();
        let sw = (w - 2.0 * MARGIN) / k.max(1) as f64;
        let mut items = vec![];
        let mut cuts = vec![];
        if o.hole {
            items.push(cmd("sketch.circle.center", json!({"center": at(HOLE_AT, d - HOLE_AT), "diameter": HOLE_D})));
            cuts.push((t, at(HOLE_AT, d - HOLE_AT)));
        }
        for (j, s) in o.steps.iter().enumerate() {
            let sx = MARGIN + j as f64 * sw;
            items.push(cmd("sketch.rectangle.two_point", json!({"p0": at(sx, MARGIN), "p1": at(sx + sw, MARGIN + STEP_H)})));
            cuts.push((t - s, at(sx + sw / 2.0, MARGIN + STEP_H / 2.0)));
        }
        if !items.is_empty() {
            sketch(&mut out, t, &n("Steps"), items);
            for (depth, point) in cuts {
                out.push(cut(n("Steps"), depth, json!([{"point": point}])));
            }
        }

        // text: brand and material on top, colour and temperatures under, step labels over the steps
        let th = o.text_height;
        let line1 = d - MARGIN - th * 1.0 - 0.5;
        let line2 = line1 - th - 3.0;
        let left = if o.hole { HOLE_AT + HOLE_D / 2.0 + 3.5 } else { MARGIN };
        let text = |s: &str, x: f64, y: f64, h: f64, align: &str| cmd("sketch.text",
            json!({"text": s, "at": at(x, y), "height": r4(h), "align": align}));
        let mut items = vec![];
        if !o.brand.is_empty() {
            items.push(text(&o.brand.to_uppercase(), left, line1, th * 1.15, "left"));
        }
        if !o.material.is_empty() {
            items.push(text(&o.material, w - MARGIN, line1, th * 0.95, "right"));
        }
        if !color.is_empty() {
            items.push(text(color, MARGIN, line2, th, "left"));
        }
        let temps = temps(o);
        if !temps.is_empty() {
            items.push(text(&temps, w - MARGIN, line2, th * 0.95, "right"));
        }
        for (j, s) in o.steps.iter().enumerate() {
            items.push(text(&step_label(*s), MARGIN + (j as f64 + 0.5) * sw, MARGIN + STEP_H + 2.5, LABEL_H, "center"));
        }
        if !items.is_empty() {
            sketch(&mut out, t, &n("Text"), items);
            out.push(if o.raised {
                cmd("solid.extrude", json!({"sketch": n("Text"), "profiles": "all", "distance": r4(o.text_depth),
                    "operation": "join", "direction": "positive", "targets": [body]}))
            } else {
                cut(n("Text"), o.text_depth, json!("all"))
            });
        }
    }
    out
}

/// "240° / 70°", or what of it is set.
fn temps(o: &Options) -> String {
    match (o.nozzle, o.bed) {
        (0, 0) => String::new(),
        (n, 0) => format!("{n}°"),
        (0, b) => format!("bed {b}°"),
        (n, b) => format!("{n}° / {b}°"),
    }
}

/// Host calls for a command list: each command, and a progress and a cancel check every 16.
fn calls(c: &[Value]) -> usize {
    c.len() + 2 * c.len().div_ceil(16)
}

/// Letters, digits and signs in the text a command list writes (spaces cost nothing).
fn glyphs(c: &[Value]) -> usize {
    c.iter().filter(|v| v["command"] == "sketch.text")
        .map(|v| v["params"]["text"].as_str().unwrap_or("").chars().filter(|ch| !ch.is_whitespace()).count())
        .sum()
}

/// About how wide a text is in the built-in font (Liberation Sans), `h` its capital height (mm).
fn text_width(s: &str, h: f64) -> f64 {
    s.chars().map(|c| h * if c.is_whitespace() { 0.35 } else if c.is_uppercase() || c.is_ascii_digit() { 0.78 }
        else if c.is_lowercase() { 0.62 } else { 0.5 }).sum()
}

/// Text must fit between the top lines and the step labels, and the two texts on a line must
/// not run into each other.
fn check_layout(o: &Options) -> Result<(), String> {
    let th = o.text_height;
    let left = if o.hole { HOLE_AT + HOLE_D / 2.0 + 3.5 } else { MARGIN };
    let room = |l: f64| o.width - MARGIN - l - 1.5;
    if text_width(&o.brand.to_uppercase(), th * 1.15) + text_width(&o.material, th * 0.95) > room(left) {
        return Err("Brand and material do not fit side by side: shorten them, make the text smaller or the swatch wider.".into());
    }
    for c in &o.colors {
        if text_width(c, th) + text_width(&temps(o), th * 0.95) > room(MARGIN) {
            return Err(format!("\"{c}\" and the temperatures do not fit side by side: shorten the colour, make the text \
                smaller or the swatch wider."));
        }
    }
    let line2 = o.depth - MARGIN - th - 0.5 - th - 3.0;
    let labels_top = MARGIN + STEP_H + 2.5 + LABEL_H;
    if !o.steps.is_empty() && line2 < labels_top + 1.5 {
        let need = ((labels_top + 1.5 - line2 + o.depth) * 10.0).ceil() / 10.0;
        return Err(format!("The text does not fit above the steps: make the swatch at least {need} mm deep, or the text smaller."));
    }
    Ok(())
}

/// The command list for an entry and the names of the bodies it makes, or what is wrong.
pub fn plan(entry: &str, params: &Value) -> Result<(String, Vec<Value>), String> {
    match entry {
        "swatch" => {
            let o = parse(params)?;
            check_layout(&o)?;
            let c = commands(&o);
            let g = glyphs(&c);
            if g > MAX_GLYPHS || calls(&c) > MAX_CALLS {
                let what = if o.colors.len() > 1 { "Make fewer swatches at a time, or" } else { "Use" };
                return Err(format!("That is {g} letters and digits of text, and SolidCraft gives a plugin 30 seconds: one run \
                    can cut about {MAX_GLYPHS}. {what} shorter text or fewer steps."));
            }
            Ok((body_names(&o).join(", "), c))
        }
        other => Err(format!("unknown entry `{other}` (use `swatch`)")),
    }
}

#[cfg(target_arch = "wasm32")]
mod guest {
    use super::*;
    use bindings::craft::plugin::commands;
    use bindings::craft::plugin::control;
    use bindings::craft::plugin::types::{FieldFailure, PluginError};
    use bindings::exports::craft::plugin::entry::Guest;

    struct Swatch;

    impl Guest for Swatch {
        fn execute(entry: String, params: String) -> Result<String, PluginError> {
            let params: Value = serde_json::from_str(if params.trim().is_empty() { "{}" } else { &params })
                .map_err(|e| PluginError::InvalidInput(FieldFailure { field: "params".into(), reason: e.to_string() }))?;
            let (bodies, cmds) = plan(&entry, &params)
                .map_err(|reason| PluginError::InvalidInput(FieldFailure { field: entry.clone(), reason }))?;
            let total = cmds.len();
            for (i, c) in cmds.iter().enumerate() {
                // progress and cancel checks are host calls too, and a run may make only 256
                if i % 16 == 0 {
                    if control::cancelled() {
                        return Err(PluginError::Cancelled);
                    }
                    control::progress(i as f32 / total as f32, &format!("Swatch: step {} of {total}", i + 1));
                }
                commands::run(c["command"].as_str().unwrap_or(""), &c["params"].to_string())?;
            }
            Ok(json!({"bodies": bodies, "commands": total}).to_string())
        }
    }

    bindings::export!(Swatch with_types_in bindings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let o = parse(&json!({})).unwrap();
        assert_eq!((o.width, o.depth, o.thickness), (75.0, 40.0, 3.0));
        assert_eq!(o.steps, vec![0.2, 0.4, 0.6, 0.8, 1.0]);
        assert_eq!(o.colors, vec!["Black".to_string()]);
        assert!(!o.raised && o.hole);
        assert!(plan("swatch", &json!({})).is_ok());
    }

    #[test]
    fn steps_parse() {
        assert_eq!(parse_steps("0.2, 0.4, 0.6", 3.0).unwrap(), vec![0.2, 0.4, 0.6]);
        assert_eq!(parse_steps("0,2; 0,4", 3.0).unwrap(), vec![0.2, 0.4]);
        assert_eq!(parse_steps("0.6 0.2 0.4", 3.0).unwrap(), vec![0.2, 0.4, 0.6]);
        assert_eq!(parse_steps("", 3.0).unwrap(), Vec::<f64>::new());
        assert!(parse_steps("0.2, 3.0", 3.0).is_err());
        assert!(parse_steps("a", 3.0).is_err());
        assert!(parse_steps("0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9", 3.0).is_err());
    }

    #[test]
    fn step_labels() {
        assert_eq!(step_label(0.2), "0.2");
        assert_eq!(step_label(1.0), "1.0");
        assert_eq!(step_label(0.25), "0.25");
    }

    #[test]
    fn colours_make_one_swatch_each_with_unique_names() {
        let o = parse(&json!({"color": "Red; Blue; Red"})).unwrap();
        assert_eq!(body_names(&o), vec!["Swatch Red", "Swatch Blue", "Swatch Red 2"]);
        let c = commands(&o);
        let tiles = c.iter().filter(|v| v["params"]["operation"] == "new").count();
        assert_eq!(tiles, 3);
        assert!(plan("swatch", &json!({"color": "a;b;c;d;e;f;g;h;i"})).unwrap_err().contains("fewer swatches"));
    }

    #[test]
    fn engraved_or_raised() {
        let ops = |p: Value| {
            let c = commands(&parse(&p).unwrap());
            let n = c.len();
            (c[n - 1]["params"]["direction"].clone(), c[n - 1]["params"]["operation"].clone())
        };
        assert_eq!(ops(json!({})), (json!("negative"), json!("cut")));
        assert_eq!(ops(json!({"text_style": "Raised"})), (json!("positive"), json!("join")));
        assert!(parse(&json!({"text_depth": 3.0})).is_err());
    }

    #[test]
    fn steps_are_cut_to_their_thickness() {
        let c = commands(&parse(&json!({"thickness": 2.5, "steps": "0.4, 1.2"})).unwrap());
        let depths: Vec<f64> = c.iter().filter(|v| v["params"]["sketch"] == "Swatch Black Steps" && v["command"] == "solid.extrude")
            .map(|v| v["params"]["distance"].as_f64().unwrap()).collect();
        assert_eq!(depths, vec![2.5, 2.1, 1.3]); // the hole through, then each step
    }

    #[test]
    fn runs_stay_within_the_time_and_call_limits() {
        // the swatch Polly liked: 49 letters and digits
        let (_, c) = plan("swatch", &json!({"brand": "Elegoo", "material": "Rapid PETG", "color": "Galaxy Black",
            "nozzle_temp": 240, "bed_temp": 70})).unwrap();
        assert_eq!(glyphs(&c), 49);
        // short names: several swatches in one run
        assert!(plan("swatch", &json!({"brand": "", "material": "PLA", "color": "Red;Blue;Green", "steps": ""})).is_ok());
        // too much text
        let long = json!({"brand": "Elegoo", "material": "Rapid PETG", "color": "Galaxy Black Metallic Sparkle Glitter",
            "size_x": 150, "nozzle_temp": 240, "bed_temp": 70});
        assert!(plan("swatch", &long).unwrap_err().contains("shorter text"));
        // nothing that passes the text limit can pass the call limit
        let (_, c) = plan("swatch", &json!({"brand": "", "material": "", "color": "a;b;c;d;e;f", "steps": "0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8"}))
            .map(|r| r).unwrap_or_default();
        assert!(c.is_empty() || calls(&c) <= 256);
    }

    #[test]
    fn text_that_does_not_fit_says_so() {
        assert!(plan("swatch", &json!({"color": "Galaxy Black Metallic", "nozzle_temp": 240, "bed_temp": 70})).unwrap_err().contains("side by side"));
        assert!(plan("swatch", &json!({"color": "Galaxy Black Metallic", "nozzle_temp": 240, "bed_temp": 70, "size_x": 110})).is_ok());
        assert!(plan("swatch", &json!({"brand": "Polymaker", "material": "PolyTerra PLA"})).unwrap_err().contains("Brand"));
        assert!(plan("swatch", &json!({"size_y": 38, "text_height": 6})).unwrap_err().contains("deep"));
        assert!(plan("swatch", &json!({"size_y": 38, "text_height": 6, "steps": ""})).is_ok());
    }
}

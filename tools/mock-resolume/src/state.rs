//! Mock state: the live composition JSON plus stored contents of the other decks. All Resolume
//! behaviour lives here as plain methods returning what changed.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct OscRecord {
    pub at: Instant,
    pub addr: String,
    pub args: Vec<i32>,
}

/// What a state change caused, for the publisher in `lib.rs`.
#[derive(Debug, Default)]
pub struct Changes {
    pub params: Vec<u64>,
    pub thumbnails: Vec<Value>,
    pub push: bool,
    pub not_found: bool,
    pub bad_request: bool,
}

impl Changes {
    fn not_found() -> Self {
        Self {
            not_found: true,
            ..Default::default()
        }
    }
}

struct DeckContent {
    columns: Value,
    layer_clips: Vec<Value>,
}

pub struct State {
    pub comp: Value,
    stored: HashMap<String, DeckContent>,
    pub custom_thumbnails: HashMap<u64, Vec<u8>>,
    clear_held: HashSet<usize>,
    pub osc_log: Vec<OscRecord>,
    pub started: Instant,
    next_id: u64,
}

fn is_connected(clip: &Value) -> bool {
    matches!(
        clip["connected"]["value"].as_str(),
        Some("Connected" | "Connected & previewing")
    )
}

fn set_connected(clip: &mut Value, value: &str, index: u64) {
    clip["connected"]["value"] = value.into();
    clip["connected"]["index"] = index.into();
}

fn is_param(v: &Value) -> bool {
    v.get("id").and_then(Value::as_u64).is_some() && v.get("valuetype").is_some()
}

impl State {
    pub fn new(live: Value, others: Vec<Value>) -> Self {
        let mut stored = HashMap::new();
        for other in others {
            let Some(name) = selected_deck_name(&other) else {
                continue;
            };
            let layer_clips = other["layers"]
                .as_array()
                .map(|ls| ls.iter().map(|l| l["clips"].clone()).collect())
                .unwrap_or_default();
            stored.insert(
                name,
                DeckContent {
                    columns: other["columns"].clone(),
                    layer_clips,
                },
            );
        }
        Self {
            comp: live,
            stored,
            custom_thumbnails: HashMap::new(),
            clear_held: HashSet::new(),
            osc_log: Vec::new(),
            started: Instant::now(),
            next_id: 9_000_000_000_000,
        }
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn live_deck(&self) -> Option<String> {
        selected_deck_name(&self.comp)
    }

    fn layer_count(&self) -> usize {
        self.comp["layers"].as_array().map_or(0, Vec::len)
    }

    pub fn clip_state(&self, layer: usize, column: usize) -> Option<String> {
        self.comp["layers"].get(layer.checked_sub(1)?)?["clips"].get(column.checked_sub(1)?)?
            ["connected"]["value"]
            .as_str()
            .map(str::to_owned)
    }

    pub fn clip_by_id(&self, id: u64) -> Option<&Value> {
        self.comp["layers"]
            .as_array()?
            .iter()
            .flat_map(|l| l["clips"].as_array().into_iter().flatten())
            .find(|c| c["id"].as_u64() == Some(id))
    }

    fn clip_by_id_mut(&mut self, id: u64) -> Option<&mut Value> {
        self.comp["layers"]
            .as_array_mut()?
            .iter_mut()
            .flat_map(|l| l["clips"].as_array_mut().into_iter().flatten())
            .find(|c| c["id"].as_u64() == Some(id))
    }

    /// Clip connect (OSC int 1, a click, or REST connect). Own Layer target: other clips on the
    /// layer disconnect. Re-triggering a connected clip changes no parameter but still pushes.
    pub fn connect(&mut self, layer: usize, column: usize) -> Changes {
        let mut changes = Changes {
            push: true,
            ..Default::default()
        };
        match self.clip_state(layer, column).as_deref() {
            None => return Changes::not_found(),
            Some("Empty") => return Changes::default(),
            _ => {}
        }
        let clips = self.comp["layers"][layer - 1]["clips"]
            .as_array_mut()
            .expect("layer has clips");
        for (i, clip) in clips.iter_mut().enumerate() {
            if i + 1 == column {
                if !is_connected(clip) {
                    set_connected(clip, "Connected", 3);
                    changes.params.extend(clip["connected"]["id"].as_u64());
                }
            } else if is_connected(clip) {
                set_connected(clip, "Disconnected", 1);
                changes.params.extend(clip["connected"]["id"].as_u64());
            }
        }
        let active = clips[column - 1].clone();
        self.comp["layers"][layer - 1]["active_clip"] = active;
        changes
    }

    pub fn clear_layer(&mut self, layer: usize) -> Changes {
        let mut changes = Changes {
            push: true,
            ..Default::default()
        };
        let Some(clips) = self
            .comp
            .get_mut("layers")
            .and_then(|l| l.get_mut(layer.wrapping_sub(1)))
            .and_then(|l| l.get_mut("clips"))
            .and_then(Value::as_array_mut)
        else {
            return Changes::not_found();
        };
        for clip in clips.iter_mut().filter(|c| is_connected(c)) {
            set_connected(clip, "Disconnected", 1);
            changes.params.extend(clip["connected"]["id"].as_u64());
        }
        self.comp["layers"][layer - 1]["active_clip"] = Value::Null;
        changes
    }

    pub fn osc(&mut self, addr: &str, ints: &[i32], at: Instant) -> Changes {
        self.osc_log.push(OscRecord {
            at,
            addr: addr.to_owned(),
            args: ints.to_vec(),
        });
        let parts: Vec<&str> = addr.trim_start_matches('/').split('/').collect();
        let num = |s: &str| s.parse::<usize>().ok();
        match parts.as_slice() {
            ["composition", "layers", l, "clips", c, "connect"] => match (num(l), num(c)) {
                (Some(l), Some(c)) if ints.first().is_none_or(|&v| v == 1) => self.connect(l, c),
                _ => Changes::default(),
            },
            ["composition", "layers", l, "clear"] => {
                let Some(l) = num(l) else {
                    return Changes::default();
                };
                let press = ints.first().is_none_or(|&v| v != 0);
                if !press {
                    self.clear_held.remove(&l);
                    Changes::default()
                } else if self.clear_held.insert(l) {
                    self.clear_layer(l)
                } else {
                    Changes::default() // still held: ignored, like Resolume
                }
            }
            ["composition", "decks", n, "select"] => match num(n) {
                Some(n) if ints.first().is_none_or(|&v| v == 1) => self.select_deck(n),
                _ => Changes::default(),
            },
            _ => Changes::default(),
        }
    }

    pub fn select_deck(&mut self, n: usize) -> Changes {
        let decks = self.comp["decks"].as_array().map_or(0, Vec::len);
        if n == 0 || n > decks {
            return Changes::not_found();
        }
        let current = (0..decks).find(|&i| self.comp["decks"][i]["selected"]["value"] == true);
        if current == Some(n - 1) {
            return Changes::default();
        }
        let mut changes = Changes {
            push: true,
            ..Default::default()
        };
        // Store the outgoing deck's content.
        if let Some(cur) = current {
            let name = self.comp["decks"][cur]["name"]["value"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let layer_clips = (0..self.layer_count())
                .map(|i| self.comp["layers"][i]["clips"].take())
                .collect();
            let columns = self.comp["columns"].take();
            self.stored.insert(
                name,
                DeckContent {
                    columns,
                    layer_clips,
                },
            );
            self.comp["decks"][cur]["selected"]["value"] = false.into();
            changes
                .params
                .extend(self.comp["decks"][cur]["selected"]["id"].as_u64());
        }
        self.comp["decks"][n - 1]["selected"]["value"] = true.into();
        changes
            .params
            .extend(self.comp["decks"][n - 1]["selected"]["id"].as_u64());
        let name = self.comp["decks"][n - 1]["name"]["value"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let content = match self.stored.remove(&name) {
            Some(c) => c,
            None => self.empty_deck(),
        };
        self.comp["columns"] = content.columns;
        for i in 0..self.layer_count() {
            let clips = content
                .layer_clips
                .get(i)
                .cloned()
                .unwrap_or_else(|| json!([]));
            self.comp["layers"][i]["clips"] = clips;
            self.comp["layers"][i]["active_clip"] = Value::Null;
        }
        // Resolume sends a burst of empty thumbnail updates on a deck switch.
        let slots = self.layer_count() * self.comp["columns"].as_array().map_or(0, Vec::len);
        for _ in 0..slots {
            changes
                .thumbnails
                .push(json!({"last_update":"","is_default":true,"id":0,"path":""}));
        }
        changes
    }

    /// A deck never seen: as many empty columns as the current one had, fresh ids.
    fn empty_deck(&mut self) -> DeckContent {
        let columns_n = 4;
        let template = self.comp["layers"][0]["clips"][0].clone();
        let mut columns = Vec::new();
        for i in 0..columns_n {
            columns.push(json!({"id": self.fresh_id(), "name": {"valuetype":"ParamString","id": self.fresh_id(), "value": format!("Column {}", i + 1)}}));
        }
        let mut layer_clips = Vec::new();
        for _ in 0..self.layer_count() {
            let mut clips = Vec::new();
            for _ in 0..columns_n {
                let mut c = template.clone();
                c["id"] = self.fresh_id().into();
                c["name"] = json!({"valuetype":"ParamString","id": self.fresh_id(),"value":""});
                c["connected"]["id"] = self.fresh_id().into();
                set_connected(&mut c, "Empty", 0);
                c["video"] = Value::Null;
                c["thumbnail"] = json!({"last_update":"0","is_default":true,"id":c["id"].clone(),"path":"/api/v1/composition/thumbnail/dummy"});
                clips.push(c);
            }
            layer_clips.push(Value::Array(clips));
        }
        DeckContent {
            columns: Value::Array(columns),
            layer_clips,
        }
    }

    fn touch_thumbnail(&mut self, id: u64) -> Option<Value> {
        let stamp = format!(
            "{}",
            17_929_000_000_000_000u64 + self.started.elapsed().as_micros() as u64
        );
        let clip = self.clip_by_id_mut(id)?;
        clip["thumbnail"] = json!({
            "last_update": stamp,
            "is_default": false,
            "id": id,
            "path": format!("/api/v1/composition/clips/by-id/{id}/thumbnail"),
        });
        Some(clip["thumbnail"].clone())
    }

    pub fn set_custom_thumbnail(&mut self, id: u64, png: Vec<u8>) -> Changes {
        let Some(thumb) = self.touch_thumbnail(id) else {
            return Changes::not_found();
        };
        self.custom_thumbnails.insert(id, png);
        Changes {
            thumbnails: vec![thumb],
            push: true,
            ..Default::default()
        }
    }

    pub fn open_clip(&mut self, id: u64, url: &str) -> Changes {
        let (name, video) = if let Some(src) = url.strip_prefix("source:///video/") {
            let src = percent_decode(src.split('/').next().unwrap_or(src));
            (src.clone(), json!({"fileinfo": null, "description": src}))
        } else if let Some(path) = url.strip_prefix("file:///") {
            let path = percent_decode(path);
            let windows = path.replace('/', "\\");
            let file = windows.rsplit('\\').next().unwrap_or(&windows).to_owned();
            let stem = file
                .rsplit_once('.')
                .map_or(file.as_str(), |(s, _)| s)
                .to_owned();
            (
                stem,
                json!({"fileinfo": {"path": windows, "exists": true}, "description": file}),
            )
        } else {
            return Changes {
                bad_request: true,
                ..Default::default()
            };
        };
        let Some(clip) = self.clip_by_id_mut(id) else {
            return Changes::not_found();
        };
        clip["name"]["value"] = name.into();
        clip["video"] = video;
        if clip["connected"]["value"] == "Empty" {
            set_connected(clip, "Disconnected", 1);
        }
        let mut changes = Changes {
            push: true,
            ..Default::default()
        };
        changes.params.extend(clip["name"]["id"].as_u64());
        changes.params.extend(clip["connected"]["id"].as_u64());
        changes.thumbnails.extend(self.touch_thumbnail(id));
        changes
    }

    pub fn clear_clip(&mut self, id: u64) -> Changes {
        let Some(clip) = self.clip_by_id_mut(id) else {
            return Changes::not_found();
        };
        clip["name"]["value"] = "".into();
        set_connected(clip, "Empty", 0);
        clip["video"] = Value::Null;
        clip["thumbnail"] = json!({"last_update":"0","is_default":true,"id":id,"path":"/api/v1/composition/thumbnail/dummy"});
        let mut changes = Changes {
            push: true,
            ..Default::default()
        };
        changes.params.extend(clip["connected"]["id"].as_u64());
        self.custom_thumbnails.remove(&id);
        changes
    }

    /// (param id, JSON pointer, API path) for every parameter of decks, layers and clips.
    fn param_locations(&self) -> Vec<(u64, String, String)> {
        let mut out = Vec::new();
        let mut scan = |obj: &Value, ptr: String, path: String, deck: bool| {
            if let Some(map) = obj.as_object() {
                for (k, v) in map {
                    if is_param(v) {
                        let api = match (k.as_str(), deck) {
                            ("connected", _) => "connect",
                            ("selected", true) => "select",
                            (k, _) => k,
                        };
                        out.push((
                            v["id"].as_u64().unwrap(),
                            format!("{ptr}/{k}"),
                            format!("{path}/{api}"),
                        ));
                    }
                }
            }
        };
        for (i, d) in self.comp["decks"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            scan(
                d,
                format!("/decks/{i}"),
                format!("/composition/decks/{}", i + 1),
                true,
            );
        }
        for (li, l) in self.comp["layers"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let lp = format!("/composition/layers/{}", li + 1);
            scan(l, format!("/layers/{li}"), lp.clone(), false);
            scan(
                &l["video"],
                format!("/layers/{li}/video"),
                format!("{lp}/video"),
                false,
            );
            for (ci, c) in l["clips"].as_array().into_iter().flatten().enumerate() {
                scan(
                    c,
                    format!("/layers/{li}/clips/{ci}"),
                    format!("{lp}/clips/{}", ci + 1),
                    false,
                );
            }
        }
        out
    }

    /// `/parameter/by-id/<id>` or a logical path → parameter id.
    pub fn resolve_param(&self, param: &str) -> Option<u64> {
        let locs = self.param_locations();
        if let Some(id) = param.strip_prefix("/parameter/by-id/") {
            let id: u64 = id.parse().ok()?;
            return locs.iter().any(|l| l.0 == id).then_some(id);
        }
        let normalized = param
            .strip_suffix("/connected")
            .map(|p| format!("{p}/connect"))
            .unwrap_or_else(|| param.to_owned());
        locs.iter()
            .find(|l| {
                l.2 == normalized
                    || (l.2.ends_with("/select")
                        && normalized.ends_with("/selected")
                        && l.2.trim_end_matches("/select")
                            == normalized.trim_end_matches("/selected"))
            })
            .map(|l| l.0)
    }

    pub fn param_message(&self, id: u64, kind: &str) -> Option<Value> {
        let (_, ptr, path) = self.param_locations().into_iter().find(|l| l.0 == id)?;
        let mut v = self.comp.pointer(&ptr)?.clone();
        v["path"] = path.into();
        v["type"] = kind.into();
        Some(v)
    }

    pub fn set_param(&mut self, id: u64, value: Value) -> Changes {
        let Some((_, ptr, path)) = self.param_locations().into_iter().find(|l| l.0 == id) else {
            return Changes::not_found();
        };
        if let Some(slot) = self.comp.pointer_mut(&format!("{ptr}/value")) {
            *slot = value;
        }
        Changes {
            params: vec![id],
            // Renames are not pushed (notes §5.3); other parameter sets are.
            push: !path.ends_with("/name"),
            ..Default::default()
        }
    }
}

fn selected_deck_name(comp: &Value) -> Option<String> {
    comp["decks"]
        .as_array()?
        .iter()
        .find(|d| d["selected"]["value"] == true)
        .and_then(|d| d["name"]["value"].as_str())
        .map(str::to_owned)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"),
                    16,
                ) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(
            serde_json::from_str(include_str!("../../../docs/samples/composition.json")).unwrap(),
            vec![serde_json::from_str(include_str!(
                "../../../docs/samples/deck1-composition.json"
            ))
            .unwrap()],
        )
    }

    #[test]
    fn connect_disconnects_others_on_the_layer() {
        let mut s = state();
        let c = s.connect(2, 1);
        assert_eq!(c.params, vec![1792956592480]);
        assert_eq!(s.clip_state(2, 1).as_deref(), Some("Connected"));
        let c = s.connect(2, 3);
        assert_eq!(c.params.len(), 2);
        assert_eq!(s.clip_state(2, 1).as_deref(), Some("Disconnected"));
        // Re-trigger: no param change, but a push.
        let c = s.connect(2, 3);
        assert!(c.params.is_empty() && c.push);
        // Empty slot: nothing.
        assert!(!s.connect(1, 1).push);
    }

    #[test]
    fn osc_clear_is_edge_triggered() {
        let mut s = state();
        let now = Instant::now();
        s.connect(2, 1);
        assert_eq!(
            s.osc("/composition/layers/2/clear", &[1], now).params.len(),
            1
        );
        s.connect(2, 1);
        assert!(
            s.osc("/composition/layers/2/clear", &[1], now)
                .params
                .is_empty(),
            "held"
        );
        s.osc("/composition/layers/2/clear", &[0], now);
        assert_eq!(
            s.osc("/composition/layers/2/clear", &[], now).params.len(),
            1,
            "no-arg = press"
        );
    }

    #[test]
    fn deck_switch_swaps_content_and_back() {
        let mut s = state();
        let c = s.select_deck(1);
        assert_eq!(s.live_deck().as_deref(), Some("1000A Text3D"));
        assert_eq!(c.params.len(), 2);
        assert_eq!(s.comp["columns"].as_array().unwrap().len(), 9);
        assert_eq!(s.clip_state(3, 2).as_deref(), Some("Disconnected"));
        s.select_deck(2);
        assert_eq!(s.comp["columns"].as_array().unwrap().len(), 10);
        assert!(s.clip_by_id(1792956592496).is_some());
        s.select_deck(3);
        assert_eq!(s.live_deck().as_deref(), Some("empty"));
        assert!(s.select_deck(9).not_found);
    }

    #[test]
    fn params_by_id_and_by_path() {
        let s = state();
        assert_eq!(
            s.resolve_param("/composition/layers/2/clips/1/connected"),
            Some(1792956592480)
        );
        assert_eq!(
            s.resolve_param("/parameter/by-id/1792956592480"),
            Some(1792956592480)
        );
        assert_eq!(
            s.resolve_param("/composition/decks/2/select"),
            Some(1792956584811)
        );
        assert_eq!(
            s.resolve_param("/composition/layers/4/video/opacity"),
            Some(1792956589220)
        );
        assert_eq!(s.resolve_param("/parameter/by-id/1"), None);
        let m = s
            .param_message(1792956592480, "parameter_subscribed")
            .unwrap();
        assert_eq!(m["path"], "/composition/layers/2/clips/1/connect");
        assert_eq!(m["value"], "Disconnected");
    }

    #[test]
    fn open_and_clear_keep_the_slot_id() {
        let mut s = state();
        let id = 1792956590653; // SEQ C1, empty
        assert!(!s.open_clip(id, "source:///video/Blank").not_found);
        assert_eq!(s.clip_by_id(id).unwrap()["name"]["value"], "Blank");
        s.open_clip(id, "file:///C:/media/my%20file.png");
        let c = s.clip_by_id(id).unwrap();
        assert_eq!(c["name"]["value"], "my file");
        assert_eq!(c["video"]["fileinfo"]["path"], "C:\\media\\my file.png");
        s.clear_clip(id);
        assert_eq!(s.clip_by_id(id).unwrap()["connected"]["value"], "Empty");
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b+c%C3%BC"), "a b cü");
        assert_eq!(percent_decode("100%"), "100%");
    }
}

//! Reading `pw-dump` output: which streams are capturing audio, and from
//! which process. Pure (no processes, no sockets), so it is tested on every
//! OS with fixtures recorded from a real PipeWire.
//!
//! `pw-dump` prints a JSON array of objects. `pw-dump --monitor` prints one
//! such array first and then another one for every change: an object again
//! in full when it was added or changed, `{"id": N, "info": null}` when it
//! was removed. [`PwState::apply`] takes one array at a time, so the same
//! code reads both.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{display_name, AudioClient};

/// A stream that captures audio from a source (a microphone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicStream {
    /// PipeWire's object id of the stream node.
    pub node_id: u32,
    /// `application.process.id`.
    pub pid: Option<u32>,
    /// `application.name`, e.g. `"Google Chrome"`.
    pub app_name: Option<String>,
    /// `application.process.binary`, e.g. `"chrome"`.
    pub binary: Option<String>,
}

/// The `application.*` properties we need, from a node or a client.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AppProps {
    pid: Option<u32>,
    name: Option<String>,
    binary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputNode {
    client_id: Option<u32>,
    app: AppProps,
    /// `stream.capture.sink`: records what a sink plays (e.g. our own
    /// recorder taking the call audio), not a microphone.
    capture_sink: bool,
}

/// What `pw-dump` has told us so far: capture-stream nodes and clients.
#[derive(Debug, Clone, Default)]
pub struct PwState {
    /// By node id, so the result comes out in a stable order.
    nodes: BTreeMap<u32, InputNode>,
    clients: BTreeMap<u32, AppProps>,
}

const NODE: &str = "PipeWire:Interface:Node";
const CLIENT: &str = "PipeWire:Interface:Client";

impl PwState {
    /// Apply one array as `pw-dump` prints it. Anything that is not an
    /// array of objects is ignored.
    pub fn apply(&mut self, batch: &Value) {
        let Some(objects) = batch.as_array() else {
            return;
        };
        for o in objects {
            let Some(id) = o.get("id").and_then(as_u32) else {
                continue;
            };
            // Removed, or an object type without info (metadata).
            let props = match o.get("info") {
                Some(info) if !info.is_null() => info.get("props"),
                _ => {
                    self.nodes.remove(&id);
                    self.clients.remove(&id);
                    continue;
                }
            };
            match o.get("type").and_then(Value::as_str) {
                Some(NODE) => {
                    let is_input = props
                        .and_then(|p| p.get("media.class"))
                        .and_then(Value::as_str)
                        == Some("Stream/Input/Audio");
                    if is_input {
                        let p = props.expect("checked above");
                        self.nodes.insert(
                            id,
                            InputNode {
                                client_id: p.get("client.id").and_then(as_u32),
                                app: app_props(p),
                                capture_sink: p.get("stream.capture.sink").is_some_and(as_bool),
                            },
                        );
                    } else {
                        self.nodes.remove(&id);
                    }
                }
                Some(CLIENT) => {
                    self.clients
                        .insert(id, props.map(app_props).unwrap_or_default());
                }
                _ => {}
            }
        }
    }

    /// Streams currently capturing from a source, in node-id order.
    /// Streams that record a sink are left out. Pulse clients put the
    /// `application.*` properties on the node; native PipeWire clients only
    /// on their client object, so each missing one comes from there.
    pub fn mic_streams(&self) -> Vec<MicStream> {
        self.nodes
            .iter()
            .filter(|(_, n)| !n.capture_sink)
            .map(|(&node_id, n)| {
                let client = n.client_id.and_then(|c| self.clients.get(&c));
                let from_client = |f: fn(&AppProps) -> Option<&String>| {
                    f(&n.app).or_else(|| client.and_then(f)).cloned()
                };
                MicStream {
                    node_id,
                    pid: n.app.pid.or_else(|| client.and_then(|c| c.pid)),
                    app_name: from_client(|a| a.name.as_ref()),
                    binary: from_client(|a| a.binary.as_ref()),
                }
            })
            .collect()
    }
}

/// Mic streams in the output of a one-shot `pw-dump`.
pub fn parse_dump(json: &str) -> serde_json::Result<Vec<MicStream>> {
    let mut state = PwState::default();
    state.apply(&serde_json::from_str(json)?);
    Ok(state.mic_streams())
}

/// One [`AudioClient`] per stream with a known pid. `exe_of` resolves a
/// pid's executable (`/proc/<pid>/exe` on Linux); when it can't, the
/// stream's `application.process.binary` stands in. The name is
/// `application.name`.
pub fn to_audio_clients(
    streams: &[MicStream],
    mut exe_of: impl FnMut(u32) -> Option<String>,
) -> Vec<AudioClient> {
    streams
        .iter()
        .filter_map(|s| {
            let Some(pid) = s.pid else {
                log::trace!("mic stream {} has no process id", s.node_id);
                return None;
            };
            let exe_path = exe_of(pid).or_else(|| s.binary.clone());
            let name = s
                .app_name
                .clone()
                .unwrap_or_else(|| display_name(pid, None, exe_path.as_deref()));
            Some(AudioClient {
                pid,
                bundle_id: None,
                exe_path,
                name,
                responsible_pid: None,
                responsible_path: None,
                binary: s.binary.clone(),
            })
        })
        .collect()
}

fn app_props(p: &Value) -> AppProps {
    AppProps {
        pid: p.get("application.process.id").and_then(as_u32),
        name: p.get("application.name").and_then(as_string),
        binary: p.get("application.process.binary").and_then(as_string),
    }
}

// pw-dump prints a property that looks like a number or a boolean without
// quotes (a host name of digits came out as a float), so every reader
// accepts both forms.

fn as_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn as_bool(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => s == "true" || s == "1",
        Value::Number(n) => n.as_u64() == Some(1),
        _ => false,
    }
}

fn as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded with PipeWire 1.6.11 in the Arch build container: a fake
    // mic and speaker (null-audio-sink), then
    //   parecord --client-name="Google Chrome" (application.process.binary=chrome)
    //   pw-record -P '{ stream.capture.sink=true application.name=sokki }' --target fake-speaker
    //   pw-record                                (a native PipeWire client)
    //   parecord --client-name="ZOOM VoiceEngine" (application.process.binary=zoom)
    // Trimmed to nodes, clients and links, without `params`.
    const STREAMS: &str = include_str!("../tests/fixtures/pw-dump-streams.json");
    // The same session from `pw-dump --monitor --no-colors`: the streams
    // start in that order and stop in that order.
    const MONITOR: &str = include_str!("../tests/fixtures/pw-dump-monitor.json");

    fn ms(node_id: u32, pid: u32, name: &str, binary: &str) -> MicStream {
        MicStream {
            node_id,
            pid: Some(pid),
            app_name: Some(name.into()),
            binary: Some(binary.into()),
        }
    }

    fn chrome() -> MicStream {
        ms(54, 30, "Google Chrome", "chrome")
    }
    // pw-record is a symlink to pw-cat; the client says so.
    fn native() -> MicStream {
        ms(70, 34, "pw-record", "pw-cat")
    }
    fn zoom() -> MicStream {
        ms(78, 36, "ZOOM VoiceEngine", "zoom")
    }

    #[test]
    fn dump_lists_mic_streams_and_skips_sink_capture() {
        let streams = parse_dump(STREAMS).unwrap();
        assert_eq!(streams, vec![chrome(), native(), zoom()]);
    }

    #[test]
    fn native_client_props_come_from_the_client_object() {
        // The pw-record node itself has application.name but no pid or
        // binary; those are on client 69.
        let v: Value = serde_json::from_str(STREAMS).unwrap();
        let node = v
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == 70)
            .unwrap();
        let props = &node["info"]["props"];
        assert!(props.get("application.process.id").is_none());
        assert_eq!(props["client.id"], 69);
        assert_eq!(parse_dump(STREAMS).unwrap()[1].pid, Some(34));
    }

    /// Splits the monitor output into the arrays pw-dump printed.
    fn batches(text: &str) -> Vec<Value> {
        serde_json::Deserializer::from_str(text)
            .into_iter::<Value>()
            .collect::<serde_json::Result<_>>()
            .unwrap()
    }

    #[test]
    fn monitor_tracks_streams_coming_and_going() {
        let mut state = PwState::default();
        let mut seen: Vec<Vec<MicStream>> = Vec::new();
        for b in batches(MONITOR) {
            state.apply(&b);
            let now = state.mic_streams();
            if seen.last() != Some(&now) {
                seen.push(now);
            }
        }
        assert_eq!(
            seen,
            vec![
                vec![],
                vec![chrome()],
                // the capture stream (node 62) never shows
                vec![chrome(), native()],
                vec![chrome(), native(), zoom()],
                vec![native(), zoom()],
                vec![zoom()],
                vec![],
            ]
        );
        assert!(state.nodes.is_empty());
    }

    #[test]
    fn fixture_streams_classify_as_zoom_then_chrome() {
        let clients = to_audio_clients(&parse_dump(STREAMS).unwrap(), |_| None);
        let apps = crate::classify_all(&clients, 1);
        let got: Vec<_> = apps.iter().map(|a| (a.id, a.pid)).collect();
        assert_eq!(got, [("zoom", 36), ("chrome", 30)]);
    }

    #[test]
    fn removal_and_reuse_of_an_id() {
        let mut state = PwState::default();
        let node = |class: &str| {
            serde_json::json!([{ "id": 7, "type": NODE, "info": { "props": {
                "media.class": class, "application.process.id": "12",
                "application.name": "x" } } }])
        };
        state.apply(&node("Stream/Input/Audio"));
        assert_eq!(state.mic_streams()[0].pid, Some(12), "pid as a string");
        state.apply(&serde_json::json!([{ "id": 7, "info": null }]));
        assert!(state.mic_streams().is_empty());
        state.apply(&node("Stream/Input/Audio"));
        state.apply(&node("Stream/Output/Audio"));
        assert!(state.mic_streams().is_empty(), "same id, now playback");
        state.apply(&serde_json::json!({ "not": "an array" }));
    }

    #[test]
    fn capture_sink_as_string_is_skipped() {
        let mut state = PwState::default();
        state.apply(
            &serde_json::json!([{ "id": 7, "type": NODE, "info": { "props": {
            "media.class": "Stream/Input/Audio", "application.process.id": 12,
            "stream.capture.sink": "true" } } }]),
        );
        assert!(state.mic_streams().is_empty());
    }

    #[test]
    fn audio_clients_use_exe_then_binary() {
        let streams = parse_dump(STREAMS).unwrap();
        let exe = |pid| (pid == 30).then(|| "/opt/google/chrome/chrome".to_string());
        let clients = to_audio_clients(&streams, exe);
        let got: Vec<_> = clients
            .iter()
            .map(|c| {
                (
                    c.pid,
                    c.name.as_str(),
                    c.exe_path.as_deref(),
                    c.bundle_id.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (30, "Google Chrome", Some("/opt/google/chrome/chrome"), None),
                (34, "pw-record", Some("pw-cat"), None),
                (36, "ZOOM VoiceEngine", Some("zoom"), None),
            ]
        );
        let unnamed = MicStream {
            node_id: 1,
            pid: Some(5),
            app_name: None,
            binary: Some("zoom".into()),
        };
        let no_pid = MicStream {
            pid: None,
            ..unnamed.clone()
        };
        let clients = to_audio_clients(&[unnamed, no_pid], |_| None);
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].name, "zoom");
    }
}

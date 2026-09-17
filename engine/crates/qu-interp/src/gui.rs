//! Retained GUI state owned by one interpreter. Hosts dispatch events serially;
//! callbacks never re-run the initialization script or run on a UI thread.
use super::*;
use serde_json::{json, Value as Json};

#[derive(Default)]
pub struct Gui {
    nodes: Vec<Node>,
    next_id: u64,
}
struct Node {
    id: String,
    parent: Option<String>,
    kind: String,
    props: serde_json::Map<String, Json>,
    handlers: HashMap<String, String>,
}
fn handle(id: String) -> Value {
    Value::Record(Arc::new(vec![("__qu_gui_handle".into(), Value::Str(id))]))
}
pub(crate) fn handle_id(value: &Value) -> Option<&str> {
    if let Value::Record(fields) = value {
        return fields.iter().find_map(|(key, value)| match (key.as_str(), value) {
            ("__qu_gui_handle", Value::Str(id)) => Some(id.as_str()), _ => None,
        });
    }
    None
}
fn encode(value: &Value) -> R<Json> {
    match value {
        Value::Str(s) => Ok(json!(s)), Value::Bool(b) => Ok(json!(b)),
        Value::Num(n) if n.is_finite() => Ok(json!(n)),
        Value::Nothing => Ok(Json::Null),
        Value::Vec(values) => values.iter().map(|n| encode(&Value::Num(*n))).collect::<R<Vec<_>>>().map(Json::Array),
        Value::List(values) => values.iter().map(encode).collect::<R<Vec<_>>>().map(Json::Array),
        _ => e("GUI properties require finite numbers, strings, booleans, or lists"),
    }
}
fn decode(value: &Json) -> R<Value> {
    match value {
        Json::Null => Ok(Value::Nothing), Json::Bool(b) => Ok(Value::Bool(*b)),
        Json::String(s) => Ok(Value::Str(s.clone())),
        Json::Number(n) => n.as_f64().filter(|n| n.is_finite()).map(Value::Num).ok_or_else(|| EvalError { msg: "Invalid event number".into() }),
        _ => e("GUI event values must be scalar"),
    }
}
fn validate(kind: &str, props: &serde_json::Map<String, Json>) -> R<()> {
    for (key, value) in props {
        let valid = match key.as_str() {
            "title" | "text" | "xlabel" | "ylabel" => value.is_string(),
            "layout" => matches!(value.as_str(), Some("row" | "column" | "grid")),
            "visible" | "disabled" | "equal_aspect" => value.is_boolean(),
            "min" | "max" | "value" if kind == "slider" || kind == "number" => value.is_number(),
            "value" if kind == "checkbox" => value.is_boolean(),
            "value" => value.is_string(),
            // Dropdown choices. Capped well below the `x`/`y` array limit
            // because these are strings a host must render as menu items,
            // not plot samples it downsamples.
            "options" => kind == "select" && value.as_array().is_some_and(|a| a.len() <= 1000 && a.iter().all(Json::is_string)),
            "x" | "y" => kind == "plot" && value.as_array().is_some_and(|a| a.len() <= 100_000 && a.iter().all(Json::is_number)),
            _ => false,
        };
        if !valid { return e(format!("Unsupported GUI property `{key}` or invalid value for {kind}")); }
    }
    if kind == "plot" {
        let len = |key| props.get(key).and_then(Json::as_array).map(Vec::len).unwrap_or(0);
        if len("x") != len("y") { return e("Plot x and y must have equal lengths; update both together with set(x=..., y=...)"); }
    }
    if kind == "select" {
        // Same atomicity discipline as plot's x/y: a value outside the
        // option list would render as a `<select>` with nothing matching,
        // so the host would show a different choice than the interpreter
        // holds -- silent disagreement, the failure mode worth refusing.
        let options = props.get("options").and_then(Json::as_array);
        if let (Some(options), Some(value)) = (options, props.get("value")) {
            if !options.contains(value) { return e("Select value must be one of its options; update both together with set(options=..., value=...)"); }
        }
    }
    if kind == "slider" || kind == "number" {
        let min = props.get("min").and_then(Json::as_f64).unwrap_or(0.0);
        let max = props.get("max").and_then(Json::as_f64).unwrap_or(100.0);
        let value = props.get("value").and_then(Json::as_f64).unwrap_or(min);
        if min >= max || value < min || value > max { return e("GUI numeric range requires min < max and a value inside that range"); }
    }
    Ok(())
}
impl Interp {
    pub fn gui_packet(&mut self) -> String {
        json!({ "gui": self.gui_snapshot(), "output": std::mem::take(&mut self.out), "error": Json::Null }).to_string()
    }
    /// Line-delimited JSON transport shared by CLI and desktop hosts.
    pub fn gui_dispatch_json(&mut self, input: &str) -> String {
        let result = (|| -> R<()> {
            let event: Json = serde_json::from_str(input).map_err(|error| EvalError { msg: error.to_string() })?;
            let id = event.get("target").and_then(Json::as_str).ok_or_else(|| EvalError { msg: "Event requires target".into() })?;
            let kind = event.get("event").and_then(Json::as_str).ok_or_else(|| EvalError { msg: "Event requires event name".into() })?;
            self.gui_event(id, kind, event.get("value").cloned().unwrap_or(Json::Null))
        })();
        json!({ "gui": self.gui_snapshot(), "output": std::mem::take(&mut self.out), "error": result.err().map(|error| error.to_string()) }).to_string()
    }
    pub(crate) fn gui_call(&mut self, method: &str, args: Vec<Value>, style: Vec<(String, Value)>) -> R<Value> {
        let create = method == "Frame";
        let parent = if create { None } else { Some(handle_id(arg0(&args)?).ok_or_else(|| EvalError { msg: "Expected a GUI handle".into() })?.to_owned()) };
        let node_index = parent.as_ref().map(|id| self.gui.nodes.iter().position(|n| &n.id == id).ok_or_else(|| EvalError { msg: "Unknown GUI handle".into() })).transpose()?;
        let mut updates = serde_json::Map::new();
        for (key, value) in style { updates.insert(key, encode(&value)?); }
        match method {
            "Frame" | "add" => {
                if self.gui.nodes.len() >= 2048 { return e("GUI limit is 2048 widgets per session"); }
                let kind = if create { "frame".to_owned() } else { text_arg(&args, 1)? };
                if !matches!(kind.as_str(), "frame" | "panel" | "label" | "button" | "slider" | "number" | "text" | "checkbox" | "select" | "plot") || (!create && kind == "frame") { return e(format!("Unknown widget kind `{kind}`")); }
                if let Some(index) = node_index {
                    if !matches!(self.gui.nodes[index].kind.as_str(), "frame" | "panel") { return e("Only frames and panels can contain widgets"); }
                    let mut ancestor = Some(index); let mut depth = 0;
                    while let Some(index) = ancestor {
                        depth += 1;
                        if depth >= 64 { return e("GUI nesting limit is 64 levels"); }
                        ancestor = self.gui.nodes[index].parent.as_ref().and_then(|id| self.gui.nodes.iter().position(|n| &n.id == id));
                    }
                }
                if create { updates.insert("title".into(), json!(text_arg(&args, 0)?)); updates.insert("visible".into(), json!(false)); }
                validate(&kind, &updates)?;
                let id = format!("gui-{}", self.gui.next_id); self.gui.next_id += 1;
                self.gui.nodes.push(Node { id: id.clone(), parent, kind, props: updates, handlers: HashMap::new() });
                Ok(handle(id))
            }
            "set" | "show" => {
                let index = node_index.unwrap();
                let node = &mut self.gui.nodes[index];
                if method == "show" { updates.insert("visible".into(), json!(true)); }
                let mut next = node.props.clone(); next.extend(updates); validate(&node.kind, &next)?;
                node.props = next;
                Ok(handle(node.id.clone()))
            }
            "on" => {
                let event = text_arg(&args, 1)?; let callback = text_arg(&args, 2)?;
                let index = node_index.unwrap();
                let kind = &self.gui.nodes[index].kind;
                if !matches!((kind.as_str(), event.as_str()), ("button", "click") | ("slider" | "number" | "checkbox" | "text" | "select", "change") | ("frame", "close")) { return e(format!("Event `{event}` is not supported by {kind}")); }
                if !self.methods.contains_key(&callback) { return e(format!("Define function `{callback}` before registering its handler")); }
                self.gui.nodes[index].handlers.insert(event, callback);
                Ok(handle(self.gui.nodes[index].id.clone()))
            }
            _ => e(format!("Unknown GUI method `{method}`")),
        }
    }
    /// JSON snapshot is a host contract, not executable source.
    pub fn gui_snapshot(&self) -> Json {
        json!({ "protocol": 1, "nodes": self.gui.nodes.iter().map(|node| {
            let mut events = node.handlers.keys().cloned().collect::<Vec<_>>(); events.sort();
            json!({ "id": node.id, "parent": node.parent, "kind": node.kind, "props": node.props, "events": events })
        }).collect::<Vec<_>>() })
    }
    /// Dispatch exactly one host event against the existing interpreter state.
    pub fn gui_event(&mut self, id: &str, event: &str, value: Json) -> R<()> {
        let index = self.gui.nodes.iter().position(|n| n.id == id).ok_or_else(|| EvalError { msg: "Unknown event target".into() })?;
        let node = &self.gui.nodes[index];
        let mut ancestor = Some(index);
        while let Some(index) = ancestor {
            let current = &self.gui.nodes[index];
            if current.props.get("disabled") == Some(&json!(true)) || current.props.get("visible") == Some(&json!(false)) { return e("Widget or its container is disabled or hidden"); }
            ancestor = current.parent.as_ref().and_then(|id| self.gui.nodes.iter().position(|n| &n.id == id));
        }
        let handler = node.handlers.get(event).cloned().ok_or_else(|| EvalError { msg: "No handler registered for this event".into() })?;
        let event_value = decode(&value)?;
        if event == "change" {
            let mut next = node.props.clone(); next.insert("value".into(), value);
            validate(&node.kind, &next)?; self.gui.nodes[index].props = next;
        }
        let payload = Value::Record(Arc::new(vec![("target".into(), handle(id.into())), ("type".into(), Value::Str(event.into())), ("value".into(), event_value)]));
        self.call_named(&handler, vec![payload], None, None, vec![])?;
        if event == "close" { self.gui.nodes[index].props.insert("visible".into(), json!(false)); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_handler_changes_widget_without_rerunning_script() {
        let mut it = Interp::new();
        it.run("t = Frame(\"Test\")\np = t.add(\"panel\")\ns = p.add(\"label\", text=\"Ready\")\nb = p.add(\"button\", text=\"Start\")\nfunction clicked(event)\n s.set(text=\"Running\")\nend\nb.on(\"click\", \"clicked\")\nt.show()\n").unwrap();
        it.gui_event("gui-3", "click", Json::Null).unwrap();
        assert_eq!(it.gui_snapshot()["nodes"][2]["props"]["text"], "Running");
        assert_eq!(it.gui.nodes.len(), 4);
        assert!(it.gui_event("gui-3", "change", json!(5)).is_err());
    }
    #[test]
    fn validates_plot_updates_atomically() {
        let mut it = Interp::new();
        it.run("t = Frame(\"Plot\")\np = t.add(\"plot\", x=[1,2], y=[3,4], equal_aspect=true)\n").unwrap();
        assert!(it.run("p.set(x=[1])").is_err());
        assert_eq!(it.gui_snapshot()["nodes"][1]["props"]["x"], json!([1.0,2.0]));
        it.run("p.set(x=[1], y=[5])").unwrap();
        assert_eq!(it.gui_snapshot()["nodes"][1]["props"]["y"], json!([5.0]));
    }
    #[test]
    fn select_keeps_its_value_inside_its_options() {
        let mut it = Interp::new();
        it.run("t = Frame(\"Pick\")\ns = t.add(\"select\", options=[\"sine\", \"square\"], value=\"sine\")\nfunction picked(event)\n print(event.value)\nend\ns.on(\"change\", \"picked\")\nt.show()\n").unwrap();
        assert!(it.run("s.set(value=\"ramp\")").is_err());
        assert_eq!(it.gui_snapshot()["nodes"][1]["props"]["value"], "sine");
        it.run("s.set(options=[\"ramp\"], value=\"ramp\")").unwrap();
        assert_eq!(it.gui_snapshot()["nodes"][1]["props"]["options"], json!(["ramp"]));
        // A host echoing a stale option back must be refused, not stored.
        assert!(it.gui_event("gui-1", "change", json!("sine")).is_err());
        it.gui_event("gui-1", "change", json!("ramp")).unwrap();
        assert!(it.out.contains("ramp"));
        assert!(it.run("t.add(\"select\", options=[1, 2])").is_err());
    }
    #[test]
    fn events_reject_invalid_values_and_hidden_controls() {
        let mut it = Interp::new();
        it.run("t = Frame(\"Test\")\ns = t.add(\"slider\", min=0, max=5, value=1)\nfunction changed(event)\n print(event.value)\nend\ns.on(\"change\", \"changed\")\nt.show()\n").unwrap();
        assert!(it.gui_event("gui-1", "change", json!(6)).is_err());
        assert_eq!(it.gui_snapshot()["nodes"][1]["props"]["value"], json!(1.0));
        it.gui_event("gui-1", "change", json!(3)).unwrap();
        assert!(it.out.contains('3'));
        it.run("t.set(visible=false)").unwrap();
        assert!(it.gui_event("gui-1", "change", json!(2)).is_err());
    }
}

//! `qu lsp` over a real pipe, with real LSP framing.
//!
//! The in-process tests in `src/lsp.rs` cover the handlers. These cover what
//! those cannot: that the BINARY has the subcommand, reads `Content-Length`
//! frames from stdin, writes them to stdout with nothing else mixed in, and
//! exits with the status the protocol asks for.
//!
//! Every assertion is on content that an empty or stubbed reply would not
//! have (a diagnostic's line, `butter`'s documented signature, a specific
//! symbol name), and every read has a timeout: a server that answers
//! nothing must fail these, not hang them.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

const URI: &str = "file:///c%3A/work/broken.qu";

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    next_id: i64,
}

impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qu"))
            .args(["lsp", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn qu lsp");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        // A reader thread, so the test side can wait with a timeout.
        std::thread::spawn(move || {
            let mut rd = BufReader::new(stdout);
            loop {
                let mut len = None;
                loop {
                    let mut line = String::new();
                    if rd.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    assert!(line.ends_with("\r\n"), "header lines end in CRLF, got {line:?}");
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    let (k, v) = line.split_once(':').unwrap_or_else(|| panic!("not a header line: {line:?}"));
                    if k.eq_ignore_ascii_case("content-length") {
                        len = Some(v.trim().parse::<usize>().expect("numeric Content-Length"));
                    }
                }
                let mut body = vec![0u8; len.expect("a Content-Length header")];
                if rd.read_exact(&mut body).is_err() {
                    return;
                }
                let v: Value = serde_json::from_slice(&body).expect("the body is exactly Content-Length bytes of JSON");
                if tx.send(v).is_err() {
                    return;
                }
            }
        });
        Client { child, stdin, rx, next_id: 0 }
    }

    fn send(&mut self, msg: Value) {
        let body = msg.to_string();
        let stdin = self.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn recv(&mut self) -> Value {
        self.rx.recv_timeout(Duration::from_secs(20)).expect("the server sent nothing within 20 s")
    }

    /// Send a request and return its `result`, failing on an error reply.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let msg = self.recv();
            if msg.get("id") == Some(&json!(id)) {
                assert!(msg.get("error").is_none(), "{method} failed: {msg}");
                assert_eq!(msg["jsonrpc"], "2.0");
                return msg["result"].clone();
            }
        }
    }

    /// The next `publishDiagnostics` notification.
    fn diagnostics(&mut self) -> Value {
        loop {
            let msg = self.recv();
            if msg["method"] == "textDocument/publishDiagnostics" {
                return msg["params"].clone();
            }
        }
    }

    fn initialize(&mut self) -> Value {
        let result = self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": null,
                "capabilities": {"textDocument": {
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                    "signatureHelp": {"signatureInformation": {"parameterInformation": {"labelOffsetSupport": true}}},
                }},
            }),
        );
        self.notify("initialized", json!({}));
        result
    }

    fn open(&mut self, text: &str) {
        self.notify("textDocument/didOpen", json!({"textDocument": {"uri": URI, "languageId": "qu", "version": 1, "text": text}}));
    }

    fn at(line: u32, character: u32) -> Value {
        json!({"textDocument": {"uri": URI}, "position": {"line": line, "character": character}})
    }

    /// `shutdown` + `exit`, returning the process exit code.
    fn finish(mut self) -> i32 {
        let r = self.request("shutdown", Value::Null);
        assert_eq!(r, Value::Null);
        self.notify("exit", Value::Null);
        self.wait()
    }

    fn wait(&mut self) -> i32 {
        for _ in 0..200 {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        panic!("qu lsp did not exit within 10 s");
    }
}

/// Line 3 (0-based) is the broken one: a stray closing parenthesis.
const BROKEN: &str = "\
#%% Design
# --- Filter design ---
fs = 1000 Hz
lp = butter(4, \"low\", 50 Hz, fs))
function gain_db(x)
    return 20 * log10(abs(x))
end function
y = but
";

#[test]
fn initialize_advertises_every_feature_the_server_implements() {
    let mut c = Client::start();
    let init = c.initialize();
    let caps = &init["capabilities"];
    assert_eq!(caps["textDocumentSync"]["openClose"], true, "{caps}");
    assert_eq!(caps["textDocumentSync"]["change"], 1, "full-text sync: {caps}");
    for provider in ["hoverProvider", "documentSymbolProvider", "definitionProvider"] {
        assert_eq!(caps[provider], true, "{provider}: {caps}");
    }
    let triggers = caps["completionProvider"]["triggerCharacters"].as_array().expect("completion trigger characters");
    for ch in ["a", "Z", "_", "."] {
        assert!(triggers.contains(&json!(ch)), "identifier character {ch:?} must trigger completion: {triggers:?}");
    }
    assert_eq!(caps["signatureHelpProvider"]["triggerCharacters"], json!(["(", ","]));
    assert_eq!(init["serverInfo"]["name"], "qu lsp");
    assert_eq!(c.finish(), 0, "shutdown then exit is a clean exit");
}

#[test]
fn a_broken_file_gets_a_diagnostic_on_the_broken_line_and_a_fix_clears_it() {
    let mut c = Client::start();
    c.initialize();
    c.open(BROKEN);
    let d = c.diagnostics();
    assert_eq!(d["uri"], URI);
    assert_eq!(d["version"], 1);
    let list = d["diagnostics"].as_array().expect("a diagnostics array");
    assert_eq!(list.len(), 1, "the parser reports its first error: {d}");
    let diag = &list[0];
    // What `qu parse` itself says about this text, asked of the real binary.
    let dir = std::env::temp_dir().join(format!("qu-lsp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("broken.qu");
    std::fs::write(&file, BROKEN).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_qu")).args(["parse", "--json"]).arg(&file).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let cli: Value = serde_json::from_slice(&out.stdout).expect("qu parse --json output");
    let expected_line = cli["diagnostics"][0]["line"].as_u64().expect("qu parse reports a line") - 1;
    let expected_col = cli["diagnostics"][0]["column"].as_u64().unwrap() - 1;

    assert_eq!(diag["range"]["start"]["line"].as_u64(), Some(expected_line), "{diag}");
    // This file is ASCII, so the parser's char column is the UTF-16 column.
    assert_eq!(diag["range"]["start"]["character"].as_u64(), Some(expected_col), "{diag}");
    assert_eq!(diag["message"], cli["diagnostics"][0]["message"], "the parser's own message, verbatim");
    assert_eq!((expected_line, expected_col), (3, 32), "the stray `)` is at 4:33 in the fixture: {cli}");
    assert_eq!(diag["severity"], 1);
    assert_ne!(diag["range"]["start"], diag["range"]["end"], "a zero-width range is invisible: {diag}");

    // Full-text change that fixes it: the list must come back empty.
    let fixed = BROKEN.replace("fs))", "fs)").replace("y = but\n", "");
    c.notify("textDocument/didChange", json!({"textDocument": {"uri": URI, "version": 2}, "contentChanges": [{"text": fixed}]}));
    let d = c.diagnostics();
    assert_eq!(d["version"], 2);
    assert_eq!(d["diagnostics"], json!([]), "{d}");
    assert_eq!(c.finish(), 0);
}

#[test]
fn hover_completion_signature_help_symbols_and_definition_answer_with_content() {
    let mut c = Client::start();
    c.initialize();
    c.open(BROKEN);
    c.diagnostics();

    // Hover on `butter` (line 3, inside the name).
    let hover = c.request("textDocument/hover", Client::at(3, 7));
    assert_eq!(hover["contents"]["kind"], "markdown", "{hover}");
    let md = hover["contents"]["value"].as_str().expect("hover text");
    assert!(md.contains("butter(order, kind, cutoff, fs)"), "the documented signature: {md}");
    assert!(md.contains("Model"), "the documented summary: {md}");
    assert_eq!(hover["range"]["start"], json!({"line": 3, "character": 5}));
    assert_eq!(hover["range"]["end"], json!({"line": 3, "character": 11}));

    // Hover on a user function: its header line.
    let fixed = format!("{}z = gain_db(y)\n", BROKEN);
    c.notify("textDocument/didChange", json!({"textDocument": {"uri": URI, "version": 2}, "contentChanges": [{"text": fixed}]}));
    c.diagnostics();
    let hover = c.request("textDocument/hover", Client::at(8, 6));
    assert!(hover["contents"]["value"].as_str().unwrap().contains("function gain_db(x)"), "{hover}");

    // Completion for `but` (line 7, after `y = but`).
    let completion = c.request("textDocument/completion", Client::at(7, 7));
    let items = completion["items"].as_array().expect("completion items");
    assert!(!items.is_empty(), "no completions for `but`");
    let butter = items.iter().find(|i| i["label"] == "butter").unwrap_or_else(|| panic!("`butter` not offered: {completion}"));
    assert_eq!(butter["detail"], "butter(order, kind, cutoff, fs)");
    assert_eq!(butter["kind"], 3);
    assert!(butter["documentation"]["value"].as_str().unwrap().contains("Model"), "{butter}");
    assert!(
        items.iter().all(|i| i["label"].as_str().unwrap().to_lowercase().starts_with("but")),
        "only names starting with the typed prefix: {completion}"
    );
    // The document's own names, and a keyword.
    let completion = c.request("textDocument/completion", Client::at(5, 5)); // `    r|eturn`
    let labels: Vec<&str> = completion["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"return"), "{labels:?}");
    let completion = c.request("textDocument/completion", Client::at(7, 5)); // `y = b|ut` -> prefix `b`
    assert!(completion["items"].as_array().unwrap().len() > 5);
    let completion = c.request("textDocument/completion", Client::at(4, 10)); // inside `gain_db`, prefix `g`
    let items = completion["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["label"] == "gain_db" && i["detail"] == "gain_db(x)"), "{completion}");
    let completion = c.request("textDocument/completion", Client::at(3, 1)); // `l|p`
    assert!(completion["items"].as_array().unwrap().iter().any(|i| i["label"] == "lp" && i["kind"] == 6), "{completion}");

    // Signature help inside `butter(4, "low", |`.
    let help = c.request("textDocument/signatureHelp", Client::at(3, 22));
    assert_eq!(help["signatures"][0]["label"], "butter(order, kind, cutoff, fs)", "{help}");
    assert_eq!(help["activeParameter"], 2, "two commas before the cursor: {help}");
    assert_eq!(help["signatures"][0]["parameters"].as_array().unwrap().len(), 4);
    assert_eq!(help["signatures"][0]["parameters"][2]["label"], json!([20, 26]), "`cutoff`");

    // Document symbols: the cell, the section inside it, the function inside that.
    let symbols = c.request("textDocument/documentSymbol", json!({"textDocument": {"uri": URI}}));
    let roots = symbols.as_array().expect("a symbol array");
    assert_eq!(roots.len(), 1, "{symbols}");
    assert_eq!(roots[0]["name"], "Design");
    let section = &roots[0]["children"][0];
    assert_eq!(section["name"], "Filter design", "{symbols}");
    let function = &section["children"][0];
    assert_eq!(function["name"], "gain_db", "{symbols}");
    assert_eq!(function["kind"], 12);
    assert_eq!(function["detail"], "(x)");
    assert_eq!(function["range"]["start"]["line"], 4);
    assert_eq!(function["range"]["end"]["line"], 6);
    assert_eq!(function["selectionRange"]["start"], json!({"line": 4, "character": 9}));

    // Definition of `gain_db` from its call site on line 8.
    let def = c.request("textDocument/definition", Client::at(8, 6));
    assert_eq!(def["uri"], URI);
    assert_eq!(def["range"]["start"], json!({"line": 4, "character": 9}), "{def}");
    assert_eq!(def["range"]["end"], json!({"line": 4, "character": 16}));

    assert_eq!(c.finish(), 0);
}

#[test]
fn a_client_without_hierarchical_symbol_support_gets_the_flat_form() {
    let mut c = Client::start();
    c.request("initialize", json!({"processId": null, "rootUri": null, "capabilities": {}}));
    c.notify("initialized", json!({}));
    c.open(BROKEN);
    let symbols = c.request("textDocument/documentSymbol", json!({"textDocument": {"uri": URI}}));
    let names: Vec<&str> = symbols.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Design", "Filter design", "gain_db"]);
    assert_eq!(symbols[2]["location"]["uri"], URI, "{symbols}");
    // And string parameter labels rather than offsets.
    let help = c.request("textDocument/signatureHelp", Client::at(3, 22));
    assert_eq!(help["signatures"][0]["parameters"][2]["label"], "cutoff", "{help}");
    assert_eq!(c.finish(), 0);
}

#[test]
fn columns_are_utf16_code_units() {
    let mut c = Client::start();
    c.initialize();
    // `𝛼` is two UTF-16 units and four UTF-8 bytes; `é` is one and two.
    c.open("s = \"𝛼é\" + butter(2, \"low\", 5, 100) +\n");
    let d = c.diagnostics();
    assert_eq!(d["diagnostics"].as_array().unwrap().len(), 1, "{d}");
    // `butter` starts at UTF-16 column 12: `s = "` is 5, `𝛼` 2, `é` 1, `" + ` 4.
    let hover = c.request("textDocument/hover", Client::at(0, 14));
    assert_eq!(hover["range"]["start"], json!({"line": 0, "character": 12}), "{hover}");
    assert_eq!(hover["range"]["end"], json!({"line": 0, "character": 18}));
    assert_eq!(c.request("textDocument/hover", Client::at(0, 6)), Value::Null, "inside the string literal");
    assert_eq!(c.finish(), 0);
}

#[test]
fn exit_without_shutdown_is_exit_code_1_and_a_closed_pipe_ends_the_server() {
    let mut c = Client::start();
    c.initialize();
    c.notify("exit", Value::Null);
    assert_eq!(c.wait(), 1, "the protocol: exit code 1 if shutdown was not requested first");

    let mut c = Client::start();
    c.initialize();
    c.stdin = None; // close the pipe: the editor died
    assert_eq!(c.wait(), 0, "end of input is not an error");
}

#[test]
fn an_unknown_request_is_an_error_reply_and_garbage_json_does_not_kill_the_server() {
    let mut c = Client::start();
    c.initialize();
    c.send(json!({"jsonrpc": "2.0", "id": 77, "method": "textDocument/rename", "params": {}}));
    let reply = c.recv();
    assert_eq!(reply["id"], 77);
    assert_eq!(reply["error"]["code"], -32601, "{reply}");

    let stdin = c.stdin.as_mut().unwrap();
    write!(stdin, "Content-Length: 5\r\n\r\n{{oops").unwrap();
    stdin.flush().unwrap();
    let reply = c.recv();
    assert_eq!(reply["error"]["code"], -32700, "{reply}");
    assert_eq!(reply["id"], Value::Null);
    // Still alive and answering.
    c.open("x = 1\n");
    assert_eq!(c.diagnostics()["diagnostics"], json!([]));
    assert_eq!(c.finish(), 0);
}

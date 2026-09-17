//! Native acquisition runs independently of rendering, including while paused.
use crate::serial_protocol::{Decoder, Lines, FRAME_SIZE};
use serde::Serialize;
use std::{collections::VecDeque, fs::File, io::{Read, Write, BufWriter}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, SystemTime, UNIX_EPOCH}};
use tauri::State;

#[derive(Clone, Serialize)]
pub struct Frame { id: u64, rows: Vec<Vec<f64>> }
#[derive(Default)]
struct Capture {
    decoder: Decoder,
    signal: VecDeque<Frame>,
    impedance: VecDeque<Frame>,
    console: VecDeque<String>,
    lines: u64,
    version: u64,
    running: bool,
    error: Option<String>,
    recording: Option<(String, BufWriter<File>)>,
}
impl Capture {
    fn ingest(&mut self, line: String) {
        self.lines += 1;
        if let Some((_, writer)) = self.recording.as_mut() {
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
            if let Err(error) = writeln!(writer, "{timestamp:.6},\"{}\"", line.replace('"', "\"\"")) {
                self.error = Some(format!("Recording stopped: {error}"));
                self.recording = None;
            }
        }
        if let Some((mode, rows)) = self.decoder.push(&line) {
            self.version += 1;
            let history = if mode == "signal" { &mut self.signal } else { &mut self.impedance };
            if history.len() == 15 { history.pop_front(); }
            history.push_back(Frame { id: self.version, rows });
        }
        if self.console.len() == 5000 { self.console.pop_front(); }
        self.console.push_back(line);
    }
    fn flush(&mut self) {
        if let Some((_, writer)) = self.recording.as_mut() {
            if let Err(error) = writer.flush() {
                self.error = Some(format!("Recording stopped: {error}"));
                self.recording = None;
            }
        }
    }
}
struct Session { id: String, capture: Arc<Mutex<Capture>>, stop: Arc<AtomicBool>, worker: Option<thread::JoinHandle<()>> }
impl Session {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}
impl Drop for Session { fn drop(&mut self) { self.stop(); } }
#[derive(Default)]
pub struct SeriPlotState(Mutex<Option<Session>>);

#[derive(Serialize)]
pub struct Port { name: String, description: String }
#[tauri::command]
pub fn seriplot_ports() -> Result<Vec<Port>, String> {
    let mut ports = serialport::available_ports().map_err(|e| e.to_string())?.into_iter().map(|port| {
        let description = match port.port_type {
            serialport::SerialPortType::UsbPort(info) => [info.manufacturer, info.product].into_iter().flatten().collect::<Vec<_>>().join(" · "),
            serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
            _ => String::new(),
        };
        Port { name: port.port_name, description }
    }).collect::<Vec<_>>();
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ports)
}

#[tauri::command]
pub fn seriplot_start(state: State<'_, SeriPlotState>, id: String, port: Option<String>, baud: u32, demo: Option<String>) -> Result<(), String> {
    if id.is_empty() { return Err("A session identifier is required".into()); }
    if !matches!(demo.as_deref(), None | Some("signal" | "impedance")) { return Err("Unknown demo mode".into()); }
    if !(300..=3_000_000).contains(&baud) { return Err("Baud rate must be between 300 and 3000000".into()); }
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if guard.as_ref().is_some_and(|s| s.capture.lock().map(|c| c.running).unwrap_or(true)) {
        return Err("Disconnect the current session first".into());
    }
    let device = if demo.is_some() { None } else {
        Some(serialport::new(port.ok_or("Choose a serial port")?, baud).timeout(Duration::from_millis(100)).open().map_err(|e| e.to_string())?)
    };
    let capture = Arc::new(Mutex::new(Capture { running: true, ..Default::default() }));
    let stop = Arc::new(AtomicBool::new(false));
    let shared = capture.clone(); let stopped = stop.clone();
    let worker = thread::Builder::new().name("qu-seriplot".into()).spawn(move || {
        if let Some(mut device) = device {
            let mut chunks = Lines::default(); let mut bytes = [0; 8192];
            let mut last_flush = std::time::Instant::now();
            while !stopped.load(Ordering::Relaxed) {
                match device.read(&mut bytes) {
                    Ok(0) => thread::sleep(Duration::from_millis(5)),
                    Ok(n) => {
                        let lines = chunks.push(&bytes[..n]);
                        let mut capture = shared.lock().unwrap();
                        for line in lines { capture.ingest(line); }
                    }
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {},
                    Err(e) => { shared.lock().unwrap().error = Some(format!("Serial connection lost: {e}")); break; }
                }
                if last_flush.elapsed() >= Duration::from_millis(100) { shared.lock().unwrap().flush(); last_flush = std::time::Instant::now(); }
            }
        } else {
            let mut tick = 0usize;
            while !stopped.load(Ordering::Relaxed) {
                {
                    let mut capture = shared.lock().unwrap();
                    if demo.as_deref() == Some("signal") {
                        for sample in 0..FRAME_SIZE {
                            let phase = sample as f64 / 45.0 + tick as f64 * 0.05;
                            capture.ingest(format!("{:.5},{:.5}", 2048.0 + 1200.0 * phase.sin(), 2048.0 + 600.0 * (phase - 0.6).sin()));
                        }
                    } else {
                        for index in 0..64 {
                            let w = 0.04 * 1.12f64.powi(index);
                            let re = 20.0 + 100.0 / (1.0 + w*w);
                            let im = -100.0 * w / (1.0 + w*w);
                            let magnitude = re.hypot(im) * (1.0 + 0.02 * (tick as f64 * 0.1).sin());
                            capture.ingest(format!("{index},{magnitude:.6},{:.6},{:.6},{:.6}", im.atan2(re).to_degrees(), re.hypot(im), im.atan2(re).to_degrees()));
                        }
                    }
                    capture.flush();
                }
                tick += 1;
                // Short intervals also bound disconnect and application shutdown.
                for _ in 0..5 { if stopped.load(Ordering::Relaxed) { break; } thread::sleep(Duration::from_millis(50)); }
            }
        }
        let mut capture = shared.lock().unwrap();
        capture.flush(); capture.recording = None; capture.running = false;
    }).map_err(|e| e.to_string())?;
    *guard = Some(Session { id, capture, stop, worker: Some(worker) });
    Ok(())
}

#[derive(Serialize)]
pub struct Snapshot {
    running: bool, mode: Option<&'static str>, lines: u64, version: u64, pending: usize,
    ignored: u64, discarded: u64, recording: Option<String>, error: Option<String>,
    console: Vec<String>, signal: Option<Vec<Frame>>, impedance: Option<Vec<Frame>>,
}
#[tauri::command]
pub fn seriplot_poll(state: State<'_, SeriPlotState>, id: String, version: u64) -> Result<Snapshot, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().filter(|s| s.id == id).ok_or("Session ended")?;
    let mut capture = session.capture.lock().map_err(|e| e.to_string())?;
    let changed = capture.version != version;
    Ok(Snapshot {
        running: capture.running, mode: capture.decoder.mode, lines: capture.lines, version: capture.version,
        pending: capture.decoder.pending.len(), ignored: capture.decoder.ignored, discarded: capture.decoder.discarded,
        recording: capture.recording.as_ref().map(|(path, _)| path.clone()), error: capture.error.clone(),
        console: capture.console.drain(..).collect(),
        signal: changed.then(|| capture.signal.iter().cloned().collect()),
        impedance: changed.then(|| capture.impedance.iter().cloned().collect()),
    })
}
#[tauri::command]
pub fn seriplot_stop(state: State<'_, SeriPlotState>, id: String) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(session) = guard.as_mut().filter(|s| s.id == id) { session.stop(); }
    Ok(())
}
#[tauri::command]
pub fn seriplot_record(state: State<'_, SeriPlotState>, id: String, path: Option<String>) -> Result<(), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().filter(|s| s.id == id).ok_or("Session ended")?;
    let mut capture = session.capture.lock().map_err(|e| e.to_string())?;
    if let Some(path) = path {
        if !capture.running { return Err("Connect before recording".into()); }
        if capture.recording.is_some() { return Err("Recording is already active".into()); }
        let mut writer = BufWriter::new(File::create(&path).map_err(|e| e.to_string())?);
        writer.write_all(b"timestamp_unix_seconds,raw_line\n").map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        capture.recording = Some((path, writer));
    } else {
        if let Some((_, writer)) = capture.recording.as_mut() { writer.flush().map_err(|e| e.to_string())?; }
        capture.recording = None;
    }
    Ok(())
}
#[tauri::command]
pub fn seriplot_buffer(state: State<'_, SeriPlotState>, id: String, mode: String) -> Result<String, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().filter(|s| s.id == id).ok_or("Session ended")?;
    let capture = session.capture.lock().map_err(|e| e.to_string())?;
    let (history, mut csv) = match mode.as_str() {
        "signal" => (&capture.signal, String::from("frame,sample,adc1,adc2\n")),
        "impedance" => (&capture.impedance, String::from("sweep,index,magnitude,phase,acc_magnitude,acc_phase\n")),
        _ => return Err("Unknown buffer mode".into()),
    };
    for (index, frame) in history.iter().enumerate() {
        for (sample, row) in frame.rows.iter().enumerate() {
            csv.push_str(&format!("{index},"));
            if mode == "signal" { csv.push_str(&format!("{sample},")); }
            csv.push_str(&row.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")); csv.push('\n');
        }
    }
    Ok(csv)
}

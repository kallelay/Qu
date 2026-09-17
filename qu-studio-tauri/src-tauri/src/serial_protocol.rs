//! ColorfulFlower / SeriPlot wire protocol. No UI or serial-driver dependency.
//! A sweep is complete only when the next zero index arrives.
pub const FRAME_SIZE: usize = 2048;
const MAX_SWEEP: usize = 65536;
const MAX_LINE: usize = 4096;

#[derive(Default)]
pub struct Lines {
    bytes: Vec<u8>,
    overflow: bool,
}
impl Lines {
    /// Preserve partial reads across timeouts; discard an oversized line in full.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if !self.overflow {
                    lines.push(String::from_utf8_lossy(&self.bytes).trim_end_matches('\r').to_owned());
                }
                self.bytes.clear();
                self.overflow = false;
            } else if !self.overflow {
                if self.bytes.len() == MAX_LINE {
                    self.bytes.clear();
                    self.overflow = true;
                } else {
                    self.bytes.push(byte);
                }
            }
        }
        lines
    }
}

#[derive(Default)]
pub struct Decoder {
    pub mode: Option<&'static str>,
    pub pending: Vec<Vec<f64>>,
    pub ignored: u64,
    pub discarded: u64,
    waiting_for_zero: bool,
}
impl Decoder {
    pub fn push(&mut self, line: &str) -> Option<(&'static str, Vec<Vec<f64>>)> {
        let row = line.split(',').map(|s| s.trim().parse::<f64>()).collect::<Result<Vec<_>, _>>();
        let row = match row {
            Ok(row) if matches!(row.len(), 2 | 5) && row.iter().all(|x| x.is_finite()) => row,
            _ => { self.ignored += 1; return None; }
        };
        if row.len() == 5 && (row[0] < 0.0 || row[0].fract() != 0.0 || row[0] > 9_007_199_254_740_991.0) {
            self.ignored += 1;
            return None;
        }
        let mode = if row.len() == 2 { "signal" } else { "impedance" };
        if self.mode != Some(mode) {
            self.discarded += self.pending.len() as u64;
            self.pending.clear();
            self.mode = Some(mode);
            self.waiting_for_zero = mode == "impedance";
        }
        if mode == "impedance" && self.waiting_for_zero {
            if row[0] != 0.0 { self.discarded += 1; return None; }
            self.waiting_for_zero = false;
        }
        let completed = if mode == "impedance" && row[0] == 0.0 && !self.pending.is_empty() {
            Some((mode, std::mem::take(&mut self.pending)))
        } else { None };
        self.pending.push(row);
        if mode == "signal" && self.pending.len() == FRAME_SIZE {
            return Some((mode, std::mem::take(&mut self.pending)));
        }
        if self.pending.len() > MAX_SWEEP {
            self.discarded += self.pending.len() as u64;
            self.pending.clear();
            self.waiting_for_zero = true;
        }
        completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunks_and_crlf_preserve_complete_lines() {
        let mut lines = Lines::default();
        assert!(lines.push(b"12,").is_empty());
        assert_eq!(lines.push(b"34\r\n5,6\n7,"), ["12,34", "5,6"]);
        assert_eq!(lines.push(b"8\n"), ["7,8"]);
        assert!(lines.push(&vec![b'x'; MAX_LINE + 1]).is_empty());
        assert_eq!(lines.push(b"1,2\n3,4\n"), ["3,4"]);
    }
    #[test]
    fn exact_signal_frames_and_mode_switch() {
        let mut decoder = Decoder::default();
        for _ in 0..FRAME_SIZE - 1 { assert!(decoder.push("1,2").is_none()); }
        let (mode, rows) = decoder.push("3,4").unwrap();
        assert_eq!(mode, "signal"); assert_eq!(rows.len(), FRAME_SIZE);
        assert_eq!(rows[FRAME_SIZE - 1], [3.0, 4.0]);
        decoder.push("1,2"); decoder.push("0,10,-90,20,-45");
        assert_eq!(decoder.discarded, 1);
        let (_, sweep) = decoder.push("0,11,-89,21,-44").unwrap();
        assert_eq!(sweep, vec![vec![0.0, 10.0, -90.0, 20.0, -45.0]]);
        assert_eq!(decoder.pending.len(), 1);
    }
    #[test]
    fn rejects_bad_rows_and_resynchronizes_at_zero() {
        let mut decoder = Decoder::default();
        for bad in ["boot v1.1", "NaN,2", "1,inf", "1,2,3", "0.5,1,2,3,4", "-1,1,2,3,4", "0,1,2,3,4junk"] {
            assert!(decoder.push(bad).is_none());
        }
        assert_eq!(decoder.ignored, 7);
        decoder.push("8,1,2,3,4"); assert!(decoder.pending.is_empty());
        decoder.push("0,1,2,3,4"); decoder.push("1,5,6,7,8");
        assert_eq!(decoder.push("0,9,10,11,12").unwrap().1.len(), 2);
    }
    #[test]
    fn missing_sweep_boundary_is_bounded() {
        let mut decoder = Decoder::default();
        decoder.push("0,1,2,3,4");
        for _ in 0..MAX_SWEEP { decoder.push("1,1,2,3,4"); }
        assert!(decoder.pending.is_empty());
        decoder.push("2,1,2,3,4"); assert!(decoder.pending.is_empty());
        decoder.push("0,1,2,3,4"); assert_eq!(decoder.pending.len(), 1);
    }
}

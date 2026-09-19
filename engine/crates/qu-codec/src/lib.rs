//! Audio codecs, as a Qu module rather than as builtins.
//!
//! `import codec` then `decode_flac(bytes)`, `decode_wav`, `decode_mp3`.
//! All three return the same `Audio`, so a caller that handles one
//! handles all of them and the interpreter has one conversion rather than
//! three. Nothing here enters the
//! global builtin table unless a script asks for it by name, which is the
//! point the module system exists to make -- see `MODULE_EXPORTS` in
//! `qu-interp`.
//!
//! The crate deals only in plain numbers: it takes bytes and returns
//! samples, a sample rate and a channel count. Turning that into a
//! `Value::Signal` or a `Value::Mat` is `qu-interp`'s job, so this crate
//! stays testable without an interpreter and reusable if anything else
//! ever wants decoded audio.

use std::io::Cursor;

/// Decoded audio: interleaving removed, one `Vec` per channel.
///
/// Per-channel rather than interleaved because that is the shape every
/// consumer wants -- a `Signal` is one channel, and a matrix is one
/// column per channel. Interleaved would make both callers de-interleave.
#[derive(Debug)]
pub struct Audio {
    /// One entry per channel, each `frames` long.
    pub channels: Vec<Vec<f64>>,
    pub sample_rate: f64,
    /// Bits per sample as the container declares them -- the only way to
    /// know what `-1.0 .. 1.0` was quantised to.
    ///
    /// `None` where the question has no answer: MP3 is lossy and its
    /// output is real numbers rather than a grid, and float WAV has a
    /// width but no quantisation step. Reporting a plausible 16 there
    /// would invite arithmetic that means nothing.
    pub bits: Option<u32>,
}

impl Audio {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

/// What a FLAC file says about itself, without decoding it.
///
/// Separate from `decode_flac` because reading a header is cheap and
/// decoding a long recording is not: a script that only needs the sample
/// rate should not pay for the samples.
#[derive(Debug)]
pub struct FlacInfo {
    pub sample_rate: f64,
    pub channels: usize,
    pub bits: u32,
    /// `None` when the stream does not declare its length, which is legal
    /// FLAC -- a live capture cannot know it in advance. Reported honestly
    /// rather than as 0, which would read as an empty file.
    pub frames: Option<u64>,
}

/// Read a FLAC stream's header.
pub fn flac_info(bytes: &[u8]) -> Result<FlacInfo, String> {
    let reader = claxon::FlacReader::new(Cursor::new(bytes)).map_err(describe)?;
    let info = reader.streaminfo();
    Ok(FlacInfo {
        sample_rate: info.sample_rate as f64,
        channels: info.channels as usize,
        bits: info.bits_per_sample,
        frames: if info.samples == Some(0) { None } else { info.samples },
    })
}

/// Decode a FLAC stream to samples in `-1.0 ..= 1.0`.
///
/// Normalised on the way out rather than left as raw integers, because
/// every signal-processing builtin in Qu works in floats and a script that
/// got 24-bit integers back would have to know the depth to do anything
/// with them. The scale is `2^(bits-1)`, so full-scale is exactly 1.0 and
/// the conversion is lossless in the direction that matters.
pub fn decode_flac(bytes: &[u8]) -> Result<Audio, String> {
    let mut reader = claxon::FlacReader::new(Cursor::new(bytes)).map_err(describe)?;
    let info = reader.streaminfo();
    let n_channels = info.channels as usize;
    if n_channels == 0 {
        return Err("decode_flac: the stream declares 0 channels".into());
    }
    if info.bits_per_sample == 0 || info.bits_per_sample > 32 {
        return Err(format!(
            "decode_flac: {} bits per sample is outside what FLAC allows (1-32)",
            info.bits_per_sample
        ));
    }
    let scale = 1.0 / (1i64 << (info.bits_per_sample - 1)) as f64;

    let hint = info.samples.unwrap_or(0) as usize;
    let mut channels: Vec<Vec<f64>> = (0..n_channels)
        .map(|_| Vec::with_capacity(hint))
        .collect();

    // `samples()` yields interleaved i32 in channel order; splitting here
    // means the caller never sees interleaving.
    let mut ch = 0usize;
    for sample in reader.samples() {
        let v = sample.map_err(describe)?;
        channels[ch].push(v as f64 * scale);
        ch = (ch + 1) % n_channels;
    }
    if ch != 0 {
        return Err(
            "decode_flac: the stream ends mid-frame -- the file is truncated".into(),
        );
    }
    Ok(Audio {
        channels,
        sample_rate: info.sample_rate as f64,
        bits: Some(info.bits_per_sample),
    })
}


/// Decode a WAV stream to samples in `-1.0 ..= 1.0`.
///
/// Integer WAV is normalised by `2^(bits-1)` exactly as FLAC is, so the
/// same recording in either container gives the same numbers. Float WAV is
/// already in that range and is passed through untouched -- rescaling it
/// would be the one case where "normalising" changes the data.
///
/// 8-bit WAV is the odd one in the format: it is UNSIGNED, centred on 128,
/// where every other depth is signed. `hound` hands it back already
/// converted to signed, so the same scale works, but it is worth knowing
/// the exception exists before someone "fixes" it.
pub fn decode_wav(bytes: &[u8]) -> Result<Audio, String> {
    let mut reader =
        hound::WavReader::new(Cursor::new(bytes)).map_err(|e| describe_wav(e, "decode_wav"))?;
    let spec = reader.spec();
    let n_channels = spec.channels as usize;
    if n_channels == 0 {
        return Err("decode_wav: the file declares 0 channels".into());
    }
    let mut channels: Vec<Vec<f64>> = vec![Vec::new(); n_channels];
    let mut ch = 0usize;
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for s in reader.samples::<f32>() {
                channels[ch].push(s.map_err(|e| describe_wav(e, "decode_wav"))? as f64);
                ch = (ch + 1) % n_channels;
            }
        }
        hound::SampleFormat::Int => {
            if spec.bits_per_sample == 0 || spec.bits_per_sample > 32 {
                return Err(format!(
                    "decode_wav: {} bits per sample is not something WAV defines",
                    spec.bits_per_sample
                ));
            }
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f64;
            for s in reader.samples::<i32>() {
                let v = s.map_err(|e| describe_wav(e, "decode_wav"))?;
                channels[ch].push(v as f64 * scale);
                ch = (ch + 1) % n_channels;
            }
        }
    }
    if ch != 0 {
        return Err("decode_wav: the data ends mid-frame -- the file is truncated".into());
    }
    Ok(Audio {
        channels,
        sample_rate: spec.sample_rate as f64,
        // Float WAV has a bit width but not a quantisation step, so
        // reporting one would invite arithmetic that means nothing.
        bits: match spec.sample_format {
            hound::SampleFormat::Int => Some(spec.bits_per_sample as u32),
            hound::SampleFormat::Float => None,
        },
    })
}

fn describe_wav(err: hound::Error, who: &str) -> String {
    match err {
        hound::Error::FormatError(what) => format!("{who}: this is not valid WAV ({what})"),
        hound::Error::Unsupported => {
            format!("{who}: valid WAV, but not a form this build decodes")
        }
        hound::Error::IoError(e) => format!("{who}: {e}"),
        other => format!("{who}: {other}"),
    }
}

/// Whether to dither on the way down to an integer depth, and how.
///
/// The writer takes an `Option<Dither>`, and the `None` case is the
/// load-bearing one: it means the caller never named `dither` at all,
/// which is NOT the same request as naming `dither = "none"`. A plain
/// `bool` could not tell "no" from "unsaid", and that distinction is the
/// whole feature -- see `encode_wav`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dither {
    /// Quantise by rounding, nothing added.
    None,
    /// Triangular PDF, +/-1 LSB. The standard choice for audio: it
    /// decorrelates the quantisation error from the signal and leaves a
    /// flat noise floor, where rounding alone leaves harmonic distortion
    /// that is plainly audible on a fade or a quiet passage.
    Tpdf,
}

/// Encode samples in `-1.0 ..= 1.0` as a WAV stream.
///
/// The inverse of `decode_wav`, and deliberately its exact mirror: the
/// same `2^(depth-1)` scale, so a file decoded and re-encoded at its own
/// depth comes back bit-identical. That is testable, and it is tested.
///
/// `depth` is 16, 24 or 32. **32 means 32-bit FLOAT**, not 32-bit integer
/// PCM -- that is what the format's users mean by it, what the spec's own
/// format list names, and what `decode_wav` reads back without rescaling.
///
/// `dither` is `None` when the caller never named it. Going to 16 bits
/// that way is an ERROR rather than a silent truncation: samples are f64
/// here, 16-bit is the depth where the difference is audible, and the
/// spec makes naming it mandatory for exactly that reason. 24-bit
/// defaults to no dither, and 32-bit float refuses the argument outright
/// because it has no quantisation step to dither.
///
/// `uniform` supplies independent draws on `[0, 1)`. Passed in rather
/// than owned so this crate keeps no RNG and no `rand` dependency, and so
/// the interpreter can hand over its own seeded generator -- which is
/// what makes a dithered write reproducible under `seed()`.
pub fn encode_wav(
    channels: &[Vec<f64>],
    sample_rate: f64,
    depth: u16,
    dither: Option<Dither>,
    uniform: &mut dyn FnMut() -> f64,
) -> Result<Vec<u8>, String> {
    if channels.is_empty() {
        return Err("encode_wav: there are no channels to write".into());
    }
    let frames = channels[0].len();
    for (i, ch) in channels.iter().enumerate() {
        if ch.len() != frames {
            return Err(format!(
                "encode_wav: channel {i} has {} samples but channel 0 has {frames} -- \
                 every channel of one file has to be the same length",
                ch.len()
            ));
        }
    }
    if !(sample_rate.is_finite() && sample_rate > 0.0 && sample_rate <= u32::MAX as f64) {
        return Err(format!(
            "encode_wav: {sample_rate} is not a sample rate a WAV file can declare \
             (it holds a whole number of Hz, 1 .. 4294967295)"
        ));
    }
    if sample_rate.fract() != 0.0 {
        return Err(format!(
            "encode_wav: a WAV header holds a whole number of Hz, so {sample_rate} cannot \
             be written exactly -- round it first if that is what you meant"
        ));
    }

    // The policy, and the reason this function takes an `Option` rather
    // than a `Dither`.
    match (depth, dither) {
        (16, None) => {
            return Err(
                "encode_wav: depth = 16 quantises 64-bit samples down to 16 bits, and doing \
                 that without saying how is an error -- silent truncation of audio is \
                 audible. Name dither = \"tpdf\" (adds +/-1 LSB of triangular noise, which \
                 keeps the quantisation error flat and uncorrelated with the signal) or \
                 dither = \"none\" (plain rounding, which distorts fades and quiet passages)."
                    .into(),
            );
        }
        (32, Some(_)) => {
            return Err(
                "encode_wav: depth = 32 writes 32-bit float, which has no quantisation step \
                 -- there is nothing for dither to do. Drop the argument, or ask for \
                 depth = 24 or depth = 16 if you meant to quantise."
                    .into(),
            );
        }
        (16, Some(_)) | (24, _) | (32, None) => {}
        (other, _) => {
            return Err(format!(
                "encode_wav: depth = {other} is not a depth this writes -- 16, 24 or 32, \
                 32 being 32-bit float"
            ));
        }
    }

    let spec = hound::WavSpec {
        channels: channels.len() as u16,
        sample_rate: sample_rate as u32,
        bits_per_sample: depth,
        sample_format: if depth == 32 {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };

    let mut buf = Cursor::new(Vec::new());
    {
        let mut w =
            hound::WavWriter::new(&mut buf, spec).map_err(|e| describe_wav(e, "encode_wav"))?;
        // Interleaved on the way out -- what WAV stores, and the exact
        // inverse of the de-interleaving `decode_wav` does on the way in.
        for i in 0..frames {
            for (c, ch) in channels.iter().enumerate() {
                let v = ch[i];
                if !v.is_finite() {
                    return Err(format!(
                        "encode_wav: channel {c} sample {i} is {v}, which has no WAV \
                         representation -- it would land as silence and nothing downstream \
                         could tell it had"
                    ));
                }
                if depth == 32 {
                    w.write_sample(v as f32)
                        .map_err(|e| describe_wav(e, "encode_wav"))?;
                } else {
                    let q = quantise(v, depth, dither.unwrap_or(Dither::None), uniform);
                    w.write_sample(q).map_err(|e| describe_wav(e, "encode_wav"))?;
                }
            }
        }
        w.finalize().map_err(|e| describe_wav(e, "encode_wav"))?;
    }
    Ok(buf.into_inner())
}

/// One sample, `-1.0 ..= 1.0` to a `depth`-bit two's-complement integer.
///
/// The scale is `2^(depth-1)`, matching `decode_wav` exactly, so `-1.0`
/// lands on full negative scale and a value that came out of a file of
/// this depth returns to the integer it started as -- exactly, because
/// the scale is a power of two and so the division that produced it was
/// lossless.
///
/// `+1.0` is the one asymmetry, and it is the format's rather than ours:
/// two's complement has no `+2^(depth-1)`, so full positive scale clips to
/// `2^(depth-1) - 1`. Every other tool does the same.
fn quantise(v: f64, depth: u16, dither: Dither, uniform: &mut dyn FnMut() -> f64) -> i32 {
    let scale = (1i64 << (depth - 1)) as f64;
    let mut x = v * scale;
    if dither == Dither::Tpdf {
        // Two independent uniforms on [0,1) subtracted give a triangular
        // density on (-1, 1) LSB. That is TPDF -- the textbook dither for
        // audio, and the one the spec names.
        x += uniform() - uniform();
    }
    x.round().clamp(-scale, scale - 1.0) as i32
}

/// Decode an MP3 stream to samples in `-1.0 ..= 1.0`.
///
/// MP3 has no bit depth to report: it is lossy and its output is real
/// numbers, not a quantised grid, so `bits` comes back as `none` rather
/// than as a plausible 16 that nothing could correctly use.
///
/// A stream that is not MP3 at all is caught by the probe. A stream that
/// IS MP3 but has junk in front (an ID3 tag, a partial frame) is normal
/// and the demuxer skips it -- MP3 has no header, only a run of frames,
/// which is why "is this MP3?" is a weaker question than it is for FLAC or
/// WAV and why the error for a non-MP3 file is about failing to find
/// frames rather than about a bad magic number.
pub fn decode_mp3(bytes: &[u8]) -> Result<Audio, String> {
    use symphonia_core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
    use symphonia_core::codecs::CodecParameters;
    use symphonia_core::formats::{FormatOptions, FormatReader, TrackType};
    use symphonia_core::io::{MediaSourceStream, MediaSourceStreamOptions};

    let source = Box::new(Cursor::new(bytes.to_vec()));
    let stream = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
    let mut format = symphonia_bundle_mp3::MpaReader::try_new(stream, FormatOptions::default())
        .map_err(describe_mp3)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "decode_mp3: the stream declares no audio track".to_string())?;
    let track_id = track.id;
    let params = match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(a)) => a.clone(),
        _ => return Err("decode_mp3: the track carries no audio codec parameters".into()),
    };
    let mut decoder =
        symphonia_bundle_mp3::MpaDecoder::try_new(&params, &AudioDecoderOptions::default())
            .map_err(describe_mp3)?;

    let mut channels: Vec<Vec<f64>> = Vec::new();
    let mut sample_rate = 0.0f64;
    let mut planar: Vec<Vec<f32>> = Vec::new();
    // `next_packet` returns `Ok(None)` at the end of the stream, so the
    // loop ends on a value rather than on an error.
    while let Some(packet) = format.next_packet().map_err(describe_mp3)? {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(describe_mp3)?;
        sample_rate = decoded.spec().rate() as f64;
        // Planar and already per-channel, so there is no interleaving to
        // undo here -- unlike FLAC and WAV, where the container hands over
        // interleaved samples.
        decoded.copy_to_vecs_planar::<f32>(&mut planar);
        if channels.len() < planar.len() {
            channels.resize(planar.len(), Vec::new());
        }
        for (dst, src) in channels.iter_mut().zip(planar.iter()) {
            dst.extend(src.iter().map(|&v| v as f64));
        }
    }
    if channels.is_empty() || channels[0].is_empty() {
        return Err("decode_mp3: no audio frames -- this is probably not an MP3".into());
    }
    Ok(Audio { channels, sample_rate, bits: None })
}

fn describe_mp3(err: symphonia_core::errors::Error) -> String {
    use symphonia_core::errors::Error;
    match err {
        Error::DecodeError(what) => format!("decode_mp3: this is not valid MP3 ({what})"),
        Error::Unsupported(what) => {
            format!("decode_mp3: valid MP3, but this build cannot decode it ({what})")
        }
        Error::IoError(e) => format!("decode_mp3: {e}"),
        other => format!("decode_mp3: {other}"),
    }
}

/// Say what is wrong in Qu's voice rather than claxon's.
///
/// The common case by far is handing it something that is not FLAC at all
/// -- a `.wav` renamed, or a path read as text instead of bytes -- and
/// "failed to read" does not say that.
fn describe(err: claxon::Error) -> String {
    match err {
        claxon::Error::FormatError(what) => {
            format!("decode_flac: this is not valid FLAC ({what})")
        }
        claxon::Error::Unsupported(what) => {
            format!("decode_flac: valid FLAC, but this build cannot decode it ({what})")
        }
        claxon::Error::IoError(e) => format!("decode_flac: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_not_flac_says_so() {
        let err = decode_flac(b"RIFF....WAVEfmt ").unwrap_err();
        assert!(err.contains("not valid FLAC"), "got: {err}");
    }

    #[test]
    fn an_empty_input_is_an_error_not_an_empty_result() {
        assert!(decode_flac(&[]).is_err());
        assert!(flac_info(&[]).is_err());
    }
}

/// A minimal FLAC encoder, for tests only.
///
/// Exists so the decoder can be tested against a stream this crate built
/// from known samples, rather than against a binary fixture checked into
/// the repository. A fixture would test that the decoder still does what
/// it did; a round trip tests that it does what FLAC *says*, and it is
/// reviewable — a reader can check the bit layout below against the format
/// specification, which they cannot do with 171 opaque bytes.
///
/// VERBATIM subframes: samples stored raw, no prediction, no Rice coding.
/// That is the one encoding simple enough to write correctly in a test
/// helper, and it exercises the whole decode path anyway — header parsing,
/// frame sync, CRC verification, channel de-interleaving and the
/// fixed-point conversion — because none of those care how a subframe was
/// compressed.
#[cfg(test)]
pub(crate) mod encode {
    /// Bit-level writer. FLAC is not byte-aligned inside a frame.
    pub struct Bits {
        pub out: Vec<u8>,
        acc: u32,
        n: u32,
    }

    impl Bits {
        pub fn new() -> Self {
            Bits { out: Vec::new(), acc: 0, n: 0 }
        }
        pub fn put(&mut self, value: u32, width: u32) {
            for i in (0..width).rev() {
                let bit = (value >> i) & 1;
                self.acc = (self.acc << 1) | bit;
                self.n += 1;
                if self.n == 8 {
                    self.out.push(self.acc as u8);
                    self.acc = 0;
                    self.n = 0;
                }
            }
        }
        pub fn put_signed(&mut self, value: i32, width: u32) {
            let mask = if width == 32 { u32::MAX } else { (1u32 << width) - 1 };
            self.put((value as u32) & mask, width);
        }
        /// Pad with zeroes to the next byte, as a frame footer requires.
        pub fn align(&mut self) {
            while self.n != 0 {
                self.put(0, 1);
            }
        }
    }

    fn crc8(data: &[u8]) -> u8 {
        let mut crc = 0u8;
        for &b in data {
            crc ^= b;
            for _ in 0..8 {
                crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
            }
        }
        crc
    }

    fn crc16(data: &[u8]) -> u16 {
        let mut crc = 0u16;
        for &b in data {
            crc ^= (b as u16) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x8005 } else { crc << 1 };
            }
        }
        crc
    }

    /// `channels` is per-channel, all the same length. 16-bit samples.
    pub fn flac(channels: &[Vec<i32>], sample_rate: u32) -> Vec<u8> {
        let n_ch = channels.len() as u32;
        let frames = channels[0].len() as u32;

        let mut out = b"fLaC".to_vec();

        // STREAMINFO, and it is the last metadata block.
        let mut si = Bits::new();
        si.put(frames, 16); // min block size
        si.put(frames, 16); // max block size
        si.put(0, 24); // min frame size, 0 = unknown
        si.put(0, 24); // max frame size, 0 = unknown
        si.put(sample_rate, 20);
        si.put(n_ch - 1, 3);
        si.put(16 - 1, 5); // bits per sample, biased by one
        si.put(0, 4); // total samples, high nibble of 36 bits
        si.put(frames, 32); // total samples, low 32
        si.align();
        let mut sib = si.out;
        sib.extend_from_slice(&[0u8; 16]); // MD5 of the audio, zero = not present
        out.push(0x80); // last-block flag | block type 0 (STREAMINFO)
        out.extend_from_slice(&(sib.len() as u32).to_be_bytes()[1..]); // 24-bit length
        out.extend_from_slice(&sib);

        // One frame holding everything.
        let mut fh = Bits::new();
        fh.put(0b11111111111110, 14); // sync
        fh.put(0, 1); // reserved
        fh.put(0, 1); // fixed block size, so the number below is a frame number
        fh.put(0b0111, 4); // block size read as a 16-bit value after the header
        fh.put(0b0000, 4); // sample rate: take it from STREAMINFO
        fh.put(n_ch - 1, 4); // channel assignment: independent channels
        fh.put(0b100, 3); // sample size: 16 bits
        fh.put(0, 1); // reserved
        fh.put(0, 8); // frame number 0, as a one-byte UTF-8-style value
        fh.put(frames - 1, 16); // the block size promised above, biased by one
        let header = fh.out;
        let mut frame = header.clone();
        frame.push(crc8(&header));

        let mut body = Bits::new();
        for ch in channels {
            body.put(0, 1); // subframe header: zero bit
            body.put(0b000001, 6); // VERBATIM
            body.put(0, 1); // no wasted bits
            for &s in ch {
                body.put_signed(s, 16);
            }
        }
        body.align();
        frame.extend_from_slice(&body.out);
        frame.extend_from_slice(&crc16(&frame).to_be_bytes());

        out.extend_from_slice(&frame);
        out
    }
}

#[cfg(test)]
mod round_trip {
    use super::encode::flac;
    use crate::{decode_flac, flac_info};

    #[test]
    fn a_mono_stream_decodes_to_the_samples_it_was_built_from() {
        // At least 16 -- FLAC's smallest legal block. Worth knowing: the
        // first version of this test used 7 and the decoder said so.
        let samples: Vec<i32> = vec![
            0, 1000, -1000, 32767, -32768, 5, -5, 12345, -12345, 1, -1, 256,
            -256, 4096, -4096, 9999, -9999, 7,
        ];
        let bytes = flac(&[samples.clone()], 44100);

        let info = flac_info(&bytes).expect("built here, so it parses");
        assert_eq!(info.sample_rate, 44100.0);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bits, 16);
        assert_eq!(info.frames, Some(samples.len() as u64));

        let audio = decode_flac(&bytes).expect("built here, so it decodes");
        assert_eq!(audio.channels.len(), 1);
        assert_eq!(audio.frames(), samples.len());
        // Normalised by 2^15, so -32768 is exactly -1.0 and full positive
        // scale is one step short of +1.0 -- which is what two's complement
        // means and what every other tool reports.
        for (got, want) in audio.channels[0].iter().zip(&samples) {
            let expect = *want as f64 / 32768.0;
            assert!((got - expect).abs() < 1e-12, "got {got}, want {expect}");
        }
        assert_eq!(audio.channels[0][4], -1.0, "-32768 is full negative scale");
    }

    /// De-interleaving is the part most likely to be wrong and least
    /// likely to look wrong: a stereo file decoded as if it were mono
    /// produces plausible audio at half the pitch.
    #[test]
    fn a_stereo_stream_comes_back_as_two_separate_channels() {
        let left: Vec<i32> = (0..20).map(|i| (i + 1) * 100).collect();
        let right: Vec<i32> = left.iter().map(|v| -v).collect();
        let bytes = flac(&[left.clone(), right.clone()], 48000);

        let audio = decode_flac(&bytes).expect("built here, so it decodes");
        assert_eq!(audio.channels.len(), 2);
        assert_eq!(audio.sample_rate, 48000.0);
        for (got, want) in audio.channels[0].iter().zip(&left) {
            assert!((got - *want as f64 / 32768.0).abs() < 1e-12);
        }
        for (got, want) in audio.channels[1].iter().zip(&right) {
            assert!((got - *want as f64 / 32768.0).abs() < 1e-12);
        }
        assert!(
            audio.channels[0][0] > 0.0 && audio.channels[1][0] < 0.0,
            "the channels must not be swapped or merged"
        );
    }

    #[test]
    fn a_truncated_stream_is_an_error_not_a_short_result() {
        let bytes = flac(&[(0..32).collect::<Vec<i32>>()], 44100);
        let cut = &bytes[..bytes.len() - 4];
        assert!(decode_flac(cut).is_err(), "half a frame must not decode as data");
    }
}

#[cfg(test)]
mod wav_round_trip {
    use crate::{decode_flac, decode_wav, Audio};

    /// `hound` encodes as well as decodes, so a WAV fixture can be built
    /// here from known samples -- no binary blob in the repository, and
    /// the expected values are visible in the test rather than implied by
    /// one.
    fn wav(channels: &[Vec<i32>], sample_rate: u32, bits: u16) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: channels.len() as u16,
            sample_rate,
            bits_per_sample: bits,
            sample_format: hound::SampleFormat::Int,
        };
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut buf, spec).expect("spec is valid");
            for i in 0..channels[0].len() {
                for ch in channels {
                    // Interleaved on the way in, which is what makes the
                    // de-interleaving on the way out worth testing.
                    w.write_sample(ch[i]).expect("in-memory write");
                }
            }
            w.finalize().expect("in-memory finalize");
        }
        buf.into_inner()
    }

    fn wav_float(channels: &[Vec<f32>], sample_rate: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: channels.len() as u16,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut buf, spec).expect("spec is valid");
            for i in 0..channels[0].len() {
                for ch in channels {
                    w.write_sample(ch[i]).expect("in-memory write");
                }
            }
            w.finalize().expect("in-memory finalize");
        }
        buf.into_inner()
    }

    fn close(a: &Audio, ch: usize, want: &[i32], bits: u32) {
        let scale = (1i64 << (bits - 1)) as f64;
        for (got, w) in a.channels[ch].iter().zip(want) {
            let expect = *w as f64 / scale;
            assert!((got - expect).abs() < 1e-9, "got {got}, want {expect}");
        }
    }

    #[test]
    fn a_mono_wav_decodes_to_the_samples_it_was_built_from() {
        let samples: Vec<i32> = vec![0, 1000, -1000, 32767, -32768, 7, -7];
        let bytes = wav(&[samples.clone()], 44100, 16);
        let a = decode_wav(&bytes).expect("built here, so it decodes");
        assert_eq!(a.channels.len(), 1);
        assert_eq!(a.sample_rate, 44100.0);
        assert_eq!(a.bits, Some(16));
        assert_eq!(a.frames(), samples.len());
        close(&a, 0, &samples, 16);
        assert_eq!(a.channels[0][4], -1.0, "-32768 is full negative scale");
    }

    /// The same numbers a FLAC of the same recording would give. If these
    /// two ever disagree, one of the normalisations is wrong and a script
    /// that switched container would silently change its results.
    #[test]
    fn wav_and_flac_normalise_identically() {
        let samples: Vec<i32> = vec![0, 1000, -1000, 32767, -32768, 7, -7, 9, -9, 11, -11, 13,
                                     -13, 15, -15, 17, -17, 19];
        let from_wav = decode_wav(&wav(&[samples.clone()], 44100, 16)).expect("wav");
        let from_flac =
            decode_flac(&crate::encode::flac(&[samples.clone()], 44100)).expect("flac");
        assert_eq!(from_wav.channels[0], from_flac.channels[0]);
        assert_eq!(from_wav.sample_rate, from_flac.sample_rate);
    }

    #[test]
    fn a_stereo_wav_comes_back_as_two_separate_channels() {
        let left: Vec<i32> = (0..16).map(|i| (i + 1) * 100).collect();
        let right: Vec<i32> = left.iter().map(|v| -v).collect();
        let a = decode_wav(&wav(&[left.clone(), right.clone()], 48000, 16)).expect("decodes");
        assert_eq!(a.channels.len(), 2);
        assert_eq!(a.sample_rate, 48000.0);
        close(&a, 0, &left, 16);
        close(&a, 1, &right, 16);
        assert!(
            a.channels[0][0] > 0.0 && a.channels[1][0] < 0.0,
            "the channels must not be swapped or merged"
        );
    }

    /// 24-bit is the depth this actually gets used at, and the one where
    /// a wrong scale is least visible: everything still sounds right, it
    /// is just 256x too quiet.
    #[test]
    fn a_24_bit_wav_scales_by_its_own_depth_not_by_16() {
        let full = (1i32 << 23) - 1;
        let samples: Vec<i32> = vec![full, -full, 0, full / 2, -(full / 2), 1, -1, 2, -2, 3,
                                     -3, 4, -4, 5, -5, 6];
        let a = decode_wav(&wav(&[samples.clone()], 96000, 24)).expect("decodes");
        assert_eq!(a.bits, Some(24));
        close(&a, 0, &samples, 24);
        assert!(
            (a.channels[0][0] - 0.99999988).abs() < 1e-6,
            "full 24-bit scale must be ~1.0, got {}",
            a.channels[0][0]
        );
    }

    /// Float WAV is already in range. Rescaling it would be the one case
    /// where "normalising" corrupts the data, so it passes through and
    /// reports no bit depth -- there is no quantisation step to report.
    #[test]
    fn float_wav_passes_through_and_reports_no_bit_depth() {
        let samples: Vec<f32> = vec![0.0, 0.5, -0.5, 1.0, -1.0, 0.25, -0.25, 0.125, -0.125,
                                     0.75, -0.75, 0.1, -0.1, 0.9, -0.9, 0.01];
        let a = decode_wav(&wav_float(&[samples.clone()], 44100)).expect("decodes");
        assert_eq!(a.bits, None, "a float format has no quantisation step");
        for (got, want) in a.channels[0].iter().zip(&samples) {
            assert!((got - *want as f64).abs() < 1e-9, "got {got}, want {want}");
        }
    }

    #[test]
    fn a_file_that_is_not_wav_says_so() {
        let err = decode_wav(b"fLaC\0\0\0\x22").unwrap_err();
        assert!(err.contains("not valid WAV"), "got: {err}");
    }
}

#[cfg(test)]
mod wav_write {
    use crate::{decode_wav, encode_wav, Dither};

    /// A deterministic uniform source, so a dithered write is a fixed
    /// stream of bytes in a test rather than a flaky one. Real callers
    /// hand over the interpreter's seeded RNG.
    fn fake_uniform() -> impl FnMut() -> f64 {
        let mut state = 0x243F_6A88_85A3_08D3u64;
        move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn no_dither() -> impl FnMut() -> f64 {
        || unreachable!("a write that names no dither must not draw a random number")
    }

    /// **The spec's one explicit, load-bearing rule for this section.**
    ///
    /// Not "16-bit works" -- 16-bit without SAYING what to do about the
    /// lost bits has to fail, and fail as a real error a script can catch,
    /// because silent truncation of audio is audible and nothing
    /// downstream can detect it after the fact.
    #[test]
    fn going_to_16_bit_without_naming_dither_is_an_error() {
        let x = vec![vec![0.0, 0.5, -0.5, 0.25]];
        let err = encode_wav(&x, 44100.0, 16, None, &mut no_dither())
            .expect_err("16-bit with no dither named must not be accepted");
        assert!(err.contains("dither"), "the error has to name the fix: {err}");
        assert!(err.contains("tpdf"), "and both options: {err}");
        assert!(err.contains("none"), "and both options: {err}");
    }

    /// The other half of that rule: `none` is a legitimate answer, and
    /// naming it must WORK. An implementation that simply rejected 16-bit
    /// would pass the test above and still be wrong.
    #[test]
    fn naming_dither_none_is_accepted() {
        let x = vec![vec![0.0, 0.5, -0.5, 0.25]];
        assert!(encode_wav(&x, 44100.0, 16, Some(Dither::None), &mut no_dither()).is_ok());
    }

    /// 24-bit does not require the argument -- the spec singles out 16 --
    /// but it must still accept it.
    #[test]
    fn depth_24_does_not_require_dither_but_allows_it() {
        let x = vec![vec![0.0, 0.5, -0.5]];
        assert!(encode_wav(&x, 44100.0, 24, None, &mut no_dither()).is_ok());
        assert!(encode_wav(&x, 44100.0, 24, Some(Dither::Tpdf), &mut fake_uniform()).is_ok());
    }

    /// 32-bit is float: there is no quantisation step, so dithering it is
    /// not a no-op to be ignored but a misunderstanding to be reported.
    #[test]
    fn depth_32_refuses_dither_rather_than_ignoring_it() {
        let x = vec![vec![0.0, 0.5, -0.5]];
        assert!(encode_wav(&x, 44100.0, 32, None, &mut no_dither()).is_ok());
        let err = encode_wav(&x, 44100.0, 32, Some(Dither::Tpdf), &mut fake_uniform())
            .expect_err("32-bit float has nothing to dither");
        assert!(err.contains("no quantisation step"), "got: {err}");
    }

    #[test]
    fn an_unsupported_depth_is_refused_by_name() {
        let x = vec![vec![0.0, 0.5]];
        for bad in [8u16, 12, 20, 64] {
            let err = encode_wav(&x, 44100.0, bad, Some(Dither::None), &mut no_dither())
                .expect_err("only 16/24/32 are written");
            assert!(err.contains(&bad.to_string()), "got: {err}");
        }
    }

    /// The round trip that makes the scale claim checkable: values that
    /// sit exactly on the 16-bit grid must come back exactly, because the
    /// scale is a power of two and the writer is the reader's mirror.
    #[test]
    fn a_16_bit_round_trip_returns_the_same_numbers() {
        let grid: Vec<f64> = [0i32, 1, -1, 1000, -1000, 32767, -32768, 7, -7, 12345]
            .iter()
            .map(|&k| k as f64 / 32768.0)
            .collect();
        let bytes = encode_wav(&[grid.clone()], 44100.0, 16, Some(Dither::None), &mut no_dither())
            .expect("writes");
        let back = decode_wav(&bytes).expect("reads back");
        assert_eq!(back.sample_rate, 44100.0);
        assert_eq!(back.bits, Some(16));
        assert_eq!(back.channels.len(), 1);
        assert_eq!(
            back.channels[0], grid,
            "a power-of-two scale makes this exact, not approximate"
        );
    }

    #[test]
    fn a_24_bit_round_trip_returns_the_same_numbers() {
        let scale = (1i64 << 23) as f64;
        let grid: Vec<f64> = [0i32, 1, -1, 8388607, -8388608, 4242, -4242]
            .iter()
            .map(|&k| k as f64 / scale)
            .collect();
        let bytes =
            encode_wav(&[grid.clone()], 96000.0, 24, None, &mut no_dither()).expect("writes");
        let back = decode_wav(&bytes).expect("reads back");
        assert_eq!(back.sample_rate, 96000.0);
        assert_eq!(back.bits, Some(24));
        assert_eq!(back.channels[0], grid);
    }

    /// Float WAV passes through the reader untouched, so a round trip
    /// through it is exact for anything f32 can hold.
    #[test]
    fn a_32_bit_float_round_trip_returns_the_same_numbers() {
        let xs: Vec<f64> = vec![0.0, 0.5, -0.5, 0.25, -0.125, 1.0, -1.0, 0.0625];
        let bytes = encode_wav(&[xs.clone()], 48000.0, 32, None, &mut no_dither()).expect("writes");
        let back = decode_wav(&bytes).expect("reads back");
        assert_eq!(back.bits, None, "a float format has no quantisation step");
        assert_eq!(back.channels[0], xs);
        // 1.0 survives here, where 16-bit has to clip it -- that is the
        // actual reason to choose float, so it is worth pinning.
        assert_eq!(back.channels[0][5], 1.0);
    }

    /// Channel order is the thing most likely to be wrong and least
    /// likely to look wrong, so it gets the same treatment on write as
    /// the decoder already gives it on read.
    #[test]
    fn a_stereo_write_keeps_the_channels_apart_and_in_order() {
        let left: Vec<f64> = (1..=16).map(|i| i as f64 / 32768.0).collect();
        let right: Vec<f64> = left.iter().map(|v| -v).collect();
        let bytes = encode_wav(
            &[left.clone(), right.clone()],
            44100.0,
            16,
            Some(Dither::None),
            &mut no_dither(),
        )
        .expect("writes");
        let back = decode_wav(&bytes).expect("reads back");
        assert_eq!(back.channels.len(), 2);
        assert_eq!(back.channels[0], left);
        assert_eq!(back.channels[1], right);
        assert!(back.channels[0][0] > 0.0 && back.channels[1][0] < 0.0);
    }

    /// A probe that can actually come out either way: dither has to
    /// CHANGE the bits, or naming it meant nothing. Half-LSB inputs are
    /// used because that is where rounding is deterministic and dither
    /// is not, so a difference here is the dither and nothing else.
    #[test]
    fn tpdf_dither_perturbs_the_quantisation_and_none_does_not() {
        let xs: Vec<f64> = (0..256).map(|i| (i as f64 + 0.5) / 32768.0).collect();
        let plain = encode_wav(&[xs.clone()], 44100.0, 16, Some(Dither::None), &mut no_dither())
            .expect("writes");
        let plain_again =
            encode_wav(&[xs.clone()], 44100.0, 16, Some(Dither::None), &mut no_dither())
                .expect("writes");
        let dithered = encode_wav(&[xs.clone()], 44100.0, 16, Some(Dither::Tpdf), &mut fake_uniform())
            .expect("writes");

        assert_eq!(plain, plain_again, "undithered writes must be deterministic");
        assert_ne!(
            plain, dithered,
            "TPDF that changed no sample would be a dither in name only"
        );

        // And it must stay a dither, not a corruption: every sample
        // within one LSB of where rounding would have put it.
        let a = decode_wav(&plain).expect("reads");
        let b = decode_wav(&dithered).expect("reads");
        for (p, d) in a.channels[0].iter().zip(&b.channels[0]) {
            assert!(
                (p - d).abs() <= 1.0 / 32768.0 + 1e-12,
                "dither moved a sample by more than 1 LSB: {p} vs {d}"
            );
        }
    }

    /// Clipping is the format's asymmetry, not a bug to be surprised by
    /// later: two's complement has no +2^15.
    #[test]
    fn full_positive_scale_clips_and_full_negative_scale_does_not() {
        let xs = vec![1.0f64, -1.0];
        let bytes =
            encode_wav(&[xs], 44100.0, 16, Some(Dither::None), &mut no_dither()).expect("writes");
        let back = decode_wav(&bytes).expect("reads");
        assert_eq!(back.channels[0][0], 32767.0 / 32768.0, "+1.0 clips by one step");
        assert_eq!(back.channels[0][1], -1.0, "-1.0 is exactly representable");
    }

    /// A NaN would be written as silence and nothing downstream could
    /// tell, which is the failure mode this whole section exists to
    /// prevent.
    #[test]
    fn a_non_finite_sample_is_an_error_not_silence() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let x = vec![vec![0.0, bad, 0.0]];
            let err = encode_wav(&x, 44100.0, 16, Some(Dither::None), &mut no_dither())
                .expect_err("must not be written as silence");
            assert!(err.contains("sample 1"), "the error names where: {err}");
        }
    }

    #[test]
    fn ragged_channels_and_impossible_rates_are_refused() {
        assert!(encode_wav(&[], 44100.0, 16, Some(Dither::None), &mut no_dither()).is_err());
        let ragged = vec![vec![0.0, 0.1], vec![0.0]];
        assert!(encode_wav(&ragged, 44100.0, 16, Some(Dither::None), &mut no_dither()).is_err());
        let x = vec![vec![0.0, 0.1]];
        for bad in [0.0, -44100.0, f64::NAN, 44100.5] {
            assert!(
                encode_wav(&x, bad, 16, Some(Dither::None), &mut no_dither()).is_err(),
                "{bad} is not a writable sample rate"
            );
        }
    }
}

#[cfg(test)]
mod mp3 {
    use crate::decode_mp3;

    /// **These are the error paths only.**
    ///
    /// There is no pure-Rust MP3 *encoder* to build a fixture with, and
    /// synthesising a valid frame by hand means Huffman-coding a
    /// granule -- far more machinery than the FLAC and WAV helpers, and
    /// machinery whose own bugs would look like decoder bugs. So the
    /// happy path is verified by hand against real files rather than in
    /// CI, and this says so rather than implying coverage it does not
    /// have.
    ///
    /// Verified by hand on 2026-09-09: a 44.1 kHz mono MP3 decoded to
    /// 769,089 samples at a peak of 0.9637, `spectrogram` and
    /// `resample_to` read its rate straight off the returned `Signal`.
    #[test]
    fn a_file_that_is_not_mp3_is_an_error_not_silence() {
        // MP3 has no magic number -- only a run of frames -- so a decoder
        // asked to read something else fails by finding no frames rather
        // than by rejecting a header. The error has to say that clearly,
        // because "no frames" is also what a truncated MP3 looks like.
        for junk in [b"RIFF....WAVEfmt ".as_slice(), b"fLaC\0\0\0\x22".as_slice(), &[]] {
            assert!(decode_mp3(junk).is_err(), "{junk:?} must not decode as audio");
        }
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! UTF-16 -> UTF-8 transcoding read adapter for the process probe.
//!
//! Some Windows management tools emit UTF-16LE on their pipes -- most
//! notably `wsl.exe --list --verbose`, plus certain `reg`/`wmic` output.
//! The byte->line pipeline ([`crate::process::read_line_bounded`]) scans
//! for the `\n` byte (`0x0A`) and lossily decodes as UTF-8. On a UTF-16LE
//! stream a newline is the byte pair `0A 00`, so the raw scan splits INSIDE
//! a code unit and every subsequent line is byte-misaligned garbage
//! ("  N A M E ...", NUL-riddled), and rule matching runs over noise.
//!
//! [`Utf16Decoder`] wraps the raw child stream and transcodes to UTF-8
//! BEFORE line splitting, so `\n` is a single `0x0A` byte and the rest of
//! the pipeline (line split, lossy decode, ANSI strip, sifter, and the RAW
//! frame stored in the context ring) is correct and unchanged. It mirrors
//! how [`crate::ansi::strip_ansi`] is a small focused helper wired at the
//! read seam: here the transcode sits one layer earlier (byte stream)
//! because the encoding decides how bytes group into lines.
//!
//! Detection is LATCHED per stream on the FIRST chunk only (no mid-stream
//! flapping):
//!   * a UTF-16LE BOM (`FF FE`) or UTF-16BE BOM (`FE FF`) is definitive; or
//!   * a heuristic over the first bytes: mostly-zero high bytes at odd
//!     offsets AND text-range low bytes at even offsets => UTF-16LE.
//! Anything else latches pass-through (byte-faithful, zero-cost).
//!
//! Source-status: live.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

/// Scratch size for one raw read from the inner stream.
const RAW_CHUNK: usize = 8192;

/// Bytes of the first chunk inspected by the BOM-less heuristic.
const SNIFF_WINDOW: usize = 256;

/// Minimum first-chunk bytes required before the BOM-less heuristic will
/// commit to UTF-16LE. Below this we lack evidence and latch pass-through.
const SNIFF_MIN: usize = 16;

/// Latched per-stream encoding decision. Decided once on the first chunk.
enum Latch {
    /// Not yet decided (before the first non-empty chunk).
    Undecided,
    /// Bytes are handed through verbatim (UTF-8 or unknown-but-not-UTF-16).
    PassThrough,
    /// UTF-16 little-endian: low byte first in each code unit.
    Le,
    /// UTF-16 big-endian: high byte first in each code unit.
    Be,
}

/// An [`AsyncRead`] adapter that transcodes a UTF-16 child stream to UTF-8.
///
/// Wrap the raw child `stdout`/`stderr` in this before the `BufReader` that
/// feeds the line reader. When the stream is not UTF-16 (the common case),
/// the decision latches to pass-through on the first chunk and reads are
/// delegated straight to the inner stream.
pub struct Utf16Decoder<R> {
    inner: R,
    latch: Latch,
    /// The first byte of a UTF-16 code unit whose second byte has not yet
    /// arrived (odd-length chunk boundary). Prepended to the next chunk.
    carry: Option<u8>,
    /// Transcoded UTF-8 bytes not yet handed to the caller.
    pending: Vec<u8>,
    /// Read cursor into `pending`.
    pending_pos: usize,
}

impl<R> Utf16Decoder<R> {
    pub const fn new(inner: R) -> Self {
        Self {
            inner,
            latch: Latch::Undecided,
            carry: None,
            pending: Vec::new(),
            pending_pos: 0,
        }
    }

    /// Consume one raw chunk (never empty): decide the latch on the first
    /// chunk, then append the decoded/verbatim bytes to `pending`.
    fn consume_raw(&mut self, input: &[u8]) {
        if matches!(self.latch, Latch::Undecided) {
            let (latch, skip) = detect(input);
            self.latch = latch;
            let rest = &input[skip..];
            match self.latch {
                Latch::PassThrough => self.pending.extend_from_slice(rest),
                Latch::Le | Latch::Be => self.decode_utf16(rest),
                Latch::Undecided => unreachable!("detect never returns Undecided"),
            }
            return;
        }
        match self.latch {
            Latch::PassThrough => self.pending.extend_from_slice(input),
            Latch::Le | Latch::Be => self.decode_utf16(input),
            Latch::Undecided => unreachable!("latch decided above"),
        }
    }

    /// Transcode UTF-16 (`self.latch` = `Le`/`Be`) `input` bytes to UTF-8,
    /// carrying a lone trailing byte across the chunk boundary.
    ///
    /// ponytail: a supplementary-plane code point (surrogate PAIR) whose two
    /// u16 units straddle a chunk boundary decodes to two U+FFFD replacement
    /// chars rather than the intended char -- only the single-BYTE carry is
    /// reassembled. This path (`wsl --list`, `reg`, `wmic`) is pure ASCII/BMP,
    /// so the degradation is unobservable there; carry a pending high-surrogate
    /// u16 too if a real supplementary-plane stream ever needs it.
    fn decode_utf16(&mut self, input: &[u8]) {
        let be = matches!(self.latch, Latch::Be);
        let mut units: Vec<u16> = Vec::with_capacity(input.len() / 2 + 1);
        let mut iter = input.iter().copied();

        if let Some(first) = self.carry.take() {
            let Some(second) = iter.next() else {
                // Chunk was a single byte: keep carrying it. `consume_raw`
                // only calls us with non-empty input, so this is the
                // 1-byte-chunk case, not EOF.
                self.carry = Some(first);
                return;
            };
            units.push(combine(first, second, be));
        }
        loop {
            match (iter.next(), iter.next()) {
                (Some(a), Some(b)) => units.push(combine(a, b, be)),
                (Some(a), None) => {
                    self.carry = Some(a);
                    break;
                }
                (None, _) => break,
            }
        }

        for decoded in char::decode_utf16(units) {
            let ch = decoded.unwrap_or('\u{FFFD}');
            let mut buf = [0u8; 4];
            self.pending
                .extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
}

/// Assemble a UTF-16 code unit from its two stream bytes in latch order.
const fn combine(first: u8, second: u8, be: bool) -> u16 {
    if be {
        u16::from_be_bytes([first, second])
    } else {
        u16::from_le_bytes([first, second])
    }
}

/// Decide the stream encoding from its first chunk. Returns the latch and how
/// many leading BOM bytes to skip.
fn detect(chunk: &[u8]) -> (Latch, usize) {
    // A BOM is definitive and cheap.
    if chunk.len() >= 2 {
        match (chunk[0], chunk[1]) {
            (0xFF, 0xFE) => return (Latch::Le, 2),
            (0xFE, 0xFF) => return (Latch::Be, 2),
            _ => {}
        }
    }

    // BOM-less heuristic (LE only -- BOM-less UTF-16BE effectively never
    // appears on this path). UTF-16LE ASCII text is "T\0 e\0 x\0 t\0 ...":
    // text-range bytes at even offsets, zero high bytes at odd offsets.
    let n = chunk.len().min(SNIFF_WINDOW);
    if n < SNIFF_MIN {
        return (Latch::PassThrough, 0);
    }
    let (mut even_text, mut even_total) = (0usize, 0usize);
    let (mut odd_zero, mut odd_total) = (0usize, 0usize);
    for (i, &b) in chunk[..n].iter().enumerate() {
        if i % 2 == 0 {
            even_total += 1;
            if (0x01..=0x7F).contains(&b) {
                even_text += 1;
            }
        } else {
            odd_total += 1;
            if b == 0 {
                odd_zero += 1;
            }
        }
    }
    // Require BOTH >= ~70% zero high bytes AND >= ~70% text low bytes. The
    // high-byte test alone would misfire on binary; the low-byte test alone
    // would misfire on plain ASCII. Together they reject plain UTF-8 (its odd
    // offsets are text, not zeros -- even with occasional embedded NULs) and
    // reject all-zero/binary blobs (their even offsets are not text bytes).
    let odd_ok = odd_zero * 10 >= odd_total * 7;
    let even_ok = even_text * 10 >= even_total * 7;
    if odd_ok && even_ok {
        (Latch::Le, 0)
    } else {
        (Latch::PassThrough, 0)
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Utf16Decoder<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();
        loop {
            // 1. Hand out any transcoded bytes still pending from a prior read.
            if me.pending_pos < me.pending.len() {
                let avail = &me.pending[me.pending_pos..];
                let n = avail.len().min(buf.remaining());
                buf.put_slice(&avail[..n]);
                me.pending_pos += n;
                return Poll::Ready(Ok(()));
            }
            me.pending.clear();
            me.pending_pos = 0;

            // 2. Once latched pass-through, delegate straight to the inner
            //    stream -- no copy, no transcode.
            if matches!(me.latch, Latch::PassThrough) {
                return Pin::new(&mut me.inner).poll_read(cx, buf);
            }

            // 3. Pull one raw chunk and transcode it into `pending`, then loop
            //    to hand it out. An empty read is EOF.
            let mut scratch = [0u8; RAW_CHUNK];
            let mut raw = ReadBuf::new(&mut scratch);
            match Pin::new(&mut me.inner).poll_read(cx, &mut raw) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(())) => {
                    let filled = raw.filled();
                    if filled.is_empty() {
                        // EOF. A dangling `carry` is a malformed odd trailing
                        // byte on a UTF-16 stream; drop it. Leaving `buf`
                        // untouched signals EOF to the caller.
                        return Poll::Ready(Ok(()));
                    }
                    me.consume_raw(filled);
                    // Loop: drain the freshly decoded bytes (or, if this chunk
                    // only advanced the carry, read again -- never a false EOF).
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    /// UTF-16LE bytes for `s`, optionally prefixed with the LE BOM.
    fn le_bytes(s: &str, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            out.extend_from_slice(&[0xFF, 0xFE]);
        }
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    /// UTF-16BE bytes for `s` with the BE BOM.
    fn be_bytes_with_bom(s: &str) -> Vec<u8> {
        let mut out = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_be_bytes());
        }
        out
    }

    /// An `AsyncRead` that yields its data in fixed-size slices so a UTF-16
    /// code unit can be forced to straddle a `poll_read` boundary.
    struct ChunkedReader {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
    }
    impl ChunkedReader {
        fn new(data: Vec<u8>, chunk: usize) -> Self {
            Self {
                data,
                pos: 0,
                chunk,
            }
        }
    }
    impl AsyncRead for ChunkedReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let remaining = self.data.len() - self.pos;
            if remaining == 0 {
                return Poll::Ready(Ok(()));
            }
            let n = remaining.min(self.chunk).min(buf.remaining());
            let start = self.pos;
            let slice = self.data[start..start + n].to_vec();
            buf.put_slice(&slice);
            self.pos += n;
            Poll::Ready(Ok(()))
        }
    }

    async fn decode_all(data: Vec<u8>, chunk: usize) -> String {
        let mut dec = Utf16Decoder::new(ChunkedReader::new(data, chunk));
        let mut out = Vec::new();
        dec.read_to_end(&mut out).await.expect("read ok");
        String::from_utf8(out).expect("decoder output is valid UTF-8")
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn le_with_bom_transcodes_and_strips_bom() {
        let s = "NAME      STATE           VERSION\nUbuntu    Running         2\n";
        let bytes = le_bytes(s, true);
        let out = rt().block_on(decode_all(bytes, RAW_CHUNK));
        assert_eq!(out, s, "BOM must be stripped and text transcoded verbatim");
    }

    #[test]
    fn le_without_bom_is_detected_by_heuristic() {
        // No BOM: the heuristic must latch UTF-16LE from the zero high bytes.
        let s = "Windows Subsystem for Linux Distributions:\nUbuntu (Default)\n";
        let bytes = le_bytes(s, false);
        let out = rt().block_on(decode_all(bytes, RAW_CHUNK));
        assert_eq!(out, s);
    }

    #[test]
    fn le_code_unit_split_across_chunk_boundary() {
        // Odd chunk size forces the low/high bytes of a code unit into
        // separate poll_read calls -- exercises the single-byte carry.
        let s = "abcdefghijklmnop\nqrstuvwx\n";
        let bytes = le_bytes(s, true);
        // 3 is odd and coprime-ish with the 2-byte unit stride, so many units
        // straddle a boundary across the stream.
        let out = rt().block_on(decode_all(bytes, 3));
        assert_eq!(out, s, "single-byte carry must reassemble split code units");
    }

    #[test]
    fn be_with_bom_transcodes() {
        let s = "Hello BE\nWorld\n";
        let out = rt().block_on(decode_all(be_bytes_with_bom(s), RAW_CHUNK));
        assert_eq!(out, s);
    }

    #[test]
    fn plain_utf8_passes_through_unchanged() {
        let s = "plain ascii line one\nplain ascii line two\n";
        let out = rt().block_on(decode_all(s.as_bytes().to_vec(), RAW_CHUNK));
        assert_eq!(out, s);
    }

    #[test]
    fn utf8_with_occasional_nuls_is_not_misdetected() {
        // Acceptance (2): embedded NULs in an otherwise UTF-8 stream must NOT
        // trip the UTF-16 heuristic. The bytes must survive verbatim.
        let mut data = b"log line with a \x00 null and more text here\n".to_vec();
        data.extend_from_slice(b"second line also has \x00 a null in it too\n");
        let out = rt().block_on(decode_all(data.clone(), RAW_CHUNK));
        assert_eq!(out.as_bytes(), data.as_slice());
    }

    #[test]
    fn short_bomless_chunk_defaults_to_passthrough() {
        // Below SNIFF_MIN with no BOM: not enough evidence -> pass-through.
        let data = b"hi\n".to_vec();
        let out = rt().block_on(decode_all(data.clone(), RAW_CHUNK));
        assert_eq!(out.as_bytes(), data.as_slice());
    }

    #[test]
    fn detect_le_bom() {
        assert!(matches!(detect(&[0xFF, 0xFE, b'a', 0]), (Latch::Le, 2)));
    }

    #[test]
    fn detect_be_bom() {
        assert!(matches!(detect(&[0xFE, 0xFF, 0, b'a']), (Latch::Be, 2)));
    }

    #[test]
    fn detect_plain_ascii_is_passthrough() {
        let data = b"this is just a normal ascii log line, no nulls";
        assert!(matches!(detect(data), (Latch::PassThrough, 0)));
    }
}

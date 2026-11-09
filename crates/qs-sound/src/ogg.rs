// SPDX-License-Identifier: GPL-3.0-or-later

//! The rerelease's music files, `music/track<NN>.ogg` (RRE-3.5): an Ogg Vorbis file decoded
//! as a [`MusicStream`], as QuakeSpasm's `snd_vorbis.c` codec streams one, with `symphonia`'s
//! Ogg reader and Vorbis decoder.

use std::fmt;
use std::io::Cursor;
use std::sync::Arc;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::default::{get_codecs, get_probe};

use crate::music::{MusicStream, Read};

/// Why an Ogg Vorbis file cannot be played (RRE-3.5).
#[derive(Debug)]
pub enum OggError {
    /// The bytes are not an Ogg Vorbis stream the decoder reads.
    Decode(SymphoniaError),
    /// The file has no audio track, or its track names no rate, a rate of 0, or no channels.
    NoAudio,
    /// The stream has other than one or two channels, which the host's music does not play.
    Channels(usize),
}

impl fmt::Display for OggError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(f, "not an Ogg Vorbis file: {error}"),
            Self::NoAudio => f.write_str("no audio track"),
            Self::Channels(n) => write!(f, "{n} channels, where the music plays one or two"),
        }
    }
}

impl std::error::Error for OggError {}

impl From<SymphoniaError> for OggError {
    fn from(error: SymphoniaError) -> Self {
        Self::Decode(error)
    }
}

/// What decoding the next packet came to.
enum Next {
    /// A packet's samples are pending.
    Packet,
    /// The end of the file.
    End,
    /// The file failed, `S_CodecReadStream`'s error.
    Failed,
}

/// The file's reader and decoder, from its start.
struct Opened {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    rate: u32,
    channels: u16,
}

/// Opens `bytes` as Ogg with one Vorbis audio track of one or two channels, sharing them.
fn open(bytes: &Arc<[u8]>) -> Result<Opened, OggError> {
    let source =
        MediaSourceStream::new(Box::new(Cursor::new(Arc::clone(bytes))), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("ogg");
    let format = get_probe().probe(
        &hint,
        source,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(OggError::NoAudio)?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or(OggError::NoAudio)?;
    let rate = params
        .sample_rate
        .filter(|&rate| rate > 0)
        .ok_or(OggError::NoAudio)?;
    let count = params.channels.as_ref().ok_or(OggError::NoAudio)?.count();
    let channels = match count {
        1 => 1,
        2 => 2,
        n => return Err(OggError::Channels(n)),
    };
    let decoder = get_codecs().make_audio_decoder(params, &AudioDecoderOptions::default())?;
    let track = track.id;
    Ok(Opened {
        format,
        decoder,
        track,
        rate,
        channels,
    })
}

/// An Ogg Vorbis file being played: its bytes, kept for a rewind, its reader and decoder, and
/// the samples of the packet decoded last that a read has not taken yet.
pub struct OggStream {
    bytes: Arc<[u8]>,
    opened: Opened,
    pending: Vec<i16>,
    taken: usize,
}

impl OggStream {
    /// The file `bytes` ready to play from its start.
    ///
    /// # Errors
    ///
    /// [`OggError::Decode`] when the bytes are not Ogg Vorbis, [`OggError::NoAudio`] when it has
    /// no audio track or a rate of 0, [`OggError::Channels`] when it has other than one or two
    /// channels.
    pub fn new(bytes: Vec<u8>) -> Result<Self, OggError> {
        let bytes: Arc<[u8]> = bytes.into();
        let opened = open(&bytes)?;
        Ok(Self {
            bytes,
            opened,
            pending: Vec::new(),
            taken: 0,
        })
    }

    /// The next packet's samples into `pending`.
    fn decode_next(&mut self) -> Next {
        loop {
            let packet = match self.opened.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Next::End,
                Err(_) => return Next::Failed,
            };
            if packet.track_id != self.opened.track {
                continue;
            }
            match self.opened.decoder.decode(&packet) {
                Ok(buffer) => {
                    self.pending.clear();
                    self.pending.resize(buffer.samples_interleaved(), 0);
                    buffer.copy_to_slice_interleaved(&mut self.pending);
                    self.taken = 0;
                    return Next::Packet;
                }
                // A packet that does not decode is skipped, as the example readers do.
                Err(SymphoniaError::DecodeError(_)) => {}
                Err(_) => return Next::Failed,
            }
        }
    }
}

impl MusicStream for OggStream {
    fn rate(&self) -> u32 {
        self.opened.rate
    }

    fn channels(&self) -> u16 {
        self.opened.channels
    }

    fn read(&mut self, out: &mut [i16]) -> Read {
        let mut written = 0_usize;
        while let Some(slot) = out.get_mut(written..).filter(|rest| !rest.is_empty()) {
            let left = self.pending.get(self.taken..).unwrap_or_default();
            if left.is_empty() {
                match self.decode_next() {
                    Next::Packet => continue,
                    Next::End => break,
                    // `S_CodecReadStream`'s error, -1.
                    Next::Failed if written == 0 => return Read::Failed(-1),
                    Next::Failed => break,
                }
            }
            let n = left.len().min(slot.len());
            if let (Some(to), Some(from)) = (slot.get_mut(..n), left.get(..n)) {
                to.copy_from_slice(from);
            }
            written = written.saturating_add(n);
            self.taken = self.taken.saturating_add(n);
        }
        if written == 0 {
            Read::End
        } else {
            Read::Samples(written)
        }
    }

    fn rewind(&mut self) -> bool {
        match open(&self.bytes) {
            Ok(opened) => {
                self.opened = opened;
                self.pending.clear();
                self.taken = 0;
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests;

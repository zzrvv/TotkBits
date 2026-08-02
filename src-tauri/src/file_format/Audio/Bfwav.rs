use std::{fs::File, path::Path};

use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u16(self, b: &[u8]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes([b[0], b[1]]),
            Self::Big => u16::from_be_bytes([b[0], b[1]]),
        }
    }
    fn i16(self, b: &[u8]) -> i16 {
        self.u16(b) as i16
    }
    fn u32(self, b: &[u8]) -> u32 {
        match self {
            Self::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Self::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        }
    }
    fn put_u16(self, out: &mut [u8], value: u16) {
        out.copy_from_slice(&match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        });
    }
    fn put_u32(self, out: &mut [u8], value: u32) {
        out.copy_from_slice(&match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        });
    }
}

fn range(data: &[u8], at: usize, size: usize) -> Result<&[u8], String> {
    data.get(at..at + size)
        .ok_or_else(|| "truncated BFWAV".into())
}

fn align(value: usize, boundary: usize) -> usize {
    (value + boundary - 1) & !(boundary - 1)
}

#[derive(Clone)]
pub struct DecodedAudio {
    pub channels: Vec<Vec<i16>>,
    pub sample_rate: u32,
    pub looping: bool,
    pub loop_start: u32,
}

pub fn decode(data: &[u8]) -> Result<DecodedAudio, String> {
    if !crate::Settings::Magic::is_bfwav(data) || data.len() < 0x20 {
        return Err("not a BFWAV file".into());
    }
    let endian = match &data[4..6] {
        b"\xfe\xff" => Endian::Big,
        b"\xff\xfe" => Endian::Little,
        _ => return Err("invalid BFWAV byte-order mark".into()),
    };
    let blocks = endian.u16(range(data, 0x10, 2)?) as usize;
    let mut info = None;
    let mut audio = None;
    for index in 0..blocks {
        let at = 0x14 + index * 12;
        let ty = endian.u16(range(data, at, 2)?);
        let offset = endian.u32(range(data, at + 4, 4)?) as usize;
        match ty {
            0x7000 => info = Some(offset),
            0x7001 => audio = Some(offset),
            _ => {}
        }
    }
    let info = info.ok_or("BFWAV has no INFO block")?;
    let audio = audio.ok_or("BFWAV has no DATA block")?;
    if range(data, info, 4)? != b"INFO" || range(data, audio, 4)? != b"DATA" {
        return Err("invalid BFWAV blocks".into());
    }
    let stream = info + 8;
    let codec = range(data, stream, 1)?[0];
    let looping = range(data, stream + 1, 1)?[0] != 0;
    let sample_rate = endian.u32(range(data, stream + 4, 4)?);
    let loop_start = endian.u32(range(data, stream + 8, 4)?);
    let sample_count = endian.u32(range(data, stream + 12, 4)?) as usize;
    let table = stream + 20;
    let channel_count = endian.u32(range(data, table, 4)?) as usize;
    if channel_count == 0 || channel_count > 32 {
        return Err(format!("invalid BFWAV channel count {channel_count}"));
    }
    let bytes_per_channel = match codec {
        0 => sample_count,
        1 => sample_count * 2,
        2 => ((sample_count + 13) / 14) * 8,
        _ => return Err(format!("unsupported BFWAV codec {codec}")),
    };
    let data_base = audio + 8;
    let mut channels = Vec::with_capacity(channel_count);
    for channel in 0..channel_count {
        let reference = table + 4 + channel * 8;
        if endian.u16(range(data, reference, 2)?) != 0x7100 {
            return Err("invalid BFWAV channel reference".into());
        }
        let channel_info = table + endian.u32(range(data, reference + 4, 4)?) as usize;
        let audio_offset = endian.u32(range(data, channel_info + 4, 4)?) as usize;
        let encoded = range(data, data_base + audio_offset, bytes_per_channel)?;
        let samples = match codec {
            0 => encoded
                .iter()
                .take(sample_count)
                .map(|&v| (v as i8 as i16) << 8)
                .collect(),
            1 => encoded
                .chunks_exact(2)
                .take(sample_count)
                .map(|v| endian.i16(v))
                .collect(),
            2 => {
                let adpcm_offset = endian.u32(range(data, channel_info + 12, 4)?) as usize;
                let adpcm = channel_info + adpcm_offset;
                let mut coefs = [0i16; 16];
                for (i, coef) in coefs.iter_mut().enumerate() {
                    *coef = endian.i16(range(data, adpcm + i * 2, 2)?);
                }
                let mut hist1 = endian.i16(range(data, adpcm + 34, 2)?) as i32;
                let mut hist2 = endian.i16(range(data, adpcm + 36, 2)?) as i32;
                let mut decoded = Vec::with_capacity(sample_count);
                for frame in encoded.chunks_exact(8) {
                    let header = frame[0];
                    let predictor = (header >> 4) as usize;
                    let scale = 1i32 << (header & 0xf);
                    for index in 0..14 {
                        let packed = frame[1 + index / 2];
                        let nibble = if index % 2 == 0 {
                            packed >> 4
                        } else {
                            packed & 0xf
                        };
                        let signed = if nibble >= 8 {
                            nibble as i32 - 16
                        } else {
                            nibble as i32
                        };
                        let value = ((signed * scale * 2048
                            + coefs[predictor * 2] as i32 * hist1
                            + coefs[predictor * 2 + 1] as i32 * hist2
                            + 1024)
                            >> 11)
                            .clamp(i16::MIN as i32, i16::MAX as i32);
                        hist2 = hist1;
                        hist1 = value;
                        decoded.push(value as i16);
                        if decoded.len() == sample_count {
                            break;
                        }
                    }
                }
                decoded
            }
            _ => return Err(format!("unsupported BFWAV codec {codec}")),
        };
        channels.push(samples);
    }
    Ok(DecodedAudio {
        channels,
        sample_rate,
        looping,
        loop_start,
    })
}

pub fn to_wav(data: &[u8]) -> Result<Vec<u8>, String> {
    let decoded = decode(data)?;
    pcm_to_wav(&decoded)
}

pub fn pcm_to_wav(decoded: &DecodedAudio) -> Result<Vec<u8>, String> {
    let channel_count = decoded.channels.len();
    if channel_count == 0 {
        return Err("audio has no channels".into());
    }
    let sample_count = decoded.channels[0].len();
    if decoded.channels.iter().any(|v| v.len() != sample_count) {
        return Err("audio channels have different lengths".into());
    }
    let data_size = sample_count * channel_count * 2;
    let mut out = Vec::with_capacity(44 + data_size);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_size as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channel_count as u16).to_le_bytes());
    out.extend_from_slice(&decoded.sample_rate.to_le_bytes());
    out.extend_from_slice(&(decoded.sample_rate * channel_count as u32 * 2).to_le_bytes());
    out.extend_from_slice(&((channel_count * 2) as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_size as u32).to_le_bytes());
    for sample in 0..sample_count {
        for channel in &decoded.channels {
            out.extend_from_slice(&channel[sample].to_le_bytes());
        }
    }
    Ok(out)
}

pub fn decode_source(path: &Path) -> Result<DecodedAudio, String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|v| v.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("unsupported audio source: {e}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or("audio source has no default track")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let mut channels: Vec<Vec<i16>> = Vec::new();
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    loop {
        let packet = match format.next_packet() {
            Ok(v) => v,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(v) => v,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        sample_rate = decoded.spec().rate;
        let count = decoded.spec().channels.count();
        if channels.is_empty() {
            channels.resize_with(count, Vec::new);
        }
        if channels.len() != count {
            return Err("audio channel count changed while decoding".into());
        }
        let mut samples = SampleBuffer::<i16>::new(decoded.capacity() as u64, *decoded.spec());
        samples.copy_interleaved_ref(decoded);
        for frame in samples.samples().chunks_exact(count) {
            for (channel, &sample) in frame.iter().enumerate() {
                channels[channel].push(sample);
            }
        }
    }
    if channels.is_empty() {
        return Err("audio source decoded no samples".into());
    }
    Ok(DecodedAudio {
        channels,
        sample_rate,
        looping: false,
        loop_start: 0,
    })
}

pub fn encode_pcm16(audio: &DecodedAudio) -> Result<Vec<u8>, String> {
    if audio.channels.is_empty() || audio.channels.len() > 32 {
        return Err("unsupported channel count".into());
    }
    let samples = audio.channels[0].len();
    if audio.channels.iter().any(|v| v.len() != samples) {
        return Err("audio channels have different lengths".into());
    }
    let endian = Endian::Big;
    let channels = audio.channels.len();
    let table_size = 4 + channels * 8;
    let channel_info_size = channels * 16;
    let info_size = align(8 + 20 + table_size + channel_info_size, 0x20);
    let header_size = 0x40;
    let info_at = header_size;
    let data_at = info_at + info_size;
    let channel_bytes = samples * 2;
    let channel_stride = align(channel_bytes, 0x20);
    let data_size = 8 + channel_stride * channels;
    let file_size = data_at + data_size;
    let mut out = vec![0u8; file_size];
    out[..4].copy_from_slice(b"FWAV");
    out[4..6].copy_from_slice(b"\xfe\xff");
    endian.put_u16(&mut out[6..8], header_size as u16);
    endian.put_u32(&mut out[8..12], 0x0001_0200);
    endian.put_u32(&mut out[12..16], file_size as u32);
    endian.put_u16(&mut out[16..18], 2);
    endian.put_u16(&mut out[0x14..0x16], 0x7000);
    endian.put_u32(&mut out[0x18..0x1c], info_at as u32);
    endian.put_u32(&mut out[0x1c..0x20], info_size as u32);
    endian.put_u16(&mut out[0x20..0x22], 0x7001);
    endian.put_u32(&mut out[0x24..0x28], data_at as u32);
    endian.put_u32(&mut out[0x28..0x2c], data_size as u32);
    out[info_at..info_at + 4].copy_from_slice(b"INFO");
    endian.put_u32(&mut out[info_at + 4..info_at + 8], info_size as u32);
    let stream = info_at + 8;
    out[stream] = 1;
    out[stream + 1] = audio.looping as u8;
    endian.put_u32(&mut out[stream + 4..stream + 8], audio.sample_rate);
    endian.put_u32(&mut out[stream + 8..stream + 12], audio.loop_start);
    endian.put_u32(&mut out[stream + 12..stream + 16], samples as u32);
    endian.put_u32(&mut out[stream + 16..stream + 20], audio.loop_start);
    let table = stream + 20;
    endian.put_u32(&mut out[table..table + 4], channels as u32);
    let infos = table + table_size;
    for channel in 0..channels {
        let reference = table + 4 + channel * 8;
        endian.put_u16(&mut out[reference..reference + 2], 0x7100);
        endian.put_u32(
            &mut out[reference + 4..reference + 8],
            (infos + channel * 16 - table) as u32,
        );
        let info = infos + channel * 16;
        endian.put_u16(&mut out[info..info + 2], 0x1f00);
        endian.put_u32(
            &mut out[info + 4..info + 8],
            (channel * channel_stride) as u32,
        );
        endian.put_u32(&mut out[info + 12..info + 16], u32::MAX);
    }
    out[data_at..data_at + 4].copy_from_slice(b"DATA");
    endian.put_u32(&mut out[data_at + 4..data_at + 8], data_size as u32);
    for (channel, values) in audio.channels.iter().enumerate() {
        let mut at = data_at + 8 + channel * channel_stride;
        for &value in values {
            out[at..at + 2].copy_from_slice(&value.to_be_bytes());
            at += 2;
        }
    }
    Ok(out)
}

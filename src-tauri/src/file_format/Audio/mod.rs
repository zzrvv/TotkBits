pub mod Bfwav;
pub mod Bwav;

pub use Bfwav::DecodedAudio;

pub fn to_wav(data: &[u8]) -> Result<Vec<u8>, String> {
    match data.get(..4) {
        Some(b"FWAV") => Bfwav::to_wav(data),
        Some(b"BWAV") => Bwav::to_wav(data),
        _ => Err("selected entry is not BFWAV or BWAV audio".into()),
    }
}

pub fn decode(data: &[u8]) -> Result<DecodedAudio, String> {
    match data.get(..4) {
        Some(b"FWAV") => Bfwav::decode(data),
        Some(b"BWAV") => Bwav::decode(data),
        _ => Err("selected entry is not BFWAV or BWAV audio".into()),
    }
}

pub fn to_mp3(data: &[u8]) -> Result<Vec<u8>, String> {
    use shine_rs::{encode_pcm_to_mp3, Mp3EncoderConfig, StereoMode};

    let decoded = decode(data)?;
    let channels = mp3_channels(&decoded)?;
    let channel_count = channels.len();
    let sample_count = channels.first().map_or(0, Vec::len);
    let capacity = sample_count
        .checked_mul(channel_count)
        .ok_or("audio is too large to export")?;
    let mut interleaved = Vec::with_capacity(capacity);
    for sample in 0..sample_count {
        for channel in &channels {
            interleaved.push(channel[sample]);
        }
    }
    let mode = if channel_count == 1 {
        StereoMode::Mono
    } else {
        StereoMode::Stereo
    };
    let config = Mp3EncoderConfig::new()
        .sample_rate(decoded.sample_rate)
        .bitrate(192)
        .channels(channel_count as u8)
        .stereo_mode(mode);
    encode_pcm_to_mp3(config, &interleaved).map_err(|error| format!("MP3 export failed: {error}"))
}

fn mp3_channels(audio: &DecodedAudio) -> Result<Vec<Vec<i16>>, String> {
    let sample_count = audio.channels.first().map_or(0, Vec::len);
    if sample_count == 0 || audio.channels.iter().any(|v| v.len() != sample_count) {
        return Err("audio channels are empty or have different lengths".into());
    }
    if audio.channels.len() <= 2 {
        return Ok(audio.channels.clone());
    }

    let mut stereo = vec![
        Vec::with_capacity(sample_count),
        Vec::with_capacity(sample_count),
    ];
    for sample in 0..sample_count {
        for output in 0..2 {
            let mut sum = 0i64;
            let mut count = 0i64;
            for (index, channel) in audio.channels.iter().enumerate() {
                if index % 2 == output {
                    sum += channel[sample] as i64;
                    count += 1;
                }
            }
            stereo[output].push((sum / count.max(1)) as i16);
        }
    }
    Ok(stereo)
}

pub fn encode_replacement(target: &[u8], source: &DecodedAudio) -> Result<Vec<u8>, String> {
    let target_channels = decode(target)?.channels.len();
    let adapted = adapt_channels(source, target_channels)?;
    match target.get(..4) {
        Some(b"FWAV") => Bfwav::encode_pcm16(&adapted),
        Some(b"BWAV") => Bwav::encode_like(target, &adapted),
        _ => Err("selected entry is not replaceable wave audio".into()),
    }
}

pub fn encode_replacement_to_limit(
    target: &[u8],
    source: &DecodedAudio,
    maximum_size: usize,
) -> Result<Vec<u8>, String> {
    let target_channels = decode(target)?.channels.len();
    let adapted = adapt_channels(source, target_channels)?;
    let original_samples = adapted.channels.first().map_or(0, Vec::len);
    if original_samples == 0 {
        return Err("replacement audio has no samples".into());
    }

    let mut sample_count = original_samples;
    loop {
        let candidate = if sample_count == original_samples {
            adapted.clone()
        } else {
            resample(&adapted, sample_count)?
        };
        let encoded = encode_replacement(target, &candidate)?;
        if encoded.len() <= maximum_size {
            return Ok(encoded);
        }
        if sample_count == 1 {
            return Err(format!(
                "the replacement cannot fit in the original {maximum_size}-byte allocation"
            ));
        }
        let ratio = maximum_size as f64 / encoded.len() as f64;
        let next =
            ((sample_count as f64 * ratio * 0.98).floor() as usize).clamp(1, sample_count - 1);
        sample_count = next;
    }
}

fn resample(audio: &DecodedAudio, output_samples: usize) -> Result<DecodedAudio, String> {
    let input_samples = audio.channels.first().map_or(0, Vec::len);
    if input_samples == 0 || output_samples == 0 {
        return Err("cannot resample empty audio".into());
    }
    let mut channels = Vec::with_capacity(audio.channels.len());
    for input in &audio.channels {
        if input.len() != input_samples {
            return Err("replacement channels have different lengths".into());
        }
        let mut output = Vec::with_capacity(output_samples);
        for index in 0..output_samples {
            let position = if output_samples == 1 {
                0.0
            } else {
                index as f64 * (input_samples - 1) as f64 / (output_samples - 1) as f64
            };
            let lower = position.floor() as usize;
            let upper = (lower + 1).min(input_samples - 1);
            let fraction = position - lower as f64;
            let value = input[lower] as f64 * (1.0 - fraction) + input[upper] as f64 * fraction;
            output.push(value.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16);
        }
        channels.push(output);
    }
    // Browsers reject WAV files with extremely low sample rates. Keep fitted
    // previews inside the broadly supported range while preserving duration.
    let sample_rate = ((audio.sample_rate as u64 * output_samples as u64)
        .div_ceil(input_samples as u64))
    .clamp(8_000, u32::MAX as u64) as u32;
    Ok(DecodedAudio {
        channels,
        sample_rate,
        looping: false,
        loop_start: 0,
    })
}

fn adapt_channels(source: &DecodedAudio, target_channels: usize) -> Result<DecodedAudio, String> {
    if source.channels.is_empty() || target_channels == 0 {
        return Err("audio has no channels".into());
    }
    let channels = if source.channels.len() == target_channels {
        source.channels.clone()
    } else if source.channels.len() == 1 {
        vec![source.channels[0].clone(); target_channels]
    } else if target_channels == 1 {
        let samples = source.channels[0].len();
        if source
            .channels
            .iter()
            .any(|channel| channel.len() != samples)
        {
            return Err("replacement channels have different lengths".into());
        }
        let mut mono = Vec::with_capacity(samples);
        for index in 0..samples {
            let sum: i64 = source
                .channels
                .iter()
                .map(|channel| channel[index] as i64)
                .sum();
            mono.push((sum / source.channels.len() as i64) as i16);
        }
        vec![mono]
    } else {
        return Err(format!(
            "cannot map {} replacement channels to {target_channels} target channels",
            source.channels.len()
        ));
    };
    Ok(DecodedAudio {
        channels,
        sample_rate: source.sample_rate,
        looping: false,
        loop_start: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bwav_exports_decodable_wav_and_mp3() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/_ss/AssassinSenior_Katagoki.bwav");
        let data = std::fs::read(&source).expect("failed to read BWAV export fixture");
        let wav = to_wav(&data).expect("failed to export WAV");
        assert!(wav.starts_with(b"RIFF"));

        let mp3 = to_mp3(&data).expect("failed to export MP3");
        assert!(!mp3.is_empty());
        let output =
            std::env::temp_dir().join(format!("totkbits-audio-export-{}.mp3", std::process::id()));
        std::fs::write(&output, mp3).expect("failed to write temporary MP3");
        let decoded = Bfwav::decode_source(&output).expect("exported MP3 did not decode");
        let _ = std::fs::remove_file(output);
        assert_eq!(decoded.sample_rate, decode(&data).unwrap().sample_rate);
        assert!(!decoded.channels.is_empty());
    }

    #[test]
    fn oversized_replacement_can_be_resampled_to_original_size() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/_ss/AssassinSenior_Katagoki.bwav");
        let data = std::fs::read(source).expect("failed to read BWAV replacement fixture");
        let mut replacement = decode(&data).expect("failed to decode BWAV replacement fixture");
        for channel in &mut replacement.channels {
            let original = channel.clone();
            channel.extend_from_slice(&original);
        }
        let oversized = encode_replacement(&data, &replacement).expect("failed to encode fixture");
        assert!(oversized.len() > data.len());

        let fitted = encode_replacement_to_limit(&data, &replacement, data.len())
            .expect("failed to fit replacement");
        assert!(fitted.len() <= data.len());
        let fitted_audio = decode(&fitted).expect("fitted replacement did not decode");
        assert!(fitted_audio.sample_rate < replacement.sample_rate);
    }
}

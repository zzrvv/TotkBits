import React, { useEffect, useRef, useState } from 'react';
import { invoke } from './DocumentState';

const AudioView = ({ activeTab, setActiveTab, setStatusText, setpaths }) => {
    const [preview, setPreview] = useState(null);
    const [busy, setBusy] = useState(false);
    const audioRef = useRef(null);

    useEffect(() => {
        const receive = (event) => setPreview(event.detail);
        window.addEventListener('totkbits:audio-preview', receive);
        return () => window.removeEventListener('totkbits:audio-preview', receive);
    }, []);

    const close = () => {
        audioRef.current?.pause();
        setPreview(null);
        setActiveTab('SARC');
    };

    const replace = async () => {
        const sourcePath = await invoke('open_audio_file_dialog');
        if (!sourcePath || !preview) return;
        setBusy(true);
        window.dispatchEvent(new CustomEvent('totkbits:audio-processing', { detail: 'Encoding replacement audio…' }));
        setStatusText('Encoding WAV/MP3 replacement…');
        try {
            let result = await invoke('replace_bfwav_node', { path: preview.path, sourcePath });
            if (result.increased) {
                const fit = window.confirm(
                    `The encoded audio increased from ${result.old_size.toLocaleString()} to ${result.new_size.toLocaleString()} bytes.\n\nCompress it to fit the original size? This lowers the sample rate and audio quality.`,
                );
                if (fit) {
                    window.dispatchEvent(new CustomEvent('totkbits:audio-processing', { detail: 'Compressing replacement to fit…' }));
                    result = await invoke('replace_bfwav_node', {
                        path: preview.path,
                        sourcePath,
                        fitToOriginal: true,
                        maximumSize: result.old_size,
                    });
                } else {
                    window.alert(`Warning: the internal audio is now ${result.new_size.toLocaleString()} bytes.`);
                }
            }
            const next = await invoke('open_bfwav_node', { path: preview.path });
            setPreview(next);
            setpaths((current) => ({
                ...current,
                modded_paths: [...new Set([...(current.modded_paths || []), preview.path])],
            }));
            setStatusText(result.compressed
                ? `Replaced ${preview.path}; compressed to ${result.new_size.toLocaleString()} bytes at ${result.sample_rate.toLocaleString()} Hz`
                : `Replaced ${preview.path}`);
        } catch (error) {
            setStatusText(`Audio replacement failed: ${error}`);
        } finally {
            setBusy(false);
            window.dispatchEvent(new CustomEvent('totkbits:audio-processing'));
        }
    };

    const exportAudio = async (format) => {
        if (!preview) return;
        setBusy(true);
        setStatusText(`Exporting ${format.toUpperCase()}…`);
        try {
            const output = await invoke('export_bfwav_node', { path: preview.path, format });
            setStatusText(output ? `Exported audio: ${output}` : 'Audio export cancelled');
        } catch (error) {
            setStatusText(`Audio export failed: ${error}`);
        } finally {
            setBusy(false);
        }
    };

    if (activeTab !== 'AUDIO') return null;
    return <section className="audio-view">
        <button type="button" className="audio-close" title="Close audio player" onClick={close}>×</button>
        {preview ? <>
            <div className="audio-title" title={preview.path}>{preview.path.split('/').pop()}</div>
            <audio
                ref={audioRef}
                controls
                controlsList="nodownload"
                onContextMenu={(event) => event.preventDefault()}
                src={preview.data_url}
            />
            <div className="audio-details">
                <div className="audio-metadata">
                    <span>{preview.sample_rate.toLocaleString()} Hz</span>
                    <span>{preview.channels} channel{preview.channels === 1 ? '' : 's'}</span>
                    <span>{preview.samples.toLocaleString()} samples</span>
                    <span>{preview.looping ? 'Looped' : 'Not looped'}</span>
                </div>
                <div className="audio-actions">
                    <button type="button" disabled={busy} onClick={replace}>Replace WAV/MP3</button>
                    <button type="button" disabled={busy} onClick={() => exportAudio('wav')}>Export WAV</button>
                    <button type="button" disabled={busy} onClick={() => exportAudio('mp3')}>Export MP3</button>
                </div>
            </div>
        </> : <p>Select a BFWAV or BWAV node.</p>}
    </section>;
};

export default AudioView;

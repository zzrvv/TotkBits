import React, { useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { invoke } from './DocumentState';
import { getDocumentsSnapshot, subscribeDocuments } from './DocumentState';
import { useSyncExternalStore } from 'react';
import './ImageView.css';

export default function ImageView({ activeTab, setStatusText }) {
    const { documents, activeDocumentId } = useSyncExternalStore(subscribeDocuments, getDocumentsSnapshot);
    const document = documents.find((value) => value.id === activeDocumentId);
    const [image, setImage] = useState(null);
    const [error, setError] = useState('');
    const [zoom, setZoom] = useState(1);
    const [showTree, setShowTree] = useState(true);
    const [selectedNode, setSelectedNode] = useState('image');
    const [ddsType, setDdsType] = useState('BC7');
    const [mipCount, setMipCount] = useState(1);
    const [revision, setRevision] = useState(0);

    useEffect(() => {
        if (activeTab !== 'IMAGE' || !document?.fullPath) return;
        let cancelled = false;
        setImage(null);
        setError('');
        invoke('render_image', { path: document.fullPath }).then((result) => {
            if (cancelled) return;
            setImage(result);
            if (result.format === 'DDS') setMipCount(result.mipCount || 1);
        }).catch((reason) => {
            if (!cancelled) setError(String(reason));
        });
        return () => { cancelled = true; };
    }, [activeTab, document?.fullPath, revision]);

    if (activeTab !== 'IMAGE') return null;
    const exportPng = async () => {
        const output = await save({
            defaultPath: `${document?.title || 'image'}.png`,
            filters: [{ name: 'PNG image', extensions: ['png'] }],
        });
        if (!output) return;
        await invoke('export_image_png', { source: document.fullPath, output });
        setStatusText(`Rendered PNG ${output}`);
    };
    const replaceDds = async () => {
        const png = await open({ multiple: false, filters: [{ name: 'PNG image', extensions: ['png'] }] });
        if (!png) return;
        await invoke('replace_dds_image', { target: document.fullPath, png, ddsType, mipCount: Number(mipCount) });
        setRevision((value) => value + 1);
        setStatusText(`Replaced ${document.title} from ${png}`);
    };
    const changeZoom = (amount) => setZoom((value) => Math.min(16, Math.max(0.05, value + amount)));
    const fileName = document?.fullPath?.replace(/\\/g, '/').split('/').pop() || document?.title || 'Image';

    return <main className="image-workspace">
        <header>
            <div><strong>{document?.title || 'Image'}</strong>{image && <span>{image.format} · {image.width} × {image.height}</span>}</div>
            <button type="button" onClick={() => setShowTree((value) => !value)}>{showTree ? 'Hide tree' : 'Show tree'}</button>
            <div className="image-zoom-controls">
                <button type="button" onClick={() => changeZoom(-0.25)} aria-label="Zoom out">−</button>
                <button type="button" onClick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button>
                <button type="button" onClick={() => changeZoom(0.25)} aria-label="Zoom in">+</button>
            </div>
            <button type="button" onClick={exportPng} disabled={!image}>Render PNG</button>
        </header>
        {showTree && <nav className="image-tree" aria-label="Image contents">
            <header>Image contents</header>
            <button type="button" className={selectedNode === 'image' ? 'selected' : ''} onClick={() => setSelectedNode('image')}>{fileName}</button>
        </nav>}
        <section className="image-canvas" onWheel={(event) => { if (event.ctrlKey) { event.preventDefault(); changeZoom(event.deltaY < 0 ? 0.1 : -0.1); } }}>
            {!image && !error && <span>Decoding image…</span>}
            {error && <div className="image-error">{error}</div>}
            {image && <img src={image.dataUrl} alt={document?.title || 'Decoded image'} style={{ width: `${image.width * zoom}px`, height: `${image.height * zoom}px` }} />}
        </section>
        <aside className="image-options">
            <header><strong>{fileName}</strong><small>{image?.format || 'Image'}</small></header>
            {image && <dl><dt>Width</dt><dd>{image.width}</dd><dt>Height</dt><dd>{image.height}</dd><dt>Mip count</dt><dd>{image.mipCount}</dd>{image.ddsType && <><dt>DDS type</dt><dd>{image.ddsType}</dd></>}</dl>}
            {image?.format === 'DDS' && selectedNode === 'image' && <section>
                <h3>Replace from PNG</h3>
                <label>DDS type<select value={ddsType} onChange={(event) => setDdsType(event.target.value)}><option>BC1</option><option>BC3</option><option>BC5</option><option>BC7</option><option>RGBA8</option></select></label>
                <label>Mip count<input type="number" min="1" max="16" value={mipCount} onChange={(event) => setMipCount(Math.max(1, Number(event.target.value)))} /></label>
                <button type="button" onClick={replaceDds}>Replace…</button>
            </section>}
        </aside>
    </main>;
}

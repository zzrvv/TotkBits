import { useEffect, useState } from 'react';

export default function AmtaView({ activeTab, setActiveTab }) {
    const [document, setDocument] = useState(null);
    useEffect(() => {
        const receive = (event) => setDocument(event.detail);
        window.addEventListener('totkbits:amta-preview', receive);
        return () => window.removeEventListener('totkbits:amta-preview', receive);
    }, []);
    if (activeTab !== 'AMTA') return null;
    return <section className="amta-view">
        <header><div><strong>{document?.path?.split('/').pop() || 'AMTA'}</strong><small>Audio metadata</small></div><button type="button" title="Close AMTA viewer" onClick={() => setActiveTab('SARC')}>×</button></header>
        {document?.metadata ? <>
            <dl className="amta-summary">
                <div><dt>Name</dt><dd>{document.metadata.name || '—'}</dd></div>
                <div><dt>Version</dt><dd>{document.metadata.version}</dd></div>
                <div><dt>Byte order</dt><dd>{document.metadata.byte_order}</dd></div>
                <div><dt>File size</dt><dd>{document.metadata.file_size.toLocaleString()} bytes</dd></div>
            </dl>
            <div className="amta-chunks">{document.metadata.chunks.map((chunk, index) => <details key={`${chunk.offset}-${index}`} open>
                <summary><strong>{chunk.magic}</strong><span>0x{chunk.offset.toString(16).toUpperCase()} · {chunk.size.toLocaleString()} bytes</span></summary>
                {chunk.strings.length ? <ul>{chunk.strings.map((value, stringIndex) => <li key={`${value}-${stringIndex}`}>{value}</li>)}</ul> : <p>No decoded strings</p>}
            </details>)}</div>
        </> : <p>No AMTA node selected.</p>}
    </section>;
}

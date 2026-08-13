import { useEffect, useState } from 'react';

const hex = (value) => `0x${value.toString(16).toUpperCase()}`;

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
                <div><dt>Declared size</dt><dd>{document.metadata.declared_file_size.toLocaleString()} bytes</dd></div>
                <div><dt>Header size</dt><dd>{document.metadata.header_size.toLocaleString()} bytes</dd></div>
                <div><dt>Sections</dt><dd>{document.metadata.chunks.length}</dd></div>
                <div><dt>Decoded strings</dt><dd>{document.metadata.total_strings}</dd></div>
                <div><dt>32-bit words</dt><dd>{document.metadata.total_words.toLocaleString()}</dd></div>
                <div><dt>Zero bytes</dt><dd>{document.metadata.total_zero_bytes.toLocaleString()}</dd></div>
            </dl>
            {document.metadata.diagnostics?.length ? <details className="amta-metadata" open>
                <summary><strong>Diagnostics</strong><span>{document.metadata.diagnostics.length}</span></summary>
                <ul>{document.metadata.diagnostics.map((value, index) => <li key={index}>{value}</li>)}</ul>
            </details> : null}
            <details className="amta-metadata" open>
                <summary><strong>Header fields</strong><span>{document.metadata.header_fields.length} fields</span></summary>
                <table><thead><tr><th>Offset</th><th>Field</th><th>Raw</th><th>Unsigned</th><th>Target</th></tr></thead>
                    <tbody>{document.metadata.header_fields.map((field) => <tr key={field.offset}>
                        <td>{hex(field.offset)}</td><td>{field.name}</td><td>{field.raw_hex}</td>
                        <td>{field.value.toLocaleString()}</td><td>{field.target == null ? '—' : hex(field.target)}</td>
                    </tr>)}</tbody>
                </table>
            </details>
            <details className="amta-metadata">
                <summary><strong>All printable strings</strong><span>{document.metadata.strings.length}</span></summary>
                <table><thead><tr><th>Offset</th><th>Value</th></tr></thead>
                    <tbody>{document.metadata.strings.map((entry, index) => <tr key={`${entry.offset}-${index}`}><td>{hex(entry.offset)}</td><td>{entry.value}</td></tr>)}</tbody>
                </table>
            </details>
            <div className="amta-chunks">{document.metadata.chunks.map((chunk, index) => <details key={`${chunk.offset}-${index}`} open>
                <summary><strong>{chunk.magic}</strong><span>{chunk.source} · {hex(chunk.offset)}–{hex(chunk.end_offset)} · {chunk.size.toLocaleString()} bytes</span></summary>
                <dl className="amta-summary">
                    <div><dt>Alignment</dt><dd>{chunk.alignment || '—'}</dd></div>
                    <div><dt>Non-zero bytes</dt><dd>{chunk.nonzero_bytes.toLocaleString()}</dd></div>
                    <div><dt>Zero bytes</dt><dd>{chunk.zero_bytes.toLocaleString()}</dd></div>
                    <div><dt>Words</dt><dd>{chunk.words.length.toLocaleString()}</dd></div>
                </dl>
                <p><code>{chunk.preview_hex}</code></p>
                {chunk.string_entries.length ? <details open><summary>Strings ({chunk.string_entries.length})</summary>
                    <table><thead><tr><th>Offset</th><th>Value</th></tr></thead><tbody>
                        {chunk.string_entries.map((entry, stringIndex) => <tr key={`${entry.offset}-${stringIndex}`}><td>{hex(entry.offset)}</td><td>{entry.value}</td></tr>)}
                    </tbody></table>
                </details> : <p>No decoded strings</p>}
                <details><summary>32-bit interpretation ({chunk.words.length.toLocaleString()} words)</summary>
                    <table><thead><tr><th>Offset</th><th>Raw</th><th>Unsigned</th><th>Signed</th><th>Float</th><th>ASCII</th><th>Target</th></tr></thead>
                        <tbody>{chunk.words.map((word) => <tr key={word.offset}>
                            <td>{hex(word.offset)}</td><td>{word.raw_hex}</td><td>{word.unsigned.toLocaleString()}</td>
                            <td>{word.signed.toLocaleString()}</td><td>{word.float == null ? '—' : word.float.toPrecision(8)}</td>
                            <td>{word.ascii || '—'}</td><td>{word.target == null ? '—' : hex(word.target)}</td>
                        </tr>)}</tbody>
                    </table>
                </details>
            </details>)}</div>
        </> : <p>No AMTA node selected.</p>}
    </section>;
}

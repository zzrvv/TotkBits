import React from 'react';

const operations = [
    ['bin_to_text', '<file-type> <input-file> <output-file>', 'Convert a supported binary file to editable text.'],
    ['text_to_bin', '<file-type> <input-file> <output-file>', 'Convert YAML, JSON, or text back to its binary format.'],
    ['extract_archive', '<zip|7z|rar|sarc> <input-archive> <output-directory>', 'Extract every archive entry.'],
    ['dir_to_archive', '<zip|7z|rar|sarc> <input-directory> <output-archive>', 'Build an archive from a directory.'],
];

export default function CommandsHelp({ isOpen, onClose }) {
    if (!isOpen) return null;
    return <div className="modal-overlay" role="dialog" aria-modal="true" aria-labelledby="commands-title">
        <div className="modal-content commands-help-modal">
            <div className="settings-header">
                <h2 id="commands-title">Command-line commands</h2>
                <button className="settings-close" onClick={onClose} aria-label="Close commands" title="Close">×</button>
            </div>
            <p>Run TotkBits with <code>-c</code> or <code>--cli</code>. All four arguments are required.</p>
            <pre>Totkbits.exe --cli &lt;operation&gt; &lt;type&gt; &lt;input&gt; &lt;output&gt;</pre>
            <div className="commands-help-list">
                {operations.map(([name, args, description]) => <section key={name}>
                    <code>{name} {args}</code>
                    <p>{description}</p>
                </section>)}
            </div>
            <h3>Supported conversion types</h3>
            <p><code>ainb, asb, byml, bcett, tagproduct, aamp, msbt, evfl, xlink, text, smo</code></p>
            <h3>Examples</h3>
            <pre>{`Totkbits.exe --cli bin_to_text ainb input.ainb output.yml
Totkbits.exe -c text_to_bin byml input.yml output.byml
Totkbits.exe --cli extract_archive 7z input.7z output-folder
Totkbits.exe -c dir_to_archive sarc input-folder output.pack`}</pre>
            <p><code>A successful command exits without opening the GUI. If validation or conversion fails, TotkBits reports the error and opens normally.</code></p>
            <div className="options-modal-footer"><button className="generic_button" onClick={onClose}>Close</button></div>
        </div>
    </div>;
}

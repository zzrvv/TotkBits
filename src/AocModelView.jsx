import { useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { OpenFileFromPath } from './ButtonClicks';
import { useEditorContext } from './StateManager';

export default function AocModelView({ activeTab }) {
    const {
        aocModelCatalog, setStatusText, setActiveTab, setLabelTextDisplay,
        setpaths, updateEditorContent,
    } = useEditorContext();
    const [filter, setFilter] = useState('');
    const query = filter.trim().toLocaleLowerCase();
    const matches = useMemo(() => {
        if (!aocModelCatalog) return [];
        const filtered = Object.entries(aocModelCatalog)
            .filter(([hash, name]) => hash.toLocaleLowerCase().includes(query)
                || String(name).toLocaleLowerCase().includes(query));
        if (filtered.length > 200) return [];
        return filtered.sort((left, right) => String(left[1]).localeCompare(String(right[1]), undefined, {
                numeric: true,
                sensitivity: 'base',
            }));
    }, [aocModelCatalog, query]);
    const preview = async (hash) => {
        setStatusText(`Locating AOC model ${hash}...`);
        try {
            const path = await invoke('preview_aoc_model', { hash });
            if (!path) {
                setStatusText(`AOC model ${hash} was not found`);
                return;
            }
            await OpenFileFromPath(
                path, setStatusText, setActiveTab, setLabelTextDisplay,
                setpaths, updateEditorContent,
            );
        } catch (error) {
            setStatusText(`Unable to preview AOC model ${hash}: ${error}`);
        }
    };
    const copyHash = async (hash) => {
        try {
            await navigator.clipboard.writeText(hash);
            setStatusText(`Copied AOC model hash ${hash}`);
        } catch (error) {
            setStatusText(`Unable to copy AOC model hash ${hash}: ${error}`);
        }
    };

    if (activeTab !== 'AOC_MODELS') return null;
    return <main className="aoc-model-view">
        <header>
            <h2>AOC models</h2>
            <input
                autoFocus
                type="search"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder="Filter by hash or name"
                aria-label="Filter AOC models"
            />
        </header>
        {filter.length > 0 && matches.length > 0 &&  <div className="aoc-model-results">
            {matches.map(([hash, name]) => <div className="aoc-model-result" key={hash}>
                <img
                    src={`/AOC/${hash.toLocaleLowerCase()}.png`}
                    onError={(event) => { event.currentTarget.onerror = null; event.currentTarget.src = '/no_preview.png'; }}
                    alt=""
                />
                <code>{hash}</code>
                <span>{name || '—'}</span>
                <button
                    className="aoc-model-copy"
                    type="button"
                    onClick={() => copyHash(hash)}
                    title={`Copy ${hash}`}
                    aria-label={`Copy AOC model hash ${hash}`}
                >
                    <img src="/clipboard.png" alt="" />
                </button>
                <button type="button" onClick={() => preview(hash)}>Preview</button>
            </div>)}
        </div>}
    </main>;
}

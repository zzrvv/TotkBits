import { useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { OpenFileFromPath } from './ButtonClicks';
import { openModelCollectionDocument } from './DocumentState';
import { useEditorContext } from './StateManager';

export default function AocModelView({ activeTab }) {
    const {
        aocModelCatalog, setStatusText, setActiveTab, setLabelTextDisplay,
        setpaths, updateEditorContent,
    } = useEditorContext();
    const minCharCount = 3;
    const maxDisplayedRecords = 1024;
    const [filter, setFilter] = useState('');
    const [hideMissingPreviews, setHideMissingPreviews] = useState(false);
    const [hideFarLod, setHideFarLod] = useState(true);
    const [minSizeMb, setMinSizeMb] = useState('');
    const [missingPreviewHashes, setMissingPreviewHashes] = useState(() => new Set());
    const [selectedHashes, setSelectedHashes] = useState(() => new Set());
    const query = filter.trim().toLowerCase();
    const minimumBytes = Math.max(0, Number(minSizeMb) || 0) * 1024 * 1024;
    const allMatches = useMemo(() => {
        if (!aocModelCatalog || query.length < minCharCount) return [];
        return Object.entries(aocModelCatalog)
            .filter(([hash, entry]) => String(hash).toLowerCase().includes(query)
                || String(entry?.name).toLowerCase().includes(query))
            .filter(([, entry]) => Number(entry?.size || 0) >= minimumBytes)
            .filter(([, entry]) => !hideFarLod || !String(entry?.name).toLowerCase().includes('far'))
            .sort((left, right) => String(left[1]?.name).localeCompare(String(right[1]?.name), undefined, {
                numeric: true,
                sensitivity: 'base',
            }));
    }, [aocModelCatalog, query, hideFarLod, minimumBytes]);
    const matches = useMemo(() => hideMissingPreviews
        ? allMatches.filter(([hash]) => !missingPreviewHashes.has(hash))
        : allMatches, [allMatches, hideMissingPreviews, missingPreviewHashes]);
    const displayedMatches = useMemo(
        () => matches.slice(0, maxDisplayedRecords),
        [matches],
    );
    const markMissingPreview = (hash, image) => {
        setMissingPreviewHashes((current) => {
            if (current.has(hash)) return current;
            const next = new Set(current);
            next.add(hash);
            return next;
        });
        image.onerror = null;
        image.src = '/no_preview.png';
    };
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
    const toggleSelected = (hash) => {
        setSelectedHashes((current) => {
            const next = new Set(current);
            if (next.has(hash)) next.delete(hash);
            else if (next.size < 5) next.add(hash);
            return next;
        });
    };
    const importSelected = async () => {
        const hashes = [...selectedHashes].slice(0, 5);
        if (hashes.length < 2) return;
        setStatusText(`Locating ${hashes.length} selected AOC models...`);
        try {
            const paths = (await Promise.all(hashes.map((hash) =>
                invoke('preview_aoc_model', { hash })))).filter(Boolean);
            if (paths.length !== hashes.length) {
                setStatusText(`Unable to locate all ${hashes.length} selected AOC models`);
                return;
            }
            openModelCollectionDocument(paths, `${hashes.length} selected AOC models`);
            setActiveTab('3D');
            setStatusText(`Loading ${hashes.length} selected AOC models...`);
        } catch (error) {
            setStatusText(`Unable to import selected AOC models: ${error}`);
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
    const displayName = (name) => {
        const value = name || '—';
        return value.length > 40 ? `${value.slice(0, 37)}...` : value;
    };
    const displaySize = (size) => `${(Number(size || 0) / (1024 * 1024)).toFixed(2)} MB`;
    if (activeTab !== 'AOC_MODELS') return null;
    const aocTitle = allMatches.length > 0 && filter.length >= minCharCount ? `AOC models (found ${allMatches.length})` : "AOC models";
    return <main className="aoc-model-view">
        <header>
            <h2>{aocTitle}</h2>
            <input
                autoFocus
                type="search"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder="Filter by hash or name"
                aria-label="Filter AOC models"
            />
            <div className="aoc-model-filters">
                <div className="aoc-model-preview-filter">
                    <input
                        type="checkbox"
                        checked={hideMissingPreviews}
                        onChange={(event) => setHideMissingPreviews(event.target.checked)}
                        aria-label="Hide entries without preview image"
                    />
                    <span>Hide entries without preview image</span>
                </div>
                <div className="aoc-model-preview-filter">
                    <input
                        type="checkbox"
                        checked={hideFarLod}
                        onChange={(event) => setHideFarLod(event.target.checked)}
                        aria-label="Hide far/LOD"
                    />
                    <span>Hide far/LOD</span>
                </div>
                <label className="aoc-model-preview-filter">
                    <span>Min size (MB)</span>
                    <input
                        className="aoc-model-min-size"
                        type="number"
                        min="0"
                        step="0.5"
                        value={minSizeMb}
                        onChange={(event) => setMinSizeMb(event.target.value)}
                        placeholder="0"
                        aria-label="Minimum model size in MB"
                    />
                </label>
            </div>
            {selectedHashes.size > 0 && <div className="aoc-model-selection-actions">
                {selectedHashes.size > 1 && <button
                    className="aoc-model-import-selected"
                    type="button"
                    onClick={importSelected}
                >Import selected ({selectedHashes.size})</button>}
                <button
                    className="aoc-model-clear-selection"
                    type="button"
                    onClick={() => setSelectedHashes(new Set())}
                >Clear</button>
                <span className="aoc-model-selection-count">
                    {selectedHashes.size}/5 selected
                </span>
            </div>}
        </header>
        {false && query && matches.length > maxDisplayedRecords && <p className="aoc-model-result-limit" role="status">
            Showing the first {maxDisplayedRecords} of {matches.length} matches. Refine the name or hash to narrow the results.
        </p>}
        {filter.length >= minCharCount && displayedMatches.length > 0 &&  <div className="aoc-model-results">
            {displayedMatches.map(([hash, entry]) => {
                const name = entry?.name || '';
                const checked = selectedHashes.has(hash);
                const selectionFull = selectedHashes.size >= 5 && !checked;
                return <div className={`aoc-model-result${checked ? ' selected' : ''}`} key={hash}>
                <input
                    className="aoc-model-select"
                    type="checkbox"
                    checked={checked}
                    disabled={selectionFull}
                    onChange={() => toggleSelected(hash)}
                    aria-label={`Select AOC model ${hash}`}
                    title={selectionFull ? 'A maximum of 5 models can be selected' : `Select ${hash}`}
                />
                <img
                    src={`/webp/${hash.toLocaleLowerCase()}.webp`}
                    onError={(event) => markMissingPreview(hash, event.currentTarget)}
                    alt=""
                />
                <code>{hash}</code>
                <span title={name || undefined}>{displayName(name)}</span>
                <span className="aoc-model-size">{displaySize(entry?.size)}</span>
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
            </div>})}
        </div>}
    </main>;
}

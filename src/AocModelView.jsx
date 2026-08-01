import { useMemo, useState } from 'react';
import { useEditorContext } from './StateManager';

export default function AocModelView({ activeTab }) {
    const { aocModelCatalog } = useEditorContext();
    const [filter, setFilter] = useState('');
    const query = filter.trim().toLocaleLowerCase();
    const matches = useMemo(() => {
        if (query.length < 4 || !aocModelCatalog) return [];
        return Object.entries(aocModelCatalog)
            .filter(([hash, name]) => hash.toLocaleLowerCase().includes(query)
                || String(name).toLocaleLowerCase().includes(query))
            .sort((left, right) => String(left[1]).localeCompare(String(right[1]), undefined, {
                numeric: true,
                sensitivity: 'base',
            }));
    }, [aocModelCatalog, query]);

    if (activeTab !== 'AOC_MODELS') return null;
    return <main className="aoc-model-view">
        <header>
            <h2>AOC models</h2>
            <input
                autoFocus
                type="search"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder="Filter by hash or name (at least 4 characters)"
                aria-label="Filter AOC models"
            />
        </header>
        {query.length >= 4 && <div className="aoc-model-results">
            {matches.map(([hash, name]) => <div className="aoc-model-result" key={hash}>
                <code>{hash}</code>
                <span>{name}</span>
            </div>)}
        </div>}
    </main>;
}

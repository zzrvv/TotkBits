import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useMemo, useState } from 'react';
import './PhysicsMerge.css';

const locationLabel = {
    disk: 'Disk',
    archive: 'Archive',
    'nested-archive': 'Nested archive',
};

function PhysicsMerge({ activeTab, setActiveTab, returnTab, setStatusText }) {
    const [documents, setDocuments] = useState([]);
    const [targetId, setTargetId] = useState('');
    const [sourceId, setSourceId] = useState('');
    const [nodes, setNodes] = useState([]);
    const [selectedNodeIds, setSelectedNodeIds] = useState(new Set());
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState('');
    const [merging, setMerging] = useState(false);

    const refreshDocuments = useCallback(async () => {
        setLoading(true);
        setError('');
        try {
            const openDocuments = await invoke('list_open_bphcl_documents');
            setDocuments(openDocuments);
            setTargetId((current) => openDocuments.some((item) => item.documentId === current)
                ? current
                : openDocuments[0]?.documentId || '');
            setSourceId((current) => {
                const target = openDocuments.some((item) => item.documentId === targetId)
                    ? targetId
                    : openDocuments[0]?.documentId;
                if (openDocuments.some((item) => item.documentId === current && item.documentId !== target)) {
                    return current;
                }
                return openDocuments.find((item) => item.documentId !== target)?.documentId || '';
            });
        } catch (reason) {
            const message = String(reason);
            setError(message);
            setStatusText(`ERROR: ${message}`);
        } finally {
            setLoading(false);
        }
    }, [setStatusText, targetId]);

    useEffect(() => {
        if (activeTab === 'PHYSICS_MERGE') refreshDocuments();
    }, [activeTab]);

    useEffect(() => {
        if (activeTab !== 'PHYSICS_MERGE' || !sourceId) {
            setNodes([]);
            setSelectedNodeIds(new Set());
            return;
        }
        let cancelled = false;
        setLoading(true);
        setError('');
        invoke('list_bphcl_selectable_nodes', { documentId: sourceId })
            .then((result) => {
                if (!cancelled) {
                    setNodes(result);
                    setSelectedNodeIds(new Set());
                }
            })
            .catch((reason) => {
                if (!cancelled) {
                    const message = String(reason);
                    setError(message);
                    setStatusText(`ERROR: ${message}`);
                }
            })
            .finally(() => {
                if (!cancelled) setLoading(false);
            });
        return () => { cancelled = true; };
    }, [activeTab, sourceId, setStatusText]);

    useEffect(() => {
        if (targetId === sourceId) {
            setSourceId(documents.find((item) => item.documentId !== targetId)?.documentId || '');
        }
    }, [documents, sourceId, targetId]);

    const groups = useMemo(() => [
        ['cloth', 'Complete cloths'],
        ['collidable', 'Standalone collidables'],
    ].map(([kind, label]) => ({ kind, label, nodes: nodes.filter((node) => node.kind === kind) })), [nodes]);

    const toggleNode = (nodeId) => {
        setSelectedNodeIds((current) => {
            const next = new Set(current);
            if (next.has(nodeId)) next.delete(nodeId);
            else next.add(nodeId);
            return next;
        });
    };

    const mergeSelection = async () => {
        if (!targetId || !sourceId || selectedNodeIds.size === 0) return;
        setMerging(true);
        setError('');
        try {
            const result = await invoke('merge_bphcl_nodes', {
                targetDocumentId: targetId,
                sourceDocumentId: sourceId,
                nodeIds: Array.from(selectedNodeIds),
            });
            setSelectedNodeIds(new Set());
            await refreshDocuments();
            const status = `Physics merge completed: ${result.importedCount} imported, ${result.skippedCount} skipped`;
            setStatusText(status);
        } catch (reason) {
            const message = String(reason);
            setError(message);
            setStatusText(`ERROR: ${message}`);
        } finally {
            setMerging(false);
        }
    };

    if (activeTab !== 'PHYSICS_MERGE') return null;

    return <section className="physics-merge-view" aria-labelledby="physics-merge-title">
        <header className="physics-merge-header">
            <div>
                <h1 id="physics-merge-title">Physics Merge</h1>
                <p>Select complete cloth graphs or standalone collidables to import into another open BPHCL document.</p>
            </div>
            <div className="physics-merge-actions">
                <button type="button" onClick={refreshDocuments} disabled={loading}>Refresh</button>
                <button type="button" onClick={() => setActiveTab(returnTab)}>Close</button>
            </div>
        </header>

        <div className="physics-merge-documents">
            <label>Target document
                <select value={targetId} onChange={(event) => setTargetId(event.target.value)}>
                    {documents.map((document) => <option key={document.documentId} value={document.documentId} disabled={document.documentId === sourceId}>
                        {document.label} — {locationLabel[document.location] || document.location}
                    </option>)}
                </select>
            </label>
            <label>Source document
                <select value={sourceId} onChange={(event) => setSourceId(event.target.value)}>
                    {documents.filter((document) => document.documentId !== targetId).map((document) => <option key={document.documentId} value={document.documentId}>
                        {document.label} — {locationLabel[document.location] || document.location}
                    </option>)}
                </select>
            </label>
        </div>

        {error && <div className="physics-merge-error" role="alert">{error}</div>}
        {loading && <div className="physics-merge-empty" role="status">Loading BPHCL nodes…</div>}
        {!loading && groups.map((group) => <div className="physics-merge-group" key={group.kind}>
            <h2>{group.label} <span>{group.nodes.length}</span></h2>
            {group.nodes.length === 0
                ? <p className="physics-merge-empty">No {group.label.toLowerCase()} in the source document.</p>
                : <div className="physics-merge-node-list">
                    {group.nodes.map((node) => <label className="physics-merge-node" key={node.nodeId}>
                        <input type="checkbox" checked={selectedNodeIds.has(node.nodeId)} onChange={() => toggleNode(node.nodeId)} />
                        <span className="physics-merge-node-name">{node.name || `Unnamed ${node.kind} ${node.index}`}</span>
                        <span className="physics-merge-node-meta">ITEM {node.itemIndex}</span>
                    </label>)}
                </div>}
        </div>)}

        <footer className="physics-merge-footer">
            <span>{selectedNodeIds.size} selected</span>
            <button type="button" onClick={mergeSelection} disabled={merging || loading || selectedNodeIds.size === 0 || !targetId || !sourceId}>
                {merging ? 'Merging…' : 'Merge selected'}
            </button>
        </footer>
    </section>;
}

export default PhysicsMerge;

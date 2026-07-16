import React, { useLayoutEffect, useRef, useSyncExternalStore } from 'react';
import * as monaco from 'monaco-editor';
import {
    activateDocument, addCleanDocument, closeDocument, getDocumentsSnapshot,
    subscribeDocuments,
} from './DocumentState';
import { useEditorContext } from './StateManager';

const emptySnapshot = () => ({
    activeTab: 'SARC', statusText: 'Ready', selectedPath: { path: '', isfile: false },
    labelTextDisplay: { sarc: '', yaml: '', rstb: '', comparer: '' },
    paths: { paths: [], added_paths: [], modded_paths: [], nested_paths: {} },
    pathsFilters: { showAll: true, showAdded: false, showModded: false },
    compareData: { decision: 'FilesFromDisk', content1: '', content2: '', filepath1: '', filepath2: '', isSmall: true, isFromDisk: false, isInternal: false, label1: '', label2: '', isTiedToMonaco: false, lang: 'yaml' },
    editorText: '', editorLanguage: 'yaml',
});

const modelUri = (id) => `inmemory://totkbits/${id}`;
const isUsableModel = (model) => Boolean(model && !model.isDisposed());
const isOwnedModel = (model, id) => isUsableModel(model) && model.uri.toString() === modelUri(id);

const snapshotFor = (context, model) => ({
    activeTab: context.activeTab, statusText: context.statusText,
    selectedPath: context.selectedPath, labelTextDisplay: context.labelTextDisplay,
    paths: context.paths, pathsFilters: context.pathsFilters, compareData: context.compareData,
    editorText: isUsableModel(model) ? model.getValue() : '',
    editorLanguage: isUsableModel(model) ? model.getLanguageId() : 'yaml',
});

export default function DocumentTabs() {
    const { documents, activeDocumentId } = useSyncExternalStore(subscribeDocuments, getDocumentsSnapshot);
    const context = useEditorContext();
    const contextRef = useRef(context);
    const previousDocumentIdRef = useRef(activeDocumentId);
    contextRef.current = context;

    useLayoutEffect(() => {
        const latest = contextRef.current;
        const previous = previousDocumentIdRef.current;
        const isInitialDocument = previous === activeDocumentId
            && !latest.documentSnapshots.current.has(activeDocumentId);

        if (isInitialDocument) {
            const initialModel = latest.editorRef.current?.getModel();
            latest.documentSnapshots.current.set(activeDocumentId, snapshotFor(latest, initialModel));
            if (isOwnedModel(initialModel, activeDocumentId)) {
                latest.documentModels.current.set(activeDocumentId, initialModel);
            }
            return;
        }

        const previousModel = latest.editorRef.current?.getModel();
        latest.documentSnapshots.current.set(previous, snapshotFor(latest, previousModel));
        if (latest.editorRef.current) {
            if (isOwnedModel(previousModel, previous)) {
                latest.documentModels.current.set(previous, previousModel);
                const previousViewState = latest.editorRef.current.saveViewState();
                if (previousViewState) latest.documentViewStates.current.set(previous, previousViewState);
            }
            const snapshot = latest.documentSnapshots.current.get(activeDocumentId) || emptySnapshot();
            let model = latest.documentModels.current.get(activeDocumentId);
            if (!isOwnedModel(model, activeDocumentId)) {
                latest.documentModels.current.delete(activeDocumentId);
                const existing = monaco.editor.getModel(monaco.Uri.parse(modelUri(activeDocumentId)));
                if (isOwnedModel(existing, activeDocumentId)) existing.dispose();
                model = monaco.editor.createModel(
                    snapshot.editorText || '',
                    snapshot.editorLanguage || 'yaml',
                    monaco.Uri.parse(modelUri(activeDocumentId)),
                );
                latest.documentModels.current.set(activeDocumentId, model);
            }
            if (isUsableModel(model)) latest.editorRef.current.setModel(model);
            const viewState = latest.documentViewStates.current.get(activeDocumentId);
            if (viewState && isUsableModel(model)) latest.editorRef.current.restoreViewState(viewState);
        }
        const snapshot = latest.documentSnapshots.current.get(activeDocumentId) || emptySnapshot();
        latest.setActiveTab(snapshot.activeTab);
        latest.setStatusText(snapshot.statusText);
        latest.setSelectedPath(snapshot.selectedPath);
        latest.setLabelTextDisplay(snapshot.labelTextDisplay);
        latest.setpaths(snapshot.paths);
        latest.setPathsFilters(snapshot.pathsFilters || { showAll: true, showAdded: false, showModded: false });
        latest.setCompareData(snapshot.compareData);
        previousDocumentIdRef.current = activeDocumentId;
    }, [activeDocumentId]);

    const handleClose = async (event, id) => {
        event.stopPropagation();
        const wasActive = id === activeDocumentId;
        if (wasActive) {
            const latest = contextRef.current;
            const currentModel = latest.editorRef.current?.getModel();
            latest.documentSnapshots.current.set(id, snapshotFor(latest, currentModel));
            if (isOwnedModel(currentModel, id)) {
                latest.documentModels.current.set(id, currentModel);
                const viewState = latest.editorRef.current.saveViewState();
                if (viewState) latest.documentViewStates.current.set(id, viewState);
            }
        }
        await closeDocument(id);
        const model = context.documentModels.current.get(id);
        const cleanup = () => {
            const attachedModel = context.editorRef.current?.getModel();
            if (isOwnedModel(model, id) && attachedModel !== model) model.dispose();
            context.documentModels.current.delete(id);
            context.documentViewStates.current.delete(id);
            context.documentSnapshots.current.delete(id);
        };
        if (wasActive) requestAnimationFrame(cleanup);
        else cleanup();
    };

    return <div className="document-tabs" role="tablist">
        {documents.map((document) => <button
            type="button" role="tab" aria-selected={document.id === activeDocumentId}
            className={`document-tab ${document.id === activeDocumentId ? 'active' : ''}`}
            key={document.id} onClick={() => activateDocument(document.id)}
            onMouseDown={(event) => {
                if (event.button === 1) {
                    event.preventDefault();
                    void handleClose(event, document.id);
                }
            }}
        >
            <span>{document.title}</span>
            <span className="document-tab-close" onClick={(event) => handleClose(event, document.id)}>×</span>
        </button>)}
        <button type="button" className="document-tab-add" title="New document" onClick={addCleanDocument}>+</button>
    </div>;
}

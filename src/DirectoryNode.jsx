import React, { useLayoutEffect, useRef, useState } from 'react';
import { invoke } from './DocumentState';
import { extractRootFolderClick, extractFolderClick, editInternalSarcFile, openBphclLeaf, removeBphclNodeClick, replaceInternalFileClick, removeInternalFileClick, addInternalFileToDir, extractFileClick, addEmptyByml,addFilesFromDirRecursively, expandNestedSarc, editNestedSarcFile, extractNestedSarcFile, mutateNestedArchive } from './ButtonClicks';
import { useEditorContext } from './StateManager';
import {compareInternalFileWithOVanila} from './Comparer';

const dirOpened = `dir_opened.png`;
const dirClosed = `dir_closed.png`;
const fileIcon = `file.png`;
const iconSize = '20px';

const buildNestedTree = (paths) => {
  const root = {};
  paths.forEach((innerPath) => innerPath.split('/').reduce((parent, part, index, parts) => {
    if (!(part in parent)) parent[part] = index === parts.length - 1 ? null : {};
    return parent[part] || {};
  }, root));
  return root;
};

const NestedDirectoryNode = ({ node, name, innerParent, outerPath, selected, onSelect }) => {
  const { settings, setStatusText, setActiveTab, setLabelTextDisplay, updateEditorContent, paths, setpaths, setPathsFilters, treeExpandedNodes, setTreeExpandedNodes, setCompareData, setRenamePromptMessage, setIsAddPrompt, setIsModalOpen } = useEditorContext();
  const [contextMenu, setContextMenu] = useState({ visible: false, x: 0, y: 0 });
  const isFile = node === null;
  const innerPath = innerParent ? `${innerParent}/${name}` : name;
  const childChain = `${outerPath}::${innerPath}`;
  const expansionKey = `nested:${childChain}`;
  const isCollapsed = !treeExpandedNodes.has(expansionKey);
  const setExpanded = (expanded) => setTreeExpandedNodes((current) => {
    const next = new Set(current);
    if (expanded) next.add(expansionKey);
    else next.delete(expansionKey);
    return next;
  });
  const toggleExpanded = () => setExpanded(isCollapsed);
  const expandedArchive = paths.nested_paths?.[childChain];
  const identity = `nested:${outerPath}:${innerPath}`;
  const selectedStyle = selected === identity ? '#303030' : 'transparent';
  const select = (event) => { event.stopPropagation(); onSelect(innerPath, isFile, identity, false); };
  const open = async (event) => {
    event.stopPropagation();
    if (isFile) {
      if (expandedArchive) { toggleExpanded(); return; }
      const expanded = await expandNestedSarc(childChain, setStatusText, setpaths, setPathsFilters);
      if (expanded) setExpanded(true);
      else editNestedSarcFile(outerPath, innerPath, setStatusText, setActiveTab, setLabelTextDisplay, updateEditorContent);
    }
    else toggleExpanded();
  };
  const mutate = async (action, options = {}) => { setContextMenu({ visible: false, x: 0, y: 0 }); await mutateNestedArchive(outerPath, options.path ?? innerPath, action, setStatusText, setpaths, options); };
  const chooseFile = async () => await invoke('open_file_dialog');
  const chooseDir = async () => await invoke('open_dir_dialog');
  const rename = () => {
    setContextMenu({ visible: false, x: 0, y: 0 });
    setRenamePromptMessage({
      message: isFile ? 'Rename the internal archive file:' : 'Rename the internal archive directory:',
      path: name,
      nestedChain: outerPath,
      nestedPath: innerPath,
    });
    setIsAddPrompt(false);
    setIsModalOpen(true);
  };
  const actions = isFile ? [
    { label: 'Edit', method: () => { setContextMenu({ visible: false, x: 0, y: 0 }); editNestedSarcFile(outerPath, innerPath, setStatusText, setActiveTab, setLabelTextDisplay, updateEditorContent); }, icon: 'context_menu/edit.png', shortcut: 'F3' },
    
    { label: 'Extract', method: () => { setContextMenu({ visible: false, x: 0, y: 0 }); extractNestedSarcFile(outerPath, innerPath, setStatusText); }, icon: 'context_menu/extract.png', shortcut: '' },
    { label: 'Replace', method: async () => { const sourcePath = await chooseFile(); if (sourcePath) await mutate('replace', { sourcePath }); }, icon: 'context_menu/replace.png', shortcut: 'Ctrl+R' },
    { label: 'Delete', method: async () => { if (window.confirm(`Delete ${innerPath}?`)) await mutate('delete'); }, icon: 'context_menu/remove.png', shortcut: '' },
    { label: 'Rename', method: rename, icon: 'context_menu/rename.png', shortcut: '' },
    { label: 'Compare', method: async () => {
      setContextMenu({ visible: false, x: 0, y: 0 });
      const content = await mutateNestedArchive(outerPath, innerPath, 'compare', setStatusText, setpaths);
      if (content?.tab === 'COMPARER') {
        const data = content.compare_data ?? {};
        const file1 = data.file1 ?? {};
        const file2 = data.file2 ?? {};
        const content1 = file1.text ?? '';
        const content2 = file2.text ?? '';
        setCompareData((previous) => ({
          ...previous,
          content1,
          content2,
          filepath1: file1.path?.full_path ?? '',
          filepath2: file2.path?.full_path ?? '',
          label1: file1.label || innerPath,
          label2: file2.label || 'Original',
          isInternal: true,
          isTiedToMonaco: false,
          isSmall: content1.length < 500000 && content2.length < 500000,
          lang: content.lang || 'yaml',
        }));
        setActiveTab('COMPARER');
      }
    }, icon: 'context_menu/compare.png', shortcut: '' },
    { label: 'Copy path', method: () => { navigator.clipboard.writeText(innerPath); setStatusText('Copied to clipboard'); setContextMenu({ visible: false, x: 0, y: 0 }); }, icon: 'context_menu/copy.png', shortcut: '' },
    { label: 'Expand archive', method: async () => { setContextMenu({ visible: false, x: 0, y: 0 }); if (await expandNestedSarc(childChain, setStatusText, setpaths, setPathsFilters)) setExpanded(true); }, icon: 'dir_opened.png', shortcut: '' },
    { label: 'Close', method: () => setContextMenu({ visible: false, x: 0, y: 0 }), icon: 'context_menu/close.png', shortcut: '' },
  ] : [
    { label: 'Add file', method: async () => { const sourcePath = await chooseFile(); if (sourcePath) { const fileName = sourcePath.replace(/\\/g, '/').split('/').pop(); await mutate('add', { sourcePath, newPath: null, path: `${innerPath}/${fileName}` }); } }, icon: 'context_menu/add_file.png', shortcut: '' },
    { label: 'Add folder', method: async () => { const sourcePath = await chooseDir(); if (sourcePath) await mutate('add_dir', { sourcePath }); }, icon: 'context_menu/add_dir.png', shortcut: '' },
    { label: 'Extract', method: () => mutate('extract_folder'), icon: 'context_menu/extract.png', shortcut: 'Ctrl+E' },
    { label: 'New byml', method: () => mutate('new_byml'), icon: 'context_menu/byml.png', shortcut: '' },
    { label: 'Delete', method: async () => { if (window.confirm(`Delete ${innerPath} and its contents?`)) await mutate('delete'); }, icon: 'context_menu/remove.png', shortcut: '' },
    { label: 'Rename', method: rename, icon: 'context_menu/rename.png', shortcut: '' },
    { label: 'Close', method: () => setContextMenu({ visible: false, x: 0, y: 0 }), icon: 'context_menu/close.png', shortcut: '' },
  ];
  return <li onClick={select}>
    <div style={{ borderRadius: '5px', width: '95%', cursor: 'pointer', display: 'flex', alignItems: 'center', color: 'white', backgroundColor: selectedStyle }}
      onDoubleClick={open}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        setContextMenu({ visible: true, x: event.clientX, y: event.clientY });
      }}>
      <img src={isFile ? fileIcon : isCollapsed ? dirClosed : dirOpened} alt={name}
        style={{ marginRight: '5px', width: iconSize, height: iconSize }}
        onClick={(event) => { event.stopPropagation(); if (!isFile) toggleExpanded(); }} />
      <span>{name}</span>
    </div>
    {isFile && !isCollapsed && expandedArchive?.length > 0 && <ul style={{ marginLeft: '40px', listStyleType: 'none', padding: 0 }}>
      {Object.entries(buildNestedTree(expandedArchive)).map(([childName, child]) => <NestedDirectoryNode key={childName}
        node={child} name={childName} innerParent="" outerPath={childChain} selected={selected} onSelect={onSelect} />)}
    </ul>}
    {!isFile && <div className={`node-children ${isCollapsed ? 'collapsed' : 'expanded'}`}>
      <ul style={{ marginLeft: '40px', listStyleType: 'none', padding: 0 }}>
        {Object.entries(node).map(([childName, child]) => <NestedDirectoryNode key={childName} node={child} name={childName}
          innerParent={innerPath} outerPath={outerPath} selected={selected} onSelect={onSelect} />)}
      </ul>
    </div>}
    {contextMenu.visible && <ContextMenu x={contextMenu.x} y={contextMenu.y}
      onClose={() => setContextMenu({ visible: false, x: 0, y: 0 })} actions={actions} settings={settings} />}
  </li>;
};

const ContextMenu = ({ x, y, onClose, actions, settings }) => {
  const menuRef = useRef(null);
  const [position, setPosition] = useState({ x, y });

  useLayoutEffect(() => {
    const bounds = menuRef.current?.getBoundingClientRect();
    if (!bounds) return;
    const margin = 8;
    setPosition({
      x: Math.max(margin, Math.min(x, window.innerWidth - bounds.width - margin)),
      y: Math.max(margin, Math.min(y, window.innerHeight - bounds.height - margin)),
    });
  }, [x, y, actions.length]);

  return (
    <ul
      ref={menuRef}
      className="context-menu"
      style={{
        fontSize: settings.contextMenuFontSize,
        position: 'fixed',
        top: position.y,
        left: position.x,
        listStyleType: 'none',
        padding: '6px',
        boxShadow: '0 2px 10px rgba(0,0,0,0.2)',
        zIndex: 100,
      }}
      onMouseLeave={onClose}
    >
      {actions.map((action, index) => (
        <li
          key={index}
          className="context-menu-item"
          onClick={() => action.method()}
          style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}
        >
          <div style={{ display: 'flex', alignItems: 'center' }}>
            <img src={action.icon} alt={action.label} style={{ marginRight: '10px', width: '20px', height: '20px' }} />
            {action.label}
          </div>
          <span style={{ marginLeft: '10px', color: '#bcbcbc' }}>{action.shortcut} </span>
        </li>
      ))}
    </ul>
  );
};
//{ editorRef, updateEditorContent, setStatusText, activeTab, setActiveTab, setLabelTextDisplay, setpaths, selectedPath, changeModal }
const DirectoryNode = ({ node, name, path, onContextMenu, sarcPaths, selected, onSelect }) => {
  const {
    settings, setSettings,
    renamePromptMessage, setRenamePromptMessage,
    isAddPrompt, setIsAddPrompt,
    activeTab, setActiveTab,
    editorContainerRef, editorRef, editorValue, setEditorValue, lang, setLang,
    statusText, setStatusText, selectedPath, setSelectedPath, labelTextDisplay, setLabelTextDisplay,
    paths, setpaths, setPathsFilters, treeExpandedNodes, setTreeExpandedNodes,
    isModalOpen, setIsModalOpen, updateEditorContent, changeModal, setCompareData, setInternalSarcPath, setReadOnly
  } = useEditorContext();

  const [contextMenu, setContextMenu] = useState({ visible: false, x: 0, y: 0 });
  const isFile = node === null;
  const fullPath = path ? `${path}/${name}` : name;
  const expansionKey = `root:${fullPath}`;
  const isCollapsed = !treeExpandedNodes.has(expansionKey);
  const setExpanded = (expanded) => setTreeExpandedNodes((current) => {
    const next = new Set(current);
    if (expanded) next.add(expansionKey);
    else next.delete(expansionKey);
    return next;
  });
  const toggleExpanded = () => setExpanded(isCollapsed);
  // const endian = "LE";
  const isSelected = selected === fullPath;

  const handleDoubleClick = async (e) => {
    e.stopPropagation(); // Prevent the click from bubbling up to parent elements
    console.log(`Double-clicked on directory: ${fullPath}`);
    if (isFile) {
      if (sarcPaths.nested_paths?.[fullPath]) {
        toggleExpanded();
        return;
      }
      const expanded = await expandNestedSarc(fullPath, setStatusText, setpaths, setPathsFilters);
      if (expanded) setExpanded(true);
      else handleOpenInternalSarcFile();
    } else {
      toggleCollapse();
    }
    // Add your custom double-click logic here
  };

  const handleSelect = (e) => {
    e.stopPropagation(); // This stops the event from bubbling up further
    console.log(`Selected: ${fullPath}`);
    onSelect(fullPath, isFile); // Pass the fullPath to the onSelect function
  };

  const handleAddClick = () => {
    closeContextMenu();
    setIsAddPrompt(true);
    setIsModalOpen(true);
  }

  const handleExtractInternalSarcFile = () => {
    closeContextMenu();
    if (isFile) {
      extractFileClick({ path: fullPath }, setStatusText);
    }
  };
  const handleExtractInternalSarcFolder = () => {
    closeContextMenu();
    if (!isFile) {
      console.log("Extracting folder:", fullPath);
      extractFolderClick(fullPath, setStatusText);
    }
  };

  const handleOpenInternalSarcFile = () => {
    closeContextMenu();
    if (isFile) {
      if (sarcPaths.read_only) openBphclLeaf(fullPath, setStatusText, setActiveTab, setLabelTextDisplay, updateEditorContent, setReadOnly);
      else editInternalSarcFile(fullPath, setStatusText, setActiveTab, setLabelTextDisplay, updateEditorContent);
    }
  };
  const handleCompareInternalSarcFile = () => {
    closeContextMenu();
    if (isFile) {
      compareInternalFileWithOVanila(selectedPath.path, setStatusText, setActiveTab, setCompareData);
    }
  };
  const handleRemoveInternalSarcFile = () => {
    closeContextMenu();
    removeInternalFileClick(fullPath, setStatusText, setpaths);
  };
  const handleRemoveBphclNode = async () => {
    closeContextMenu();
    if (window.confirm(`Delete ${name}?`)) {
      await removeBphclNodeClick(fullPath, setStatusText, setpaths);
    }
  };
  const handleReplaceInternalSarcFile = () => {
    closeContextMenu();
    replaceInternalFileClick(fullPath, setStatusText, setpaths);
  };
  const handleAddInternalSarcFileToDir = () => {
    closeContextMenu();
    addInternalFileToDir(fullPath, setStatusText, setpaths);
  };  
  const handleAddFilesFromDirRecursively = () => {
    closeContextMenu();
    addFilesFromDirRecursively(fullPath, setStatusText, setpaths);
  };
  const handleAddEmptyByml = () => {
    closeContextMenu();
    addEmptyByml(fullPath, setStatusText, setpaths);
  };

  const handleRenameInternalSarcFile = () => {
    closeContextMenu();
    if (isFile) {
      setRenamePromptMessage({ message: "Rename the internal SARC file:", path: name });
    } else {
      setRenamePromptMessage({ message: "Rename the internal SARC directory:", path: name });
    }
    setIsAddPrompt(false);
    setIsModalOpen(true);

  }
  const handlePathToClipboard = (text) => {
    closeContextMenu();
    navigator.clipboard.writeText(text).then(() => {
      console.log('Text copied to clipboard');
    }).catch(err => {
      console.error('Failed to copy text: ', err);
    });
    setStatusText(`Copied to clipboard`);
  }


  const nodeStyle = {
    borderRadius: '5px',
    width: '95%',
    cursor: 'pointer',
    display: 'flex',
    alignItems: 'center',
    color: isSelected && isFile ? 'white' : 'white',
    backgroundColor: //isFile ?
      isSelected ?
        sarcPaths.added_paths.includes(fullPath) ? '#2D8589' :
          sarcPaths.modded_paths.includes(fullPath) ? '#B78F00' :
            '#303030' :
        sarcPaths.added_paths.includes(fullPath) ? '#1E595B' :
          sarcPaths.modded_paths.includes(fullPath) ? '#826C00' :
            'transparent'// :
    // 'transparent'


  };

  const toggleCollapse = () => {
    toggleExpanded();
    closeContextMenu();
  };

  const handleIconClick = (e) => {
    if (!isFile) {
      toggleCollapse();
    }
    e.stopPropagation();
  };

  const handleIconContextMenu = (e) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({
      visible: true,
      x: e.clientX,
      y: e.clientY,
    });
    onContextMenu && onContextMenu(fullPath);
  };

  const closeContextMenu = () => {
    setContextMenu({ visible: false, x: 0, y: 0 });
  };

  const bphclNode = sarcPaths.read_only && (fullPath.includes('/Cloth/') || fullPath.includes('/Collidables/'));
  const contextMenuActions = bphclNode ? [
    { label: 'View', method: handleOpenInternalSarcFile, icon: 'context_menu/edit.png', shortcut: 'F3' },
    { label: 'Extract', method: handleExtractInternalSarcFile, icon: 'context_menu/extract.png', shortcut: 'Ctrl+E' },
    { label: 'Delete', method: handleRemoveBphclNode, icon: 'context_menu/remove.png', shortcut: '' },
    { label: 'Copy path', method: () => handlePathToClipboard(fullPath), icon: 'context_menu/copy.png', shortcut: '' },
    { label: 'Close', method: () => closeContextMenu(), icon: 'context_menu/close.png', shortcut: '' },
  ] : isFile ? [
    { label: 'Edit', method: handleOpenInternalSarcFile, icon: 'context_menu/edit.png', shortcut: 'F3' },
    { label: 'Compare', method: handleCompareInternalSarcFile, icon: 'context_menu/compare.png', shortcut: '' },
    { label: 'Extract', method: handleExtractInternalSarcFile, icon: 'context_menu/extract.png', shortcut: 'Ctrl+E' },
    { label: 'Replace', method: handleReplaceInternalSarcFile, icon: 'context_menu/replace.png', shortcut: 'Ctrl+R' },
    { label: 'Delete', method: handleRemoveInternalSarcFile, icon: 'context_menu/remove.png', shortcut: '' },
    { label: 'Rename', method: handleRenameInternalSarcFile, icon: 'context_menu/rename.png', shortcut: '' },
    { label: 'Copy path', method: () => handlePathToClipboard(fullPath), icon: 'context_menu/copy.png', shortcut: '' },
    { label: 'Expand archive', method: async () => { closeContextMenu(); if (await expandNestedSarc(fullPath, setStatusText, setpaths, setPathsFilters)) setExpanded(true); }, icon: 'dir_opened.png', shortcut: '' },
    { label: 'Close', method: () => closeContextMenu(), icon: 'context_menu/close.png', shortcut: '' },
  ] : [
    { label: 'Add file', method: handleAddInternalSarcFileToDir, icon: 'context_menu/add_file.png', shortcut: '' },
    { label: 'Add folder', method: handleAddFilesFromDirRecursively, icon: 'context_menu/add_dir.png', shortcut: '' },
    { label: 'Extract', method: handleExtractInternalSarcFolder, icon: 'context_menu/extract.png', shortcut: 'Ctrl+E' },
    { label: 'New byml', method: handleAddEmptyByml, icon: 'context_menu/byml.png', shortcut: '' },
    { label: 'Delete', method: handleRemoveInternalSarcFile, icon: 'context_menu/remove.png', shortcut: '' },
    { label: 'Rename', method: handleRenameInternalSarcFile, icon: 'context_menu/rename.png', shortcut: '' },
    { label: 'Close', method: () => closeContextMenu(), icon: 'context_menu/close.png', shortcut: '' },
  ];

  return (
    <li onClick={handleSelect}>
      <div style={nodeStyle}
        onContextMenu={handleIconContextMenu}
        onDoubleClick={handleDoubleClick}>
        <img
          src={isFile ? fileIcon : isCollapsed ? dirClosed : dirOpened}
          alt={name}
          style={{ marginRight: '5px', width: iconSize, height: iconSize }}
          onClick={handleIconClick}
          onContextMenu={handleIconContextMenu}
        />
        <span onClick={toggleCollapse}>{name}</span>
      </div>
      {isFile && !isCollapsed && sarcPaths.nested_paths?.[fullPath]?.length > 0 && (
        <ul style={{ marginLeft: '40px', listStyleType: 'none', padding: 0 }}>
          {Object.entries(buildNestedTree(sarcPaths.nested_paths[fullPath])).map(([nestedName, nestedNode]) => (
            <NestedDirectoryNode key={nestedName} node={nestedNode} name={nestedName} innerParent=""
              outerPath={fullPath} selected={selected} onSelect={onSelect} />
          ))}
        </ul>
      )}
      {!isFile && (
        <div className={`node-children ${isCollapsed ? 'collapsed' : 'expanded'}`}>
          <ul style={{ marginLeft: '40px', listStyleType: 'none', padding: 0 }}>
            {Object.entries(node).map(([key, value]) => (
              <DirectoryNode
                key={key}
                node={value}
                name={key}
                path={fullPath}
                onContextMenu={onContextMenu}
                sarcPaths={sarcPaths}
                selected={selected} // Make sure this is passed correctly
                onSelect={onSelect} // Make sure this is passed correctly
              />
            ))}
          </ul>
        </div>
      )}
      {contextMenu.visible && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={closeContextMenu}
          actions={contextMenuActions}
          settings={settings}
        />
      )}
    </li>
  );
};


export default DirectoryNode;

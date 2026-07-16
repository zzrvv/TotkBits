import { invoke } from './DocumentState';
import * as monaco from "monaco-editor";
import { OpenFileFromPath } from './ButtonClicks';
import { getActiveDocumentId } from './DocumentState';


const InitializeEditor = (props) => {
  const {
    editorRef,
    editorContainerRef,
    editorValue,
    // lang,
    setStatusText,
    setActiveTab,
    setLabelTextDisplay,
    setpaths,
    updateEditorContent,
    settings,
    setSettings,
  } = props;

  console.log("Initializing Monaco editor");

  const initialDocumentId = getActiveDocumentId();
  const initialModel = monaco.editor.createModel(
    editorValue,
    settings.lang,
    monaco.Uri.parse(`inmemory://totkbits/${initialDocumentId}`),
  );
  // Create the editor with default options
  editorRef.current = monaco.editor.create(editorContainerRef.current, {
    model: initialModel,
    theme: settings.theme,
    minimap: { enabled: settings.minimap },
    wordWrap: 'on',
    fontSize: settings.fontSize,
  });
  if (props.documentModels) {
    props.documentModels.current.set(initialDocumentId, initialModel);
  }

  invoke('get_startup_data').then((data) => {
    // Use object spread to combine default settings with fetched data
    const updatedSettings = { ...settings, ...data };
    // setSettings(updatedSettings);  // Update state for future re-renders
    settings.argv1 = updatedSettings.argv1;  
    settings.fontSize = updatedSettings.fontSize;  
    settings.theme = updatedSettings.theme;  
    settings.minimap = updatedSettings.minimap;  
    settings.contextMenuFontSize = updatedSettings.contextMenuFontSize;  
    settings.zstd_msg = data.zstd_msg;  
    


    console.log("settings:", settings);
    console.log("received data:", data);

    // Update all configurable options at once
    editorRef.current.updateOptions({
      fontSize: settings.fontSize,
      theme: settings.theme,
      minimap: { enabled: settings.minimap }
    });

    if (settings.argv1) {
      console.log('Received command-line argument:', settings.argv1);
      OpenFileFromPath(settings.argv1, setStatusText, setActiveTab, setLabelTextDisplay, setpaths, updateEditorContent);
    } else {
      console.log('No command-line argument provided.');
    }
  }).catch((error) => {
    console.error('Error fetching startup data:', error);
  });
};

export default InitializeEditor;

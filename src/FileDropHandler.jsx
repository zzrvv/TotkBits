import { useEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { OpenFileFromPath } from './ButtonClicks';

export const useFileDropHandler = (setStatusText, setActiveTab, setLabelTextDisplay, setpaths, updateEditorContent) => {
  const canProcessEvent = useRef(true);
  const [isFileHovering, setIsFileHovering] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten = null;

    const handleDragDropEvent = async ({ payload }) => {
      if (payload.type === 'enter' || payload.type === 'over') {
        setIsFileHovering(true);
        return;
      }

      setIsFileHovering(false);
      if (payload.type === 'drop' && canProcessEvent.current && payload.paths.length > 0) {
        canProcessEvent.current = false; // Set the flag to false to block processing
        const file = payload.paths[0];

        try {
          await OpenFileFromPath(file, setStatusText, setActiveTab, setLabelTextDisplay, setpaths, updateEditorContent);
        } catch (error) {
          console.error('Error processing file:', error);
        }

        // Reset the flag after 0.7 seconds
        setTimeout(() => {
          canProcessEvent.current = true;
        }, 700);
      }
    };

    const setupListener = async () => {
      try {
        const stopListening = await getCurrentWebview().onDragDropEvent(handleDragDropEvent);
        if (disposed) {
          stopListening();
        } else {
          unlisten = stopListening;
        }
      } catch (error) {
        console.error('Failed to set up listener:', error);
      }
    };

    setupListener();

    return () => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  return isFileHovering;
};


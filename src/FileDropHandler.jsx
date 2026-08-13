import { useEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { message } from '@tauri-apps/plugin-dialog';
import { OpenFileFromPath } from './ButtonClicks';

export const useFileDropHandler = (setStatusText, setActiveTab, setLabelTextDisplay, setpaths, updateEditorContent) => {
  const canProcessEvent = useRef(true);
  const [isFileHovering, setIsFileHovering] = useState(false);
  const [parsingFile, setParsingFile] = useState('');

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
        const failedFiles = [];
        try {
          for (const file of payload.paths) {
            try {
              setParsingFile(file.replace(/\\/g, '/').split('/').pop());
              const opened = await OpenFileFromPath(file, setStatusText, setActiveTab, setLabelTextDisplay, setpaths, updateEditorContent, true);
              if (!opened) failedFiles.push(file);
              await new Promise((resolve) => requestAnimationFrame(resolve));
            } catch (_) {
              failedFiles.push(file);
            }
          }
          setParsingFile('');
          setStatusText("Ready");
          if (failedFiles.length > 0) {
            const fileList = failedFiles.map((file) => `• ${file}`).join('\n');
            await message(
              `${failedFiles.length} file${failedFiles.length === 1 ? '' : 's'} failed to open:\n\n${fileList}`,
              { title: 'TotkBits - Open errors', kind: 'error' },
            );
          }
        } catch (error) {
          console.error('Error processing file:', error);
          setParsingFile('');
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

  return { isFileHovering, parsingFile };
};


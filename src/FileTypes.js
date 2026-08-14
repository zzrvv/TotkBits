const UNSAVEABLE_FILE_TYPES = new Set(['BPHCL', 'HKCL', 'BPHHB', 'GLB', 'MII', 'OTHER']);

export const isFileTypeSaveable = (fileType) => !UNSAVEABLE_FILE_TYPES.has(fileType || 'NONE');
